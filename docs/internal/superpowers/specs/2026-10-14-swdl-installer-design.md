# mibpkg standalone SWDL installer (Red-menu install from SD) — design

Status: design approved in brainstorming 2026-10-14; spec awaiting review.
Builds on: [2026-09-26-mibpkg-design.md](2026-09-26-mibpkg-design.md) (bootstrap, boot hook, GEM
screens, SD collection format, on-car layout). Independent of M.I.B. — this replaces the M.I.B.
"Run Custom Script" bootstrap (`shell/custom.sh`) with mibpkg's own SWDL package.

## Goal

Install mibpkg on a MIB2 unit directly from the Red/Engineering menu's SW-Update flow, off a clean
SD card, with **no MIBonk (M.I.B.) files on the card and no M.I.B. install required first**. The
card is mibpkg's own SWDL package end to end.

Drivers (from brainstorming): **independence** (a stranger installs with one SD card, no M.I.B.
download / wiki dance) and **not relying on M.I.B.'s tooling** (their hashed final-script, their
firmware-extracted `updatePersistence`, their redistributed `GEM412` jar — licensing: their files
say "FREE for all / not for commercial use").

Success:
- A from-scratch `metainfo2.txt` + final-script, with zero MIBonk files present, is listed and run
  by the Red menu and installs mibpkg to completion.
- On any unit, the install always yields a working **C backbone**: boot hook + supervisor + web UI.
- Where feasible, the install also lights up the **in-car GEM picker** (B2 layer), including on old
  firmware whose stock GEM would not otherwise render mibpkg's `.esd`.
- A failed B2 step never produces a failed install or a half-flashed unit — it degrades to C.
- The whole card is emitted reproducibly by host-side `mibpkg` tooling; no hand assembly.

## Decisions (from brainstorming)

- **Approach 1: thin shell final-script + fat Rust binary.** The only shell on the card is a tiny,
  hash-registered stub shaped like `shell/custom.sh`; all trust-critical / fragile logic (GEM
  enable, jar patch, file landing semantics) lives in tested Rust subcommands. Rejected: a fat ksh
  final-script (M.I.B.'s shape — untested shell in the 300 s SWDL window) and a two-stage
  "SWDL seeds, first boot finishes" split (invisible first-boot failures; GEM-enable must happen in
  the window anyway).
- **Layering is the spine.** `C` = headless install (boot hook + supervisor + web UI), always runs,
  no on-car unknowns. `B2` = enable GEM + patch the stock jar so the in-car picker renders;
  best-effort, spike-gated, degrades cleanly to C.
- **B2 = B2 (targeted binary patch), not a whole jar.** Ship a binary *diff* against the unit's own
  jar; the stock jar never leaves the unit and no whole jar is redistributed. Rejected: B1 (ship a
  full replacement jar) and B3 (no jar; constrain `.esd` to a stock-renderable subset) — keep B3 in
  mind as the ultimate fallback if B2 proves unshippable on a target.
- **`enable-gem` sourcing, in order:** (a) stock `pc` (`/eso/bin/apps/pc`, already driven by
  `src/sys.rs`); (b) our own tiny persistence writer, shipped in the payload, built by us; (c) a
  stock-path `updatePersistence` only as last resort. Never ship M.I.B.'s firmware-extracted copy.
- **GEM left enabled after a C-only outcome** (harmless; lets a later jar patch light up the picker
  without re-flashing). Full teardown is explicit via a new `mibpkg disable-gem`.
- **Two on-car spikes gate B2, not C** (the car sleeps when off, so they are scheduled work):
  - **S1 — SWDL mechanics:** a no-MIBonk-files `metainfo2.txt` + `install.sh` is listed and run;
    `FinalScriptChecksum` is honored or waived; RCC context + `/net/mmx` landing confirmed.
  - **S2 — B2 in-car:** `enable-gem` via path (a) or (b) turns the picker on, and a patched old-GEM
    jar renders mibpkg's `.esd`.

## Architecture

Four components, three of them new, one reused:

1. **SWDL entry (shell, ours, hash-registered)** — `metainfo2.txt` + the `install.sh` final-script
   stub. The only hashed file; kept minimal so the build's re-hash is a one-liner and the
   RCC-context surface is small.
2. **mibpkg binary + boot payload (reused)** — `mibpkg`, `launch.sh`, `gem.sh`, `ca.pem`, exactly
   what `shell/custom.sh` lands into `/mnt/app/root/mibpkg` today.
3. **New Rust surface (small)** — `mibpkg enable-gem`, `mibpkg disable-gem`,
   `mibpkg gem-patch <apply|status|revert>`, wired in `src/cli.rs` next to `bootstrap`.
4. **Host build tooling** — `mibpkg swdl-card` (emit the card), `mibpkg gem-patch build` (author a
   patch), and `mibpkg verify` extended to validate a built card.

### Layering contract

- **C backbone — always runs, no spike needed.** Land files → `mibpkg bootstrap` (startup.sh hook +
  dirs + self-register, as today) → supervisor starts on reboot → **web UI is the primary
  interface.** Works on any unit regardless of GEM state.
- **B2 layer — best-effort, spike-gated, degrades to C.** `enable-gem` + `gem-patch apply` light up
  the in-car picker. No matching patch or a failed verify → logged and skipped → the install is a
  clean C install (web UI only), never a failure.

## Card layout

FAT32 root, everything under mibpkg's own `/mibpkg` namespace (none of M.I.B.'s `Launcher/`,
`apps/`, `esd/`, `mod/` trees):

