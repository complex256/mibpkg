# mibpkg web interface — design

Status: design approved in brainstorming 2026-10-05; spec awaiting review.
Builds on: [2026-09-26-mibpkg-design.md](2026-09-26-mibpkg-design.md),
[2026-09-30-online-updates-design.md](2026-09-30-online-updates-design.md).
Evidence: [docs/unit-facts.md](../../unit-facts.md). Mockups (local, not committed):
`.superpowers/brainstorm/*/content/appstore.html`, `upload-v2.html`, `upload-settings.html`.

## Goal

A web interface served by the car itself, reachable from a phone on the car's Wi-Fi hotspot or from
the user's tailnet, that does everything the GEM screens do today and more, and later replaces them
as the main interface.

Success:
- From a phone on the hotspot, a user can search the card and online collections, install, update,
  restart, roll back and remove packages, and watch each action's output live.
- A user whose car has no internet can upload a `.mpk` from the phone and install or upgrade it.
- Logs, package config files and online sources can be viewed and changed from the browser.
- No client request can make the car run a command it did not intend (see Security).
- The front end is plain, readable, well-commented HTML/CSS/JS with no build step.
- GEM keeps working unchanged in milestone 1; milestone 2 slims it to one recovery page.

## Decisions (from brainstorming)

- Process: a `mibpkg web` subcommand in the same binary, started and restarted by the supervisor
  as a built-in service (approach A). Not a thread in the supervisor (a panic would abort every
  service, `panic = "abort"`), not a separate package (duplicate code, version skew).
- Reach: default listen on the car hotspot and Tailscale; the user can change it, including to any
  interface. Optional PIN.
- Uploads: install after a confirmation page (same trust model as SD: the user vets the file).
  An upgrade keeps the package's recorded collection. A new install asks which collection it
  belongs to only when that is ambiguous (see Uploads).
- v1 extras: service logs, online sources management, package config editing, live action output.
- Look: phone-first, App Store style, bottom tabs Discover · Updates · Installed · More.
- Front end: vanilla ES modules, no framework, no build step, embedded in the binary.
- Milestone 2 (after M1 is proven on the car): GEM shrinks to a single recovery page.

## Architecture

```
phone/laptop ──HTTP──> mibpkg web  (child of the supervisor, restarted with backoff)
                          │  reads: state, status, indexes, logs, configs  (in-process)
                          │  writes: spawn /tmp/mibpkg gem <verb> <args>  (no shell)
                          ▼
                       same action path as GEM buttons: action lock, action(), record_result,
                       GEM screen regeneration, live persistence rows
```

### Process

- `mibpkg web` binds `0.0.0.0:<port>` once. The supervisor starts it as built-in service
  `mibpkg.web` (not a boot.d entry) when `web.enabled` is true, with the usual restart backoff and a
  capped log at `/tmp/mibpkg.mibpkg.web.log`. No start condition.
- The DISABLE card stops it like everything else mibpkg starts.
- Port: `80` by default; if bind fails with "address in use", fall back to `8080` and log it. The URL
  actually in use is written to `/tmp/mibpkg.web.url` for GEM and `mibpkg status`.
- Concurrency: one std thread per connection, at most 6 at once; extra connections get `503`.
- HTTP/1.1 subset: request line, headers, `Content-Length` bodies. No keep-alive
  (`Connection: close`), no chunked request bodies, no TLS. Responses are gzip-compressed with
  `flate2` when the client accepts it.
- No new crate dependencies.

### Interface filter

On every accepted connection the server reads the socket's `local_addr()` and compares it with the
current addresses of the allowed interfaces. Those addresses are refreshed when older than 10 s using
the same `ifconfig` probing the start conditions use. Tailscale appears ~114 s after boot, so
filtering per connection avoids rebinding. A connection on a disallowed address is closed without a
response. `any` skips the check.

Listen names: `hotspot`, `tailscale`, `any`. The interface name behind `hotspot` is an on-car check
(only `ecm0` = SIM data and `tun*` = Tailscale are known today); it is a constant mapping in code
with a `web.toml` override, like start-condition aliases.

### Actions and jobs

- A write request becomes a **job**: the server spawns `/tmp/mibpkg gem <verb> <args…>` with
  `std::process::Command` (argv, never a shell), captures stdout+stderr line by line into memory
  (capped at 64 KB per job), and records the exit status and the final `OK:`/`FAILED:` line.
