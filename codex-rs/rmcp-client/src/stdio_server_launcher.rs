//! Launch MCP stdio servers and return the transport rmcp should use.
//!
//! This module owns the "where does the server process run?" decision:
//!
//! - [`LocalStdioServerLauncher`] starts the configured command as a child of
//!   the orchestrator process.
//! - [`ExecutorStdioServerLauncher`] starts the configured command through the
//!   executor process API.
//!
//! Both paths return [`StdioServerTransport`], so `RmcpClient` can hand the
//! resulting byte stream to rmcp without knowing where the process lives. The
//! executor-specific byte adaptation lives in `executor_process_transport`.
//! Unix local servers use the stdio-only descriptor policy.

use std::collections::HashMap;
use std::ffi::OsString;
use std::future::Future;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(windows)]
use std::sync::Mutex as StdMutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
#[cfg(unix)]
use std::thread::sleep;
#[cfg(unix)]
use std::thread::spawn;
use std::time::Duration;

use anyhow::Result;
use anyhow::anyhow;
use codex_config::types::McpServerEnvVar;
use codex_exec_server::ExecBackend;
use codex_exec_server::ExecEnvPolicy;
use codex_exec_server::ExecParams;
use codex_exec_server::ExecProcess;
use codex_protocol::config_types::ShellEnvironmentPolicyInherit;
use codex_utils_path_uri::LegacyAppPathString;
use codex_utils_path_uri::PathUri;
use codex_utils_pty::Command;
#[cfg(unix)]
use codex_utils_pty::DescriptorPolicy;
use codex_utils_pty::ProcessMode;
#[cfg(unix)]
use codex_utils_pty::process_group::kill_process_group;
#[cfg(unix)]
use codex_utils_pty::process_group::terminate_process_group;
use futures::FutureExt;
use futures::future::BoxFuture;
#[cfg(windows)]
use futures::future::Shared;
use rmcp::service::RoleClient;
use rmcp::service::RxJsonRpcMessage;
use rmcp::service::TxJsonRpcMessage;
use rmcp::transport::Transport;
#[cfg(windows)]
use sha2::Digest;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::sync::watch;
use tokio::time::Instant;
use tracing::info;
use tracing::warn;

use crate::executor_process_transport::ExecutorProcessTransport;
use crate::local_stdio_transport::LocalStdioStderr;
use crate::local_stdio_transport::LocalStdioTransport;
use crate::program_resolver;
use crate::protocol_mode::McpProtocolMode;
use crate::utils::create_env_for_mcp_server;
use crate::utils::create_env_overlay_for_remote_mcp_server;
use crate::utils::remote_mcp_env_var_names;

// General purpose public code.

/// Launches an MCP stdio server and returns the transport for rmcp.
///
/// This trait is the boundary between MCP lifecycle code and process placement.
/// `RmcpClient` owns MCP operations such as `initialize` and `tools/list`; the
/// launcher owns starting the configured command and producing an rmcp
/// [`Transport`] over the server's stdin/stdout bytes.
pub trait StdioServerLauncher: private::Sealed + Send + Sync {
    /// Start the configured stdio server and return its rmcp-facing transport.
    fn launch(
        &self,
        command: StdioServerCommand,
    ) -> BoxFuture<'static, io::Result<StdioServerTransport>>;
}

/// Command-line process shape shared by stdio server launchers.
#[derive(Clone)]
pub struct StdioServerCommand {
    program: OsString,
    args: Vec<OsString>,
    env: Option<HashMap<OsString, OsString>>,
    env_vars: Vec<McpServerEnvVar>,
    cwd: Option<String>,
    protocol_mode: McpProtocolMode,
}

/// Client-side rmcp transport for a launched MCP stdio server.
///
/// The concrete process placement stays private to this module. `RmcpClient`
/// only sees the standard rmcp transport abstraction and can pass this value
/// directly to `rmcp::service::serve_client`.
pub struct StdioServerTransport {
    inner: StdioServerTransportInner,
    process: StdioServerProcessHandle,
}

enum StdioServerTransportInner {
    Local(LocalStdioTransport),
    Executor(ExecutorProcessTransport),
}

impl Transport<RoleClient> for StdioServerTransport {
    type Error = io::Error;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleClient>,
    ) -> impl Future<Output = std::result::Result<(), Self::Error>> + Send + 'static {
        // Both variants already implement rmcp's transport contract. This
        // wrapper keeps process placement private while leaving rmcp's send
        // semantics unchanged.
        match &mut self.inner {
            StdioServerTransportInner::Local(transport) => transport.send(item).boxed(),
            StdioServerTransportInner::Executor(transport) => transport.send(item).boxed(),
        }
    }

    fn receive(&mut self) -> impl Future<Output = Option<RxJsonRpcMessage<RoleClient>>> + Send {
        // rmcp reads from the same transport shape for both placements. The
        // executor variant turns pushed process-output events back into the
        // line-delimited JSON stream expected by rmcp.
        match &mut self.inner {
            StdioServerTransportInner::Local(transport) => transport.receive().boxed(),
            StdioServerTransportInner::Executor(transport) => transport.receive().boxed(),
        }
    }

    async fn close(&mut self) -> std::result::Result<(), Self::Error> {
        self.process.terminate().await?;
        match &mut self.inner {
            StdioServerTransportInner::Local(transport) => transport.close().await,
            StdioServerTransportInner::Executor(transport) => transport.close().await,
        }
    }
}

impl StdioServerTransport {
    pub(crate) fn process_handle(&self) -> StdioServerProcessHandle {
        self.process.clone()
    }
}

impl StdioServerCommand {
    /// Build the stdio process parameters before choosing where the process
    /// runs.
    pub(super) fn new(
        program: OsString,
        args: Vec<OsString>,
        env: Option<HashMap<OsString, OsString>>,
        env_vars: Vec<McpServerEnvVar>,
        cwd: Option<String>,
        protocol_mode: McpProtocolMode,
    ) -> Self {
        Self {
            program,
            args,
            env,
            env_vars,
            cwd,
            protocol_mode,
        }
    }
}

