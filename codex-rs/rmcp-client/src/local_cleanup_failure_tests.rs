// Modified for OpenDIY Codex R28 (2026-10-29); see MODIFICATIONS.md.
use super::*;
use crate::local_stdio_transport::{CleanupFault, LocalChildProcess};
use std::sync::atomic::AtomicUsize;

fn fault_owner(key: LocalLaunchKey, fault: Arc<CleanupFault>) -> StdioServerProcessHandle {
    let child = Arc::new(tokio::sync::Mutex::new(Box::new(LocalChildProcess::Fault(
        fault,
    ))));
    StdioServerProcessHandle::local(
        "fixture".into(),
        Some(LocalProcessTerminator::Child(child)),
        None,
    )
    .with_launch_key(key)
}

#[tokio::test]
async fn permanent_failure_is_bounded_retains_exact_owner_and_blocks_reentry() {
    let key = [91; 32];
    let fault = Arc::new(CleanupFault::default());
    let owner = fault_owner(key, Arc::clone(&fault));
    // Healthy parallel generations are unchanged; only cleanup owns the fence.
    ensure_launch_not_fenced(key).await.unwrap();
    ensure_launch_not_fenced(key).await.unwrap();
    let weak = Arc::downgrade(&owner.inner);
    let started = Instant::now();
    let results = tokio::time::timeout(
        Duration::from_secs(4),
        futures::future::join_all((0..8).map(|_| owner.terminate())),
    )
    .await
    .expect("permanent cleanup must return bounded Unconfirmed");
    assert!(results.iter().all(Result::is_err));
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "all callers must receive Unconfirmed"
    );
    assert!(!owner.inner.terminated.load(Ordering::Acquire));
    let calls = fault.kill_calls.load(Ordering::SeqCst);
    assert!(calls > 1 && calls < 100);
    let fence_count = retained_local_owners()
        .lock()
        .unwrap()
        .get(&key)
        .unwrap()
        .len();
    assert_eq!(
        fence_count, 1,
        "concurrent waiters must join one cleanup generation"
    );
    for _ in 0..32 {
        assert!(owner.terminate().await.is_err());
        assert!(ensure_launch_not_fenced(key).await.is_err());
    }
    assert_eq!(
        fault.kill_calls.load(Ordering::SeqCst),
        calls,
        "completed failed cleanup must not start more tasks or issue more kills"
    );
    drop(owner);
    assert!(
        weak.upgrade().is_some(),
        "request drop must retain the exact Child and fence"
    );
    assert!(ensure_launch_not_fenced(key).await.is_err());
    // Recovery is established by the exact original Child's wait, never a TTL.
    fault.confirmed.store(true, Ordering::SeqCst);
    ensure_launch_not_fenced(key).await.unwrap();
    assert!(
        weak.upgrade().is_none(),
        "confirmed generation must release its owner exactly once"
    );
    ensure_launch_not_fenced(key).await.unwrap();
    assert!(!retained_local_owners().lock().unwrap().contains_key(&key));
}

#[tokio::test]
async fn cancelled_caller_does_not_lose_owner_or_spawn_duplicate_cleanup() {
    let key = [92; 32];
    let fault = Arc::new(CleanupFault::default());
    let owner = fault_owner(key, Arc::clone(&fault));
    let waiter = owner.clone();
    let caller = tokio::spawn(async move { waiter.terminate().await });
    while fault.kill_calls.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }
    caller.abort();
    assert!(owner.terminate().await.is_err());
    assert_eq!(
        retained_local_owners()
            .lock()
            .unwrap()
            .get(&key)
            .unwrap()
            .len(),
        1
    );
    fault.confirmed.store(true, Ordering::SeqCst);
    owner.terminate().await.unwrap();
    assert!(owner.inner.terminated.load(Ordering::Acquire));
    assert!(!retained_local_owners().lock().unwrap().contains_key(&key));
}

#[tokio::test]
async fn transient_failure_confirms_and_exact_generation_release_keeps_other_fence() {
    let key = [93; 32];
    let temporary = Arc::new(CleanupFault::default());
    temporary.transient_errors.store(3, Ordering::SeqCst);
    let owner = fault_owner(key, Arc::clone(&temporary));
    owner.terminate().await.unwrap();
    assert_eq!(temporary.kill_calls.load(Ordering::SeqCst), 3);
    assert!(!retained_local_owners().lock().unwrap().contains_key(&key));
    let a = Arc::new(CleanupFault::default());
    let b = Arc::new(CleanupFault::default());
    let owner_a = fault_owner(key, Arc::clone(&a));
    let owner_b = fault_owner(key, Arc::clone(&b));
    let (a_result, b_result) = tokio::join!(owner_a.terminate(), owner_b.terminate());
    assert!(a_result.is_err() && b_result.is_err());
    assert_eq!(
        retained_local_owners()
            .lock()
            .unwrap()
            .get(&key)
            .unwrap()
            .len(),
        2
    );
    a.confirmed.store(true, Ordering::SeqCst);
    owner_a.terminate().await.unwrap();
    assert!(ensure_launch_not_fenced(key).await.is_err());
    assert_eq!(
        retained_local_owners()
            .lock()
            .unwrap()
            .get(&key)
            .unwrap()
            .len(),
        1
    );
    b.confirmed.store(true, Ordering::SeqCst);
    owner_b.terminate().await.unwrap();
    ensure_launch_not_fenced(key).await.unwrap();
}

