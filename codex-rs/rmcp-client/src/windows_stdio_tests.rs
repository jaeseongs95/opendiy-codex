// Modified for OpenDIY Codex R29 (2026-10-30); see MODIFICATIONS.md.
use super::output_pipe;
use std::io::Write;
use std::time::Duration;
use tokio::io::AsyncReadExt;

#[tokio::test]
async fn output_pipe_preserves_bytes_and_closes_with_its_writer() -> anyhow::Result<()> {
    let (mut reader, mut writer) = output_pipe()?;
    writer.write_all(b"MCP\r\n\0bytes")?;
    drop(writer);
    let mut bytes = [0; 11];
    tokio::time::timeout(Duration::from_secs(2), reader.read_exact(&mut bytes)).await??;
    assert_eq!(&bytes, b"MCP\r\n\0bytes");
    let end = tokio::time::timeout(Duration::from_secs(2), reader.read_u8()).await?;
    assert!(
        matches!(end, Err(ref error) if matches!(error.kind(), std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::BrokenPipe))
    );
    Ok(())
}

#[test]
fn quiet_output_pipe_does_not_reserve_a_filesystem_worker() -> anyhow::Result<()> {
    let fixture = tempfile::NamedTempFile::new()?;
    std::fs::write(fixture.path(), b"fresh-config")?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(1)
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let (mut reader, writer) = output_pipe()?;
        let pending = tokio::spawn(async move { reader.read_u8().await });
        tokio::task::yield_now().await;
        let data =
            tokio::time::timeout(Duration::from_secs(2), tokio::fs::read(fixture.path())).await??;
        assert_eq!(data, b"fresh-config");
        // Dropping the peer, not an unrelated process, completes the outstanding read.
        drop(writer);
        assert!(
            tokio::time::timeout(Duration::from_secs(2), pending)
                .await??
                .is_err()
        );
        Ok::<_, anyhow::Error>(())
    })
}

#[tokio::test]
async fn canceling_an_output_read_does_not_pin_a_background_reader() -> anyhow::Result<()> {
    let (mut reader, mut writer) = output_pipe()?;
    assert!(
        tokio::time::timeout(Duration::from_millis(20), reader.read_u8())
            .await
            .is_err()
    );
    writer.write_all(b"x")?;
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), reader.read_u8()).await??,
        b'x'
    );
    drop(reader);
    // Mio requests CancelIoEx on drop. Its IOCP completion releases the last
    // handle reference asynchronously, so the first small write may still work.
    // Keep total writes below the pipe buffer, and bound the time to peer close.
    for _ in 0..100 {
        if writer.write_all(b"x").is_err() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    anyhow::bail!("dropped output reader did not release its peer within one second")
}

#[test]
fn output_pipe_works_without_a_temporary_directory() -> anyhow::Result<()> {
    const CHILD: &str = "CODEX_TEST_PIPE_WITHOUT_TEMP_DIRECTORY";
    if std::env::var_os(CHILD).is_some() {
        let runtime = tokio::runtime::Runtime::new()?;
        return runtime.block_on(async {
            let (mut reader, mut writer) = output_pipe()?;
            writer.write_all(b"x")?;
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(2), reader.read_u8()).await??,
                b'x'
            );
            Ok(())
        });
    }
    let fixture = tempfile::tempdir()?;
    let blocked_temp = fixture.path().join("temp-path-is-a-file");
    std::fs::write(&blocked_temp, b"not a directory")?;
    let output = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "windows_stdio::tests::output_pipe_works_without_a_temporary_directory",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env("TEMP", &blocked_temp)
        .env("TMP", &blocked_temp)
        .output()?;
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    assert_eq!(std::fs::read(&blocked_temp)?, b"not a directory");
    assert_eq!(std::fs::read_dir(fixture.path())?.count(), 1);
    Ok(())
}
