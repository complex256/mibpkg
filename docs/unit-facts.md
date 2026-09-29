# Unit facts (verified)

Facts the code depends on, with how and when they were verified.

| Fact | Value | Verified |
|---|---|---|
| Toolchain rustc | rustc 1.99.0-nightly (87e5904f5 2026-07-20) | Task 1, 2026-09-26 |
| All deps cross-compile (anyhow, flate2, semver, serde, serde_json, sha2, toml) | yes | Task 1, 2026-09-26 |
| QNX 6.5 libgcc lacks `_Unwind_FindEnclosingFunction` (std backtrace via anyhow) | mibpkg defines it for `target_os = "nto"` in `src/main.rs` | Task 1, 2026-09-26 (link error, then clean link) |
| Binary runs on the unit | `mibpkg list` from /tmp: unit MHI2Q, reads /fs/sda0 collection (carmqtt, mibpkg 0.1.0) | Task 19, 2026-09-26 |
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
| `File::try_clone` | fails EINVAL (`F_DUPFD_CLOEXEC` unsupported) — open the file twice instead | Task 20, 2026-09-26 |
| fsync on `/tmp` (/dev/shmem) | ENOSYS — treated as success (`fsutil::sync_result`) | Task 20, 2026-09-26 |
| rename in `/tmp` (/dev/shmem) | EXDEV even within one dir — copy+remove fallback (`fsutil::rename`); `mv` does this itself | Task 20, 2026-09-26 |
| Background supervisor from an ssh action | survives the ssh session closing (`trap '' HUP` wrapper) | Task 20, 2026-09-26 |
| Clock | unset at boot (files dated Jan 01 00:0x) | Task 20, 2026-09-26 |
| GEM screen refresh | GEM caches loaded .esd screens while open; rewritten files show only after leaving and re-entering GEM (no reboot). Console panel is the only live view | Task 20, 2026-09-26 |
| First real install | GEM install of carmqtt 0.1.0: migrated hand install, service running under supervisor, old hook removed | Task 20, 2026-09-26 |
| DISABLE card | `/fs/sda0/mibpkg/DISABLE` present at boot: launch.sh logs it and starts nothing (no supervisor, no carmqtt); tailscale unaffected | Task 20, 2026-09-26 |
| DISABLE removed | next boot: supervisor + carmqtt autostart again | Task 20, 2026-09-26 |
| Update path | GEM Rescan (new card) -> re-enter GEM -> "Update to 0.1.1": carmqtt 0.1.0 -> 0.1.1, supervisor restarted it on the new version, 0.1.0 kept for rollback | Task 20, 2026-09-26 |
| Boot autostart | after a real reboot: supervisor + carmqtt up at ~49 s after boot (30 s hook sleep + launch wait); no DISABLE logged | Task 20, 2026-09-26 |
| Network readiness at boot | Tailscale route to the HA broker not usable until ~110-170 s after boot; `/tmp/ppp_connected` exists when SIM data is up | Task 20, 2026-09-26 |
| Shell tools | also missing: `tail`, `wc`, `grep -A/-B`; `netstat` is `/mnt/app/armle/usr/bin/netstat` (not on ssh PATH) | Task 20, 2026-09-26 |
| `netstat -rn` (Tailscale up) | BSD format; `default` via `ecm0`; Tailscale adds per-peer `/32` routes, `100.100.100.100/32` and subnet `192.168/16` via `tun9` — **no `100.64/10` route**. `tailnet` default is therefore `route:100.100.100.100@tun` | Task 6, 2026-09-28 (fixture `netstat-rn-unit-tailnet-up.txt`) |
| `ifconfig <missing>` | `SIOCGIFFLAGS nosuch0: No such device or address`, exit 1 → treated as not up | Task 6, 2026-09-28 |
| HMI markers | `/tmp/smFlagStartupDone`, `/tmp/startupFinishedFlag`, `/tmp/MMX-startup-finished` all exist and predate `/tmp/mibpkg.pid` (1 s mtime resolution, tied): `hmi-ready` is already true at the default boot hook; default stays `smFlagStartupDone` | Task 6, 2026-09-28 |
| Network timing at boot | `/tmp/ppp_connected` (SIM data) and `/tmp/tailscaled.pid` are newer than `/tmp/mibpkg.pid`: internet and tailnet come up after the supervisor starts | Task 6, 2026-09-28 |
| Self-update (0.1.x → 0.2.0) | GEM Update on mibpkg: new binaries in bin/ and /tmp, new supervisor pid, carmqtt restarted under it; the old binary ran the update, so no step lines that time | Task 7, 2026-09-28 |
| Start conditions on a real boot | carmqtt 0.1.2 (`after = ["tailnet"]`): started at 114 s (supervisor up ~49 s), first MQTT connect succeeded at 115 s (vs. 172 s after minutes of retries before) | Task 7, 2026-09-28 |
| GEM Console step lines | shown for the carmqtt 0.1.2 update (mibpkg 0.2.0) | Task 7, 2026-09-28 |
| Boot timeline (no hook sleep) | supervisor 19 s; `MMX-startup-finished`/`startupFinishedFlag`/`smFlagStartupDone` 38 s; `tailscaled.sock` 50 s; `ppp_connected`/`net.external` 55 s; tailnet route ~114 s; first broker packet a few s after the route (carmqtt connected 120 s) | 2026-09-28 (mibpkg.log markers) |
| Persistence client `pc` | `/eso/bin/apps/pc s:<ns>:<key> ["value"]`; needs `IPL_CONFIG_DIR=/etc/eso/production` (else aborts + writes a core dump to /mnt/ota/system/core) | 2026-09-28 |
| Persistence namespaces | /etc/eso/production/persistence.json maps ns 0-37 and listed ids to RCC (unknown keys → TIMEOUT); any unlisted ns falls to the SQLite default backend (/mnt/persist/var/fw/persistence.sqlite) which accepts new keys. mibpkg probe ns 1835626603 (0x6d69706b "mipk") key 1 | 2026-09-28 |
| GEM live values | `keyValue value String per <ns> <key> poll 1000` re-reads while the screen is open: probe stepped 1→2→3→4 live, no re-entry | 2026-09-28 |

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