// Local public implementation.

/// Starts MCP stdio servers as local child processes.
///
/// This is the existing behavior for local MCP servers: the orchestrator
/// process spawns the configured command and rmcp talks to the child's local
/// stdin/stdout pipes directly.
#[derive(Clone)]
pub struct LocalStdioServerLauncher {
    fallback_cwd: PathBuf,
    require_job_containment: bool,
}

impl LocalStdioServerLauncher {
    /// Creates a local stdio launcher.
    ///
    /// `fallback_cwd` is used when the MCP server config omits `cwd`, so
    /// relative commands resolve from the caller's runtime working directory.
    pub fn new(fallback_cwd: PathBuf) -> Self {
        Self {
            fallback_cwd,
            require_job_containment: false,
        }
    }
}

impl LocalStdioServerLauncher {
    /// Private request work requires a fence covering the entire local process tree.
    pub fn with_required_job_containment(mut self, required: bool) -> Self {
        self.require_job_containment = required;
        self
    }
}

impl StdioServerLauncher for LocalStdioServerLauncher {
    fn launch(
        &self,
        command: StdioServerCommand,
    ) -> BoxFuture<'static, io::Result<StdioServerTransport>> {
        let fallback_cwd = self.fallback_cwd.clone();
        let require_job_containment = self.require_job_containment;
        async move {
            // Keep synchronous program resolution and process creation from blocking the
            // caller's startup deadline.
            tokio::task::spawn_blocking(move || {
                Self::launch_server(command, fallback_cwd, require_job_containment)
            })
            .await
            .map_err(io::Error::other)?
        }
        .boxed()
    }
}

// Local private implementation.

#[cfg(unix)]
const PROCESS_GROUP_TERM_GRACE_PERIOD: Duration = Duration::from_secs(2);

// Keep queued stderr diagnostics before closing the reader, even when an
// escaped descendant prevents the pipe from reaching EOF.
const STDERR_READER_DRAIN_GRACE_PERIOD: Duration = Duration::from_millis(250);

#[cfg(windows)]
async fn confirm_local_termination(
    probe: impl Fn() -> io::Result<bool>,
    request: impl Fn(),
) -> io::Result<()> {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if matches!(probe(), Ok(true)) {
                return;
            }
            request();
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .map_err(|_| unconfirmed_termination())
}

#[cfg(windows)]
pub(super) fn unconfirmed_termination() -> io::Error {
    unconfirmed_termination_with_detail("no exit evidence before the cleanup deadline".into())
}

#[cfg(windows)]
pub(super) fn unconfirmed_termination_with_detail(detail: String) -> io::Error {
    io::Error::other(LocalTerminationUnconfirmed(detail))
}

#[derive(Debug, thiserror::Error)]
#[error("MCP process termination unconfirmed; execution ownership retained: {0}")]
struct LocalTerminationUnconfirmed(String);

/// Preserve a typed cleanup failure across startup's existing error envelope.
pub fn is_termination_unconfirmed(error: &anyhow::Error) -> bool {
    fn matches(cause: &(dyn std::error::Error + 'static)) -> bool {
        if cause.is::<LocalTerminationUnconfirmed>() {
            return true;
        }
        // io::Error's source skips its directly contained custom error on some
        // wrappers. Inspect that owned payload as well as the normal chain.
        if let Some(inner) = cause
            .downcast_ref::<io::Error>()
            .and_then(io::Error::get_ref)
        {
            if matches(inner) {
                return true;
            }
        }
        cause.source().is_some_and(matches)
    }
    error.chain().any(matches)
}

#[cfg(windows)]
type LocalLaunchKey = [u8; 32];

// Only failed cleanup generations are retained. The key is never logged, and
// includes the effective command rather than the configurable server alias.
#[cfg(windows)]
fn retained_local_owners()
-> &'static StdMutex<HashMap<LocalLaunchKey, Vec<Arc<StdioServerProcessHandleInner>>>> {
    static OWNERS: std::sync::OnceLock<
        StdMutex<HashMap<LocalLaunchKey, Vec<Arc<StdioServerProcessHandleInner>>>>,
    > = std::sync::OnceLock::new();
    OWNERS.get_or_init(StdMutex::default)
}

#[cfg(windows)]
fn local_launch_key(
    program: &Path,
    args: &[OsString],
    cwd: &Path,
    env: &HashMap<OsString, OsString>,
) -> LocalLaunchKey {
    use std::os::windows::ffi::OsStrExt;
    fn append(hash: &mut sha2::Sha256, value: &std::ffi::OsStr) {
        let words: Vec<u16> = value.encode_wide().collect();
        hash.update((words.len() as u64).to_le_bytes());
        for word in words {
            hash.update(word.to_le_bytes());
        }
    }
    let mut hash = sha2::Sha256::new();
    let program = std::fs::canonicalize(program).unwrap_or_else(|_| program.to_path_buf());
    let cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    append(&mut hash, program.as_os_str());
    append(&mut hash, cwd.as_os_str());
    hash.update((args.len() as u64).to_le_bytes());
    for arg in args {
        append(&mut hash, arg);
    }
    let mut env: Vec<_> = env
        .iter()
        .map(|(k, v)| (k.to_ascii_uppercase(), v))
        .collect();
    env.sort_by(|a, b| a.0.cmp(&b.0));
    for (key, value) in env {
        append(&mut hash, &key);
        append(&mut hash, value);
    }
    hash.finalize().into()
}

#[cfg(windows)]
async fn release_confirmed_owner(inner: &Arc<StdioServerProcessHandleInner>) -> bool {
    let confirmed = match &inner.kind {
        StdioServerProcessKind::Local(Some(terminator)) => terminator.confirmed().await,
        _ => false,
    };
    if confirmed {
        inner.terminated.store(true, Ordering::Release);
        if let Some(key) = inner.local_launch_key {
            let mut owners = retained_local_owners()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(generations) = owners.get_mut(&key) {
                generations.retain(|owner| !Arc::ptr_eq(owner, inner));
                if generations.is_empty() {
                    owners.remove(&key);
                }
            }
        }
    }
    confirmed
}

