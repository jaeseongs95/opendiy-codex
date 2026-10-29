// Modified for OpenDIY Codex R28 (2026-10-29); see MODIFICATIONS.md.
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
pub(super) type LocalChild = std::sync::Arc<tokio::sync::Mutex<Box<LocalChildProcess>>>;

#[cfg(all(windows, not(test)))]
type LocalChildProcess = Child;

#[cfg(all(windows, test))]
pub(super) enum LocalChildProcess {
    Real(Child),
    Fault(std::sync::Arc<CleanupFault>),
}

#[cfg(all(windows, test))]
#[derive(Default)]
pub(super) struct CleanupFault {
    pub(super) confirmed: std::sync::atomic::AtomicBool,
    pub(super) kill_calls: std::sync::atomic::AtomicUsize,
    pub(super) wait_calls: std::sync::atomic::AtomicUsize,
    pub(super) transient_errors: std::sync::atomic::AtomicUsize,
}

#[cfg(all(windows, test))]
impl From<Child> for LocalChildProcess {
    fn from(child: Child) -> Self {
        Self::Real(child)
    }
}

#[cfg(all(windows, test))]
impl LocalChildProcess {
    pub(super) fn id(&self) -> Option<u32> {
        match self {
            Self::Real(child) => child.id(),
            Self::Fault(fault) => {
                (!fault.confirmed.load(std::sync::atomic::Ordering::SeqCst)).then_some(1)
            }
        }
    }
    pub(super) async fn kill(&mut self) -> io::Result<()> {
        match self {
            Self::Real(child) => child.kill().await,
            Self::Fault(fault) => {
                use std::sync::atomic::Ordering;
                fault.kill_calls.fetch_add(1, Ordering::SeqCst);
                if fault
                    .transient_errors
                    .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                    == Ok(1)
                {
                    fault.confirmed.store(true, Ordering::SeqCst);
                }
                if fault.confirmed.load(Ordering::SeqCst) {
                    Ok(())
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "injected kill failure",
                    ))
                }
            }
        }
    }
    pub(super) async fn wait(&mut self) -> io::Result<std::process::ExitStatus> {
        match self {
            Self::Real(child) => child.wait().await,
            Self::Fault(fault) => {
                use std::os::windows::process::ExitStatusExt;
                use std::sync::atomic::Ordering;
                fault.wait_calls.fetch_add(1, Ordering::SeqCst);
                if fault.confirmed.load(Ordering::SeqCst) {
                    Ok(std::process::ExitStatus::from_raw(0))
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "injected wait failure",
                    ))
                }
            }
        }
    }
}

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
        let child = std::sync::Arc::new(tokio::sync::Mutex::new(Box::new(child.into())));
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
}

#[cfg(windows)]
pub(super) async fn terminate_local_child(child: &LocalChild) -> io::Result<()> {
    let mut reason = "exact Child was busy before the cleanup deadline".to_string();
    let attempt = async {
        let mut child = child.lock().await;
        loop {
            match tokio::time::timeout(Duration::from_millis(250), child.kill()).await {
                Ok(Ok(())) => return,
                Ok(Err(error)) => reason = format!("kill failed: {error}"),
                Err(_) => reason = "kill did not finish before its attempt deadline".into(),
            }
            match tokio::time::timeout(Duration::from_millis(50), child.wait()).await {
                Ok(Ok(_)) => return,
                Ok(Err(error)) => reason.push_str(&format!("; wait failed: {error}")),
                Err(_) => reason.push_str("; wait has no exit evidence"),
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(3), attempt)
        .await
        .map_err(|_| crate::stdio_server_launcher::unconfirmed_termination_with_detail(reason))
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
