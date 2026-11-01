// Modified for OpenDIY Codex R31 (2026-11-01); see MODIFICATIONS.md.
//! Protocol framing and child lifetime for locally spawned MCP servers.
//!
//! Process creation is platform-specific; protocol selection and shutdown are shared.

use std::future::Future;
use std::io;
use std::time::Duration;

use futures::FutureExt;
use rmcp::model::ClientRequest;
use rmcp::model::ErrorCode;
use rmcp::model::JsonRpcMessage;
use rmcp::model::RequestId;
use rmcp::model::ServerResult;
use rmcp::service::RoleClient;
use rmcp::service::RxJsonRpcMessage;
use rmcp::service::TxJsonRpcMessage;
use rmcp::transport::Transport;
use rmcp::transport::async_rw::AsyncRwTransport;
use tokio::io::AsyncRead;
#[cfg(not(windows))]
use tokio::process::ChildStderr;
use tokio::process::ChildStdin;
#[cfg(not(windows))]
use tokio::process::ChildStdout;
use tokio::sync::oneshot;

use crate::bounded_stdio_transport::BoundedStdioTransport;
use crate::protocol_mode::McpProtocolMode;
use codex_utils_pty::Child;
use codex_utils_pty::Command;

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
    child: Option<Box<Child>>,
    // Spawn identity only; liveness is established by the exact Child's wait result.
    process_id: Option<u32>,
    transport: StdioTransport,
    discovery: Option<Discovery>,
}

pub(super) enum DiscoveryOutcome {
    Reply,
    RetryLegacy,
    Late {
        reply: Box<RxJsonRpcMessage<RoleClient>>,
        after_reoffer: bool,
    },
}

struct Discovery {
    finished: Option<oneshot::Sender<DiscoveryOutcome>>,
    outcome: Option<DiscoveryOutcome>,
    retry_at: Option<tokio::time::Instant>,
    probe_id: Option<RequestId>,
    initialize_id: Option<RequestId>,
}

impl Drop for Discovery {
    fn drop(&mut self) {
        // A version re-offer can supersede an earlier reply. Publish only when startup ends.
        if let Some(tx) = self.finished.take()
            && let Some(outcome) = self.outcome.take()
        {
            let _ = tx.send(outcome);
        }
    }
}

impl Discovery {
    /// Track discovery replies and skip messages that do not answer the active handshake.
    fn handle_reply(&mut self, message: &mut Option<RxJsonRpcMessage<RoleClient>>) -> bool {
        let Some(reply) = message.as_ref() else {
            self.outcome.get_or_insert(DiscoveryOutcome::RetryLegacy);
            return false;
        };
        let (id, modern) = match reply {
            JsonRpcMessage::Response(response) => (
                Some(&response.id),
                matches!(response.result, ServerResult::DiscoverResult(_)),
            ),
            JsonRpcMessage::Error(error) => (
                error.id.as_ref(),
                matches!(
                    error.error.code,
                    ErrorCode::UNSUPPORTED_PROTOCOL_VERSION
                        | ErrorCode::HEADER_MISMATCH
                        | ErrorCode::MISSING_REQUIRED_CLIENT_CAPABILITY
                ),
            ),
            _ => return false,
        };
        let matches_id = |sent: &RequestId| match (sent, id) {
            (RequestId::Number(sent), Some(RequestId::String(received))) => {
                received.parse::<i64>().ok() == Some(*sent)
            }
            (_, Some(received)) => sent == received,
            (_, None) => false,
        };
        let legacy = self.initialize_id.is_some();
        let matches_probe = self.probe_id.as_ref().is_some_and(matches_id);
        let idless = self.probe_id.is_some() && id.is_none();
        if legacy
            && (matches_probe || idless)
            && matches!(reply, JsonRpcMessage::Error(error) if error.error.code == ErrorCode::METHOD_NOT_FOUND)
        {
            // If initialize also fails, try it as the first request on a fresh helper.
            self.outcome.get_or_insert(DiscoveryOutcome::RetryLegacy);
        }
        // With one request rmcp accepts an ID-less error. With two, only a modern
        // protocol error can be classified as a discovery reply without an ID.
        if matches_probe || (idless && (!legacy || modern)) {
            self.probe_id = None;
            if legacy {
                // Auto sent initialize before receiving the reply to its earlier probe.
                if !modern && matches!(reply, JsonRpcMessage::Error(_)) {
                    return true;
                }
                // Stop the obsolete initialize and let the caller classify the reply.
                self.outcome = message.take().map(|reply| DiscoveryOutcome::Late {
                    reply: Box::new(reply),
                    after_reoffer: matches!(self.outcome, Some(DiscoveryOutcome::Reply)),
                });
                return false;
            }
            if modern {
                self.retry_at = None;
                self.outcome = Some(DiscoveryOutcome::Reply);
            }
        } else if self.probe_id.is_some()
            && id.is_some()
            && !self.initialize_id.as_ref().is_some_and(matches_id)
        {
            // rmcp would abort the handshake before reading the reply to discover or initialize.
            return true;
        } else if legacy && matches!(reply, JsonRpcMessage::Response(_)) {
            // Once initialize wins, rmcp owns the session and can ignore the obsolete probe.
            self.probe_id = None;
            self.retry_at = None;
            self.outcome = Some(DiscoveryOutcome::Reply);
        }
        false
    }
}