#[cfg(windows)]
async fn ensure_launch_not_fenced(key: LocalLaunchKey) -> io::Result<()> {
    let owners = retained_local_owners()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&key)
        .cloned()
        .unwrap_or_default();
    for owner in owners {
        if !release_confirmed_owner(&owner).await {
            return Err(unconfirmed_termination());
        }
    }
    Ok(())
}

#[cfg(windows)]
async fn retry_after_failed_launch<T>(
    cleanup: impl Future<Output = io::Result<()>>,
    retry: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    cleanup.await?;
    retry()
}

#[cfg(unix)]
struct LocalProcessTerminator {
    process_group_id: u32,
}

#[cfg(windows)]
enum LocalProcessTerminator {
    Job(
        codex_utils_pty::JobObject,
        crate::local_stdio_transport::LocalChild,
    ),
    Child(crate::local_stdio_transport::LocalChild),
}

#[cfg(not(any(unix, windows)))]
struct LocalProcessTerminator;

#[derive(Clone)]
pub(crate) struct StdioServerProcessHandle {
    inner: Arc<StdioServerProcessHandleInner>,
}

struct StdioServerProcessHandleInner {
    program_name: String,
    kind: StdioServerProcessKind,
    terminated: AtomicBool,
    #[cfg(windows)]
    local_cleanup: StdMutex<Option<Shared<BoxFuture<'static, Result<(), Arc<str>>>>>>,
    #[cfg(windows)]
    local_launch_key: Option<LocalLaunchKey>,
    // An escaped descendant can keep stderr open after the MCP server exits.
    stderr_reader: Option<watch::Sender<()>>,
}

enum StdioServerProcessKind {
    Local(Option<LocalProcessTerminator>),
    Executor(Arc<dyn ExecProcess>),
}

mod private {
    pub trait Sealed {}
}

impl private::Sealed for LocalStdioServerLauncher {}

impl LocalStdioServerLauncher {
    fn launch_server(
        command: StdioServerCommand,
        fallback_cwd: PathBuf,
        require_job_containment: bool,
    ) -> io::Result<StdioServerTransport> {
        #[cfg(windows)]
        {
            Self::launch_server_with_job(
                command,
                fallback_cwd,
                require_job_containment,
                codex_utils_pty::JobObject::create_without_breakaway(),
            )
        }
        #[cfg(not(windows))]
        {
            Self::launch_server_with_job(command, fallback_cwd, require_job_containment)
        }
    }