```
/metainfo2.txt                     # mibpkg's SWDL manifest (skip* flags, one marker group, final-script hook + sha1)
/mibpkg/
  final/
    install.sh                     # hash-registered final-script stub (tiny)
  payload/
    mibpkg  launch.sh  gem.sh  ca.pem
    gem-patch/
      patches.toml                 # stock-jar sha1 -> patch file + expected-result sha1
      <sha1>.patch                 # one bsdiff-style binary patch per supported stock jar
  collection/                      # OPTIONAL seed: index.json + packages/ (first-package install)
    index.json
    packages/
/_Swdlautorun.txt                  # ships DISABLED; rename to Swdlautorun.txt for insert-and-go
```

### metainfo2.txt (authored from scratch)

`[common]` carries: `vendor = "complex256"`, `release = "mibpkg installer vX.Y"`, the bypass flags
`skipCheckSignatureAndVariant`, `skipMetaCRC`, `skipFileCopyCrc`, `skipCheckVariant`,
`skipCheckRegion`, `skipSaveTrainName` (all `"true"`), a broad `variant = "FM?-*-*-*-*"` + all
regions, and the hook: `FinalScript = "./mibpkg/final/install.sh"`, `FinalScriptChecksum = <sha1>`,
`FinalScriptMaxTime = "300"`, `FinalScriptName = "mibpkg install"`.

Plus **one minimal download group** that copies a tiny version-marker file (e.g. to
`/HBpersistence/SWDL/Log/service/mibpkg`) purely so the package is a valid group; the real work is
all in the final-script.

The `FinalScriptChecksum` is computed honestly and included — we do not bet that a `skip*` flag
waives it (that bet is exactly what S1 resolves).

### install.sh (the thin stub)

`shell/custom.sh` re-shaped for SWDL's arg convention (`$1` = HW, `$2` = SD mount), reusing the
proven RCC→MMX forward (`exec on -f mmx …`). Strict order:

1. env (`PATH`/`LD_LIBRARY_PATH`, `unset LD_PRELOAD`); resolve `$2`; forward to MMX if needed.
2. `mount -uw /mnt/app`; land payload → `/mnt/app/root/mibpkg/bin` (`.new` → `mv`).
3. copy `mibpkg` → `/tmp`; **`mibpkg bootstrap`**.
4. `mibpkg enable-gem` — best-effort, non-fatal.
5. `mibpkg gem-patch apply --from $2/mibpkg/payload/gem-patch` — best-effort, non-fatal.
6. rename `Swdlautorun.txt` → `_Swdlautorun.txt` (our own code, so insert-and-go does not loop).
7. `touch /tmp/SWDLScript.Result`; return 0.

All echo progress to the RCC log / UART for debugging.

## Install flow (end to end)

| Phase | What happens |
|-------|--------------|
| 0 prep | user writes card, inserts **SD1**, Red menu → SW Update → SD → "mibpkg installer vX.Y" → START (or insert-and-go if `Swdlautorun.txt` enabled) |
| 1 SWDL | `skip*` waives sig/variant/region/CRC; marker group copies; `install.sh` invoked on **RCC**, 300 s budget |
| 2 install.sh | steps 1–7 above |
| 3 reboot(s) | SWDL reboots up to 3× (per wiki); startup.sh hook → `launch.sh` detached → honors `/mibpkg/DISABLE` → `mibpkg … supervise`; web UI up; seed `collection/` installed first-boot-only (marker-guarded, outside the 300 s window) |
| 4 outcome | GEM on + jar ok → in-car picker + web UI; GEM on + jar too old → web UI only (C fallback); any case → web UI + seed packages running |

### Failure semantics (the line in the sand)

- Step 2 land fails → **abort, no `SWDLScript.Result`** → SWDL reports failure, unit effectively
  untouched.
- Step 3 `bootstrap` fails → **abort**; files on disk but no hook is harmless, and the step is
  re-runnable.
- **Once step 3 succeeds the install is a success** (C backbone committed) and `SWDLScript.Result`
  will be touched. Steps 4–5 only decide whether the in-car picker also lights up; nothing after
  step 3 turns a good install into a failure.
- Steps 4–5 log and continue on any error.

### Recovery properties (reuse existing mibpkg behavior)

- **Idempotent re-run:** `bootstrap`, `enable-gem`, `gem-patch apply` all check current state, so
  re-running the SWDL update repairs a partial install.
