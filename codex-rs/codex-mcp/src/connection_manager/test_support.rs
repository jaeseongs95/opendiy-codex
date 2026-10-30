use super::*;

// 이 크레이트의 단위시험과 의존 크레이트의 시험 장치가 같은 삽입 경계를 쓴다.
impl McpConnectionSet {
    pub(crate) fn insert_test_client(
        &mut self,
        name: impl Into<String>,
        client: AsyncManagedClient,
    ) {
        let name = name.into();
        self.servers.insert(
            name,
            McpServerView {
                refresh_tool_list: true,
                refresh_timeout: DEFAULT_STARTUP_TIMEOUT,
                tool_filter: ToolFilter::default(),
                protocol_mode: crate::McpProtocolMode::Legacy,
                startup_readiness: Default::default(),
                connection: Arc::new(McpServerConnection {
                    identity: None,
                    client,
                    startup_timeout: DEFAULT_STARTUP_TIMEOUT,
                    startup_trigger: None,
                    _diagnostics_guard: LIVE_CONNECTIONS.track(),
                }),
                metadata: McpServerMetadata {
                    environment_id: String::new(),
                    pollutes_memory: true,
                    origin: None,
                    supports_parallel_tool_calls: false,
                    default_tools_approval_mode: None,
                    tool_approval_modes: HashMap::new(),
                },
                tool_timeout: None,
                catalog_item_limit: crate::pagination::MAX_MCP_CATALOG_ITEMS,
            },
        );
    }
}