#[tokio::test]
async fn failed_launch_cleanup_never_retries_on_unconfirmed_result() {
    let attempts = AtomicUsize::new(0);
    let result = retry_after_failed_launch(async { Err(unconfirmed_termination()) }, || {
        attempts.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
    .await;
    assert!(result.is_err());
    assert_eq!(attempts.load(Ordering::SeqCst), 0);
}

#[test]
fn launch_fence_identity_is_independent_of_server_alias_and_env_order() {
    let cwd = std::env::current_dir().unwrap();
    let program = std::env::current_exe().unwrap();
    let env_a = HashMap::from([
        ("Path".into(), "x".into()),
        ("TOKEN".into(), "fixture-only".into()),
    ]);
    let env_b = HashMap::from([
        ("token".into(), "fixture-only".into()),
        ("PATH".into(), "x".into()),
    ]);
    let args = vec!["fixture".into()];
    assert_eq!(
        local_launch_key(&program, &args, &cwd, &env_a),
        local_launch_key(&program, &args, &cwd, &env_b)
    );
    assert_ne!(
        local_launch_key(&program, &args, &cwd, &env_a),
        local_launch_key(&program, &["other".into()], &cwd, &env_b)
    );
}

#[tokio::test]
async fn actual_launch_reentry_uses_effective_command_fence_before_spawn() {
    let cwd = std::env::current_dir().unwrap();
    let program = std::env::current_exe().unwrap();
    let args = vec!["--help".into()];
    let fixture_env = Some(HashMap::from([(
        "CODEX_TEST_UNCONFIRMED_OWNER_PROBE".into(),
        "unique-fixture".into(),
    )]));
    let env = create_env_for_mcp_server(fixture_env.clone(), &[]).unwrap();
    let resolved = program_resolver::resolve(program.clone().into_os_string(), &env, &cwd).unwrap();
    let key = local_launch_key(Path::new(&resolved), &args, &cwd, &env);
    let fault = Arc::new(CleanupFault::default());
    let owner = fault_owner(key, Arc::clone(&fault));
    assert!(owner.terminate().await.is_err());
    let command = StdioServerCommand::new(
        program.into_os_string(),
        args,
        fixture_env,
        Vec::new(),
        None,
        McpProtocolMode::Legacy,
    );
    let started = Instant::now();
    // The actual public launcher, not just a duplicate test registry, must reject
    // another server alias with the same resolved command before process spawn.
    for _ in 0..8 {
        // Recreate the launcher, as a fresh private manager or reconnect does.
        let result = LocalStdioServerLauncher::new(cwd.clone())
            .launch(command.clone())
            .await;
        assert!(result.is_err());
    }
    assert_eq!(
        spawn_counts()
            .lock()
            .unwrap()
            .get(&key)
            .copied()
            .unwrap_or(0),
        0,
        "new manager/alias must not spawn a duplicate child"
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    let calls = fault.kill_calls.load(Ordering::SeqCst);
    assert!(calls > 1);
    assert_eq!(
        retained_local_owners()
            .lock()
            .unwrap()
            .get(&key)
            .unwrap()
            .len(),
        1
    );
    fault.confirmed.store(true, Ordering::SeqCst);
    ensure_launch_not_fenced(key).await.unwrap();
    let mut transport = LocalStdioServerLauncher::new(cwd)
        .launch(command)
        .await
        .expect("confirmed owner permits a new generation");
    assert_eq!(spawn_counts().lock().unwrap().get(&key).copied(), Some(1));
    transport.close().await.unwrap();
}

fn spawn_counts() -> &'static StdMutex<HashMap<LocalLaunchKey, usize>> {
    static COUNTS: std::sync::OnceLock<StdMutex<HashMap<LocalLaunchKey, usize>>> =
        std::sync::OnceLock::new();
    COUNTS.get_or_init(StdMutex::default)
}
pub(super) fn record_spawn(key: LocalLaunchKey) {
    *spawn_counts().lock().unwrap().entry(key).or_default() += 1;
}

#[test]
fn unconfirmed_marker_survives_nested_startup_io_errors() {
    let original = anyhow::Error::new(unconfirmed_termination());
    assert!(is_termination_unconfirmed(&original));
    let wrapped = anyhow::Error::new(crate::rmcp_client::stdio_startup_io_error(original));
    assert!(is_termination_unconfirmed(&wrapped));
    assert!(!is_termination_unconfirmed(&anyhow::anyhow!(
        "ordinary startup failure"
    )));
}

std::thread_local! {
    static LAST_DROP_BARRIER: std::cell::RefCell<Option<Arc<std::sync::Barrier>>> = const { std::cell::RefCell::new(None) };
}

pub(super) fn synchronize_last_drop() {
    LAST_DROP_BARRIER.with(|barrier| {
        if let Some(barrier) = barrier.borrow().as_ref() {
            barrier.wait();
        }
    });
}

#[tokio::test]
async fn concurrent_last_handles_start_one_retained_cleanup() {
    let key = [94; 32];
    let fault = Arc::new(CleanupFault::default());
    let owner = fault_owner(key, Arc::clone(&fault));
    let weak = Arc::downgrade(&owner.inner);
    let handles = (0..4).map(|_| owner.clone()).collect::<Vec<_>>();
    drop(owner);
    let barrier = Arc::new(std::sync::Barrier::new(handles.len()));
    let runtime = tokio::runtime::Handle::current();
    let threads = handles
        .into_iter()
        .map(|owner| {
            let barrier = Arc::clone(&barrier);
            let runtime = runtime.clone();
            std::thread::spawn(move || {
                let _runtime = runtime.enter();
                LAST_DROP_BARRIER.with(|slot| *slot.borrow_mut() = Some(barrier));
                drop(owner);
                LAST_DROP_BARRIER.with(|slot| slot.borrow_mut().take());
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().unwrap();
    }
    assert!(
        weak.upgrade().is_some(),
        "simultaneous last users must not lose the exact Child"
    );
    assert_eq!(
        retained_local_owners()
            .lock()
            .unwrap()
            .get(&key)
            .map(Vec::len),
        Some(1)
    );
    let inner = weak.upgrade().unwrap();
    let cleanup = inner
        .local_cleanup
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .clone();
    drop(inner);
    assert!(
        tokio::time::timeout(Duration::from_secs(4), cleanup)
            .await
            .unwrap()
            .is_err()
    );
    let calls = fault.kill_calls.load(Ordering::SeqCst);
    assert!(calls > 1);
    for _ in 0..8 {
        assert!(ensure_launch_not_fenced(key).await.is_err());
    }
    assert_eq!(fault.kill_calls.load(Ordering::SeqCst), calls);
    fault.confirmed.store(true, Ordering::SeqCst);
    ensure_launch_not_fenced(key).await.unwrap();
    assert!(weak.upgrade().is_none());
}

#[tokio::test]
async fn actual_launch_joins_pending_cleanup_before_admitting_next_generation() {
    let cwd = std::env::current_dir().unwrap();
    let program = std::env::current_exe().unwrap();
    let args = vec!["--help".into()];
    let fixture_env = Some(HashMap::from([(
        "CODEX_TEST_PENDING_OWNER_PROBE".into(),
        "unique-fixture".into(),
    )]));
    let env = create_env_for_mcp_server(fixture_env.clone(), &[]).unwrap();
    let resolved = program_resolver::resolve(program.clone().into_os_string(), &env, &cwd).unwrap();
    let key = local_launch_key(Path::new(&resolved), &args, &cwd, &env);
    let fault = Arc::new(CleanupFault::default());
    let owner = fault_owner(key, Arc::clone(&fault));
    let StdioServerProcessKind::Local(Some(LocalProcessTerminator::Child(child))) =
        &owner.inner.kind
    else {
        unreachable!();
    };
    let child_lock = child.lock().await;
    // Hold the actual Child lock so cleanup is in progress, not yet Unconfirmed.
    let cleanup = owner.start_local_cleanup();
    let command = StdioServerCommand::new(
        program.into_os_string(),
        args,
        fixture_env,
        Vec::new(),
        None,
        McpProtocolMode::Legacy,
    );
    let launch = LocalStdioServerLauncher::new(cwd).launch(command);
    tokio::pin!(launch);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut launch)
            .await
            .is_err(),
        "reload must wait for pending cleanup instead of failing or spawning"
    );
    assert_eq!(spawn_counts().lock().unwrap().get(&key).copied(), None);
    fault.confirmed.store(true, Ordering::SeqCst);
    drop(child_lock);
    let mut transport = tokio::time::timeout(Duration::from_secs(5), launch)
        .await
        .unwrap()
        .expect("ordinary completed cleanup permits reload");
    cleanup.await.unwrap();
    assert!(owner.inner.terminated.load(Ordering::Acquire));
    assert!(!retained_local_owners().lock().unwrap().contains_key(&key));
    assert_eq!(spawn_counts().lock().unwrap().get(&key).copied(), Some(1));
    transport.close().await.unwrap();
}
