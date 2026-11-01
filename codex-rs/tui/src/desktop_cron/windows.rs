// Modified for OpenDIY Codex R31 (2026-11-01); see MODIFICATIONS.md.
//! 윈도 클라이언트 통로 하나와 작성자 하나만 소유한다. 재연결·재전송은 없다.

use super::wire;
use super::{CANCELLED, PendingRequest, WAITING};
use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::io::{self, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};
use tokio::sync::oneshot;
use tokio::time::{Instant, timeout_at};
use windows_sys::Win32::Foundation::{ERROR_NOT_FOUND, FILETIME, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::IO::CancelIoEx;
use windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId;
use windows_sys::Win32::System::Threading::{
    GetProcessId, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, WaitForSingleObject,
};

const TIMEOUT: Duration = Duration::from_secs(5);

struct Pipe(NamedPipeClient);
impl Drop for Pipe {
    fn drop(&mut self) {
        // 자기 통로의 입출력만 취소한다. 버퍼와 OVERLAPPED는 기존 IOCP가 회수한다.
        if unsafe { CancelIoEx(self.0.as_raw_handle(), std::ptr::null()) } == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_NOT_FOUND as i32) {
                tracing::warn!("예약 제어 통로 입출력 취소 실패: {error}");
            }
        }
    }
}

struct Process {
    handle: OwnedHandle,
    pid: u32,
    created: u64,
}
impl Process {
    fn open(pid: u32) -> io::Result<Self> {
        // 조회와 종료 확인만 허용하며 핸들은 자식에게 상속하지 않는다.
        let raw = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
        let actual_pid = unsafe { GetProcessId(raw) };
        if actual_pid == 0 {
            return Err(io::Error::last_os_error());
        }
        if actual_pid != pid {
            return Err(denied());
        }
        let (mut created, mut exited, mut kernel, mut user): (
            FILETIME,
            FILETIME,
            FILETIME,
            FILETIME,
        ) = unsafe { std::mem::zeroed() };
        if unsafe { GetProcessTimes(raw, &mut created, &mut exited, &mut kernel, &mut user) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let process = Self {
            handle,
            pid,
            created: (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime),
        };
        process.ensure_running()?;
        Ok(process)
    }

    fn ensure_running(&self) -> io::Result<()> {
        match unsafe { WaitForSingleObject(self.handle.as_raw_handle(), 0) } {
            WAIT_TIMEOUT => Ok(()),
            WAIT_OBJECT_0 => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "예약 제어 상대가 종료됐습니다.",
            )),
            _ => Err(io::Error::last_os_error()),
        }
    }
}

fn server_pid(pipe: &Pipe) -> io::Result<u32> {
    let mut pid = 0;
    if unsafe { GetNamedPipeServerProcessId(pipe.0.as_raw_handle(), &mut pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if pid == 0 {
        return Err(denied());
    }
    Ok(pid)
}

fn verify_server(pipe: &Pipe, bootstrap: &wire::Bootstrap) -> io::Result<Process> {
    let pid = server_pid(pipe)?;
    let process = Process::open(pid)?;
    if process.pid != bootstrap.host_pid.get()
        || process.created != bootstrap.host_created_filetime.get()
        || server_pid(pipe)? != process.pid
    {
        return Err(denied());
    }
    process.ensure_running()?;
    Ok(process)
}

fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "예약 제어 상대의 소유 신원이 일치하지 않습니다.",
    )
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "예약 제어 자료의 형식이나 상한이 올바르지 않습니다.",
    )
}
fn elapsed() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "예약 제어 대기 기한을 넘었습니다.")
}

async fn read_frame<T: DeserializeOwned>(
    pipe: &mut Pipe,
    total_deadline: Option<Instant>,
) -> io::Result<T> {
    read_frame_received(pipe, total_deadline)
        .await
        .map(|(value, _)| value)
}

async fn read_frame_received<T: DeserializeOwned>(
    pipe: &mut Pipe,
    total_deadline: Option<Instant>,
) -> io::Result<(T, Instant)> {
    let mut prefix = [0; 4];
    // 인증 후 유휴 시간은 무제한이며 첫 바이트 이후에만 프레임 기한을 시작한다.
    if let Some(deadline) = total_deadline {
        timeout_at(deadline, pipe.0.read_exact(&mut prefix[..1]))
            .await
            .map_err(|_| elapsed())??;
    } else {
        pipe.0.read_exact(&mut prefix[..1]).await?;
    }
    let deadline = total_deadline.map_or_else(
        || Instant::now() + TIMEOUT,
        |total| total.min(Instant::now() + TIMEOUT),
    );
    timeout_at(deadline, async {
        pipe.0.read_exact(&mut prefix[1..]).await?;
        let length = u32::from_le_bytes(prefix) as usize;
        if length == 0 || length > wire::MAX_FRAME_BYTES {
            return Err(invalid());
        }
        let mut bytes = vec![0; length];
        pipe.0.read_exact(&mut bytes).await?;
        let received = Instant::now();
        wire::from_json(&bytes)
            .map(|value| (value, received))
            .map_err(|_| invalid())
    })
    .await
    .map_err(|_| elapsed())?
}

