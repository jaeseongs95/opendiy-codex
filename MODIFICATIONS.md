# r22 local modifications

OpenDIY Codex is an independent project. The upstream Apache-2.0 license and copyright notices are retained; upstream LICENSE and NOTICE are also preserved under `upstream/`.

The official source baseline is `6905ecede7816e91373e9eabda3f6b0368fc747b`. Local r22 changes are frozen at `f74c7574ae39b95550d6b445f828ba1010787f74`. The exact original change is `upstream/r22.patch`; its SHA-256 and the 23 changed files are listed in `SOURCE_BASELINE.json`.

The r22 change set covers MCP read lifetime/cancellation handling, Windows stdio and child/Job lifetime handling, related regression tests, and dependency lock updates. This description identifies the source change set and does not claim that a public release or all runtime behavior has been validated.

On 2026-10-26, a prominent one-line OpenDIY Codex modification notice was added to each of these 23 files using the file format's comment syntax (`//` for Rust and `#` for Cargo TOML/lock files). Existing copyright and license text was retained. Apart from this notice prefix, published source bytes equal the frozen Git blob. No uncommitted r23 source changes were imported.

`sourceSha256` identifies the frozen Git blob bytes; `publishedSha256` includes the notice prefix. `builtSourceSha256` separately identifies the recorded r22 Windows build input, which uses CRLF in 19 of the files. These hashes intentionally describe different byte representations. The published tree is not presented as byte-identical to the old build input or as a reproduced binary.

Unmodified third-party source remains under its existing license. `upstream/NOTICE` includes the original Ratatui attribution. The vendored bubblewrap license and copyright notices, including its `COPYING` text, are preserved unchanged.
