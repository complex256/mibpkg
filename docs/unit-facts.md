# Unit facts (verified)

Facts the code depends on, with how and when they were verified.

| Fact | Value | Verified |
|---|---|---|
| Toolchain rustc | rustc 1.99.0-nightly (87e5904f5 2026-07-20) | Task 1, 2026-09-26 |
| All deps cross-compile (anyhow, flate2, semver, serde, serde_json, sha2, toml) | yes | Task 1, 2026-09-26 |
| QNX 6.5 libgcc lacks `_Unwind_FindEnclosingFunction` (std backtrace via anyhow) | mibpkg defines it for `target_os = "nto"` in `src/main.rs` | Task 1, 2026-09-26 (link error, then clean link) |
| Smoke binary runs on the unit | pending | Task 19 |
| `/mnt/app` free space | 233346 KB (~228 MB) free of 995312 KB (77% used) | Task 19, 2026-09-26 (`df -k`) |
| `/mnt/system` free space | 3023 KB free | Task 19, 2026-09-26 |
| `df -k` format | `<dev> <1k-blocks> <used> <avail> <cap%> <mount>`: 4th field = available (matches `free_bytes`) | Task 19, 2026-09-26 |
| `/mnt/app`, `/mnt/system` filesystem | qnx6 (atomic rename) | Task 19, 2026-09-26 (`mount`) |
| SD cards | `/fs/sda0` (M.I.B. card), `/fs/sdb0` (music) — both FAT32, **mounted read-only**; both in default media roots | Task 19, 2026-09-26 |
| Unit identity | img_ver.txt line 1 `#DEV_MMX2QC_AU_NAR_G22_061PROD` → MHI2Q; `/dev/nvsku/project` = 453230002 | Task 19, 2026-09-26 |
| Tool paths | `/bin/ksh` (`/bin/sh` → ksh), `/bin/mount`, `/bin/df`, `/bin/sync`; no `which`, `cmp`, `od` | Task 19, 2026-09-26 |
| `/proc/self/exefile` | exists (so `current_exe` has a source) | Task 19, 2026-09-26 |
| GEM button scripts | run as root (uid 0), PATH includes /bin and /sbin, can `mount -uw /mnt/app` | Task 19, 2026-09-26 (probe) |
| GEM new .esd pickup | new `screen <name> Main` file appears on re-entering GEM, no reboot | Task 19, 2026-09-26 (probe) |
| GEM script output | tapping a `script` button opens `<screen>/Console`, which shows stdout **and** stderr live | Task 19, 2026-09-26 (probe) |
| Missing tools | no `printf`, `which`, `cmp`, `od`, `uniq`, `mkfifo`; `tee` and `cat` exist | Task 19, 2026-09-26 |

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
