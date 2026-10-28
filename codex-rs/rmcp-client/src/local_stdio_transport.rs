//! Protocol framing and child lifetime for locally spawned MCP servers.
//!
//! Process creation is platform-specific; protocol selection and shutdown are shared.

use std::future::Future;
use std::io;
use std::time::Duration;

use codex_utils_pty::Command;
use futures::FutureExt;
use rmcp::service::RoleClient;
use rmcp::service::RxJsonRpcMessage;
use rmcp::service::TxJsonRpcMessage;
use rmcp::transport::Transport;
use rmcp::transport::async_rw::AsyncRwTransport;
#[cfg(not(windows))]
use tokio::process::ChildStderr;
use tokio::process::ChildStdin;
#[cfg(not(windows))]
use tokio::process::ChildStdout;

use crate::bounded_stdio_transport::BoundedStdioTransport;
use crate::protocol_mode::McpProtocolMode;
use codex_utils_pty::Child;

#[cfg(windows)]
pub(super) type LocalStdioStdout = tokio::net::windows::named_pipe::NamedPipeServer;
#[cfg(windows)]
pub(super) type LocalStdioStderr = tokio::net::windows::named_pipe::NamedPipeServer;
#[cfg(not(windows))]
pub(super) type LocalStdioStdout = ChildStdout;
#[cfg(not(windows))]
pub(super) type LocalStdioStderr = ChildStderr;

#[cfg(windows)]
pub(super) type LocalChild = std::sync::Arc<tokio::sync::Mutex<Box<Child>>>;

pub(super) struct LocalStdioTransport {
    #[cfg(windows)]
    child: LocalChild,
    #[cfg(not(windows))]
    child: Box<Child>,
    // Spawn identity only; liveness is established by the exact Child's wait result.
    process_id: Option<u32>,
    transport: StdioTransport,
}

enum StdioTransport {
    /// Preserve rmcp's existing framing for servers using the initialize handshake.
    Legacy(AsyncRwTransport<RoleClient, LocalStdioStdout, ChildStdin>),
    /// Bound frames and skip messages unknown to the client during 2026-07-28 discovery.
    V20260728(BoundedStdioTransport),
}

impl LocalStdioTransport {
    pub(super) fn spawn(
        mut command: Command,
        program_name: String,
        protocol_mode: McpProtocolMode,
    ) -> io::Result<(Self, Option<LocalStdioStderr>)> {
        #[cfg(windows)]
        let (stdout, stderr) = {
            let (stdout, child_stdout) = crate::windows_stdio::output_pipe()?;
            let (stderr, child_stderr) = crate::windows_stdio::output_pipe()?;
            command.stdout_file(child_stdout).stderr_file(child_stderr);
            (stdout, Some(stderr))
        };
        let mut child = command.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("MCP server stdin was not piped"))?;
        #[cfg(not(windows))]
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("MCP server stdout was not piped"))?;
        #[cfg(not(windows))]
        let stderr = child.stderr.take();
        let transport = match protocol_mode {
            McpProtocolMode::Legacy => StdioTransport::Legacy(AsyncRwTransport::new(stdout, stdin)),
            McpProtocolMode::V20260728 => {
                StdioTransport::V20260728(BoundedStdioTransport::new(stdin, stdout, program_name))
            }
        };
        let process_id = child.id();
        #[cfg(windows)]
        let child = std::sync::Arc::new(tokio::sync::Mutex::new(Box::new(child)));
        #[cfg(not(windows))]
        let child = Box::new(child);
        Ok((
            Self {
                child,
                process_id,
                transport,
            },
            stderr,
        ))
    }

    pub(super) fn id(&self) -> Option<u32> {
        self.process_id
    }

    #[cfg(windows)]
    pub(super) fn child_termination_handle(&self) -> LocalChild {
        std::sync::Arc::clone(&self.child)
    }

    #[cfg(windows)]
    pub(super) async fn terminate_failed_launch(&mut self) {
        terminate_local_child(&self.child).await;
    }
}

#[cfg(windows)]
pub(super) async fn terminate_local_child(child: &LocalChild) {
    let mut child = child.lock().await;
    // Retain Tokio's exact child handle across a post-spawn launcher error.
    // Dropping a suspended child only requests an asynchronous kill.
    loop {
        match child.kill().await {
            Ok(()) => return,
            Err(error) => {
                tracing::warn!("Failed MCP launch cleanup remains unconfirmed: {error}")
            }
        }
        if let Ok(Ok(_)) = tokio::time::timeout(Duration::from_millis(50), child.wait()).await {
            return;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

impl Transport<RoleClient> for LocalStdioTransport {
    type Error = io::Error;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleClient>,
    ) -> impl Future<Output = io::Result<()>> + Send + 'static {
        match &mut self.transport {
            StdioTransport::Legacy(transport) => transport.send(item).boxed(),
            StdioTransport::V20260728(transport) => transport.send(item).boxed(),
        }
    }

    fn receive(&mut self) -> impl Future<Output = Option<RxJsonRpcMessage<RoleClient>>> + Send {
        match &mut self.transport {
            StdioTransport::Legacy(transport) => transport.receive().boxed(),
            StdioTransport::V20260728(transport) => transport.receive().boxed(),
        }
    }

    async fn close(&mut self) -> io::Result<()> {
        match &mut self.transport {
            StdioTransport::Legacy(transport) => transport.close().await?,
            StdioTransport::V20260728(transport) => transport.close().await?,
        }
        #[cfg(windows)]
        let mut child = self.child.lock().await;
        #[cfg(not(windows))]
        let child = &mut self.child;
        match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
            Ok(status) => status.map(|_| ()),
            Err(_) => child.kill().await,
        }
    }
}
