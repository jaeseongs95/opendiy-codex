use anyhow::Context;
use clap::Parser;
use codex_app_server_client::RemoteAppServerClient;
use codex_app_server_client::RemoteAppServerConnectArgs;
use codex_app_server_client::RemoteAppServerEndpoint;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadCleanupCompletedParams;
use codex_app_server_protocol::ThreadCleanupCompletedResponse;
use std::io::IsTerminal;
use std::io::Write;

#[derive(Debug, Parser)]
#[command(about = "현재 부모 아래 로컬 2세대 서브에이전트의 종료 대화를 미리 보고 정리합니다.", long_about = "읽기 전용 미리보기가 기본입니다. --apply는 미리보기 뒤 명시 확인을 받습니다. 실행 중이거나 상태가 불명확한 항목, 다른 세대·원격 소유·이전 방식 항목은 보존합니다. --thread를 반복하면 정확한 선택 집합만 정리하며 선택하지 않은 후손은 보존합니다.")]
pub(crate) struct CleanupAgentsCommand {
    /// 현재 대화 UUID. 생략하면 CODEX_THREAD_ID를 사용합니다.
    #[arg(long, env = "CODEX_THREAD_ID")]
    current: String,
    /// 정리 범위의 부모 UUID. 생략하면 현재 대화입니다.
    #[arg(long)]
    parent: Option<String>,
    /// 정리할 정확한 UUID. 반복할 수 있으며 생략하면 미리보기의 모든 종료 후보입니다.
    #[arg(long = "thread")]
    threads: Vec<String>,
    /// 미리보기 후 확인을 받아 실제로 정리합니다. 생략하면 읽기 전용입니다.
    #[arg(long)]
    apply: bool,
    /// 미리보기의 선택 항목 삭제를 명시적으로 승인합니다.
    #[arg(long, requires = "apply")]
    confirm: bool,
    #[command(flatten)]
    remote: crate::InteractiveRemoteOptions,
}

pub(crate) async fn run(
    command: CleanupAgentsCommand,
    root_remote: Option<String>,
    root_auth: Option<String>,
) -> anyhow::Result<()> {
    let explicit = crate::resolve_remote_endpoint(
        command.remote.remote.or(root_remote),
        command.remote.remote_auth_token_env.or(root_auth),
    )?;
    let local = explicit.is_none();
    let endpoint = match explicit {
        Some(endpoint) => endpoint,
        None => {
            let home = codex_core::config::find_codex_home()?;
            let socket = codex_app_server_client::app_server_control_socket_path(&home)?;
            let socket = codex_app_server_client::verified_local_daemon_socket(socket.as_path())?
                .context(
                "실행 중인 로컬 서버를 확인할 수 없습니다. 대상 서버의 --remote 주소를 지정하세요.",
            )?;
            RemoteAppServerEndpoint::UnixSocket {
                socket_path: socket,
            }
        }
    };
    let args = RemoteAppServerConnectArgs {
        endpoint,
        client_name: "codex-cli".into(),
        client_version: env!("CARGO_PKG_VERSION").into(),
        experimental_api: true,
        mcp_server_openai_form_elicitation: false,
        opt_out_notification_methods: Vec::new(),
        channel_capacity: codex_app_server_client::DEFAULT_IN_PROCESS_CHANNEL_CAPACITY,
    };
    #[cfg(windows)]
    let client = if local {
        RemoteAppServerClient::connect_local_daemon(args).await?
    } else {
        RemoteAppServerClient::connect(args).await?
    };
    #[cfg(not(windows))]
    let client = {
        let _ = local;
        RemoteAppServerClient::connect(args).await?
    };
    let parent = command.parent.unwrap_or_else(|| command.current.clone());
    let preview: ThreadCleanupCompletedResponse = client
        .request_typed(ClientRequest::ThreadCleanupCompleted {
            request_id: RequestId::Integer(1),
            params: ThreadCleanupCompletedParams {
                parent_thread_id: parent.clone(),
                current_thread_id: command.current.clone(),
                confirmed: false,
                selection: Vec::new(),
            },
        })
        .await?;
    println!("{}", serde_json::to_string_pretty(&preview)?);
    let mut selected = Vec::new();
    for result in preview.results {
        if command.threads.is_empty() || command.threads.contains(&result.thread_id) {
            if let Some(snapshot) = result.snapshot {
                selected.push(snapshot);
            }
        }
    }
    for id in &command.threads {
        if !selected.iter().any(|item| &item.thread_id == id) {
            anyhow::bail!("선택한 항목은 종료 상태를 확정할 수 없어 보존됩니다: {id}");
        }
    }
    if !command.apply || selected.is_empty() {
        client.shutdown().await?;
        return Ok(());
    }
    if !command.confirm {
        if !std::io::stdin().is_terminal() {
            anyhow::bail!("실제 정리에는 터미널 확인 또는 --apply --confirm이 필요합니다.");
        }
        eprint!(
            "위 미리보기에서 선택한 {}개 대화를 영구 삭제하려면 '정리'를 입력하세요: ",
            selected.len()
        );
        std::io::stderr().flush()?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if answer.trim() != "정리" {
            client.shutdown().await?;
            return Ok(());
        }
    }
    let result: ThreadCleanupCompletedResponse = client
        .request_typed(ClientRequest::ThreadCleanupCompleted {
            request_id: RequestId::Integer(2),
            params: ThreadCleanupCompletedParams {
                parent_thread_id: parent,
                current_thread_id: command.current,
                confirmed: true,
                selection: selected,
            },
        })
        .await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    client.shutdown().await?;
    if result
        .results
        .iter()
        .any(|entry| entry.outcome != "deleted")
    {
        anyhow::bail!(
            "일부 항목을 보존했거나 정리 결과를 확정하지 못했습니다. 출력된 항목별 결과를 확인하세요."
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_is_default_and_confirmation_requires_apply() {
        let command =
            CleanupAgentsCommand::try_parse_from(["cleanup-agents", "--current", "current"])
                .unwrap();
        assert!(!command.apply);
        assert!(!command.confirm);
        assert!(
            CleanupAgentsCommand::try_parse_from([
                "cleanup-agents",
                "--current",
                "current",
                "--confirm"
            ])
            .is_err()
        );
        let command = CleanupAgentsCommand::try_parse_from([
            "cleanup-agents",
            "--current",
            "current",
            "--apply",
            "--confirm",
            "--thread",
            "one",
            "--thread",
            "two",
        ])
        .unwrap();
        assert_eq!(command.threads, ["one", "two"]);
    }
}
