// Modified for OpenDIY Codex R28 (2026-10-29); see MODIFICATIONS.md.
//! Cancellation owns the MCP read future; acceptance is not a terminal response.
use std::future::Future;

use codex_app_server_protocol::JSONRPCErrorError;
use tokio_util::sync::CancellationToken;

pub(crate) fn supports(method: &str) -> bool {
    matches!(method, "mcpServerStatus/list" | "mcpServer/resource/read")
}

pub(crate) async fn run<T>(
    cancellation: CancellationToken,
    work: impl Future<Output = Result<T, JSONRPCErrorError>>,
) -> Result<T, JSONRPCErrorError> {
    if cancellation.is_cancelled() {
        return Err(cancelled_error());
    }
    // A ready completed operation wins the race. On cancellation the owned
    // future is dropped before this function returns to the response sender.
    tokio::select! {
        biased;
        result = work => result,
        () = cancellation.cancelled() => Err(cancelled_error()),
    }
}

pub(crate) fn cancelled_error() -> JSONRPCErrorError {
    JSONRPCErrorError {
        code: -32800,
        message: "MCP read cancelled".to_string(),
        data: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
    use tokio::sync::oneshot;

    struct WorkGuard(Arc<AtomicBool>);
    impl Drop for WorkGuard {
        fn drop(&mut self) {
            self.0.store(false, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn cancellation_drops_actual_owned_work_before_terminal_error() {
        let cancellation = CancellationToken::new();
        let active = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&active);
        let (started, ready) = oneshot::channel();
        let work = async move {
            observed.store(true, Ordering::SeqCst);
            let _guard = WorkGuard(observed);
            let _ = started.send(());
            std::future::pending::<Result<(), JSONRPCErrorError>>().await
        };
        let task = tokio::spawn(run(cancellation.clone(), work));
        ready.await.expect("native read started");
        assert!(active.load(Ordering::SeqCst));
        cancellation.cancel();
        assert_eq!(
            task.await
                .expect("read joined")
                .expect_err("cancelled")
                .code,
            -32800
        );
        assert!(
            !active.load(Ordering::SeqCst),
            "terminal error precedes no owned work"
        );
    }

    #[tokio::test]
    async fn completed_work_wins_a_simultaneous_cancel() {
        let cancellation = CancellationToken::new();
        let cancel_at_completion = cancellation.clone();
        assert_eq!(
            run(cancellation, async move {
                cancel_at_completion.cancel();
                Ok(7)
            })
            .await
            .expect("completed"),
            7
        );
    }

    #[tokio::test]
    async fn already_cancelled_request_never_polls_queued_work() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let error = run(cancellation, async {
            panic!("cancelled queued work started");
            #[allow(unreachable_code)]
            Ok(())
        })
        .await
        .expect_err("cancelled before polling");
        assert_eq!(error.code, -32800);
    }

    #[test]
    fn mutations_and_unreviewed_blocking_reads_are_not_cancellable() {
        assert!(supports("mcpServerStatus/list"));
        assert!(supports("mcpServer/resource/read"));
        for method in [
            "turn/start",
            "thread/resume",
            "plugin/installed",
            "userVerification/verify",
        ] {
            assert!(!supports(method));
        }
    }
}
