// Modified for OpenDIY Codex r22 (2026-10-26); see MODIFICATIONS.md.
//! Verifies console suppression through contained launches and both containment fallbacks.

use super::*;
use pretty_assertions::assert_eq;
use std::os::windows::process::CommandExt;
use std::process::Stdio;
use std::time::Duration;
use winapi::um::winnt::JOB_OBJECT_LIMIT_ACTIVE_PROCESS;

#[tokio::test]
async fn mcp_read_lifetime_owned_job_and_process_fences() -> anyhow::Result<()> {
    use tokio::io::AsyncBufReadExt;
    const TEST: &str = "win::job::tests::mcp_read_lifetime_owned_job_and_process_fences";
    const PHASE: &str = "CODEX_TEST_OWNED_TERMINATION_PHASE";
    let executable = std::env::current_exe()?;
    match std::env::var(PHASE).as_deref() {
        Ok("leaf") => {
            std::thread::sleep(Duration::from_secs(30));
            return Ok(());
        }
        Ok("root") => {
            let _leaf = std::process::Command::new(&executable)
                .args(["--exact", TEST])
                .env(PHASE, "leaf")
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()?;
            println!("owned tree ready");
            std::io::Write::flush(&mut std::io::stdout())?;
            std::thread::sleep(Duration::from_secs(30));
            return Ok(());
        }
        _ => {}
    }
    let job = JobObject::create_without_breakaway()?;
    let mut command = Command::new(&executable);
    command
        .args(["--exact", TEST, "--nocapture"])
        .env(PHASE, "root")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = job.spawn_contained(&mut command)?;
    let handle = JobObject::open_process_handle(
        child
            .id()
            .ok_or_else(|| io::Error::other("child PID missing"))?,
    )?;
    let mut stdout = tokio::io::BufReader::new(
        child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("stdout missing"))?,
    );
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut line = String::new();
        loop {
            line.clear();
            anyhow::ensure!(
                stdout.read_line(&mut line).await? > 0,
                "fixture exited before readiness"
            );
            if line.contains("owned tree ready") {
                break;
            }
        }
        Ok::<(), anyhow::Error>(())
    })
    .await??;
    assert!(!job.is_empty()?);
    assert!(!JobObject::process_handle_is_terminated(&handle)?);
    job.terminate()?;
    tokio::time::timeout(Duration::from_secs(10), async {
        while !job.is_empty()? || !JobObject::process_handle_is_terminated(&handle)? {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Ok::<(), io::Error>(())
    })
    .await??;
    assert!(job.is_empty()?, "the owned grandchild also left the job");
    assert!(JobObject::process_handle_is_terminated(&handle)?);
    child.wait().await?;
    Ok(())
}

#[tokio::test]
async fn background_launches_keep_consoles_hidden_when_containment_fails() -> anyhow::Result<()> {
    const TEST_NAME: &str =
        "win::job::tests::background_launches_keep_consoles_hidden_when_containment_fails";
    const PHASE: &str = "CODEX_TEST_BACKGROUND_JOB_PHASE";
    let executable = std::env::current_exe()?;
    match std::env::var(PHASE).as_deref() {
        Ok("probe") => {
            assert!(unsafe { winapi::um::wincon::GetConsoleWindow() }.is_null());
            println!("background probe completed");
            return Ok(());
        }
        Ok("hold") => {
            std::thread::sleep(Duration::from_secs(/*secs*/ 30));
            return Ok(());
        }
        Ok("parent") => {}
        _ => {
            let output = std::process::Command::new(executable)
                .args(["--exact", TEST_NAME, "--nocapture"])
                .env(PHASE, "parent")
                .creation_flags(winapi::um::winbase::DETACHED_PROCESS)
                .output()?;
            assert!(output.status.success(), "detached probe failed: {output:?}");
            return Ok(());
        }
    }

    // Fill a one-process job so assigning the next suspended child must fail.
    // The parent remains outside this job, allowing the uncontained retry to run.
    let full_job = JobObject::create()?;
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_ACTIVE_PROCESS
        | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        | JOB_OBJECT_LIMIT_BREAKAWAY_OK;
    limits.BasicLimitInformation.ActiveProcessLimit = 1;
    let configured = unsafe {
        SetInformationJobObject(
            full_job.handle.as_raw_handle().cast(),
            JobObjectExtendedLimitInformation,
            std::ptr::addr_of_mut!(limits).cast(),
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    anyhow::ensure!(configured != 0, "{}", io::Error::last_os_error());
    let mut holder_command = Command::new(&executable);
    holder_command
        .args(["--exact", TEST_NAME])
        .env(PHASE, "hold");
    let mut holder = full_job.spawn_contained(&mut holder_command)?;

    for (case, job, expected_containment) in [
        (
            "creation failure",
            Err(io::Error::other("forced creation failure")),
            false,
        ),
        ("assignment failure", Ok(full_job), false),
        ("contained", JobObject::create(), true),
    ] {
        let mut command = Command::new(&executable);
        command
            .args(["--exact", TEST_NAME, "--nocapture"])
            .env(PHASE, "probe")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let (child, job) = JobObject::spawn_background_with_job(&mut command, job)?;
        assert_eq!(job.is_some(), expected_containment, "{case}");
        let output =
            tokio::time::timeout(Duration::from_secs(/*secs*/ 10), child.wait_with_output())
                .await??;
        assert!(output.status.success(), "{case}: {output:?}");
        assert!(String::from_utf8_lossy(&output.stdout).contains("background probe completed"));
    }
    tokio::time::timeout(Duration::from_secs(/*secs*/ 10), holder.wait()).await??;
    Ok(())
}
