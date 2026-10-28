# Background terminal contract

Source: delegated implementation task and ../cli-features-design-20261028/DESIGN-r04.ko.md (F6/r03 background sections).

- F6 and /background request early return of the exact current thread unified-exec startup collector. Ctrl+B remains editor movement.
- Preserve one process, PID/PTY/environment/output and the existing response/process store. Never reexecute, kill, or replace the PTY for yielding.
- Thread/call/process ownership must match. A queued request from an earlier widget/backend-reconnect generation must send zero RPCs.
- No target, multiple targets, approval before spawn, hooks/one-shot execution, stale owners, and unsupported backends must not report a successful conversion or use cancellation fallback.
- Yield/exit/cancel/deadline keep one existing response path. A request ACK is not turn completion or slot release.
- Existing /ps, write_stdin, stop, output limits and process cleanup remain the lifecycle owners.
- Existing custom key bindings must not be overwritten. The new action participates in normal chord/main/editor conflict validation.
- Native build is independently running. Full Cargo/ThinLTO work requires resource coordination; no install/push/global changes are authorized for this branch.

Verification obligations: real spawn/PID/PTY/environment continuity and stop; output already drained before early yield; both event orders for exit/cancellation/deadline; duplicate/stale ownership; Ctrl+B/F6/remap conflicts; unsupported backend and actual Windows terminal keys.