    fn launch_server_with_job(
        command: StdioServerCommand,
        fallback_cwd: PathBuf,
        _require_job_containment: bool,
        #[cfg(windows)] job_result: io::Result<codex_utils_pty::JobObject>,
    ) -> io::Result<StdioServerTransport> {
        #[cfg(windows)]
        let require_job_containment = _require_job_containment;
        let StdioServerCommand {
            program,
            args,
            env,
            env_vars,
            cwd,
            protocol_mode,
        } = command;
        let program_name = program.to_string_lossy().into_owned();
        let envs = create_env_for_mcp_server(env, &env_vars).map_err(io::Error::other)?;
        let cwd = cwd.map(PathBuf::from).unwrap_or(fallback_cwd);
        let resolved_program =
            program_resolver::resolve(program, &envs, &cwd).map_err(io::Error::other)?;

        #[cfg(windows)]
        let launch_key = local_launch_key(Path::new(&resolved_program), &args, &cwd, &envs);
        #[cfg(windows)]
        tokio::runtime::Handle::current().block_on(ensure_launch_not_fenced(launch_key))?;

        let build_command = || {
            let mut command = Command::new(&resolved_program);
            command.current_dir(&cwd).envs(&envs).args(&args);
            command.process_mode(ProcessMode::NewGroup);
            // MCP uses only stdio; select Explicit to exclude unrelated
            // orchestrator descriptors from the server and commands it launches.
            // Descriptor allowlisting is Unix-only. Windows can still inherit unrelated
            // handles and needs a handle allowlist in the shared spawn backend.
            #[cfg(unix)]
            command.descriptor_policy(DescriptorPolicy::Explicit);
            command
        };
        #[cfg(windows)]
        let mut command = build_command();
        #[cfg(not(windows))]
        let command = build_command();
        #[cfg(windows)]
        let job = match job_result {
            Ok(job) => {
                command.prepare_suspended_spawn(&job);
                Some(job)
            }
            Err(error) if require_job_containment => return Err(error),
            Err(error) => {
                warn!("Windows MCP process job containment unavailable: {error}");
                None
            }
        };

        let spawn_transport = |command: Command| -> io::Result<(
            StdioServerTransportInner,
            Option<LocalStdioStderr>,
            Option<u32>,
        )> {
            let (transport, stderr) =
                LocalStdioTransport::spawn(command, program_name.clone(), protocol_mode)?;
            #[cfg(all(windows, test))]
            local_cleanup_failure_tests::record_spawn(launch_key);
            let process_id = transport.id();
            Ok((
                StdioServerTransportInner::Local(transport),
                stderr,
                process_id,
            ))
        };
        let (transport, stderr, process_id) = spawn_transport(command)?;
        #[cfg(windows)]
        let (transport, stderr, _process_id, job) = match job {
            Some(job) => match process_id
                .ok_or_else(|| io::Error::other("missing suspended MCP server process id"))
                .and_then(|process_id| {
                    if require_job_containment {
                        job.assign_and_resume_process_strict(process_id)
                            .map(|()| true)
                    } else {
                        job.assign_and_resume_process(process_id)
                    }
                }) {
                Ok(true) => (transport, stderr, process_id, Some(job)),
                Ok(false) => (transport, stderr, process_id, None),
                Err(error) => {
                    warn!(
                        required = require_job_containment,
                        "Windows MCP process job containment failed: {error}"
                    );
                    let child = match &transport {
                        StdioServerTransportInner::Local(local) => local.child_termination_handle(),
                        _ => unreachable!("local launch returned executor transport"),
                    };
                    let owner = StdioServerProcessHandle::local(
                        program_name.clone(),
                        Some(LocalProcessTerminator::Job(job, child)),
                        None,
                    )
                    .with_launch_key(launch_key);
                    let cleanup = async {
                        let result = owner.terminate().await;
                        drop(transport);
                        drop(stderr);
                        result
                    };
                    if require_job_containment {
                        tokio::runtime::Handle::current().block_on(cleanup)?;
                        return Err(error);
                    }
                    let (transport, stderr, process_id) = tokio::runtime::Handle::current()
                        .block_on(retry_after_failed_launch(cleanup, || {
                            spawn_transport(build_command())
                        }))?;
                    (transport, stderr, process_id, None)
                }
            },
            None => (transport, stderr, process_id, None),
        };
        #[cfg(windows)]
        let terminator = match job {
            Some(job) => match &transport {
                StdioServerTransportInner::Local(local) => Some(LocalProcessTerminator::Job(
                    job,
                    local.child_termination_handle(),
                )),
                _ => unreachable!("local launch returned executor transport"),
            },
            None => match &transport {
                // The transport already owns the exact Child, including after process exit.
                // Reopening its PID can fail or race reuse; use its kill/wait evidence instead.
                StdioServerTransportInner::Local(transport) => Some(LocalProcessTerminator::Child(
                    transport.child_termination_handle(),
                )),
                StdioServerTransportInner::Executor(_) => None,
            },
        };
        #[cfg(not(windows))]
        let terminator = process_id.map(LocalProcessTerminator::new);
        let stderr_reader = stderr.map(|stderr| {
            let program_name = program_name.clone();
            let (stop_tx, mut stop_rx) = watch::channel(());
            std::mem::drop(tokio::spawn(async move {
                let mut reader = BufReader::new(stderr).lines();
                // Give queued diagnostics time to reach the logs without waiting
                // indefinitely for a descendant that still has stderr open.
                let drain_deadline = tokio::time::sleep(STDERR_READER_DRAIN_GRACE_PERIOD);
                tokio::pin!(drain_deadline);
                let mut draining = false;
                loop {
                    tokio::select! {
                        biased;
                        _ = &mut drain_deadline, if draining => break,
                        _ = stop_rx.changed(), if !draining => {
                            draining = true;
                            drain_deadline.as_mut().reset(
                                Instant::now() + STDERR_READER_DRAIN_GRACE_PERIOD
                            );
                        }
                        line = reader.next_line() => {
                            match line {
                                Ok(Some(line)) => {
                                    info!("MCP server stderr ({program_name}): {line}");
                                }
                                Ok(None) => break,
                                Err(error) => {
                                    warn!("Failed to read MCP server stderr ({program_name}): {error}");
                                    break;
                                }
                            }
                        },
                    }
                }
            }));
            stop_tx
        });
        let process = StdioServerProcessHandle::local(program_name, terminator, stderr_reader);
        #[cfg(windows)]
        let process = process.with_launch_key(launch_key);

        Ok(StdioServerTransport {
            inner: transport,
            process,
        })
    }
}

impl LocalProcessTerminator {
    #[cfg(windows)]
    async fn confirmed(&self) -> bool {
        let (child, job_empty) = match self {
            Self::Job(job, child) => (child, matches!(job.is_empty(), Ok(true))),
            Self::Child(child) => (child, true),
        };
        if !job_empty {
            return false;
        }
        let Ok(mut child) = child.try_lock() else {
            return false;
        };
        matches!(
            tokio::time::timeout(Duration::from_millis(50), child.wait()).await,
            Ok(Ok(_))
        )
    }

    #[cfg(windows)]
    async fn terminate_confirmed(&self) -> io::Result<()> {
        let child = match self {
            Self::Job(_, child) | Self::Child(child) => child,
        };
        if let Self::Job(job, _) = self {
            let _ = job.terminate();
        }
        crate::local_stdio_transport::terminate_local_child(child).await?;
        if let Self::Job(job, _) = self {
            confirm_local_termination(
                || job.is_empty(),
                || {
                    let _ = job.terminate();
                },
            )
            .await?;
        }
        Ok(())
    }
    #[cfg(not(windows))]
    fn new(process_group_id: u32) -> Self {
        #[cfg(unix)]
        {
            Self { process_group_id }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = process_group_id;
            Self
        }
    }

    #[cfg(unix)]
    fn terminate(&self) {
        let process_group_id = self.process_group_id;
        let should_escalate = match terminate_process_group(process_group_id) {
            Ok(exists) => exists,
            Err(error) => {
                warn!("Failed to terminate MCP process group {process_group_id}: {error}");
                false
            }
        };
        if should_escalate {
            spawn(move || {
                sleep(PROCESS_GROUP_TERM_GRACE_PERIOD);
                if let Err(error) = kill_process_group(process_group_id) {
                    warn!("Failed to kill MCP process group {process_group_id}: {error}");
                }
            });
        }
    }

    #[cfg(windows)]
    fn terminate(&self) {
        if let Self::Job(job, _) = self {
            let _ = job.terminate();
        }
    }

    #[cfg(not(any(unix, windows)))]
    fn terminate(&self) {}
}

impl StdioServerProcessHandle {
    fn local(
        program_name: String,
        terminator: Option<LocalProcessTerminator>,
        stderr_reader: Option<watch::Sender<()>>,
    ) -> Self {
        Self {
            inner: Arc::new(StdioServerProcessHandleInner {
                program_name,
                kind: StdioServerProcessKind::Local(terminator),
                terminated: AtomicBool::new(false),
                #[cfg(windows)]
                local_cleanup: StdMutex::new(None),
                #[cfg(windows)]
                local_launch_key: None,
                stderr_reader,
            }),
        }
    }

