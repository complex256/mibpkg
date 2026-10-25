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
| Live GEM status (0.2.1) | supervisor/service/result values reach persistence (`pc` ~0.17 s, one 5 s stall seen); probe rows with big keys and either indentation update live; the real carmqtt sub-screen did not update after an action had rewritten its .esd while open, then worked after re-entering GEM. 0.2.2 stops rewriting unchanged files — confirm on the next visit | 2026-09-28 |
| Live GEM status (0.2.2) | CONFIRMED: after updating to 0.2.2, tapping Restart on mibpkg_carmqtt updates the status and last-result rows live on that sub-screen. Root cause of the 0.2.1 miss: every action rewrote all .esd files, and GEM stops polling an open screen whose file changed | 2026-09-30 |

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

## Online updates on the unit (2026-10-02, mibpkg 0.3.0/0.3.1, MHI2Q over SIM data)
- SNTP (UDP 123) to time.cloudflare.com and pool.ntp.org answers; offset ~1.79e9 s (clock reads uptime).
- ed25519-dalek verification runs (ed25519spike: valid=true tampered=false).
- libcurl.so.2 via dlopen with RTLD_GLOBAL preloads (libz/libcrypto/libssl/libcares) works without
  LD_LIBRARY_PATH; github.com releases/latest/download/index.json fetched through the redirect to
  release-assets.githubusercontent.com with our CA bundle and the SNTP-based verify time.
- A unit upgraded from 0.2.2 has no bin/ca.pem; the fetcher used pkgs/mibpkg/0.3.0/bin/ca.pem.
- The supervisor's background check ran 2 s after it started (internet = default route via ecm0).
- End to end: GEM "Check for updates" -> "Update to 0.3.1" downloaded from GitHub and self-updated.
- GitHub's releases/latest redirect lagged ~20 s after publishing a new release.
- The car's SD card is mounted read-only (write collections on the Mac).

## Web interface facts (2026-10-05, MHI2Q, mibpkg 0.3.1 installed)
| Fact | Value | Verified |
|---|---|---|
| Interfaces (`ifconfig`, no args, lists all; fixture `ifconfig-all-unit.txt`) | uap0 10.255.1.1 = car Wi-Fi hotspot; mmx0 10.255.0.15 = internal vehicle network (many services listen on it); ecm0 = SIM data; tun9 100.64.0.55 = Tailscale; mlan0/ecm1 no address | ssh, 2026-10-05 |
| Hotspot rule | only `uap*` counts as hotspot (mmx0 is private too and must never be) | test `on_the_real_unit_only_the_hotspot_and_tailscale_are_classed` |
| pf variants | the connectivity manager loads `pf.<uplink>.conf` (`pf.ecm0.conf` with SIM data up) after boot; `pf.conf`, `pf.ecm0.conf`, `pf.mlan0.conf`, `pf.ppp0.conf` all carry the same hotspot block line, so mibpkg edits every one present (a rule only in pf.conf never went live) | `pfctl -sr` after a reboot, 2026-10-05 |
| Port 80 | free; 127.0.0.1:8080 is already used by a stock process | `netstat -an`, 2026-10-05 |
| `/dev/urandom` | present | `ls -l`, 2026-10-05 |
| Hotspot firewall | stock pf (`/etc/pf.conf` = `/mnt/system/etc/pf.conf`, same inode) drops everything to the car on uap0 except DNS, ssh (M.I.B. block), 25010 EXLAP, 49152+ audio; port 80 needs a pass rule before `block in quick on $wlan_if from any to ($wlan_if)` | `pfctl -sr` (`/mnt/app/armle/sbin/pfctl`), 2026-10-05 |
| **Never `pfctl -f` at runtime** | reloading pf.conf resets anchors `mmi_data`, `dns_data`, `wlan_data` to `block return-icmp quick all` (`/etc/pf.data.default`); the connectivity manager had filled them in, so the unit and hotspot clients lost the internet ("sendto: No route to host"). mibpkg edits pf.conf on flash only (effective next boot) | broke and diagnosed on the unit, 2026-10-05 |
| Persistence store (live GEM values) | `/mnt/persist/var/fw/persistence.sqlite` (104 KB, qnx6, 8 MB partition on the eMMC). A `pc` write updates it in place (same inode and size; a rollback journal comes and goes), so one write is a few pages. The stock software did not touch it in 17 min after boot: mibpkg is likely its main writer, hence short values written only on change (0.5.1). `pc s:<ns>:<key>` with no value prints the stored value | probe write + read, 2026-10-07 |
| Self-update race (fixed 0.5.2) | the `gem` child that installed a new mibpkg ran its "make sure a supervisor runs" step in the gap between the old supervisor exiting and launch.sh starting the new one, starting a supervisor from its own OLD binary; launch.sh's new one then exited ("already running"). The web server still reported the new version (started fresh from /tmp/mibpkg). Now a fresh `/tmp/mibpkg.selfupdate.pending` defers that step to launch.sh, and `mibpkg status` shows the supervisor's own version | 0.5.0 -> 0.5.1 on the unit, 2026-10-07 |
| Job output via a /tmp file | a reader following a growing /tmp (shmem) file written by another process lost the first 5-6 bytes (also with reopen + absolute seek); job output uses pipes instead | web job logs, 2026-10-05 |

