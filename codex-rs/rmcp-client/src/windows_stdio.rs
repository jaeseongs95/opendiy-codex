// Modified for OpenDIY Codex r22 (2026-10-26); see MODIFICATIONS.md.
//! IOCP-backed MCP output pipes. Quiet servers must not occupy Tokio's filesystem pool.

use std::fs::File;
use std::fs::OpenOptions;
use std::io;

use futures::FutureExt;
use tokio::net::windows::named_pipe::NamedPipeServer;
use tokio::net::windows::named_pipe::ServerOptions;

pub(super) fn output_pipe() -> io::Result<(NamedPipeServer, File)> {
    // Reserve a private unpredictable name before creating the single-instance pipe.
    // Only the parent's read end is overlapped; ordinary child stdio stays synchronous.
    let unique = tempfile::Builder::new()
        .prefix("codex-mcp-stdio-")
        .rand_bytes(24)
        .tempdir()?;
    let name = format!(
        r"\\.\pipe\{}",
        unique
            .path()
            .file_name()
            .ok_or_else(|| io::Error::other("missing pipe name"))?
            .to_string_lossy()
    );
    let reader = ServerOptions::new()
        .access_inbound(true)
        .access_outbound(false)
        .first_pipe_instance(true)
        .max_instances(1)
        .reject_remote_clients(true)
        .create(&name)?;
    // Connect our exact child endpoint before publishing the server or spawning a child.
    // A collision/competing connection fails closed rather than accepting another client.
    let writer = OpenOptions::new().write(true).open(&name)?;
    reader
        .connect()
        .now_or_never()
        .ok_or_else(|| io::Error::other("MCP output pipe was not connected synchronously"))??;
    Ok((reader, writer))
}

#[cfg(test)]
#[path = "windows_stdio_tests.rs"]
mod tests;
