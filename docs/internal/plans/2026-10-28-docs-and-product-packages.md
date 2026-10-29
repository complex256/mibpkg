# mibpkg: human-first documentation and one package per product

Date: 2026-10-28
Status: approved for implementation

Two changes are combined here because they touch the same pages and examples:

1. Rewrite the documentation for humans, publish a docs site, and remove OEM brand names.
2. Stop shipping a product's native daemon and its SDK status plugin as two packages.

## Decisions

- Public docs avoid OEM vehicle-brand names. Platform codes (`MIB2`, `MHI2`, `MHI2Q`) and
  third-party tool names (`M.I.B.`, `GEM`) stay.
- No generated Rust API documentation. The CLI and package reference are hand-written and checked
  against `src/cli.rs`.
- Internal engineering records move to `docs/internal/` and are excluded from the site.
- `mibpkg new` becomes runnable without editing a file.
- One package per product. No migration work: the separate packages are not in use.
- Version after this work: **0.10.0**.

## Part 1: documentation and publishing

### Structure

```text
README.md                       landing page
INSTALL.md                      short pointer to the install guide
CHANGELOG.md                    Keep a Changelog
docs/
  index.md
  getting-started/
    index.md
    requirements.md
    install.md
    install-custom-script.md
    first-use.md
    update.md
    remove-and-recover.md
  guides/
    web-interface.md
    packages.md
    plugin-packages.md
    real-packages.md
    online-sources.md
    config-logs-restart.md
    web-settings-pin.md
    recovery-disable.md
    troubleshooting.md
    your-first-package.md
    publishing-a-collection.md
    github-action.md
  reference/
    cli.md
    manifest.md
    package-layout.md
    collection-index.md
    service-conditions.md
    paths-and-status.md
    units.md
    web-api.md
  explanation/
    architecture.md
    safety-and-recovery.md
    boot-and-supervisor.md
    online-updates.md
    install-methods.md
  contributing/
    index.md
    build.md
    test.md
    release.md
    docs-style-guide.md
  internal/
    ideas.md
    unit-facts.md
    superpowers/
    plans/
  screenshots/
  install-images/
  stylesheets/extra.css
  requirements.txt
mkdocs.yml
.github/workflows/docs.yml
tools/check_docs.py
```

### README

Short landing page: what mibpkg is, screenshot, three-step install, features, supported units,
docs links, contributing, MIT. The `[plugin]`/`[sdk]` details move to `reference/manifest.md`.

### Install

`INSTALL.md` becomes a one-paragraph pointer. The guide splits into:

- requirements;
- engineering-menu install;
- custom-script install;
- first use;
- update;
- remove and recovery.

Existing screenshots are used. Missing install-image placeholders are omitted rather than shipped
broken.

### Reference

- `cli.md`: host and on-unit commands, flags, SDK verbs, exit codes.
- `manifest.md`: every table and rule, including `[plugin].id`.
- `package-layout.md`: source tree, `.mpk` tree, on-unit install tree.
- `collection-index.md`: `index.json`, `packages/`, `--flat`, collection ids.
- `service-conditions.md`: aliases and raw conditions.
- `paths-and-status.md`: `/mnt/app`, `/mnt/persist`, `/tmp`, state and logs.
- `units.md`: `MHI2`/`MHI2Q`, `[compat]`, unsupported units.
- `web-api.md`: endpoints, jobs, uploads, settings, busy behavior.

### Explanation

- architecture: host CLI, on-unit binary, supervisor, web server, installer hook, GEM screens.
- safety and recovery: atomic installs, backups, rollback, DISABLE, remount holders.
- boot and supervisor: phases, conditions, restarts, timing.
- online updates: sources, cache, fetch, GitHub releases, index carry-forward.
- install methods: SWDL card vs custom script, signed header.

### Examples from the plugin repos

- `guides/plugin-packages.md`: plugin-only `.mpk` packages built by the SDK, permissions,
  approval, install/update/remove.
- `guides/real-packages.md`: one package per product.
  - `tailscale`: native daemon + `[plugin] tailscale-status`.
  - `carmqtt`: native daemon + `[plugin] carmqtt-status`.
  - `wireguard`: native only.
  - `bike-rack`, `hello-menu`, `inputs-demo`: plugin only.
- `reference/manifest.md` uses the real manifests as examples.
- Docs link to `mib2q-plugins` and the SDK docs site.

### Publishing

- MkDocs Material, `--strict`.
- GitHub Actions builds and deploys to `gh-pages`.
- Site: `https://complex256.github.io/mibpkg/`.
- Pages enabled from `gh-pages` after the first deploy.
- No rustdoc.

