# Unit facts (verified)

Facts the code depends on, with how and when they were verified.

| Fact | Value | Verified |
|---|---|---|
| Toolchain rustc | rustc 1.99.0-nightly (87e5904f5 2026-07-20) | Task 1, 2026-09-26 |
| All deps cross-compile (anyhow, flate2, semver, serde, serde_json, sha2, toml) | yes | Task 1, 2026-09-26 |
| QNX 6.5 libgcc lacks `_Unwind_FindEnclosingFunction` (std backtrace via anyhow) | mibpkg defines it for `target_os = "nto"` in `src/main.rs` | Task 1, 2026-09-26 (link error, then clean link) |
| Smoke binary runs on the unit | pending (car asleep) | Task 19 |
