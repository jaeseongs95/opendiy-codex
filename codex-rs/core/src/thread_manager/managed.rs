// Modified for OpenDIY Codex R31 (2026-11-01); see MODIFICATIONS.md.
//! Runs a thread for its owner's chosen lifetime, including cancellation during startup.
//! Cleanup remains tracked when the caller drops its startup future or stops waiting.

use std::future::Future;
use std::sync::Arc;

use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use super::NewThread;
use super::StartThreadOptions;
use super::ThreadManager;
use super::ThreadManagerState;
use crate::CodexThread;
use crate::session::startup::SessionStartup;

impl ThreadManager {
    /// Starts a new isolated thread and owns it until `until` completes or the thread exits.
    /// Dropping the startup future also cancels initialization. `tasks` tracks the
    /// entire lifetime, including partial startup cleanup and deregistration.
    /// Cleanup waits for any in-flight persistence acquisition before releasing its writer.
    /// Once the session runtime starts, cleanup follows normal shutdown history rules,
    /// even if the caller has not received the thread yet.
    /// Resumed threads are excluded because they can refer to another owner's live runtime.
    pub async fn start_thread_until(
        self: &Arc<Self>,
        options: StartThreadOptions,
        until: impl Future<Output = ()> + Send + 'static,
        tasks: &TaskTracker,
    ) -> CodexResult<NewThread> {
        if matches!(
            options.initial_history,
            codex_history::InitialHistory::Resumed(_)
        ) {
            return Err(CodexErr::InvalidRequest(
                "owned thread startup requires new or forked history".to_owned(),
            ));
        }
        if options
            .thread_extension_init
            .get::<codex_extension_api::SessionIsolation>()
            .as_deref()
            != Some(&codex_extension_api::SessionIsolation::Isolated)
        {
            return Err(CodexErr::InvalidRequest(
                "owned thread startup requires explicit session isolation".to_owned(),
            ));
        }
        let task = tasks.token();
        let manager = Arc::clone(self);
        let state = Arc::downgrade(&self.state);
        let (sender, receiver) = oneshot::channel();
        let abandoned = CancellationToken::new();
        let handoff = abandoned.clone().drop_guard();
        tokio::spawn(async move {
            let _task = task;
            let startup = Arc::new(SessionStartup::default());
            tokio::pin!(until);
            // Drop initialization before cleanup; startup still owns any unfinished acquisition.
            let result = {
                let start = manager.start_thread_inner(
                    options,
                    /*forked_from_thread_id*/ None,
                    Some(Arc::clone(&startup)),
                );
                tokio::select! {
                    biased;
                    _ = &mut until => Err(CodexErr::TurnAborted),
                    _ = abandoned.cancelled() => Err(CodexErr::TurnAborted),
                    result = start => result,
                }
            };
            // The lifetime task must not keep the manager and its parent threads alive.
            drop(manager);
            let running = match result {
                Ok(thread) => {
                    let running = Arc::clone(&thread.thread);
                    if sender.send(Ok(thread)).is_ok() {
                        tokio::select! {
                            _ = abandoned.cancelled() => {}
                            _ = &mut until => {}
                            _ = running.wait_until_terminated() => {}
                        }
                    }
                    Some(running)
                }
                Err(error) => {
                    let _ = sender.send(Err(error));
                    None
                }
            };
            startup.cleanup().await;
            if let Some(state) = state.upgrade()
                && let Some(thread) = running
                && let Err(error) = state.remove_stopped_thread_if_matches(&thread, || {}).await
            {
                tracing::warn!(thread_id = %thread.session.thread_id(), %error, "owned thread cleanup remains unconfirmed");
            }
        });
        let thread = receiver.await.map_err(|_| CodexErr::InternalAgentDied)??;
        handoff.disarm();
        Ok(thread)
    }
}

impl ThreadManagerState {
    /// Keep operation capture while targeting the runtime already owned by this caller.
    pub(crate) async fn submit_shutdown_to_thread(
        &self,
        thread: &Arc<CodexThread>,
    ) -> CodexResult<String> {
        if let Some(ops_log) = &self.ops_log
            && let Ok(mut log) = ops_log.lock()
        {
            log.push((
                thread.session.thread_id(),
                codex_protocol::protocol::Op::Shutdown,
            ));
        }
        thread
            .io
            .submit(codex_protocol::protocol::Op::Shutdown)
            .await
    }