- **DISABLE card:** `/mibpkg/DISABLE` on any media root → supervisor starts nothing (safe boot).
- **Uninstall:** `mibpkg uninstall-self` (hook + GEM screens) + `gem-patch revert` (restore stock
  jar from the backup registry) + `disable-gem` → unit back to pre-mibpkg, jar included.

## New Rust surface

### `mibpkg enable-gem` / `mibpkg disable-gem`

Write the dev-mode adaptation (persistence key `3221356557`, ns 0, type byte = 1) and commit it
(M.I.B.'s second write, key `1` = store). Idempotent, non-fatal, logs which sourcing path it used:

- (a) **stock `pc`** (`/eso/bin/apps/pc`, via `src/sys.rs`) — preferred, nothing shipped;
- (b) **our own tiny writer** against the persistence lib, shipped in the payload — if `pc` can't
  reach that key/ns/type;
- (c) stock-path `updatePersistence` — last resort only.

The design commits to the *interface*; which of (a)/(b)/(c) works is decided by S2. `disable-gem`
clears the flag for full teardown. Lives next to `src/sys.rs` / `src/persist.rs`.

### `mibpkg gem-patch <apply|status|revert>`

All in Rust, host-testable:

- `apply --from <dir>`: sha1 the stock jar (`AppDevelopment.jar` / `GEM.jar` / `lsd.jar`, same
  detection M.I.B. does) → look up in `patches.toml` → if matched, back the stock jar into mibpkg's
  existing backup registry, apply the binary patch (Rust `bspatch`), verify the result sha1 against
  the expected value, swap in via atomic rename. **No match or bad verify → log, exit
  non-zero-but-non-fatal; installer continues as C.**
- `status`: detected jar version + patch available/applied (feeds web UI and GEM screen).
- `revert`: restore the stock jar from the backup registry.

New small module; reuses the backup/undo registry `bootstrap` already uses for the startup.sh line.

## Host build tooling

- **`mibpkg swdl-card`** — emits the whole card reproducibly: writes `metainfo2.txt` with the
  `skip*` flags and the **computed** `FinalScriptChecksum` (sha1 of the `install.sh` it also
  writes), lays down `/mibpkg/payload/*`, the optional seed `/mibpkg/collection/*`, and a disabled
  `/_Swdlautorun.txt`. The hash can't drift from the script.
- **`mibpkg gem-patch build --stock <jar> --patched <jar>`** — produces `<sha1>.patch`
  (bsdiff-style binary diff) + the `patches.toml` entry, from a stock jar and the patched jar S2
  produces. Rust-side `bspatch` applies on-car, so no external tool on the unit.
- **`mibpkg verify` extended** — parses the emitted `metainfo2.txt`, checks `install.sh`'s hash
  matches, payload present, `patches.toml` ↔ patch files consistent. CI pre-flight (the dormant
  release CI / `action.yml` grows one card-build + verify step).

## Independence / licensing

- Every card file is ours (manifest, `install.sh`, marker). No MIBonk files, no `GEM412` jar, no
  shipped `updatePersistence`.
- B2 ships a **binary diff against the unit's own jar** — the stock jar never leaves the unit and no
  whole jar is redistributed. This is the lighter-weight position for a derivative-of-Audi artifact.
  **Judgment call to revisit at go-public:** "ship a diff" vs. "ship a jar" vs. fall back to B3
  (stock-renderable `.esd` subset, no jar) for jars we cannot or should not patch.
- Installer files under the repo's existing license.

## Testing

- **Host unit tests:** `enable-gem` via the `--root` / no-persistence seam `src/sys.rs` already has;
  `gem-patch apply/verify/revert` against fixture jars (bsdiff round-trip); `metainfo2` emission +
  hash; `verify` on a built card.
- **CI gate:** `mibpkg verify` on the built card.
- **On-car integration:** the two spikes (S1, S2) are the integration tests; after they pass, a
  full install dry-run on the verified MHI2Q before this is called done.

## Out of scope / deferred

- B1 (full replacement jar) and B3 (stock-renderable `.esd` subset) — B3 is held as the final
  fallback if B2 is unshippable on a target.
- Signatures on the SWDL package (tracked with the broader signatures deferral).
- Replacing the M.I.B. bootstrap path for existing M.I.B. users — `shell/custom.sh` stays for them;
  the SWDL card is the independent alternative, not a forced migration.
- **Card builder webpage (fast-follow, separate work).** A static page hosted on GitHub Pages where
  a user checkbox-selects packages and gets a ready-to-write zip — the SWDL card with a seed
  `collection/` built from their selection. Rides on `mibpkg swdl-card` + the seed-collection
  mechanism this spec defines (so this spec must keep the card layout stable and the seed collection
  in the standard `index.json` format), but the page itself is its own spec/plan. Captured in
  `docs/ideas.md`.

## Open questions (resolved by the spikes, not before the plan)

- S1: is a from-scratch `metainfo2.txt` listed/run with no MIBonk files? is `FinalScriptChecksum`
  enforced? exact accepted manifest skeleton.
- S2: which `enable-gem` path works (a/b/c)? at least one proven `stock-sha1 → patch → render`, or a
  documented "this jar is C-only."