## Sources and publishing (2026-10-14, MHI2Q, mibpkg 0.5.3 -> 0.6.0)
| Fact | Value | Verified |
|---|---|---|
| Online self-update | web Updates -> mibpkg 0.5.3 -> 0.6.0 from release v3 of complex256/mibpkg-packages; `mibpkg list` shows `mibpkg 0.6.0 [complex256-mibpkg]` | web UI + ssh, 2026-10-14 |
| Web sessions after self-update | the restart drops every web login (sessions live in memory); the PIN is asked again | web UI, 2026-10-14 |
| Community list | Sources screen shows the list fetched with the right wall-clock time; the official source is listed under Enabled ("built in", 2 packages) | web UI, 2026-10-14 |
| Card sources.toml | `mibpkg/sources.toml` entry "card test" appears under Available with the SD card label right after the update (no Rescan needed) | web UI, 2026-10-14 |
| Package moved | carmqtt installed from `complex256` with the card offering it only under `complex256-carmqtt`: Updates row "0.2.2 -> 0.2.3 · now from …", package button "Update to 0.2.3 from complex256-carmqtt"; the install said "now from collection complex256-carmqtt (was complex256)"; `mibpkg list` then shows `carmqtt 0.2.3 [complex256-carmqtt]`; carmqtt restarted and connected to MQTT on the first attempt | web UI + ssh + /tmp/carmqtt.log, 2026-10-14 |
| carmqtt log | carmqtt writes its own `/tmp/carmqtt.log` (config `[log] file`); `/tmp/mibpkg.carmqtt.carmqtt.log` (its stdout/stderr) stays empty | ssh, 2026-10-14 |

Screenshots (dark mode, 390x844): docs/screenshots/.

## Pending on-car checks (web interface, mibpkg 0.4.0)
- A phone on the hotspot reaches `http://10.255.1.1/`.
- `mibpkg web` idle and busy memory/CPU.
- Phone flow: upload -> install -> logs -> config edit -> restart -> roll back -> remove.
- mibpkg self-update from the web UI: the page shows "restarting" and reloads on the new version.
- A web action while GEM runs one (and the reverse): both show busy, nothing breaks.
- DISABLE card: the web server does not start either.
- Tailscale: the page loads from a laptop on the tailnet.

## Standalone SWDL installer (2026-10-14)
mibpkg ships its own SWDL card: `metainfo2.txt` (skip* flags, FinalScript hook) + thin hashed
`shell/install.sh` → lands payload → `mibpkg bootstrap` (C backbone) → `enable-gem` + `gem-patch
apply` (B2, best-effort). New commands: `mibpkg enable-gem|disable-gem`, `mibpkg gem-patch
<apply|status|revert> --from DIR`, host `mibpkg gem-patch build --stock --patched`, host `mibpkg
swdl-card <out> --payload DIR [--seed DIR] --version V`, and `mibpkg verify <card-dir>`.
- `enable-gem` writes persistence key 3221356557 ns0 = "1" then commit key 1 ns0 = "0" via stock
  `pc` (path a). UNVERIFIED on car — spike S1/S2 pending: does `pc`'s string set reach the
  byte-typed block-5F key, or is an own writer (path b) needed? Does a from-scratch metainfo2 get
  listed/run with no MIBonk files, and is FinalScriptChecksum enforced?
