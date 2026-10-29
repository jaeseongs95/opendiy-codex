use super::connect_local_daemon;
use codex_uds::UnixListener;
use codex_uds::UnixStream;
use std::io::ErrorKind;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn implicit_proxy_rejects_broad_directory_before_connecting() {
    let temp = tempfile::TempDir::new().unwrap();
    let path = temp.path().join("socket");
    let mut listener = UnixListener::bind(&path).await.unwrap();
    let error = match connect_local_daemon(&path).await {
        Ok(_) => panic!("broad directory was accepted"),
        Err(error) => error,
    };
    assert_eq!(
        error.downcast_ref::<std::io::Error>().unwrap().kind(),
        ErrorKind::PermissionDenied
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(200), listener.accept())
            .await
            .is_err()
    );
    // The listener is reachable: refusal came from policy, not a dead socket.
    let _raw = UnixStream::connect(&path).await.unwrap();
    let _accepted = listener.accept().await.unwrap();
}

#[tokio::test]
async fn implicit_proxy_does_not_create_or_repair_missing_directory() {
    let temp = tempfile::TempDir::new().unwrap();
    let directory = temp.path().join("missing");
    assert!(
        connect_local_daemon(&directory.join("socket"))
            .await
            .is_err()
    );
    assert!(!directory.exists());
}

#[tokio::test]
async fn implicit_proxy_authenticates_real_peer_before_transferring_bytes() {
    let temp = tempfile::TempDir::new().unwrap();
    let directory = temp.path().join("private");
    codex_uds::prepare_private_socket_directory(&directory)
        .await
        .unwrap();
    let path = directory.join("socket");
    let mut listener = UnixListener::bind(&path).await.unwrap();
    // An independent OS query determines whether this process's actual token is
    // elevated; neither the caller nor this test changes the token or its rights.
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", "([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let elevated = match String::from_utf8(output.stdout).unwrap().trim() {
        "True" => true,
        "False" => false,
        other => panic!("unexpected elevation result: {other}"),
    };
    let client = connect_local_daemon(&path).await;
    let mut server = listener.accept().await.unwrap();
    if elevated {
        let error = match client {
            Ok(_) => panic!("elevated peer was accepted"),
            Err(error) => error,
        };
        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap().kind(),
            ErrorKind::PermissionDenied
        );
        let mut byte = [0];
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), server.read(&mut byte))
                .await
                .unwrap()
                .unwrap(),
            0
        );
    } else {
        let mut client = client.unwrap();
        client.write_all(b"request").await.unwrap();
        let mut bytes = [0; 7];
        server.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"request");
        server.write_all(b"response").await.unwrap();
        let mut bytes = [0; 8];
        client.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"response");
    }
}
