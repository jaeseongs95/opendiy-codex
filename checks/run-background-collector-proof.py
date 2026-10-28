"""Compile exact production collector functions against the frozen build graph.

No Cargo resolution, shared target writes, production installation, or copied logic.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
NATIVE = ROOT.parent / "official-cli-patch-20261028"
OUT = ROOT / "checks"


def sha(value):
    return hashlib.sha256(value).hexdigest()


def read(path):
    return path.read_bytes().decode("utf-8").replace("\r\n", "\n")


def between(value, start, end):
    return value[value.index(start):value.index(end, value.index(start))]


manager = read(ROOT / "codex-rs/core/src/unified_exec/process_manager.rs")
process = read(ROOT / "codex-rs/core/src/unified_exec/process.rs")
tests = read(ROOT / "codex-rs/core/src/unified_exec/process_manager_tests.rs")
module = read(ROOT / "codex-rs/core/src/unified_exec/mod.rs")
collector = between(manager, "    pub(super) async fn collect_output_until_deadline<", "    fn prune_processes_if_needed")
buffers = between(process, "#[derive(Default)]\npub(crate) struct OutputBuffers", "struct OutputTaskGuard")
policy = between(process, "#[derive(Clone, Copy, PartialEq, Eq)]\npub(super) enum OutputDrainPolicy", "/// Transport-specific process handle")
selected_tests = between(tests, "#[tokio::test]\nasync fn output_collection_early_yield", '#[cfg(any(unix, windows))]\n#[tokio::test(flavor = "multi_thread", worker_threads = 2)]\nasync fn background_terminal_real_startup')
marker_fn = between(module, "pub(crate) fn format_output_omission_marker", "pub(crate) fn generate_chunk_id")
head_tail = between(read(ROOT / "codex-rs/core/src/unified_exec/head_tail_buffer.rs"), "use crate::unified_exec::UNIFIED_EXEC_OUTPUT_MAX_BYTES;", "#[cfg(test)]\n#[path =")

# Reuse the completed native build graph, verifying selected library hashes before linking.
graph_path = NATIVE / "compiled-graph-v1.json"
graph = json.loads(graph_path.read_text())
assert graph["sourceCommit"] == "7673c44501be6b9172b662c67ac2e319a381e493"
names = ("tokio", "tokio_util", "tracing")
libraries = {name: graph["allTargetLibraries"][name] for name in names}
for name, path in libraries.items():
    assert Path(path).exists(), path
    if name in graph["selected"]:
        assert sha(Path(path).read_bytes()) == graph["selected"][name]["sha256"], name

env = os.environ.copy()
env.update(json.loads(subprocess.check_output(["pwsh", "-NoProfile", "-File", str(NATIVE / "export-build-env.ps1")])))
env["CARGO_HOME"] = str(OUT / "unused-cargo-home")
env["CARGO_TARGET_DIR"] = str(OUT / "unused-target")

for label in ("green", "red"):
    destination = OUT / ("collector-proof-" + label)
    destination.mkdir(exist_ok=True)
    body = collector
    if label == "red":
        # Remove only early-yield observation. The same committed test must fail.
        block = between(body, "            if early_yield.is_some_and(CancellationToken::is_cancelled) {", "            if !has_drained_output {")
        body = body.replace(block, "")
        block = between(body, "                    _ = async {\n                        match early_yield {", "                    _ = tokio::time::sleep(remaining)")
        body = body.replace(block, "")
    source = '''#![allow(dead_code)]
mod unified_exec {
    pub(crate) const UNIFIED_EXEC_OUTPUT_MAX_BYTES: usize = 1024 * 1024;
''' + marker_fn + "    mod head_tail_buffer {\n" + head_tail + "\n}\n" + '''
    mod process {
        use std::sync::Arc;
        use std::sync::atomic::AtomicBool;
        use tokio::sync::{Mutex, Notify};
        use tokio_util::sync::CancellationToken;
        use super::UNIFIED_EXEC_OUTPUT_MAX_BYTES;
        use super::head_tail_buffer::HeadTailBuffer;
''' + buffers + policy + '''
    }
    struct UnifiedExecProcessManager;
    mod process_manager {
        use super::UnifiedExecProcessManager;
        use super::head_tail_buffer::HeadTailBuffer;
        use super::process::{OutputBuffers, OutputHandles, OutputDrainPolicy};
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        use tokio::sync::watch;
        use tokio::time::{Duration, Instant};
        use tokio_util::sync::CancellationToken;
        impl UnifiedExecProcessManager {
''' + body + '''
        }
        #[cfg(test)] mod tests {
            use super::*;
            use tokio::sync::Notify;
''' + selected_tests + "\n        }\n    }\n}\n"
    probe = destination / "proof.rs"
    probe.write_text(source, encoding="utf-8", newline="\n")
    exe = destination / "proof.exe"
    args = [env["RUSTC"], str(probe), "--crate-name", "background_collector_proof", "--test", "--edition=2024", "--target", "x86_64-pc-windows-msvc", "-C", "opt-level=0", "-C", "debuginfo=0", "-C", "target-feature=+crt-static", "-C", "linker=" + env["CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER"], "-L", "dependency=" + str(NATIVE / "target/x86_64-pc-windows-msvc/release/deps"), "-L", "dependency=" + str(NATIVE / "target/release/deps"), "-o", str(exe)]
    for name, path in libraries.items():
        args.extend(["--extern", name + "=" + path])
    for path in (NATIVE / "cargo-home/registry/src/index.crates.io-1949cf8c6b5b557f").glob("windows_x86_64_msvc-*/lib"):
        args.extend(["-L", "native=" + str(path)])
    with (destination / "compile.log").open("wb") as log:
        built = subprocess.run(args, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=120)
    receipt = dict(label=label, compileExit=built.returncode, compileArgv=args, nativeGraphSha256=sha(graph_path.read_bytes()), collectorSha256=sha(collector.encode()), headTailSha256=sha(head_tail.encode()), outputDefinitionsSha256=sha(buffers.encode()), selectedTestsSha256=sha(selected_tests.encode()), harnessSha256=sha(probe.read_bytes()), libraries={name: dict(path=path, sha256=sha(Path(path).read_bytes())) for name, path in libraries.items()}, scope="Exact production collector/pause helpers and committed buffer tests; HeadTailBuffer production body extracted unchanged. Not full core/RPC/TUI or process-spawn integration.")
    if built.returncode == 0:
        with (destination / "run.log").open("wb") as log:
            run = subprocess.run([str(exe), "--nocapture"], env=env, stdout=log, stderr=subprocess.STDOUT, timeout=15)
        receipt["runExit"] = run.returncode
    (destination / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(dict(label=label, compileExit=built.returncode, runExit=receipt.get("runExit"))), flush=True)
    if built.returncode != 0:
        sys.exit(built.returncode)
    assert (receipt["runExit"] == 0) == (label == "green"), receipt