### Brand names

Public docs, README and INSTALL remove OEM names (`Audi`, `VW`, `Volkswagen`, other vehicle
brands). `Audi-signed` becomes `manufacturer-signed`. Test fixture data avoids brand names too.
`tools/check_docs.py` enforces the rule.

## Part 2: one package per product

### Manifest model

`[plugin]` may coexist with `[[file]]`, `[[config]]`, `[[service]]` and `[scripts]`.

```toml
[package]
name = "tailscale"
version = "1.4.0"

[[file]]
src = "bin/tailscaled"
mode = "755"

[[service]]
name = "tailscale"
exec = "@pkg/bin/tailscale.sh"
args = ["serve", "@config"]

[plugin]
id = "tailscale-status"
jar = "tailscale-status.jar"
api = 6
permissions = ["network", "ui.slot.statusbar"]
icons = ["tailscale-on", "tailscale-off"]
```

- `[plugin].id` is optional and defaults to the package name.
- The jar must be `<id>.jar`.
- `[sdk]` stays declarative-only.
- Plugin-only packages remain valid.

### mibpkg code

- `PluginSpec`: add `id: Option<String>` and an `id()` helper.
- `manifest.rs`: allow plugin + native tables; validate the plugin id and jar name.
- `plugin_pkg`: use the plugin id for the jar, stamp, grants, icons, data and reload marker.
- `install_package`: activate the plugin by plugin id.
- `install::remove`: remove the package, then the plugin by plugin id.
- `install::rollback`: redeploy the plugin half from the previous package directory.
- `sdk_pkg::plugin_packages`: return plugin ids.
- `sdk remove`: refuse a bundled plugin and point at the product package.
- `sdk grant`: resolve the owning package manifest by plugin id.
- SDK uninstall: report product packages, not plugin ids, when they block it.
- `web/upload`: approval planning uses the plugin id; package identity stays the product name.
- `scaffold`: generate an executable `bin/<name>` service, `.gitignore`, README and workflow.

### mib2q-plugins

- `tailscale` package combines `bin/`, configs, service and `[plugin] tailscale-status`.
- `carmqtt` package combines the daemon, required config, service and `[plugin] carmqtt-status`.
- `wireguard` stays native-only.
- Plugin-only samples stay plugin-only.
- `packageDaemon` copies the plugin jar, stamp and icons into the native stage before one
  `mibpkg build`.
- CI builds one product `.mpk` per product.

## Publishing and version

- `Cargo.toml` 0.10.0; `Cargo.lock` updated.
- `CHANGELOG.md` 0.10.0.
- Tag `v0.10.0`.
- GitHub release so `host-release.yml` attaches the Linux host binary.
- Docs workflow publishes the site.

## Gates

- `cargo fmt --check`
- `cargo test`
- `mkdocs build --strict`
- `python3 tools/check_docs.py`
- combined package builds and verifies;
- install/update/rollback/remove of a combined package;
- plugin id different from package name;
- `mibpkg new` builds without manual edits;
- brand-name check.

## Out of scope

- Migrating existing separate `tailscale-status` / `carmqtt-status` installs.
- Generated Rust API documentation.
- New install screenshots.
- Rewriting the internal design specs.

## Follow-up 2026-10-29

### SD-card formatting steps

The install guide now has annotated step illustrations for both macOS and Windows:

- `docs/install-images/sd-macos-1-view.svg`
- `docs/install-images/sd-macos-2-select.svg`
- `docs/install-images/sd-macos-3-erase.svg`
- `docs/install-images/sd-windows-1-select.svg`
- `docs/install-images/sd-windows-2-format.svg`

They are generated by `tools/make_install_images.py`. They are diagrams, not captures of the real
dialogs; replace them with captures when the docs are next revised.

### Phone uplink investigation

A no-SIM car still has the hotspot, so local installs work; online updates need an uplink. The
feasibility and design work is in `docs/internal/plans/2026-10-29-phone-uplink.md`.

Summary: an SDK plugin alone cannot do this because it cannot run `wpa_supplicant` or `dhcp.client`.
The proposed package is a combined `wifi-uplink` product: a native Rust daemon plus an SDK status
plugin, configured through mibpkg package config (not the car speller). The blocking question is
whether `mlan0` (client) and `uap0` (hotspot) can be up at the same time; if not, USB tethering on
`ecm1`/`usb0` is the fallback. Both need a bench test before implementation.
