# MIB2 package manager — ideas & to-do backlog

Living list from brainstorming (started 2026-09-26). Design: docs/superpowers/specs/2026-09-26-mibpkg-design.md.
Tags: [v1] = in the first implementation, [later] = design must allow it, not built yet,
[?] = open question / needs investigation.

## Status (2026-09-26)
v1 shipped to main and verified end to end on the MHI2Q (see docs/unit-facts.md): M.I.B. bootstrap,
GEM install with carmqtt migration, reboot autostart, DISABLE card. Deferred review minors are in the
"Review minors" section below.

## Decisions so far (2026-09-26)
- v1 = M.I.B. bootstrap, SD install/update/remove, in-car GEM picker, multiple collections, boot .d
  folder, service supervisor, carmqtt migrated. NOT v1: online (v2), signatures (after v2), slim zip, other migrations.
- Own package manager + on-unit supervisor, written in Rust (qnx65-armv7-toolchain).
- Primary audience: M.I.B. users (no ssh needed). SSH is a power-user bonus.
- Bootstrap: M.I.B. "Run Custom Script" from SD installs the manager once.
- Offline-first (most cars offline); online update support for cars that have it.
- Separate repo, private now, intended to go public (keep names/secrets clean for that).
- Boot: manager edits startup.sh ONCE (single line → our runner); packages drop files
  into a .d folder the runner owns. No stock .d folder exists (checked P5087 dump).

## Boot integration
- [v1] Single startup.sh hook, installed/removed only by the manager, ksh -n validated, backup kept.
- [v1] Runner provides drop-in dir (e.g. boot.d/) — packages add files, never touch startup.sh.
- [v1] Detached, best-effort, never blocks boot (same pattern as tailscale/mibwg hooks).
- [designing 2026-09-26] Start conditions: spec docs/superpowers/specs/2026-09-26-start-conditions-design.md
  (per-service `after = ["internet"|"tailnet"|"hmi-ready"|file:|iface:|route:]`, opt-in `after_timeout`).
  `phase = "early"` (earlier startup.sh hook, before HMI/nav): no concrete use yet (user, 2026-09-26) —
  reserved, not built. `service:` dependencies fit the same syntax later.
- [original note] Boot phases/ordering: packages declare WHEN they start, e.g.
  - early (before HMI / before navigation starts)
  - after HMI/framework up
  - after filesystems / SD mounted
  - after network: WLAN/hotspot, Tailscale, SIM data (pppd on /dev/NAD/data_port)
  - after GPS fix / time sync
  Needs readiness signals per phase ([?] which /dev, /ramdisk/pps, or process markers to watch).
- [later] Dependencies between packages' services (carmqtt after tailscale).
  Evidence 2026-09-26 (first managed boot): carmqtt started at ~49 s, broker over Tailscale unreachable
  ("no route to host", then timeouts) until ~110-170 s; with carmqtt's 60 s backoff cap it connected at
  172 s, so HA was stale ~3 min after boot. An "after tailnet / after network" phase fixes this;
  readiness signals seen in /tmp: `ppp_connected` (SIM data), `tailscaled.sock`, `net.external`.
- [done 2026-09-26] carmqtt MQTT reconnect cap 60 s -> 10 s (carmqtt 0.1.1, installed on the car via mibpkg update).
- [?] Alternatives to editing startup.sh: /armle/scripts/common.sh is sourced by startup.sh
  (stock file, sourced by many scripts — rejected for now); mcd AUTORUN / autorunner (unknown behaviour).

## On-car layout (approved 2026-09-26)
- [v1] /mnt/app/root/mibpkg/{bin,boot.d,pkgs/<name>/<ver>,pkgs/<name>/current,state/installed.json,state/backups,collections}
- [v1] /tmp flat runtime: mibpkg, mibpkg.<pkg>.<bin>, mibpkg.<svc>.log, mibpkg.status. Logs RAM-only, size-capped.
- [v1] boot.d files carry a `phase` field now; v1 only implements "default" (late, after inetd).
- [v1] Install order: sha256 → unpack new version dir → patches (with backups) → boot.d → flip `current` last.
- [v1] Package store path (pkgs/) is a setting, not hardcoded — enables SD storage later.
- [later] Package payloads on the SD card (per-package or user choice). Must stay internal regardless:
  launcher, mibpkg, boot.d, state + stock backups, anything stock processes load (hooks .so, lsd jars),
  secret-bearing config. Needs a supervisor "waiting for card" state + start on card insert.
- [done 2026-09-26, see unit-facts.md] VERIFY ON CAR: /mnt/app free space (if tight, SD storage becomes v1); SD mount path as GEM sees it;
  QNX6 atomic rename/power-loss behaviour on /mnt/app; which SD slot M.I.B. requires (SD1).