- One action job at a time. A second request while one runs gets `409 {busy, job}` with the running
  job's id so the UI can attach to it. If the action lock is held by GEM (no web job), `409 {busy}`.
- Finished jobs are kept for 10 minutes.
- Reads (status, packages, logs, configs, sources, settings) are served in-process through the same
  functions the CLI uses; no spawn.

### New action verbs (shared with the CLI and GEM)

Added to `cli::action` so every write takes one path under the action lock:

| Verb | Args | Does |
|---|---|---|
| `install-file` | `<upload-id> <collection-or-->` | Install/upgrade from `/tmp/mibpkg.upload.<id>.mpk` (see Uploads) |
| `config-set` | `<pkg> <n> <upload-id>` | Replace config file *n* of `<pkg>` with the uploaded text, then request a restart of its services |
| `source-add` | `<source>` | Add an online source, then run an online check |
| `source-remove` | `<source>` | Remove or disable an online source, drop its cache |
| `web-settings` | `<id>` | Save `web.toml` from a server-written JSON file |
| `web-reset` / `web-restart` | — | Delete `web.toml` and restart the web service / just restart it |

`/mnt/app` is read-only and its rw remount is refcounted per process, so every flash write —
including web settings — runs as one of these verbs under the action lock.

Text and file contents never travel as arguments: they go to a server-written file in `/tmp`
named by a server-generated id.

### Where settings live

- `settings.toml` stays hand-owned: mibpkg never rewrites it (rewriting would lose comments, and a
  parse error there stops every mibpkg command).
- Web settings live in `<home>/web.toml`, written only by mibpkg (serializer, atomic write). If it is
  missing or unreadable the web server logs why and uses the defaults; it never refuses to start.

```toml
enabled = true
port = 80
listen = ["hotspot", "tailscale"]   # or ["any"]
pin = "<hex salt>:<hex sha-256(salt + pin)>"   # absent = no PIN
# interfaces = { hotspot = "uap0" }  # optional override of the interface names
```

- Sources added in the web UI live in `state/sources.json` (`added` and `disabled` lists). The
  effective list is `settings.online_sources` + `added` − `disabled`. Sources from `settings.toml`
  show a "from settings.toml" note; "remove" on those disables them.

## Security

### No command injection

1. **No shell on the web path.** Actions run `/tmp/mibpkg` directly via `Command::new(..).args(..)`
   (execve with an argv array). `gem.sh` is not used. Shell metacharacters are plain bytes inside
   one argument.
2. **Allow-listed actions.** The client sends typed JSON (`{"action":"restart","package":"x"}`),
   parsed into a Rust `enum` with one variant per verb. Anything else is `400`. The server builds
   the argv; the client never sends a command or a path.
3. **Validated fields**, each checked before a job is created:
   - package names: `manifest::valid_name` (tightened, below) and installed or present in a known
     collection;
   - collection ids: one that exists in state, on a card, or online (or `-` for none);
   - upload and config ids: server-generated, 128-bit random hex, mapped to paths server-side;
   - config file *n*: an index into the package's manifest `[[config]]` list (no client paths);
   - sources: `online::index_url` must accept them (`github:owner/repo` or `https://…/index.json`).
4. **Argument injection fix (applies to GEM and the CLI too).** `valid_name` accepts names made of
   `-` and letters, so `--root` is a valid package name today, and `cli::run` takes `--root <dir>`
   from any position. Fix both: names must start with a lowercase letter or digit, and `run` stops
   reading global flags at the command word.
5. **Escape by default in the browser.** Package text, log lines and config contents are inserted
   only with `textContent`, never `innerHTML`.

Boundary: a package's own install scripts run in a shell by design. That is the package's trust,
accepted on the confirmation page (which says when a package uses scripts); no request field reaches it.

### Cross-site requests and DNS rebinding

- Every write (`POST`/`PUT`/`DELETE`) must carry `X-Mibpkg: 1` and a JSON or
  `application/octet-stream` body. A form post from another site cannot set that header without a
  CORS preflight, which the server never approves (no CORS headers at all).
- The `Host` header must be an IP literal (v4 or bracketed v6) or `localhost`, optionally with the
  port; otherwise `403`.

### PIN