enum StdioTransport {
    /// Preserve rmcp's existing framing for servers using the initialize handshake.
    Legacy(AsyncRwTransport<RoleClient, Box<dyn AsyncRead + Unpin + Send>, ChildStdin>),
    /// Bound frames and skip messages unknown to the client during 2026-07-28 discovery.
    Modern(BoundedStdioTransport),
    Closed,
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
            McpProtocolMode::Legacy => {
                StdioTransport::Legacy(AsyncRwTransport::new(Box::new(stdout), stdin))
            }
            McpProtocolMode::V20260728 | McpProtocolMode::DiscoverLocalStdio => {
                StdioTransport::Modern(BoundedStdioTransport::new(stdin, stdout, program_name))
            }
        };
        let process_id = child.id();
        #[cfg(windows)]
        let child = std::sync::Arc::new(tokio::sync::Mutex::new(Box::new(child.into())));
        #[cfg(not(windows))]
        let child = Some(Box::new(child));
        Ok((
            Self {
                child,
                process_id,
                transport,
                discovery: None,
            },
            stderr,
        ))
    }

    pub(super) fn id(&self) -> Option<u32> {
        self.process_id
    }

    pub(super) fn watch_discovery(
        &mut self,
        timeout: Option<Duration>,
    ) -> oneshot::Receiver<DiscoveryOutcome> {
        let (tx, rx) = oneshot::channel();
        self.discovery = Some(Discovery {
            finished: Some(tx),
            outcome: None,
            // Aim for three seconds of discovery. Reserve at least a quarter of short
            // timeouts and half of longer ones for a fresh initialize.
            retry_at: timeout.and_then(|limit| {
                let probe_timeout = Duration::from_secs(3).clamp(limit / 2, limit - limit / 4);
                tokio::time::Instant::now().checked_add(probe_timeout)
            }),
            probe_id: None,
            initialize_id: None,
        });
        rx
    }

    #[cfg(unix)]
    pub(super) fn take_child(&mut self) -> Option<Box<Child>> {
        self.child.take()
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
        if self.discovery.as_ref().is_some_and(|probe| {
            probe
                .finished
                .as_ref()
                .is_none_or(oneshot::Sender::is_closed)
        }) {
            self.discovery = None;
        }
        if let Some(probe) = &mut self.discovery
            && let JsonRpcMessage::Request(request) = &item
            && matches!(request.request, ClientRequest::DiscoverRequest(_))
        {
            probe.probe_id = Some(request.id.clone());
        }
        if let Some(probe) = &mut self.discovery
            && let JsonRpcMessage::Request(request) = &item
            && matches!(request.request, ClientRequest::InitializeRequest(_))
            && matches!(self.transport, StdioTransport::Modern(_))
        {
            let StdioTransport::Modern(transport) =
                std::mem::replace(&mut self.transport, StdioTransport::Closed)
            else {
                unreachable!();
            };
            match transport.into_legacy_pipes() {
                Ok((stdout, stdin)) => {
                    probe.initialize_id = Some(request.id.clone());
                    self.transport = StdioTransport::Legacy(AsyncRwTransport::new(stdout, stdin));
                }
                Err(error) => return futures::future::err(error).boxed(),
            }
        }
        match &mut self.transport {
            StdioTransport::Legacy(transport) => transport.send(item).boxed(),
            StdioTransport::Modern(transport) => transport.send(item).boxed(),
            StdioTransport::Closed => futures::future::err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "MCP stdin closed",
            ))
            .boxed(),
        }
    }

    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleClient>> {
        loop {
            let receive = async {
                match &mut self.transport {
                    StdioTransport::Legacy(transport) => transport.receive().await,
                    StdioTransport::Modern(transport) => transport.receive().await,
                    StdioTransport::Closed => None,
                }
            };
            let mut message = match self.discovery.as_ref().and_then(|probe| probe.retry_at) {
                Some(retry_at) => tokio::time::timeout_at(retry_at, receive)
                    .await
                    .unwrap_or_default(),
                None => receive.await,
            };
            if self
                .discovery
                .as_mut()
                .is_some_and(|probe| probe.handle_reply(&mut message))
            {
                continue;
            }
            return message;
        }
    }

    async fn close(&mut self) -> io::Result<()> {
        match &mut self.transport {
            StdioTransport::Legacy(transport) => transport.close().await?,
            StdioTransport::Modern(transport) => transport.close().await?,
            StdioTransport::Closed => {}
        }
        #[cfg(windows)]
        let mut child = self.child.lock().await;
        #[cfg(not(windows))]
        let Some(child) = &mut self.child else {
            return Ok(());
        };
        match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
            Ok(status) => status.map(|_| ()),
            Err(_) => child.kill().await,
        }
    }
}
