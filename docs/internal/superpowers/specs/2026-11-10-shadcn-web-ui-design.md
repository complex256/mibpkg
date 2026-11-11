# mibpkg web interface: shadcn/ui front end and screen redesign — design

Status: design approved in brainstorming 2026-11-10. Builds on
[2026-10-05-web-interface-design.md](2026-10-05-web-interface-design.md).

## Goal

Replace the hand-written DOM views under `web/js/` with a React + TypeScript + Tailwind CSS +
`shadcn/ui` + `lucide-react` front end, bundled by Vite into `web/dist/` and embedded by
`src/web/assets.rs` exactly as before. Alongside the stack change, calm down the screens that
carry too much at once — most of all the package page.

Success:
- A view composes components (`Button`, `Card`, `Switch`, `Sheet`, `Collapsible`) instead of
  assembling raw tags, class names, spinners and disabled states by hand.
- An installed package fits one phone viewport; permissions, removal and rollout approvals are
  bottom sheets, and technical details are collapsed.
- Discover, Updates and Installed stop repeating the same package lists.
- `cargo test`, GitHub Actions and `scripts/build.sh` still build with plain `cargo`: the built
  bundle is committed and no Node runs inside the QNX toolchain container.
- No request leaves the car: no CDN fonts, icons, styles or scripts.
- The existing safety copy and behaviour survive unchanged: loud permissions lead with
  `Warning:`, `Update to <ver> from <collection>` for moved packages, `Update all` never
  approves permissions, web settings still confirm a self-lockout (`409 needs_confirm`).

## Decisions

- **Stack**: React 19 + TypeScript, Tailwind CSS v4 (Vite plugin), `shadcn/ui` component sources
  committed under `web/src/components/ui/`, Radix primitives for Sheet/Switch/Collapsible/
  DropdownMenu, `lucide-react` for icons, Vite for bundling.
- **Committed bundle**: `web/dist/` is checked in and `scripts/bundle.sh`/CI rebuild it before
  reading it. `include_bytes!` keeps working; the QNX container only runs `rustc`.
- **Dark by default**, light through `prefers-color-scheme`, with shadcn CSS variables mapped in
  `styles.css`.
- **One HTTP API**: `/api/*`, verbs and job polling are untouched.

## Screen designs

### Package page (the main rework)

```
‹ <tab it was opened from>                        [•••]
┌──────────────────────────────────────────────────────┐
│ [C]  carmqtt                          [ Update ]      │
│      Car status to Home Assistant over MQTT           │
│      v0.2.2 → 0.2.3 · 454 KB · complex256-carmqtt     │
└──────────────────────────────────────────────────────┘
  job console / missing-config alert only when relevant

  SERVICE & CONFIGURATION
┌──────────────────────────────────────────────────────┐
│ ● carmqtt.carmqtt   running        Logs ›   Restart   │
│ ⚙ carmqtt.toml                     missing    Edit ›  │
│ 🛡 Permissions                     3 of 3 granted ›   │  (plugins only)
└──────────────────────────────────────────────────────┘

▸ Technical details    (source, offers, scripts, last result)
```

- `[•••]` (DropdownMenu): Roll back to X, Remove. Both open a Sheet; Remove's Sheet holds the
  acknowledgement Switch and the purge option.
- Permissions and approvals: a Sheet with one Switch per declared permission (granted = on),
  the all-or-nothing warning, and Save. Installing/updating/rolling back a plugin that asks for
  new permissions opens the approval Sheet first; Cancel sends nothing.
- A running job for this package replaces the header action with a spinning primary Button and
  disables other actions until it finishes; arriving mid-job attaches to it.

### Tabs

- **Discover**: Available packages; the search box filters available *and* installed. With no
  query and nothing available, an empty-state Card says everything is installed and offers
  Check for updates and Install from file. Card/online update notices stay.
- **Updates**: Pending updates and `Update all` first; `Up to date (N)` is a collapsed section.
  Check now and Update all wait while any job runs.
- **Installed**: the full installed list, with a service health Badge per row.

### Other screens

- **More**: rows as before; Remove mibpkg opens a Sheet with the acknowledgement Switch gating
  both destructive buttons.
- **Web settings**: Reachable at, Listen on (Switch cards), Port, PIN up front; the interface
  table and hotspot-interface override sit in an Advanced network interfaces Collapsible, open
  only when no hotspot URL is known or an override is set.
- **Config**: conflict diff as two Cards (side by side when wide) with loading Buttons.
- **Logs, Sources, Install from file, Internet, PIN pad**: same flows, rebuilt with the
  components; Sources keeps its job output above the lists and a spinning button.

## File layout

```
web/
  package.json  tsconfig.json  vite.config.ts  index.html
  src/main.tsx                 router, status poll, banner + toast
  src/styles.css               Tailwind + shadcn variables (dark, light media query)
  src/lib/{api,jobs,permissions,tabs,utils}.ts
  src/components/ui/*.tsx      shadcn sources we use
  src/components/{package-icon,package-row,job-console,permission-sheet,page-header}.tsx
  src/views/*.tsx              one file per screen
  tests/*.test.tsx             Vitest + Testing Library
  dist/                        committed bundle embedded by assets.rs
```

`src/web/assets.rs` embeds `/`, `/style.css` and `/app.js` from `web/dist/`; its test walks
`web/dist`. `tests/web_server.rs` keeps asserting each embedded file is served with its type and
that unchanged files answer `304`. `MIBPKG_WEB_DIR` points at `web/dist` for development.

## Verification

- `npm --prefix web test` (Vitest) covers: routing and back-tab memory, package header/busy
  states, permission approval routing, Sources/Updates/More busy states, Web settings advanced
  section, More's remove Sheet, Discover/Updates de-duplication.
- `npm --prefix web run build` produces `web/dist`.
- `cargo test` (assets + web server) and `python3 tools/check_docs.py`.
- Rendered check at 390×844 through the host server with `MIBPKG_WEB_DIR=web/dist`, plus an
  axe-core pass on each screen.