- Optional. Stored salted and hashed (sha-256, already a dependency) in `web.toml`.
- `POST /api/login {pin}` sets a random session cookie: `HttpOnly; SameSite=Strict; Path=/`.
  Sessions live in memory (a web restart logs everyone out) and expire after 7 days idle.
- After 5 wrong PINs, logins are refused for 60 s, doubling per further failure, up to 1 hour
  (counted globally, in memory).
- The PIN travels in plain HTTP. On the WPA hotspot and on Tailscale the link is encrypted; the
  settings page warns that with `any` it can be read off the SIM data link.
- Lock-out recovery: `PUT /api/settings` that would exclude the interface the request came in on is
  refused unless it carries `confirm: true`; GEM has a "Reset web settings" button (M1 and M2).

## HTTP API

Everything under `/api/` returns JSON, except logs (`text/plain`). Errors are
`{"error": "<plain words>"}` with `400` invalid input, `401` PIN needed, `403` forbidden (header or
host), `404`, `409` busy, `413` too large, `503` too many connections. Static files (`/`,
`/style.css`, `/js/…`) carry the mibpkg version as `ETag`.

### Session

| Endpoint | Purpose |
|---|---|
| `GET /api/session` | `{pin_required, authed}` |
| `POST /api/login` `{pin}` | Sets the session cookie |
| `POST /api/logout` | Clears it |

### Reads

| Endpoint | Returns |
|---|---|
| `GET /api/status` | Unit, mibpkg version, supervisor state, online-check line, DISABLE flag, running job id, update count, web URLs |
| `GET /api/packages` | One merged list. Per name: `installed` (version, collection, previous, services with state, uses_scripts, missing required config, last result) and `available` (per collection: version, origin card/online, size, description, compatible). The browser filters it for search. |
| `GET /api/logs/<id>?bytes=N` | Tail of `mibpkg`, `mibpkg.web` or `<pkg>.<service>` (id must be a known log) |
| `GET /api/packages/<name>/config/<n>` | `{path, exists, required, content, example}` for manifest config *n* |
| `GET /api/sources` | Effective sources with origin (default/settings/web), enabled, last fetch result |
| `GET /api/settings` | `{enabled, port, listen, pin_set, urls, interfaces_up}` (never the PIN) |
| `GET /api/jobs/<id>?since=N` | `{lines, next, done, ok, result}` |

### Writes (need `X-Mibpkg: 1`)

| Endpoint | Does |
|---|---|
| `POST /api/actions` `{action, package?, collection?, purge?}` | `rescan`, `check`, `update-all`, `install`, `rollback`, `restart`, `remove` → `202 {job}` or `409` |
| `POST /api/uploads` (octet-stream) | Save and inspect a `.mpk` → upload summary (see Uploads) |
| `POST /api/uploads/<id>/install` `{collection}` | `install-file` job; `collection` ignored for upgrades |
| `DELETE /api/uploads/<id>` | Discard |
| `PUT /api/packages/<name>/config/<n>` (text, ≤ 64 KB) | `config-set` job |
| `POST /api/sources` `{source}` / `DELETE /api/sources` `{source}` | `source-add` / `source-remove` job |
| `PUT /api/settings` `{listen, port, pin?, clear_pin?, confirm?}` | Save `web.toml`; the web process exits after responding and the supervisor restarts it; the response carries the new URLs |

Request limits: headers ≤ 16 KB, bodies ≤ 64 KB except uploads (≤ `fetch::MAX_PACKAGE`), checked
against `Content-Length` before reading. Read timeout 10 s per connection (60 s for upload bodies).

## Uploads

1. The browser sends the file as the raw request body (no multipart parsing). The server writes it
   to `/tmp/mibpkg.upload.<id>.mpk`. One pending upload at a time; a new one replaces it.
2. The server opens the archive and validates the manifest exactly as an install would, then returns:
   `{upload_id, name, version, description, size, sha256, kind, compatible, uses_scripts, services,
   problems[], candidates[], preselected, ask}` where `kind` is `fresh | upgrade | same | older`.
3. Problems (corrupt archive, bad manifest, incompatible unit, `same` or `older` version) are listed
   in plain words and Install is disabled. Older versions point to Roll back.
4. **Collection for a fresh install** — preselection, first rule that applies:
   1. the file's SHA-256 equals an entry in a known index → that collection;
   2. exactly one known collection (card or online) has the name → that one;
   3. several have it → the one offering the newest version (official collection wins ties);
   4. none → "None" (recorded as collection `uploaded`; only another upload updates it).

   `ask` is true only in case 3 when no SHA matched: the confirm page then opens the choice with a
   short explanation. Otherwise the choice is a collapsed "Updates from: X · Change" line.