    pub(crate) fn executor(program_name: String, process: Arc<dyn ExecProcess>) -> Self {
        Self {
            inner: Arc::new(StdioServerProcessHandleInner {
                program_name,
                kind: StdioServerProcessKind::Executor(process),
                terminated: AtomicBool::new(false),
                #[cfg(windows)]
                local_cleanup: StdMutex::new(None),
                #[cfg(windows)]
                local_launch_key: None,
                stderr_reader: None,
            }),
        }
    }

    #[cfg(windows)]
    fn with_launch_key(mut self, key: LocalLaunchKey) -> Self {
        Arc::get_mut(&mut self.inner)
            .expect("new process owner is unique")
            .local_launch_key = Some(key);
        self
    }

    #[cfg(windows)]
    fn start_local_cleanup(&self) -> Shared<BoxFuture<'static, Result<(), Arc<str>>>> {
        let mut cleanup = self
            .inner
            .local_cleanup
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cleanup.get_or_insert_with(|| {
            let inner = Arc::clone(&self.inner);
            // Publish ownership before spawning: caller cancellation or task panic
            // must not lose the execution fence or the exact process handles.
            if let Some(key) = inner.local_launch_key {
                retained_local_owners().lock().unwrap_or_else(std::sync::PoisonError::into_inner).entry(key).or_default().push(Arc::clone(&inner));
            }
            let task = tokio::spawn(async move {
                let result = match &inner.kind {
                    StdioServerProcessKind::Local(Some(terminator)) => terminator.terminate_confirmed().await,
                    _ => Err(unconfirmed_termination()),
                };
                if let Some(stderr_reader) = &inner.stderr_reader { stderr_reader.send_replace(()); }
                match result {
                    Ok(()) => {
                        release_confirmed_owner(&inner).await;
                        Ok(())
                    }
                    Err(error) => {
                        warn!("MCP process cleanup unconfirmed; retaining owner and execution fence");
                        Err(Arc::<str>::from(error.to_string()))
                    }
                }
            });
            async move { task.await.unwrap_or_else(|_| Err(Arc::from("MCP cleanup task failed"))) }.boxed().shared()
        }).clone()
    }

    pub(crate) async fn terminate(&self) -> io::Result<()> {
        #[cfg(windows)]
        if matches!(self.inner.kind, StdioServerProcessKind::Local(_)) {
            if self.inner.terminated.load(Ordering::Acquire)
                || release_confirmed_owner(&self.inner).await
            {
                return Ok(());
            }
            return self
                .start_local_cleanup()
                .await
                .map_err(|error| io::Error::other(LocalTerminationUnconfirmed(error.to_string())));
        }
        if self.inner.terminated.swap(true, Ordering::AcqRel) {
            return Ok(());
        }

        let result = match &self.inner.kind {
            StdioServerProcessKind::Local(Some(terminator)) => {
                terminator.terminate();
                Ok(())
            }
            StdioServerProcessKind::Local(None) => Ok(()),
            StdioServerProcessKind::Executor(process) => match process.terminate().await {
                Ok(()) => Ok(()),
                Err(error) => {
                    self.inner.terminated.store(false, Ordering::Release);
                    Err(io::Error::other(error))
                }
            },
        };
        if let Some(stderr_reader) = &self.inner.stderr_reader {
            stderr_reader.send_replace(());
        }
        result
    }
}

#[cfg(windows)]
impl Drop for StdioServerProcessHandle {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) == 1
            && !self.inner.terminated.load(Ordering::Acquire)
            && matches!(self.inner.kind, StdioServerProcessKind::Local(_))
        {
            if tokio::runtime::Handle::try_current().is_ok() {
                std::mem::drop(self.start_local_cleanup());
            }
        }
    }
}

impl Drop for StdioServerProcessHandleInner {
    fn drop(&mut self) {
        if self.terminated.swap(true, Ordering::AcqRel) {
            return;
        }

        match &self.kind {
            StdioServerProcessKind::Local(Some(terminator)) => {
                terminator.terminate();
            }
            StdioServerProcessKind::Local(None) => {}
            StdioServerProcessKind::Executor(process) => {
                let process = Arc::clone(process);
                let program_name = self.program_name.clone();
                let Ok(handle) = tokio::runtime::Handle::try_current() else {
                    warn!(
                        "Could not schedule remote MCP server process termination on drop ({}): no Tokio runtime is available",
                        self.program_name
                    );
                    return;
                };

                std::mem::drop(handle.spawn(async move {
                    if let Err(error) = process.terminate().await {
                        warn!(
                            "Failed to terminate remote MCP server process on drop ({program_name}): {error}"
                        );
                    }
                }));
            }
        }
        if let Some(stderr_reader) = &self.stderr_reader {
            stderr_reader.send_replace(());
        }
    }
}

// Remote public implementation.

/// Starts MCP stdio servers through the executor process API.
///
/// MCP framing still runs in the orchestrator. The executor only owns the
/// child process and transports raw stdin/stdout/stderr bytes, so it does not
/// need to know about MCP methods such as `initialize` or `tools/list`.
///
/// Windows executor-backed servers retain the executor's normal descendant
/// lifetime. MCP-specific containment requires negotiated process ownership:
/// caller-controlled process IDs cannot safely select a destructive policy,
/// and a wrapper may exit while its descendants continue serving requests.
#[derive(Clone)]
pub struct ExecutorStdioServerLauncher {
    exec_backend: Arc<dyn ExecBackend>,
}

impl ExecutorStdioServerLauncher {
    /// Creates a stdio server launcher backed by the executor process API.
    pub fn new(exec_backend: Arc<dyn ExecBackend>) -> Self {
        Self { exec_backend }
    }
}

