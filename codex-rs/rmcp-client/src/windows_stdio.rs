// Modified for OpenDIY Codex R31 (2026-11-01); see MODIFICATIONS.md.
//! IOCP-backed MCP output pipes. Quiet servers must not occupy Tokio's filesystem pool.

use std::fs::File;
use std::fs::OpenOptions;
use std::io;

use futures::FutureExt;
use tokio::net::windows::named_pipe::NamedPipeServer;
use tokio::net::windows::named_pipe::ServerOptions;

pub(super) fn output_pipe() -> io::Result<(NamedPipeServer, File)> {
    // Reuse the existing OS-seeded OAuth randomness without creating any filesystem entry.
    // Keep this unpredictable pipe name private until the first-instance server owns it.
    // Only the parent's read end is overlapped; child stdio stays synchronous.
    let name = format!(
        r"\\.\pipe\codex-mcp-stdio-{}",
        oauth2::CsrfToken::new_random_len(/*num_bytes*/ 24).secret(),
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