5. **Upgrade**: the recorded collection is kept; no choice is shown.
6. Same-name rules match collection installs: one install per name; `install-file` refuses a
   collection different from the recorded one.
7. The upload file is deleted after install, on cancel, after 10 minutes, and at web start.

`uploaded` is a reserved collection id: `pick_collection` treats it as "no collection", so Check for
updates offers nothing for it and Install from a collection is refused with "installed from an
uploaded file; remove it first". `mibpkg index` and collection loading reject `uploaded` as an id.

## Front end

### Screens (mockups: `appstore.html`, `upload-v2.html`, `upload-settings.html`)

- **Discover**: search; an "Updates available" banner when there are updates (no curated featured
  content in v1); Available and Installed sections of app-style rows (letter icon, name, one-line
  description, GET/UPDATE pill). Header button: Install from file.
- **Updates** (badge = count): last check time, Check now, Update all, per-package rows with a
  progress bar and live log lines while running; Up to date section.
- **Installed**: installed packages with status dots (running, waiting, failed, stopped).
- **Package page**: large icon, name, collection, main action; facts strip (installed version,
  service state, size); description; "Config needed" warning; actions (Restart, Roll back to X,
  Remove with purge option); Logs and Configuration links; source, compatibility, install scripts,
  last result.
- **More**: Install from file, Logs, Sources, Web settings, Rescan SD card, About.
- **Install from file**: drop zone and progress → confirm page → install log (spinner, live lines,
  ✓ Installed with "Open package page", or ✕ Failed with the log kept and "Back").
- **Logs**: pick a log, show the tail, refresh button, auto-scroll.
- **Config editor**: textarea with the file (or its example when missing), Save → job log.
- **Sources**: list with origin and last fetch result, add (`github:owner/repo` or URL), remove.
- **Web settings**: current URLs, listen checkboxes (hotspot, Tailscale, any interface — any greys
  out the others and shows the PIN warning), PIN change/remove, Save, the GEM recovery hint.
- **Login**: PIN pad, shown when the API answers `401`.
- **Self-update**: after a job that updated mibpkg, "mibpkg is restarting…", poll `/api/status`
  until the version changes, then reload the page.
- **Busy / restarted**: when the lock is held with no attachable job, "An action is running" until
  it frees, then the last result.

### Code

```
web/
  index.html      page shell: header, <main> that views render into, tab bar
  style.css       :root design tokens first, then one commented section per view
  js/
    app.js        boot: session check, router start
    api.js        every call to the car: fetch wrapper, X-Mibpkg header, 401 -> login, errors
    router.js     #/discover, #/updates, #/installed, #/more, #/package/<name>, … -> view
    dom.js        h(tag, attrs, children) helper; text is always textContent
    jobs.js       start an action, poll /api/jobs every 500 ms, append lines to a log box
    icons.js      letter icon with a colour from a hash of the package name
    views/        discover, updates, installed, package, upload, logs, config, sources,
                  settings, login, more — one file each
```

- Plain ES modules, no framework, no build step, no npm. Files are embedded unchanged with
  `include_str!` (`src/web/assets.rs`); what is in the repo is what runs.
- Each file starts with a comment saying what it is for and what it talks to. Comments explain why.
  Short functions, full-word names, blank lines between steps, no clever one-liners.
- Each view exports `render(params)` that fetches its data and returns DOM; views never touch each
  other and navigate by changing `location.hash`.
- Dark by default; `prefers-color-scheme: light` supported via the tokens.
- Budget: about 40–60 KB of front end before gzip.
- Development: `mibpkg --root <dir> web` runs on the Mac; `MIBPKG_WEB_DIR=./web` serves the files
  from disk instead of the embedded copies.

## Rust layout