## Package format (approved 2026-09-26)
- [v1] .mpk = tar.gz with manifest.toml + payload; mibpkg unpacks itself (no reliance on unit tar/gzip).
- [v1] Manifest: [package], [compat], [[file]], [[config]], [[service]], optional [scripts]; placeholders @pkg/@config.
- [v1] [[config]] = user-owned files: supplied on SD under /mibpkg/config/<pkg>/, copied to internal
  state/config/<pkg>/, never overwritten by updates, kept on uninstall unless purged; required config
  missing → "needs config" in UI, service not started.
- [v1] Collection = index.json (format, collection id/name, packages[name,version,file,sha256,size,compat,
  uses_scripts], reserved empty `signatures`) + packages/. Generated by host-side mibpkg build tooling.
- [later] [[patch]] (json-merge etc.) when carplay-rgi is packaged — name reserved in the format now.
  Backup/undo registry still built in v1 (mibpkg uses it for its own startup.sh line).
- [partly done: MHI2Q verified; MHI2 (non-Q) still needs a report] VERIFY: which PROJ_ID/SKU/img_ver values distinguish MHI2 vs MHI2Q; firmware version format.
- [v1] One-time migration of the hand-installed carmqtt (/mnt/app/root/carmqtt + its startup.sh line,
  backup startup.sh.pre-carmqtt) into the managed package.

## In-car UI + bootstrap (approved 2026-09-26)
- [v1] mibpkg generates its own GEM .esd screens (static format: `screen <name> <parent>`, script/keyValue):
  main screen (SD summary, Rescan, Update all) + one sub-screen per package (state, Install/Update/Remove/Restart,
  last result, "runs scripts" warning). Buttons → wrapper script → /tmp/mibpkg <action> → regenerate screens.
- [v1] Bootstrap once via M.I.B. Run Custom Script → copy mibpkg + launch.sh, `mibpkg bootstrap` adds the
  startup.sh line (ksh -n + backup) and installs the GEM screens. M.I.B. custom slot is free afterwards.
- [v1] mibpkg self-updates as a package in the official collection.
- [done 2026-09-26, see unit-facts.md] VERIFY ON CAR: are new .esd files picked up on GEM re-open or only after reboot; can a screen show live
  output during a script (progress) or only results after; exact parent menu name; avoid clashes with M.I.B. esd names.

