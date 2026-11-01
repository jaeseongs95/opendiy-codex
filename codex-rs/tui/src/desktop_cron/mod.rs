// Modified for OpenDIY Codex R31 (2026-11-01); see MODIFICATIONS.md.
//! 데스크톱 예약 제어의 일회 소유 경계.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) mod wire;
#[cfg(windows)]
pub(crate) use wire::*;

/// 동기 진입점에서 한 번 취득한다. 복제·형식 출력·직렬화는 제공하지 않는다.
#[derive(Default)]
pub struct DesktopCronBootstrap {
    #[cfg(windows)]
    inner: Option<(wire::Bootstrap, wire::SecretToken)>,
}

struct EnvironmentBytes(Vec<u8>);
impl Drop for EnvironmentBytes {
    fn drop(&mut self) {
        for byte in &mut self.0 {
            // 제거한 환경의 임시 사본도 메모리에 남기지 않는다.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}

/// 실행기와 자식 디스패치 전에 호출해야 한다. 잘못된 값도 환경에서는 제거한다.
pub fn take_desktop_cron_bootstrap_from_env() -> DesktopCronBootstrap {
    const BOOTSTRAP: &str = "CODEX_DESKTOP_CRON_BOOTSTRAP";
    const TOKEN: &str = "CODEX_DESKTOP_CRON_TOKEN";
    let mut bootstrap = None;
    let mut token = None;
    let mut duplicate = false;
    for (key, value) in std::env::vars_os() {
        let name = key.to_string_lossy();
        let slot = if name.eq_ignore_ascii_case(BOOTSTRAP) {
            &mut bootstrap
        } else if name.eq_ignore_ascii_case(TOKEN) {
            &mut token
        } else {
            continue;
        };
        // 동기 main 첫 문장이며 다른 스레드·환경을 읽는 자식은 아직 없다.
        unsafe { std::env::remove_var(&key) };
        duplicate |= slot.is_some();
        *slot = Some(EnvironmentBytes(value.into_encoded_bytes()));
    }
    #[cfg(windows)]
    {
        let inner = (|| {
            if duplicate {
                return None;
            }
            let bootstrap = wire::from_json(&bootstrap.as_ref()?.0).ok()?;
            let token = std::str::from_utf8(&token.as_ref()?.0).ok()?;
            Some((bootstrap, wire::SecretToken::from_hex(token).ok()?))
        })();
        DesktopCronBootstrap { inner }
    }
    #[cfg(not(windows))]
    {
        let _ = (bootstrap, token, duplicate);
        DesktopCronBootstrap::default()
    }
}

#[cfg(windows)]
use std::sync::Arc;
#[cfg(windows)]
use std::sync::atomic::{AtomicU8, Ordering};
#[cfg(windows)]
use tokio::sync::oneshot;
#[cfg(windows)]
use tokio::time::Instant;

#[cfg(windows)]
const WAITING: u8 = 0;
#[cfg(windows)]
const CLAIMED: u8 = 1;
#[cfg(windows)]
const CANCELLED: u8 = 2;

/// 소유 루프는 변경 직전에 claim과 현재 바인딩을 확인한다.
#[cfg(windows)]
pub(crate) struct PendingRequest {
    pub(crate) request: wire::Request,
    pub(crate) received_deadline: Instant,
    pub(crate) reply: oneshot::Sender<wire::Response>,
    state: Arc<AtomicU8>,
}

#[cfg(windows)]
impl std::fmt::Debug for PendingRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PendingRequest { .. }")
    }
}

#[cfg(windows)]
impl PendingRequest {
    #[cfg(test)]
    pub(crate) fn for_test(
        request: wire::Request,
        ttl: std::time::Duration,
    ) -> (Self, oneshot::Receiver<wire::Response>) {
        let (reply, receive) = oneshot::channel();
        (
            Self {
                request,
                received_deadline: Instant::now() + ttl,
                reply,
                state: Arc::new(AtomicU8::new(WAITING)),
            },
            receive,
        )
    }

    pub(crate) fn claim(&self) -> bool {
        if Instant::now() >= self.received_deadline {
            self.cancel();
            return false;
        }
        if self
            .state
            .compare_exchange(WAITING, CLAIMED, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        if Instant::now() >= self.received_deadline {
            self.state.store(CANCELLED, Ordering::Release);
            return false;
        }
        true
    }

    fn cancel(&self) {
        let _ =
            self.state
                .compare_exchange(WAITING, CANCELLED, Ordering::AcqRel, Ordering::Acquire);
    }
}

/// 소유 future가 오류·취소로 끝나도 연결 작업을 남기지 않는다.
#[cfg(windows)]
pub(crate) struct ClientTask(Option<tokio::task::JoinHandle<()>>);

#[cfg(windows)]
impl ClientTask {
    pub(crate) async fn stop(mut self) {
        if let Some(task) = self.0.take() {
            task.abort();
            if let Err(error) = task.await
                && !error.is_cancelled()
            {
                tracing::warn!("예약 제어 작업을 회수하지 못했습니다: {error}");
            }
        }
    }
}

#[cfg(windows)]
impl Drop for ClientTask {
    fn drop(&mut self) {
        if let Some(task) = &self.0 {
            task.abort();
        }
    }
}

#[cfg(windows)]
pub(crate) fn start(
    bootstrap: DesktopCronBootstrap,
    owner_instance_id: wire::UuidText,
    sender: crate::app_event_sender::AppEventSender,
) -> Option<ClientTask> {
    let (bootstrap, token) = bootstrap.inner?;
    Some(ClientTask(Some(tokio::spawn(async move {
        if let Err(error) = windows::run(bootstrap, token, owner_instance_id, sender).await {
            tracing::warn!("예약 제어 연결을 닫았습니다: {error}");
        }
    }))))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn pending(deadline: Instant) -> PendingRequest {
        let request = wire::from_json(br#"{"v":1,"type":"request","id":"87654321-4321-4765-8321-cba987654321","ownerInstanceId":"aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee","targetThreadId":"12345678-1234-4567-8123-123456789abc","scope":null,"body":{"type":"list"}}"#).unwrap();
        let (reply, _) = oneshot::channel();
        PendingRequest {
            request,
            received_deadline: deadline,
            reply,
            state: Arc::new(AtomicU8::new(WAITING)),
        }
    }

    #[tokio::test]
    async fn late_or_cancelled_request_never_claims_and_accepted_request_survives_reply_loss() {
        let expired = pending(Instant::now());
        assert!(!expired.claim());
        let cancelled = pending(Instant::now() + std::time::Duration::from_secs(5));
        cancelled.cancel();
        assert!(!cancelled.claim());
        let accepted = pending(Instant::now() + std::time::Duration::from_secs(5));
        assert!(accepted.claim());
        accepted.cancel();
        assert_eq!(accepted.state.load(Ordering::Acquire), CLAIMED);
        assert!(!accepted.claim());
        assert_eq!(format!("{accepted:?}"), "PendingRequest { .. }");
    }
}
