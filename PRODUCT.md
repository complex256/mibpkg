# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

**Primary: the car owner, phone in hand, in or next to the car.** They are technical enough to
install an SD-card payload from the engineering menu, but they do not want a laptop or a diagnostic
tool at the car. Their job is to see what is available, install or update it, and get a service
running — usually in a few minutes, often while standing in a garage or driveway.

**Secondary: package publishers.** People who build a `.mpk`, test it and publish a collection that
cars can update from. They are served by the documentation site, not by the in-car interface; the
phone UI stays owner-facing.

**Maintainers** use the same web UI for health, logs, sources and recovery, but the product is not
designed around them.

## Product Purpose

mibpkg is an app store and package manager for the MIB2 High head unit. It installs, updates,
removes and rolls back software from a phone-friendly web page served by the car itself, with a
supervisor that starts package services at boot and keeps them running.

Success means the owner can add or change software on the unit without a PC, without a special
cable, and without fear of bricking it: every action has a visible outcome, a failed action leaves a
working unit, and a bad update can be undone.

## Positioning

Everything runs on the unit. There is no companion server, cloud account or desktop installer in the
loop: the car serves the interface over its own hotspot or Tailscale, and packages can come from the
SD card or from online collections when the car has internet. Safety is built into the same product
rather than bolted on — atomic installs, once-only backups of stock files, one-tap rollback, a
`DISABLE` card, and a web server locked to the hotspot and tailnet by default.

A neighbouring package manager could copy the file format; it could not truthfully copy “the car is
the app store, and the phone in your hand is the client.”

## Operating Context

- The unit is a **MIB2 High** (`MHI2` / `MHI2Q`) running **QNX 6.5** on ARMv7. It is the only
  supported hardware family.
- The owner reaches the page by joining the car's Wi-Fi hotspot or the tailnet; the car's **GEM →
  mibpkg** screen shows the address. GEM is a deliberately slim recovery page; package work lives on
  the web.
- The page is used one-handed on a phone, often in poor light, and sometimes with gloves or while
  distracted. It may be opened, backgrounded and reloaded mid-action.
- Cars may have no SIM data. Offline use — card packages, local installs, logs, settings — must stay
  complete; online sources and updates are the optional layer.
- The unit's filesystem is unusual: `/mnt/app` is read-only by default behind a holder protocol,
  `/tmp` is a RAM disk without normal directory/rename/fsync behaviour, and the clock can be unset at
  boot. These are product facts, not implementation trivia: they explain why some operations take
  time, why one action runs at a time, and why recovery paths exist.
- Package authors work from `manifest.toml`, `.mpk` archives and collection indexes, and use the CLI
  (`mibpkg verify --unit ...`) before cars see a release.

## Capabilities and Constraints

- **Four tabs**: Discover (card and online collections), Updates (available versions, update all),
  Installed (open, configs, logs, restart, rollback, remove), More (install from file, internet,
  logs, sources, web settings, health).
- **One package per product**: a native daemon and its optional SDK status plugin ship together.
- **One action at a time.** A second request while a job runs is refused as busy and names the
  running job; a reloaded page reattaches to it. Row actions show progress, and a package page's main
  button becomes a spinner while its job runs.
- **Permissions are explicit.** Plugin permissions are shown in plain words; a new or escalated
  permission needs approval, including on updates and rollbacks. Update-all never approves on the
  owner's behalf. A revoked permission stays revoked until granted.
- **Uploads expire after 10 minutes** and are inspected (name, version, compatibility, services,
  scripts, permissions) before install.
- **PIN protection** is optional, 4–12 digits, with recovery through GEM (remove PIN / reset web
  settings).
- **The web front end ships as a built bundle, and the Rust build never needs Node.** The React +
  TypeScript + Tailwind (shadcn/ui) sources live in `web/src/`; Vite writes `web/dist` and those
  three files are committed and embedded in the mibpkg binary with `include_bytes!`. Rebuild
  `web/dist` when the UI changes; a plain `cargo build` uses what is committed.
- **Publicly documented surface**: the JSON web API is documented for contributors and other
  clients; the CLI and manifest format are documented for publishers.
- **Terminology is fixed**: package, collection, source, service, config, job, rollback, GEM,
  Discover/Updates/Installed/More. UI copy uses these words rather than synonyms.
- No pricing, paid tier, account system or telemetry is confirmed. Do not invent them.

## Brand Commitments

- **Name**: mibpkg. Lower-case in prose and UI chrome.
- **Public voice**: plain, factual, human-first. Documentation avoids OEM vehicle-brand names and
  marketing claims; the published site is the reference for that rule.
- **Licence**: MIT. The project is public open source now, with an outside audience of owners and,
  over time, publishers.
- **Safety language is literal**: error and recovery copy names what happened and what to do next; it
  does not soften a refusal or promise an undo the product cannot perform.

## Evidence on Hand

- Real interface screenshots: `docs/screenshots/discover.png`, `updates.png`, `installed.png`,
  `more.png`, `sources.png`, `pin.png`, `self-update.png`, `package-updated.png`,
  `package-moved.png`. These are captures of the shipped UI, used across the README and docs.
- A published MkDocs documentation site (`mkdocs.yml`, `docs/`) covering install, first use, guides,
  reference and internal engineering records.
- A changelog of shipped behaviour (`CHANGELOG.md`, current 0.12.2) and a test suite (`tests/`,
  `web/tests/`) that encodes real cases.
- **Absences future work must respect**: no testimonials, customer logos, benchmarks, download
  counts, pricing or partner claims exist. Do not fabricate them. Do not invent unit models,
  firmware support or package counts beyond the documented MIB2 High family.

## Product Principles

1. **The car is the server; the phone is the client.** Any design that needs a PC, cloud account or
   second device to complete an owner task is wrong for this product.
2. **Visible, reversible action.** Every job shows what it is doing and ends in a clear result;
   destructive or uncertain steps stay undoable through rollback, backups or the `DISABLE` card.
3. **Offline is the baseline, online is a bonus.** The interface must be complete and honest with no
   internet; online features degrade to a clear, non-blocking state.
4. **Say the true thing.** Busy, incompatible, unapproved, expired and failed are named plainly, with
   the next step. Never hide a refusal behind a generic error.
5. **Owner-first, publisher-literate.** Optimise the interface for the person at the car; keep the
   documentation precise enough that the person building packages is never guessing.

## Accessibility & Inclusion

Phone-first, with no declared conformance standard. Current practice to preserve: system dark/light
colour scheme, semantic landmarks, `aria-live` for jobs and banners, labelled controls, touch targets
sized for one-handed use, and no interaction that depends on hover or precise pointing.