- Seed collection lands at `mibpkg/collections/seed/` (media::scan convention), installed once on
  first boot (marker `state/seeded`).
- CI smoke-gate to wire at go-public: `mibpkg swdl-card … && mibpkg verify out/card`. No dormant
  test workflow exists today (only host-release.yml); not added to the live release plumbing.

## MIB SDK packages (mibpkg 0.9.0)
| Fact | Value | Verified |
|---|---|---|
| HMI build | the first `HMI = ...` line of `/ifs/version_info.txt`, whitespace removed; supported `HMI_AU_MIB2_HQC_NAR_G22_C10_180803` | bench e2e |
| SDK install root | `/mnt/app/eso/hmi/lsd/mibsdk/`: `sdk.properties`, `grants.properties`, `SAFE_MODE`, `permissions.json`, `plugins/<id>.jar`, `plugins/<id>.reload` | bench e2e |
| SDK jars | `/mnt/app/eso/hmi/lsd/jars/mibsdk-{runtime,adapter-hqc,rt-retrotranslator}.jar` | bench e2e |
| Persist root | `/mnt/persist/mibsdk/`: `boot.state`, `ledger.properties`, `plugins.properties`, `statusline.properties`, `data/<id>.properties`; writable without a remount | bench e2e |
| `plugins.properties` | user on/off, one `<id>.off=true` line per switched-off plugin; mibpkg drops the line on remove | bench e2e |
| `statusline.properties` | user status-line choices: `order=<id>,<id>,...` and one `<id>.hidden=true` line per hidden item; mibpkg drops the plugin's hidden line and its `order` entry on remove (the `order` line goes when empty); written like `plugins.properties` (`.tmp` then rename) | runtime spec |
| `grants.properties` | `<id>=<perm>,<perm>`; mibpkg rewrites only the exact-key line, all others byte for byte | bench e2e |
| Ledger v2 | `<key>=<pluginId>\|<scope>\|<prior>\|<active>\|<A or R>` (`A` held, `R` restoring); a 4-field line is legacy, counts as held; read from the persist file and the old install-root file (and its `.tmp`) | bench e2e |
| rw holder protocol | lock `/tmp/mibpkg.rwlock` (`create_new`, pid inside); holders `/tmp/mibpkg.rw.<app or system>.<pid>`; marker `mibpkg.rw.<tag>.withdrawn` is never a holder; remount read-only only when no live holder remains | bench e2e |
| `/mnt/app` remounted read-only by others | any process may do so at any time (F8): re-assert `mount -uw` before each write, verify by read-back; a 0.7 supervisor ignores holders | bench e2e F8 |
| `/tmp` | `/dev/shmem`: files only, no directories | bench e2e |
| `HMISystem=` takeover | `/mnt/app/eso/hmi/lsd/bundles_dsitracer_enabled.properties` line `HMISystem=de.audi.tghu.system.hmi.Activator` (CRLF) is rewritten to `HMISystem=mibsdk.adapter.hqc.ui.SdkSystemActivator`, its CR kept; only that line changes (on the bench's 10171-byte file, line 62), and remove restores the stock file byte for byte. Managed only when `[sdk]` sets `hmisystem_stock`/`hmisystem`; without them the line stays stock | bench e2e (menu-icons) |
| Plugin icons | `/mnt/app/eso/hmi/lsd/mibsdk/icons/<plugin-id>/`: one `<sha16>.png` per icon (first 16 lowercase hex characters of the file's SHA-256) plus `index.properties` (`<name>=<sha16>,<w>,<h>`, sorted by name, LF); removed with the plugin | bench e2e (menu-icons) |
| SDK built-in icons | `/mnt/app/eso/hmi/lsd/mibsdk/icons-sdk/`: the same layout, 24 PNGs (8 names x `-20`/`-72`/`-72r`) plus `index.properties`; removed with the SDK, together with the then empty `icons/` | bench e2e (menu-icons) |
