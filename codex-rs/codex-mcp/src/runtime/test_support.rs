// Modified for OpenDIY Codex R30 (2026-10-30); see MODIFICATIONS.md.
//! 시험 전용 시작 장치다. 프로세스·네트워크·정리 판정은 대체하지 않는다.
use super::*;
use crate::rmcp_client::AsyncManagedClient;
use crate::rmcp_client::StartupOutcomeError;
use futures::FutureExt;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

/// 실제 MCP 종료가 시작을 취소한 뒤 시작 정리 결과를 장벽에 대기시킨다.
/// 알림은 허가를 하나 보존하므로 시간 지연에 의존하지 않는다.
pub struct UnconfirmedStartupProbe {
    shutdown_entered: Arc<Notify>,
    release: Arc<Notify>,
}

impl UnconfirmedStartupProbe {
    pub async fn wait_for_shutdown(&self) {
        self.shutdown_entered.notified().await;
    }

    pub fn release(&self) {
        self.release.notify_one();
    }
}

impl Drop for UnconfirmedStartupProbe {
    fn drop(&mut self) {
        self.release();
    }
}

impl McpRuntime {
    /// 연결 집합 시험과 동일한 시작 실패 클라이언트 경계를 설치한다.
    /// 기존 연결이 없는 런타임만 허용하고 게시된 스레드 입력은 보존한다.
    /// 시작 결과만 제어하며 AsyncManagedClient·McpConnectionSet·McpRuntime의
    /// 종료와 core의 모든 종료 처리기·정리 소유자는 실제 구현을 유지한다.
    pub fn install_unconfirmed_startup_for_test(&self) -> UnconfirmedStartupProbe {
        let previous = self.current.load_full();
        assert!(
            !previous.connections.has_servers(),
            "test fixture must not replace live MCP clients"
        );
        let probe = UnconfirmedStartupProbe {
            shutdown_entered: Arc::new(Notify::new()),
            release: Arc::new(Notify::new()),
        };
        let cancel_token = CancellationToken::new();
        let startup_cancel = cancel_token.clone();
        let shutdown_entered = Arc::clone(&probe.shutdown_entered);
        let release = Arc::clone(&probe.release);
        let client = async move {
            startup_cancel.cancelled().await;
            shutdown_entered.notify_one();
            release.notified().await;
            Err(StartupOutcomeError::Failed {
                error: "test startup cleanup termination unconfirmed".into(),
                is_authentication_required: false,
                is_ema_authentication_required: false,
                is_termination_unconfirmed: true,
            })
        }
        .boxed()
        .shared();
        let mut connections = McpConnectionSet::empty(true);
        connections.insert_test_client(
            "unconfirmed-startup",
            AsyncManagedClient {
                ema_authority: None,
                client,
                is_codex_apps_mcp_server: false,
                server_capabilities: Arc::new(Mutex::new(None)),
                cached_server_info: None,
                codex_apps_tools_cache_context: None,
                tool_catalog_cache_context: None,
                startup_complete: Arc::new(AtomicBool::new(false)),
                startup_reconnect: None,
                cancel_token,
            },
        );
        self.current.store(Arc::new(PublishedMcpRuntime {
            connections: Arc::new(connections),
            config: previous.config.clone(),
            auth: previous.auth.clone(),
            auth_token: previous.auth_token.clone(),
            auth_manager: previous.auth_manager.clone(),
            auth_generation: Arc::clone(&previous.auth_generation),
            plugins_available: previous.plugins_available,
            ready_selected_capability_roots: previous.ready_selected_capability_roots.clone(),
            environment_selections: Arc::clone(&previous.environment_selections),
            ready_environments: previous.ready_environments.clone(),
            cached_binding: Mutex::new(None),
        }));
        probe
    }
}