## Runtime / supervisor
- [v1] Binaries copied flash → /tmp (RAM) before exec (flash can't exec). /tmp is flat — no subdirs.
- [v1] Supervisor: restart on crash with backoff; log rotation; per-service status shown in the car.
- [later] Stop services cleanly at shutdown (unit stops ExLAP before our services).

## Install / update
- [v1] Atomic install (stage → switch), keep previous version for rollback.
- [v1] Remount /mnt/app & /mnt/system rw only around writes; always restore ro.
- [v1] Stock-file edit registry: back up original once, restore on uninstall, detect conflicts
  between packages editing the same file (carplay-rgi: smartphone_integrator.json, dio_manager.json, lsd jars).
- [v1] Compatibility metadata: unit (MHI2 / MHI2Q), region, firmware train/version.
- [later] Dependencies between packages.
- [v1] Package model (decided 2026-09-26): declarative manifest first — files, services, boot phase,
  patches to stock files (e.g. json-merge) — so the manager can back up/undo/detect conflicts.
  Optional install/uninstall scripts allowed for odd cases; the in-car screen flags packages that use them.
- [?] Patch types needed: json-merge (smartphone_integrator.json, dio_manager.json), jar drop-in to lsd/jars,
  text line insert? Inventory from carplay-rgi custom.sh.
- [?] Script sandboxing/limits for third-party scripts (timeout, which env/paths, rw-mount handling).

## Distribution
- [v1] Offline: SD card / USB transport. Full bundle zip (everything) copied to SD; car picks.
- [later] Slimmed-down zip: PC/phone web tool where the user ticks packages and downloads a
  smaller SD bundle (user wants this — people may not want everything on the card).
- [later — v2, first after v1] Online: static index (GitHub Releases / R2) pulled when the car has network.
  Spike 2026-09-26 (spikes/README.md): pure-Rust TLS does not build for QNX 6.5 arm (ring/aws-lc via
  cc-rs: unknown arch; rustls-rustcrypto: `libc` crate fails for nto65). Unit ships libcurl 7.56.0 +
  OpenSSL 1.0.2j (TLS 1.2) — plan: dlopen libcurl.so.2 with a hand-written FFI (probe builds, 235 KB),
  ship our own CA bundle (unit bundles are old: VeriSign/GeoTrust/Equifax/GlobalSign + VW CAs),
  and only fetch once the clock is sane (unit boots at 1970; TLS validity needs real time) —
  a natural `clock-set` start condition. Alternative if TLS proves flaky: plain HTTP from our own
  host + ed25519-signed index (verification needs no randomness/libc) — pulls signatures forward.
  [done 2026-09-28] VERIFIED ON CAR: libcurl via dlopen + our CA bundle + OpenSSL verify time set via
  CURLOPT_SSL_CTX_FUNCTION/X509_VERIFY_PARAM_set_time = full HTTPS to github.com (200), cloudflare (200).
  Needs a network-time offset (SNTP in std Rust, or Tailscale's /tmp/ts_clock_offset); system clock untouched.
- [v1] SHA-256 per package in the index — integrity only (catches corrupt/truncated SD copies), not trust.
- [later, after first online pass] Signature verification (ed25519/minisign style). DEFERRED 2026-09-26:
  user unsure it's wanted long-term since offline users already vet what they copy. v1 index format
  reserves room for signatures so adding them later is not a format break.
- [v1] Multiple collections ("repos"): official complex256 collection + third-party collections the
  user adds (decided 2026-09-26). Without signing, a collection = name + where it came from.
- [?] How a user adds a third-party collection offline (collection folder on SD? confirm in GEM?).
- [v1] Same name in two collections: shown side by side with collection id; installing one while the other is
  installed is refused. [later] priority/pinning.
- [?] mcd AUTORUN on SD/USB — could offline installs/updates happen on card insert?

## UI
- [v1] In-car picker: GEM (engineering menu) screen — installed / update / install / remove, then Apply.
  (Decided 2026-09-26: offline users choose IN THE CAR from what's on the SD card.)
- [later] Web UI on the hotspot.
- [later] Live values on GEM screens: GEM caches .esd files while open (verified 2026-09-26), so status
  rows are snapshots until GEM is re-entered. Stock screens use `keyValue ... poll N` rows that read
  live values (persistence/sys) — research whether a row can poll a file or value mibpkg writes.
- [done 2026-09-26] "last result" timestamps show 1970 because the unit clock is unset at boot — omit the stamp
  when the clock is obviously unset (< 2020).
- [done 2026-09-26] Print progress lines during install/update (GEM Console shows stdout live, verified).

## Migration of existing projects
- [v1] Migrate carmqtt only (the only project with a single developer) and remove its startup.sh line
  (.pre-carmqtt backup).
- [later] tailscale, wireguard, carplay-rgi — other contributors involved; coordinate before packaging.
  carplay-rgi is the stress test for stock-file patches.

## Before going public
- [later] Scrub history for excluded names; licensing; contribution/signing model for third-party authors.

## Review minors (deferred from the v1 final review, 2026-09-26)
- [done 2026-09-27, robustness pass] all v1 review minors: fsync staged startup.sh; boot.d write-then-prune;
  repair after a power loss between `current` flip and state save; stale `.new` cleanup on every action;
  lock pid-write race; orphans waited for before new copies; stale stop/selfupdate requests ignored;
  reserved package names; collision-free GEM script names; newest *compatible* version offered;
  unknown free space reported. (Earlier: /tmp cleanup on removal, capped mibpkg.log, bootstrap registers mibpkg.)

## Start-conditions review minors (deferred, 2026-09-26)
- [done 2026-09-26] netstat/ifconfig calls limited to 3 s each (hung call -> "timed out", service keeps waiting).
- A long wait spawns netstat every second forever; consider every 5 s after the first minute.
- `[conditions]` edits in settings.toml need a supervisor restart (reload doesn't re-read settings).
- Rolling mibpkg back to 0.1.0 with `[conditions]` in settings.toml breaks settings loading — note in release notes.

## Robustness-pass review minors (deferred, 2026-09-27)
- Repair uses the card's manifest; if a same-numbered version was rebuilt with other services, boot.d could
  point at missing files — use the on-disk manifest in repair (or refuse when they differ).
- Orphans that ignore SIGTERM still overlap after the grace period — add a `kill -9` fallback to Sys.
- pid reuse after a supervisor crash (no reboot): verify `/proc/<pid>/exefile` is `/tmp/mibpkg.<id>` before
  terminating a recorded pid.
- Lock: an empty lock older than a few seconds (mtime) should count as stale, not after a fixed 100 ms.
- Installed version marked incompatible on the card: the "older than installed" message should mention compatibility.
- Tiny race: uninstall-self touching `stop` between a new supervisor's pid write and its stale-stop cleanup.
- RESERVED: `lock` isn't a real collision; add a comment explaining the `<pkg>.<svc>` reasoning.
- mibpkg self: power loss after state save but before self_update leaves old binaries; Equal branch never retries.