    /// A closed receiver is not a cleanup fence, and a completed owner cannot remove its successor.
    pub(crate) async fn remove_stopped_thread_if_matches(
        &self,
        expected: &Arc<CodexThread>,
        on_removed: impl FnOnce(),
    ) -> CodexResult<bool> {
        expected.io.ensure_shutdown_complete()?;
        let thread_id = expected.session.thread_id();
        let mut threads = self.threads.write().await;
        if threads
            .get(&thread_id)
            .is_some_and(|current| Arc::ptr_eq(current, expected))
        {
            threads.remove(&thread_id);
            expected.session.services.mcp_runtime.revoke_ema();
            // Identity-bound metadata must be retired before a successor can register.
            on_removed();
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_extension_api::SessionIsolation;
    use core_test_support::PathBufExt;
    use std::time::Duration;

    async fn fixture() -> (tempfile::TempDir, Arc<ThreadManager>, StartThreadOptions) {
        let home = tempfile::tempdir().expect("tempdir");
        let mut config = crate::config::test_config().await;
        config.codex_home = home.path().to_path_buf().abs();
        config.cwd = config.codex_home.clone();
        let manager = Arc::new(ThreadManager::with_models_provider_and_home_for_tests(
            codex_login::CodexAuth::from_api_key("dummy"),
            config.model_provider.clone(),
            config.codex_home.to_path_buf(),
            Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
        ));
        let mut options = StartThreadOptions::new(config);
        options
            .thread_extension_init
            .insert(SessionIsolation::Isolated);
        (home, manager, options)
    }

    #[tokio::test]
    async fn owned_cleanup_failure_preserves_its_registered_runtime() {
        struct PanicOnThreadStop;
        impl codex_extension_api::ThreadLifecycleContributor<crate::config::Config> for PanicOnThreadStop {
            fn on_thread_stop<'a>(
                &'a self,
                _input: codex_extension_api::ThreadStopInput<'a>,
            ) -> codex_extension_api::ExtensionFuture<'a, ()> {
                Box::pin(async { panic!("synthetic owned cleanup failure") })
            }
        }
        let (_home, manager, mut options) = fixture().await;
        let mut extensions =
            codex_extension_api::ExtensionRegistryBuilder::<crate::config::Config>::new();
        extensions.thread_lifecycle_contributor(Arc::new(PanicOnThreadStop));
        options
            .thread_extension_init
            .insert(codex_extension_api::IsolatedSessionExtensions(Arc::new(
                extensions.build(),
            )));
        let cancel = CancellationToken::new();
        let tasks = TaskTracker::new();
        let owned = manager
            .start_thread_until(options, cancel.clone().cancelled_owned(), &tasks)
            .await
            .expect("start owned thread");
        cancel.cancel();
        tasks.close();
        tokio::time::timeout(Duration::from_secs(5), tasks.wait())
            .await
            .expect("failed cleanup finishes its owner task");
        assert!(owned.thread.io.ensure_shutdown_complete().is_err());
        assert!(Arc::ptr_eq(
            &manager
                .get_thread(owned.thread_id)
                .await
                .expect("failed runtime remains inspectable"),
            &owned.thread
        ));
        // The injected stop hook precedes writer closure. Close this synthetic writer explicitly.
        if let Some(live_thread) = owned.thread.session.live_thread() {
            live_thread
                .shutdown()
                .await
                .expect("release synthetic writer");
        }
    }

    #[tokio::test]
    async fn owned_cleanup_preserves_a_different_thread_handle_for_the_same_session() {
        let (_home, manager, options) = fixture().await;
        let cancel = CancellationToken::new();
        let tasks = TaskTracker::new();
        let owned = manager
            .start_thread_until(options, cancel.clone().cancelled_owned(), &tasks)
            .await
            .expect("start owned thread");
        // Sharing the same stopped session deliberately catches checks of Session identity
        // that would still remove a different CodexThread and its runtime identity.
        let replacement = Arc::new(CodexThread::new(
            Arc::clone(&owned.thread.session),
            owned.thread.io.clone(),
            (&owned.session_configured).into(),
            owned.thread.rollout_path(),
            owned.thread.session_source.clone(),
        ));
        manager
            .state
            .threads
            .write()
            .await
            .insert(owned.thread_id, Arc::clone(&replacement));
        cancel.cancel();
        tasks.close();
        tokio::time::timeout(Duration::from_secs(5), tasks.wait())
            .await
            .expect("normal cleanup finishes its owner task");
        assert!(Arc::ptr_eq(
            &manager
                .get_thread(owned.thread_id)
                .await
                .expect("replacement remains registered"),
            &replacement
        ));
        assert!(
            manager
                .state
                .remove_stopped_thread_if_matches(&replacement, || {})
                .await
                .expect("confirmed current runtime can be removed")
        );
        assert!(manager.get_thread(owned.thread_id).await.is_err());
    }
}