struct LimitedFrame(Vec<u8>);
impl Write for LimitedFrame {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > wire::MAX_FRAME_BYTES.saturating_sub(self.0.len()) {
            return Err(invalid());
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

async fn write_frame<T: Serialize>(
    pipe: &mut Pipe,
    message: &T,
    deadline: Instant,
) -> io::Result<()> {
    if Instant::now() >= deadline {
        return Err(elapsed());
    }
    let mut bytes = LimitedFrame(Vec::new());
    serde_json::to_writer(&mut bytes, message).map_err(|_| invalid())?;
    if bytes.0.is_empty() {
        return Err(invalid());
    }
    timeout_at(deadline, async {
        if Instant::now() >= deadline {
            return Err(elapsed());
        }
        pipe.0
            .write_all(&(bytes.0.len() as u32).to_le_bytes())
            .await?;
        pipe.0.write_all(&bytes.0).await
    })
    .await
    .map_err(|_| elapsed())?
}

struct Admission(Arc<AtomicU8>);
impl Drop for Admission {
    fn drop(&mut self) {
        // 연결 작업 취소나 시간 초과 뒤 소유 루프에 남은 미접수 이벤트를 무효화한다.
        let _ = self
            .0
            .compare_exchange(WAITING, CANCELLED, Ordering::AcqRel, Ordering::Acquire);
    }
}

pub(super) async fn run(
    bootstrap: wire::Bootstrap,
    token: wire::SecretToken,
    owner: wire::UuidText,
    sender: AppEventSender,
) -> io::Result<()> {
    let deadline = Instant::now() + TIMEOUT;
    // 먼저 만들어진 서버에 한 번만 연다. busy나 실패에도 재시도하지 않는다.
    let mut pipe = Pipe(ClientOptions::new().open(bootstrap.pipe.as_str())?);
    let peer = verify_server(&pipe, &bootstrap)?;
    let current = Process::open(std::process::id())?;
    let hello = wire::hello(&bootstrap, owner.clone(), current.pid, current.created)
        .map_err(|_| invalid())?;
    timeout_at(deadline, async {
        write_frame(&mut pipe, &hello, deadline).await?;
        let challenge = read_frame(&mut pipe, Some(deadline)).await?;
        peer.ensure_running()?;
        // 운영체제 상대 확인과 desktop 역할 증명 검증 뒤에만 tui 증명을 보낸다.
        let auth =
            wire::client_auth(token, &bootstrap, &hello, &challenge).map_err(|_| denied())?;
        write_frame(&mut pipe, &auth, deadline).await?;
        let ready: wire::Ready = read_frame(&mut pipe, Some(deadline)).await?;
        if ready.owner_instance_id != owner {
            return Err(denied());
        }
        peer.ensure_running()
    })
    .await
    .map_err(|_| elapsed())??;

    loop {
        peer.ensure_running()?;
        let (request, received): (wire::Request, _) = read_frame_received(&mut pipe, None).await?;
        let received_deadline = received + TIMEOUT;
        if request.owner_instance_id != owner {
            return Err(denied());
        }
        let id = request.id.clone();
        let scope = request.scope.clone();
        let target = request.target_thread_id.clone();
        let is_list = matches!(request.body, wire::CronRequest::List {});
        let mut close_after_response = false;
        let response = if request.validate().is_err() {
            error_response(
                id.clone(),
                owner.clone(),
                scope.clone(),
                wire::ErrorCode::InvalidRequest,
            )
        } else {
            // 한 프레임을 전달한 뒤 응답을 기다리므로 대기는 최대 한 요청·1MiB다.
            let (reply, receive) = oneshot::channel();
            let admission = Admission(Arc::new(AtomicU8::new(WAITING)));
            let pending = PendingRequest {
                request,
                received_deadline,
                reply,
                state: admission.0.clone(),
            };
            // 세션 기록용 송신기를 우회하여 프롬프트 원문을 기록하지 않는다.
            sender
                .app_event_tx
                .send(AppEvent::DesktopCronRequest(Box::new(pending)))
                .map_err(|_| {
                    io::Error::new(io::ErrorKind::BrokenPipe, "예약 소유 루프가 종료됐습니다.")
                })?;
            let result = timeout_at(received_deadline, receive).await;
            drop(admission);
            // 미접수 이벤트가 소유 루프 큐에 남아도 다음 요청을 누적하지 않는다.
            close_after_response = !matches!(&result, Ok(Ok(_)));
            match result {
                Ok(Ok(response)) => response,
                _ if is_list => error_response(
                    id.clone(),
                    owner.clone(),
                    scope.clone(),
                    wire::ErrorCode::RequestTimeout,
                ),
                _ => wire::Response {
                    v: wire::VersionOne,
                    kind: wire::ResponseType::Value,
                    id: id.clone(),
                    owner_instance_id: owner.clone(),
                    scope: scope.clone(),
                    result: Some(wire::ResultBody::Mutation(wire::MutationResult {
                        status: wire::MutationStatus::Unknown,
                        reason: Some("resultUnknown".into()),
                        job_id: None,
                    })),
                    error: None,
                },
            }
        };
        if response.id != id
            || response.owner_instance_id != owner
            || scope
                .as_ref()
                .is_some_and(|scope| response.scope.as_ref() != Some(scope))
            || response
                .scope
                .as_ref()
                .is_some_and(|scope| scope.thread_id != target)
            || response.result.is_some() == response.error.is_some()
        {
            return Err(invalid());
        }
        if let Some(result) = &response.result {
            result.validate().map_err(|_| invalid())?;
            if matches!(result, wire::ResultBody::List(_)) != is_list {
                return Err(invalid());
            }
        }
        peer.ensure_running()?;
        write_frame(&mut pipe, &response, Instant::now() + TIMEOUT).await?;
        if close_after_response {
            return Ok(());
        }
    }
}

fn error_response(
    id: wire::UuidText,
    owner_instance_id: wire::UuidText,
    scope: Option<wire::Scope>,
    code: wire::ErrorCode,
) -> wire::Response {
    wire::Response {
        v: wire::VersionOne,
        kind: wire::ResponseType::Value,
        id,
        owner_instance_id,
        scope,
        result: None,
        error: Some(wire::ResponseError { code }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Payload {
        value: String,
    }

    async fn pair() -> (NamedPipeServer, Pipe) {
        let name = format!(
            r"\\.\pipe\OpenDIY-TuiControl-{}",
            uuid::Uuid::new_v4().simple()
        );
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .max_instances(1)
            .reject_remote_clients(true)
            .create(&name)
            .unwrap();
        let client = Pipe(ClientOptions::new().open(&name).unwrap());
        server.connect().await.unwrap();
        (server, client)
    }

    #[tokio::test]
    async fn real_pipe_preserves_split_prefix_utf8_and_rejects_length_before_payload() {
        let (mut server, mut client) = pair().await;
        let body = r#"{"value":"가"}"#.as_bytes();
        let bytes = [(body.len() as u32).to_le_bytes().as_slice(), body].concat();
        let write = async {
            for byte in bytes {
                server.write_all(&[byte]).await.unwrap();
                tokio::task::yield_now().await;
            }
        };
        let read = read_frame::<Payload>(&mut client, Some(Instant::now() + TIMEOUT));
        let (_, value) = tokio::join!(write, read);
        assert_eq!(value.unwrap().value, "가");
        for length in [0, wire::MAX_FRAME_BYTES as u32 + 1] {
            let (mut server, mut client) = pair().await;
            server.write_all(&length.to_le_bytes()).await.unwrap();
            assert!(
                matches!(read_frame::<Payload>(&mut client, Some(Instant::now() + TIMEOUT)).await, Err(error) if error.kind() == io::ErrorKind::InvalidData)
            );
        }
    }

    #[tokio::test]
    async fn real_pipe_identity_must_match_creation_generation_and_typed_json_rejects_duplicates() {
        let (mut server, mut client) = pair().await;
        let current = Process::open(std::process::id()).unwrap();
        let name = format!(
            r"\\.\pipe\OpenDIY-TuiControl-{}",
            uuid::Uuid::new_v4().simple()
        );
        let mut bootstrap = wire::Bootstrap::new(
            &name,
            current.pid,
            current.created,
            "11111111-2222-4333-8444-555555555555",
            1,
        )
        .unwrap();
        assert!(verify_server(&client, &bootstrap).is_ok());
        bootstrap.host_created_filetime = wire::DecimalU64::new(current.created.wrapping_add(1));
        assert!(verify_server(&client, &bootstrap).is_err());
        let bytes = r#"{"value":"첫 값","value":"두 값"}"#.as_bytes();
        server
            .write_all(&(bytes.len() as u32).to_le_bytes())
            .await
            .unwrap();
        server.write_all(bytes).await.unwrap();
        assert!(
            matches!(read_frame::<Payload>(&mut client, Some(Instant::now() + TIMEOUT)).await, Err(error) if error.kind() == io::ErrorKind::InvalidData)
        );
    }

    #[tokio::test]
    async fn real_pipe_incomplete_prefix_times_out_and_serialization_bounds_escape_expansion() {
        let (mut server, mut client) = pair().await;
        server.write_all(&[1]).await.unwrap();
        assert!(
            matches!(read_frame::<Payload>(&mut client, Some(Instant::now() + Duration::from_millis(20))).await, Err(error) if error.kind() == io::ErrorKind::TimedOut)
        );
        let (mut server, mut client) = pair().await;
        assert!(
            write_frame(
                &mut client,
                &"\0".repeat(wire::MAX_FRAME_BYTES / 6 + 1),
                Instant::now() + TIMEOUT
            )
            .await
            .is_err()
        );
        let mut byte = [0];
        assert!(
            tokio::time::timeout(Duration::from_millis(20), server.read(&mut byte))
                .await
                .is_err()
        );
    }
}