| File | Responsibility |
|---|---|
| `src/web/mod.rs` | `run()`: settings, bind, accept loop, connection cap, interface filter |
| `src/web/http.rs` | Request parsing and response writing (limits, timeouts, gzip) |
| `src/web/routes.rs` | URL → handler, auth and header checks |
| `src/web/api.rs` | Read handlers (JSON built with serde) |
| `src/web/actions.rs` | Action enum, field validation, argv building, job spawning |
| `src/web/jobs.rs` | Job table, line capture, expiry |
| `src/web/upload.rs` | Upload storage, inspection, preselection |
| `src/web/auth.rs` | PIN hash, sessions, lockout |
| `src/web/settings.rs` | `web.toml` load/save, defaults, interface names |
| `src/web/assets.rs` | Embedded front-end files |

Changes elsewhere: `cli.rs` (`web` command, new verbs, global-flag parsing fix), `manifest.rs`
(`valid_name`), `install.rs` (install from a file, reserved `uploaded` collection), `online.rs`
(effective source list), `supervisor.rs` (built-in `mibpkg.web` service), `gem.rs` (Web interface
screen), `status`/`print_status` (web URL). The spawn goes through a small trait so tests can record
argv without running anything.

## GEM

- **M1**: unchanged, plus a "Web interface" sub-screen: running/stopped, URLs, listen mode, PIN set,
  Restart web server, Reset web settings.
- **M2** (done, mibpkg 0.5.0; plan docs/superpowers/plans/2026-10-06-slim-gem.md): GEM becomes a single mibpkg page: web server status and URLs, listen mode and PIN set
  (read-only), Restart web server, Reset web settings, Update mibpkg from SD card. All package
  screens are removed; packages, logs and sources are web-only.
  It also has Remove PIN (shown when a PIN is set): whoever can open GEM is authorized.

## Errors and robustness

- Slow or hostile clients: per-connection timeouts and size limits; at most one of six threads tied
  up per client; the supervisor is a separate process.
- Web process dies: the supervisor restarts it. A running `mibpkg gem` child finishes on its own and
  records its result; the UI shows the busy state, then the last result.
- GEM and web at once: the action lock decides; the loser sees "busy".
- Unreadable `web.toml`: defaults, logged. Port 80 taken: 8080, logged and shown in GEM.
- RAM: one upload, job logs capped, uploads and finished jobs expire.
- Flash: no timed writes; only saves and actions write.
- Times: relative times ("4 min ago") are computed on the car and sent as strings.

## Testing

Rust (`tests/web_*.rs`, temp `--root`, fake `Sys`, recording spawner):
- HTTP parsing: valid requests, oversized headers, missing or wrong `Content-Length`, timeouts, junk.
- Routing and auth: writes without `X-Mibpkg` → `403`; bad `Host` → `403`; PIN login, session
  cookie flags, lockout doubling.
- Interface filter with recorded `ifconfig` fixtures: allowed, disallowed, `any`, interface that
  appears later.
- Injection table: `;reboot`, `$(id)`, backticks, `\n`, `--root`, `-x`, `../../etc`, very long
  strings, non-ASCII — in every field of every action → rejected before spawn; recorded argv is
  only validated parts. `valid_name` and global-flag parsing regressions.
- Uploads: fresh, upgrade, same, older, incompatible, corrupt, uses scripts; all four preselection
  rules and when `ask` is true; upgrade keeps the collection; `uploaded` collection behaviour.
- New verbs end to end against the temp root. Jobs: capture, `since` paging, `409`, expiry.
- Settings: defaults on a broken `web.toml`; lock-out guard needs `confirm`.

Front end: `mibpkg --root <fixture> web` on the Mac with a fixture root (installed, update pending,
missing config, card and online collections), then a Playwright smoke pass of every screen and flow;
screenshots in the PR.

On the car (results go into `docs/unit-facts.md`):
- hotspot interface name and address; a phone on the hotspot reaches the server;
- port 80 free or not;
- idle and busy memory/CPU of `mibpkg web`;
- full phone flow: upload → install → logs → config edit → restart → roll back;
- mibpkg self-update from the web UI through restart and page reload;
- a web action and a GEM tap at the same time;
- DISABLE card stops the web server;
- Tailscale access from a laptop.

## Milestones

1. **M1 — web interface**: everything above; GEM gains the Web interface sub-screen. Verified on
   the car.
2. **M2 — slim GEM**: GEM reduced to the single recovery page.

## Out of scope (logged in docs/ideas.md)

- Conflicts between differently named packages (`conflicts = [...]` in the manifest).
- Package icons, release notes / "what's new", curated featured packages (need index/manifest fields).
- HTTPS for the web server; multiple users or roles; installing from a URL typed in the browser.