impl StdioServerLauncher for ExecutorStdioServerLauncher {
    fn launch(
        &self,
        command: StdioServerCommand,
    ) -> BoxFuture<'static, io::Result<StdioServerTransport>> {
        let exec_backend = Arc::clone(&self.exec_backend);
        async move { Self::launch_server(command, exec_backend).await }.boxed()
    }
}

// Remote private implementation.

impl private::Sealed for ExecutorStdioServerLauncher {}

impl ExecutorStdioServerLauncher {
    async fn launch_server(
        command: StdioServerCommand,
        exec_backend: Arc<dyn ExecBackend>,
    ) -> io::Result<StdioServerTransport> {
        let StdioServerCommand {
            program,
            args,
            env,
            env_vars,
            cwd,
            protocol_mode: _,
        } = command;
        let Some(cwd) = cwd else {
            return Err(io::Error::other(
                "executor stdio server requires an explicit cwd",
            ));
        };
        let cwd: PathUri = LegacyAppPathString::from_path(Path::new(&cwd))
            .try_into()
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;
        let program_name = program.to_string_lossy().into_owned();
        let envs = create_env_overlay_for_remote_mcp_server(env, &env_vars);
        let remote_env_vars = remote_mcp_env_var_names(&env_vars);
        // The executor protocol carries argv/env as UTF-8 strings. Local stdio can
        // accept arbitrary OsString values because it calls the OS directly; remote
        // stdio must reject non-Unicode command, argument, or environment data
        // before sending an executor request.
        let argv = Self::process_api_argv(&program, &args).map_err(io::Error::other)?;
        let env = Self::process_api_env(envs).map_err(io::Error::other)?;
        let process_id = ExecutorProcessTransport::next_process_id();
        // Start the MCP server process on the executor with raw pipes. `tty=false`
        // keeps stdout as a clean protocol stream, while `pipe_stdin=true` lets
        // rmcp write JSON-RPC requests after the process starts.
        let started = exec_backend
            .start(ExecParams {
                metadata: Default::default(),
                process_id,
                argv,
                cwd,
                shell_snapshot: None,
                env_policy: Some(Self::remote_env_policy(&remote_env_vars, &env)),
                env,
                tty: false,
                pipe_stdin: true,
                arg0: None,
                sandbox: None,
                enforce_managed_network: false,
                managed_network: None,
                network_proxy: None,
            })
            .await
            .map_err(io::Error::other)?;

        let process =
            StdioServerProcessHandle::executor(program_name.clone(), Arc::clone(&started.process));
        Ok(StdioServerTransport {
            inner: StdioServerTransportInner::Executor(ExecutorProcessTransport::new(
                started.process,
                program_name,
            )),
            process,
        })
    }

    fn process_api_argv(program: &OsString, args: &[OsString]) -> Result<Vec<String>> {
        let mut argv = Vec::with_capacity(args.len() + 1);
        argv.push(Self::os_string_to_process_api_string(
            program.clone(),
            "command",
        )?);
        for arg in args {
            argv.push(Self::os_string_to_process_api_string(
                arg.clone(),
                "argument",
            )?);
        }
        Ok(argv)
    }

    fn process_api_env(env: HashMap<OsString, OsString>) -> Result<HashMap<String, String>> {
        env.into_iter()
            .map(|(key, value)| {
                Ok((
                    Self::os_string_to_process_api_string(key, "environment variable name")?,
                    Self::os_string_to_process_api_string(value, "environment variable value")?,
                ))
            })
            .collect()
    }

    fn os_string_to_process_api_string(value: OsString, label: &str) -> Result<String> {
        value
            .into_string()
            .map_err(|_| anyhow!("{label} must be valid Unicode for remote MCP stdio"))
    }

