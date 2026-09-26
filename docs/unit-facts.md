# Unit facts (verified)

Facts the code depends on, with how and when they were verified.

| Fact | Value | Verified |
|---|---|---|
| Toolchain rustc | rustc 1.99.0-nightly (87e5904f5 2026-07-20) | Task 1, 2026-09-26 |
| All deps cross-compile (anyhow, flate2, semver, serde, serde_json, sha2, toml) | yes | Task 1, 2026-09-26 |
| QNX 6.5 libgcc lacks `_Unwind_FindEnclosingFunction` (std backtrace via anyhow) | mibpkg defines it for `target_os = "nto"` in `src/main.rs` | Task 1, 2026-09-26 (link error, then clean link) |
| Smoke binary runs on the unit | pending (car asleep) | Task 19 |

## Pending checks for Task 19 (added by the final review, 2026-09-26)

- GEM script environment: PATH, user, and whether `mount -uw /mnt/app` works from a button.
- Whether GEM can run two button scripts at once, and whether it kills a script's process
  group when it ends (would take a supervisor started by a GEM action with it).
- `df -k /mnt/app` output format (4th column = available?).
- `std::env::current_exe` on QNX 6.5 (`/proc/self/exefile`).
- Replacing `/tmp/mibpkg` (rename) while the old binary is running from /dev/shmem.
- `mount -ur /mnt/app` while another process has files open for writing.
- GEM parent name `Main`, and whether label-only `keyValue` rows render.
- carmqtt LWT / `Value::Lost` behaviour across supervisor restarts.