    fn remote_env_policy(
        remote_env_vars: &[String],
        env: &HashMap<String, String>,
    ) -> ExecEnvPolicy {
        let include_only = if remote_env_vars.is_empty() {
            Vec::new()
        } else {
            // `source = "remote"` means the value is read from the executor's
            // environment, not copied from Codex. Start from `All` only so the
            // named remote variable is available to the filter below; the
            // effective child env is still limited by `include_only`.
            // The orchestrator may be Unix while the executor is Windows.
            // Preserve Windows runtime initialization and temporary-directory
            // inputs even when explicit remote vars activate this allowlist.
            crate::utils::DEFAULT_ENV_VARS
                .iter()
                .chain(["SYSTEMROOT", "TEMP", "TMP", "SYSTEMDRIVE", "LOCALAPPDATA"].iter())
                .map(|name| (*name).to_string())
                .chain(remote_env_vars.iter().cloned())
                .collect()
        };
        ExecEnvPolicy {
            inherit: if remote_env_vars.is_empty() {
                ShellEnvironmentPolicyInherit::Core
            } else {
                ShellEnvironmentPolicyInherit::All
            },
            ignore_default_excludes: true,
            exclude: Vec::new(),
            // Apply overrides through the policy's Windows-aware merge, including
            // on older executors. Keep the final overlay for names filtered out below.
            r#set: env.clone(),
            include_only,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[tokio::test]
    async fn mcp_read_lifetime_jobless_child_proves_exit_without_pid_reopen() -> anyhow::Result<()>
    {
        const TEST: &str = "stdio_server_launcher::tests::mcp_read_lifetime_jobless_child_proves_exit_without_pid_reopen";
        const PHASE: &str = "CODEX_TEST_JOBLESS_MCP_CHILD";
        match std::env::var(PHASE).as_deref() {
            Ok("hold") => {
                std::thread::sleep(Duration::from_secs(30));
                return Ok(());
            }
            Ok("exit") => return Ok(()),
            _ => {}
        }
        for phase in ["hold", "exit"] {
            let cwd = tempfile::tempdir()?;
            let command = StdioServerCommand::new(
                std::env::current_exe()?.into_os_string(),
                vec!["--exact".into(), TEST.into(), "--nocapture".into()],
                Some(HashMap::from([(PHASE.into(), phase.into())])),
                Vec::new(),
                None,
                McpProtocolMode::Legacy,
            );
            let cwd_path = cwd.path().to_path_buf();
            let transport = tokio::task::spawn_blocking(move || {
                LocalStdioServerLauncher::launch_server_with_job(
                    command,
                    cwd_path,
                    false,
                    Err(io::Error::other("injected optional job creation failure")),
                )
            })
            .await??;
            let process = transport.process_handle();
            let StdioServerProcessKind::Local(Some(LocalProcessTerminator::Child(child))) =
                &process.inner.kind
            else {
                panic!("jobless transport must retain its exact Child without reopening a PID");
            };
            let child = Arc::clone(child);
            if phase == "exit" {
                tokio::time::timeout(Duration::from_secs(10), child.lock().await.wait()).await??;
                assert!(
                    child.lock().await.id().is_none(),
                    "waited Child must expose no live PID"
                );
            }
            tokio::time::timeout(Duration::from_secs(5), process.terminate()).await??;
            assert!(
                child.lock().await.id().is_none(),
                "terminal cleanup must reap the same Child"
            );
            // A second owner joins the completed cleanup; it cannot start another child.
            tokio::time::timeout(Duration::from_secs(1), process.clone().terminate()).await??;
            drop(transport);
        }
        Ok(())
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn mcp_read_lifetime_required_job_never_runs_an_uncontained_command() -> anyhow::Result<()>
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::{
            JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };
        const TEST: &str = "stdio_server_launcher::tests::mcp_read_lifetime_required_job_never_runs_an_uncontained_command";
        const PHASE: &str = "CODEX_TEST_REQUIRED_MCP_JOB_PHASE";
        match std::env::var(PHASE).as_deref() {
            Ok("hold") => {
                std::thread::sleep(Duration::from_secs(30));
                return Ok(());
            }
            Ok("probe") => {
                std::fs::write(
                    std::env::var_os("CODEX_TEST_REQUIRED_MCP_MARKER").expect("marker"),
                    b"ran",
                )?;
                return Ok(());
            }
            _ => {}
        }
        let temp = tempfile::tempdir()?;
        let marker = temp.path().join("uncontained-command-ran");
        let executable = std::env::current_exe()?;
        let make_command = || {
            StdioServerCommand::new(
                executable.clone().into_os_string(),
                vec!["--exact".into(), TEST.into(), "--nocapture".into()],
                Some(HashMap::from([
                    (PHASE.into(), "probe".into()),
                    (
                        "CODEX_TEST_REQUIRED_MCP_MARKER".into(),
                        marker.clone().into_os_string(),
                    ),
                ])),
                Vec::new(),
                None,
                McpProtocolMode::Legacy,
            )
        };
        let result = LocalStdioServerLauncher::launch_server_with_job(
            make_command(),
            temp.path().to_path_buf(),
            true,
            Err(io::Error::other("forced job creation failure")),
        );
        assert!(
            result.is_err(),
            "required creation failure must fail before spawn"
        );
        assert!(!marker.exists());

        let full_job = codex_utils_pty::JobObject::create_without_breakaway()?;
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_ACTIVE_PROCESS | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        limits.BasicLimitInformation.ActiveProcessLimit = 1;
        anyhow::ensure!(
            unsafe {
                SetInformationJobObject(
                    full_job.as_raw_handle().cast(),
                    JobObjectExtendedLimitInformation,
                    (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    std::mem::size_of_val(&limits) as u32,
                )
            } != 0,
            "{}",
            io::Error::last_os_error()
        );
        let mut holder_command = tokio::process::Command::new(&executable);
        holder_command.args(["--exact", TEST]).env(PHASE, "hold");
        let mut holder = full_job.spawn_contained(&mut holder_command)?;
        let command = make_command();
        let cwd = temp.path().to_path_buf();
        let result = tokio::task::spawn_blocking(move || {
            LocalStdioServerLauncher::launch_server_with_job(command, cwd, true, Ok(full_job))
        });
        // The launcher retains the suspended child through confirmed failure cleanup.
        let result = tokio::time::timeout(Duration::from_secs(10), result).await??;
        assert!(
            result.is_err(),
            "assignment failure cannot retry without containment"
        );
        holder.wait().await?;
        assert!(
            !marker.exists(),
            "failed assignment resumed an uncontained command"
        );
        Ok(())
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn mcp_read_lifetime_failed_launch_waits_before_successful_or_failed_retry() {
        use std::sync::atomic::AtomicUsize;
        for succeeds in [false, true] {
            let (release, wait) = tokio::sync::oneshot::channel::<()>();
            let retried = Arc::new(AtomicUsize::new(0));
            let observed = Arc::clone(&retried);
            let task = tokio::spawn(retry_after_failed_launch(
                async move {
                    wait.await.expect("cleanup release");
                    Ok(())
                },
                move || {
                    observed.fetch_add(1, Ordering::SeqCst);
                    if succeeds {
                        Ok(7)
                    } else {
                        Err(io::Error::other("fallback spawn failed"))
                    }
                },
            ));
            tokio::task::yield_now().await;
            assert!(
                !task.is_finished(),
                "unconfirmed first attempt cannot publish a startup outcome"
            );
            assert_eq!(retried.load(Ordering::SeqCst), 0);
            release.send(()).expect("release confirmation");
            assert_eq!(task.await.expect("retry task").is_ok(), succeeds);
            assert_eq!(retried.load(Ordering::SeqCst), 1);
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn mcp_read_lifetime_cleanup_waits_for_confirmation_and_survives_caller_drop() {
        use std::sync::atomic::AtomicUsize;
        let confirmed = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(AtomicUsize::new(0));
        let terminal = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&confirmed);
        let attempts = Arc::clone(&requests);
        let completed = Arc::clone(&terminal);
        let task = tokio::spawn(async move {
            confirm_local_termination(
                || Ok(observed.load(Ordering::Acquire)),
                || {
                    // The first request fails to stop the process. Even subsequent
                    // accepted requests do not signal completion until the probe does.
                    attempts.fetch_add(1, Ordering::AcqRel);
                },
            )
            .await
            .expect("confirmed termination");
            completed.store(true, Ordering::Release);
        });
        let cleanup = async move {
            task.await.expect("cleanup task");
        }
        .boxed()
        .shared();
        let caller_a = tokio::spawn(cleanup.clone());
        let caller_b = tokio::spawn(cleanup);
        tokio::time::timeout(Duration::from_secs(5), async {
            while requests.load(Ordering::Acquire) < 2 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("failed request retried");
        assert!(
            !terminal.load(Ordering::Acquire),
            "unconfirmed termination cannot publish a terminal result"
        );
        assert!(!caller_b.is_finished());
        caller_a.abort();
        confirmed.store(true, Ordering::Release);
        tokio::time::timeout(Duration::from_secs(5), caller_b)
            .await
            .expect("confirmed cleanup finishes")
            .expect("remaining caller");
        assert!(terminal.load(Ordering::Acquire));
    }
    use codex_protocol::config_types::EnvironmentVariablePattern;
    use codex_protocol::config_types::ShellEnvironmentPolicy;
    use codex_protocol::shell_environment;
    use pretty_assertions::assert_eq;

    #[test]
    fn remote_env_policy_uses_core_env_without_remote_source_vars() {
        let policy = ExecutorStdioServerLauncher::remote_env_policy(&[], &HashMap::new());

        assert_eq!(policy.inherit, ShellEnvironmentPolicyInherit::Core);
        assert!(policy.include_only.is_empty());
    }

    #[test]
    fn remote_env_policy_includes_remote_source_vars_without_full_env() {
        let policy = ExecutorStdioServerLauncher::remote_env_policy(
            &["REMOTE_TOKEN".to_string()],
            &HashMap::new(),
        );

        assert_eq!(policy.inherit, ShellEnvironmentPolicyInherit::All);
        assert!(
            policy.include_only.contains(&"REMOTE_TOKEN".to_string()),
            "remote source var should be included in executor env policy"
        );
        assert!(
            policy
                .include_only
                .contains(&crate::utils::DEFAULT_ENV_VARS[0].to_string()),
            "remote default env vars should remain available"
        );
    }

    #[test]
    fn remote_env_policy_effectively_filters_unrequested_vars() {
        let exec_policy = ExecutorStdioServerLauncher::remote_env_policy(
            &["REMOTE_TOKEN".to_string()],
            &HashMap::new(),
        );
        let mut policy = ShellEnvironmentPolicy {
            inherit: exec_policy.inherit,
            ignore_default_excludes: exec_policy.ignore_default_excludes,
            exclude: exec_policy
                .exclude
                .iter()
                .map(|pattern| EnvironmentVariablePattern::new_case_insensitive(pattern))
                .collect(),
            r#set: exec_policy.r#set,
            include_only: exec_policy
                .include_only
                .iter()
                .map(|pattern| EnvironmentVariablePattern::new_case_insensitive(pattern))
                .collect(),
            use_profile: false,
        };

        let env = shell_environment::create_env_from_vars(
            [
                ("PATH".to_string(), "/remote/bin".to_string()),
                ("SystemRoot".to_string(), r"C:\Windows".to_string()),
                ("SystemDrive".to_string(), "C:".to_string()),
                (
                    "LocalAppData".to_string(),
                    r"C:\Users\test\AppData\Local".to_string(),
                ),
                (
                    "TEMP".to_string(),
                    r"C:\Users\test\AppData\Local\Temp".to_string(),
                ),
                (
                    "TMP".to_string(),
                    r"C:\Users\test\AppData\Local\Temp".to_string(),
                ),
                ("REMOTE_TOKEN".to_string(), "remote-secret".to_string()),
                (
                    "UNREQUESTED_SECRET".to_string(),
                    "must-not-pass".to_string(),
                ),
            ],
            &policy,
            /*thread_id*/ None,
        );

        assert_eq!(env.get("PATH").map(String::as_str), Some("/remote/bin"));
        assert_eq!(env.get("SystemDrive").map(String::as_str), Some("C:"));
        assert_eq!(
            env.get("LocalAppData").map(String::as_str),
            Some(r"C:\Users\test\AppData\Local")
        );
        assert_eq!(
            env.get("SystemRoot").map(String::as_str),
            Some(r"C:\Windows")
        );
        for name in ["TEMP", "TMP"] {
            assert_eq!(
                env.get(name).map(String::as_str),
                Some(r"C:\Users\test\AppData\Local\Temp")
            );
        }
        assert_eq!(
            env.get("REMOTE_TOKEN").map(String::as_str),
            Some("remote-secret")
        );
        assert!(!env.contains_key("UNREQUESTED_SECRET"));

        let overrides = HashMap::from([
            ("SYSTEMDRIVE".to_string(), "D:".to_string()),
            ("LOCALAPPDATA".to_string(), r"D:\McpProfile".to_string()),
            ("EXPLICIT_ONLY".to_string(), "configured".to_string()),
        ]);
        policy.r#set = ExecutorStdioServerLauncher::remote_env_policy(
            &["REMOTE_TOKEN".to_string()],
            &overrides,
        )
        .r#set;
        let mut child_env =
            shell_environment::create_env_from_vars(env.clone(), &policy, /*thread_id*/ None);
        assert_eq!(
            child_env.get("LOCALAPPDATA").map(String::as_str),
            Some(r"D:\McpProfile")
        );
        // Older executors apply the final overlay with a case-sensitive merge.
        child_env.extend(overrides.clone());
        let mut expected = env;
        if cfg!(windows) {
            expected.remove("SystemDrive");
            expected.remove("LocalAppData");
        }
        expected.extend(overrides);
        assert_eq!(child_env, expected);
    }
}

#[cfg(all(test, windows))]
#[path = "local_cleanup_failure_tests.rs"]
mod local_cleanup_failure_tests;
