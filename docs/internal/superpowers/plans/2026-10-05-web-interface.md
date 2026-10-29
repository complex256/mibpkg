# mibpkg web interface (M1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `mibpkg web` server, started by the supervisor, serving an App-Store-style phone UI that
does everything GEM does plus uploads, logs, config editing and sources, with no way for a request
to inject a command.

**Architecture:** Same binary, new `src/web/` module tree. Reads are served in-process from the same
modules the CLI uses. Every write to flash runs `mibpkg gem <verb> <args…>` as a child process
(argv, no shell) under the existing action lock, so GEM and web share one write path. The front end
is plain ES modules in `web/`, embedded with `include_str!`.

**Tech Stack:** Rust 2021, std only plus existing deps (anyhow, flate2, semver, serde, serde_json,
sha2, toml); vanilla HTML/CSS/JS; Playwright (from the agent session) for the browser smoke test.

**Spec:** `docs/superpowers/specs/2026-10-05-web-interface-design.md`

## Global Constraints

- No new crate dependencies. No async runtime. No JS framework, npm, or build step.
- Every flash write goes through `mibpkg gem <verb>` under `Lock` (`/mnt/app` is read-only and its
  rw remount is refcounted per process only).
- Child processes are started with `Command::new(exe).args(argv)`; never `sh -c`, never `gem.sh`.
- Client input reaches argv only as validated tokens: package names (`valid_name` + known),
  collection ids (known or `-`), server-generated 32-hex ids, decimal indexes, sources accepted by
  `online::index_url`.
- Writes need `X-Mibpkg: 1`; `Host` must be an IP literal or `localhost` (optional port).
- Limits: headers ≤ 16 KB; bodies ≤ 64 KB except uploads ≤ `fetch::MAX_PACKAGE` (50 MB); read
  timeout 10 s (60 s while reading an upload body); ≤ 6 concurrent connections (`503` beyond).
- Defaults: port 80 (fallback 8080 on "address in use"), listen `["hotspot", "tailscale"]`, no PIN.
  Loopback is always allowed.
- Jobs: one at a time, output capped at 64 KB, kept 10 min after finishing. One pending upload,
  expires after 10 min.
- PIN: `"<salt hex>:<sha256(salt||pin) hex>"`; lockout after 5 failures, 60 s doubling, max 1 h;
  sessions in memory, 7 days idle expiry; cookie `HttpOnly; SameSite=Strict; Path=/`.
- Front end: escape by default (`textContent` only for car-supplied text), one view per file,
  header comment per file, comments say why, 40–60 KB total before gzip.
- Release: bump to mibpkg 0.4.0.

## Review Focus

- A phone on the hotspot whose interface name we guessed wrong: the UI must still be reachable
  over loopback/Tailscale, and GEM's Web interface screen must show which interfaces are up so the
  user can set `interfaces.hotspot` → test in Task 7 (unknown interface is reported, not fatal).
- A GEM tap racing a web action: the web side must show busy, never a raw lock error → Task 9 test
  (lock held by another live pid → `409 {"busy":true}` with no job).
- A self-update of mibpkg started from the web: the job's child is killed when the supervisor
  restarts everything; the UI must not show "Failed" → Task 9 (job ends without an `OK:`/`FAILED:`
  line → `result: null, done: true`) and Task 14 (UI treats that as "restarting", polls status).
- Uploading a file that is not a .mpk at all (a .zip, an HTML page): inspect returns a plain
  `problems` entry, never a 500 → Task 10 test.
- A browser that sends `Expect: 100-continue` with a large upload (curl does; some phones do): the
  server must answer `100 Continue` or simply read the body → Task 6 test.

---

## File Structure

| File | Responsibility |
|---|---|
| `src/manifest.rs` (modify) | `valid_name` must start with `[a-z0-9]` |
| `src/cli.rs` (modify) | global flags only before the command; new verbs; `web` command |
| `src/index.rs` (modify) | reserved collection id `uploaded` |
| `src/install.rs` (modify) | split `install` → `install_package`; `install_file`; `uploaded` in `pick_collection` |
| `src/online.rs` (modify) | `state/sources.json`; `effective_sources`; add/remove |
| `src/catalog.rs` (create) | merged per-package view of installed + available (card/online) |
| `src/supervisor.rs` (modify) | built-in `mibpkg.web` service |
| `src/gem.rs` (modify) | "Web interface" sub-screen |
| `src/web/mod.rs` | `run()`, accept loop, connection cap |
| `src/web/settings.rs` | `web.toml` model, defaults, PIN hash, random ids |
| `src/web/http.rs` | request parse / response write / gzip |
| `src/web/net.rs` | `ifconfig` parsing, interface classes, allowed-address cache |
| `src/web/auth.rs` | sessions, login lockout, header/host checks |
| `src/web/jobs.rs` | `Runner` trait, real + recording runners, job table |
| `src/web/actions.rs` | `Action` enum, validation, argv |
| `src/web/upload.rs` | upload store, inspection, collection preselection |
| `src/web/api.rs` | JSON read handlers |
| `src/web/routes.rs` | URL → handler |
| `src/web/assets.rs` | embedded front end, `MIBPKG_WEB_DIR` override |
| `web/**` | front end (see spec) |
| `tests/web_*.rs` | tests per web module; `tests/hardening.rs`; `tests/install_file.rs`; `tests/sources.rs` |
| `scripts/web-fixture.sh` | builds a demo root for local UI work |

---

### Task 1: Hardening (name rule, global flags, reserved collection id)

**Files:** Modify `src/manifest.rs:184`, `src/cli.rs:33-50`, `src/index.rs:59-104`; Test: `tests/hardening.rs`

**Interfaces:** Produces `manifest::valid_name(&str) -> bool` (stricter), `index::RESERVED_COLLECTION: &str = "uploaded"`.

- [ ] **Step 1: failing tests**

```rust
use mibpkg::manifest::valid_name;
#[test]
fn names_must_start_with_letter_or_digit() {
    for bad in ["--root", "-x", "-", "", "a/b", "A", "x;y", &"a".repeat(41)] {
        assert!(!valid_name(bad), "{bad:?}");
    }
    for good in ["carmqtt", "0day", "car-mqtt", "a"] {
        assert!(valid_name(good), "{good:?}");
    }
}
#[test]
fn global_flags_stop_at_the_command_word() {
    // `install --root /etc` must treat "--root" as the package argument, not a global flag.
    let f = tempfile::tempdir().unwrap();
    let args: Vec<String> = ["--root", f.path().to_str().unwrap(), "--no-supervisor", "install", "--root", "/etc"]
        .iter().map(|s| s.to_string()).collect();
    let e = mibpkg::cli::run(&args).unwrap_err();
    assert!(format!("{e:#}").contains("not in any collection") || format!("{e:#}").contains("invalid"), "{e:#}");
}
#[test]
fn collection_id_uploaded_is_reserved() { /* index json with id "uploaded" → Collection::load errs "reserved" */ }
```

- [ ] **Step 2:** `cargo test --test hardening` → FAIL.
- [ ] **Step 3:** `valid_name`: first byte `is_ascii_lowercase() || is_ascii_digit()`. In `cli::run`,
  only consume `--root`/`--no-supervisor` while no command word has been seen. In `Collection::load`
  and `from_online`, and in `build_index*`, bail if id == `RESERVED_COLLECTION`. In
  `install::install`/`pick_collection` reject package arguments failing `valid_name` with
  `"invalid package name"` before touching anything.
- [ ] **Step 4:** `cargo test` → all PASS (existing suites unaffected).
- [ ] **Step 5:** commit `fix: names start with a letter or digit; global flags only before the command; reserve collection id "uploaded"`.

### Task 2: Install from a file

**Files:** Modify `src/install.rs`; Test: `tests/install_file.rs`

**Interfaces:**
- Produces `install::install_package(ctx: &Ctx, media: &[PathBuf], pkg: &Package, coll_id: &str, coll_name: &str) -> Result<String>` — everything from the repair check onward (space check, extract, scripts, commit, state, migrate, self-update, reload).
- Produces `install::install_file(ctx: &Ctx, media: &[PathBuf], colls: &[Collection], path: &Path, name: &str, chosen: Option<&str>) -> Result<String>`. `chosen`: `Some("-")` → `uploaded`; `Some(id)` must be a loaded collection id; ignored for upgrades.
- `pick_collection` with recorded collection `uploaded` → `bail!("{name} was installed from an uploaded file; remove it first to install it from a collection")`.

- [ ] **Step 1: failing tests** (fixture helpers from `tests/common`): fresh install from file with
  `chosen=Some("complex256")` records that collection; `Some("-")` records `uploaded` and
  `updates_available` ignores it; upgrade keeps the recorded collection even when `chosen` names
  another; same version → `"demo 1.0.0 is already installed"`; older → error mentions "Roll back";
  name mismatch (`name` arg ≠ manifest) → error; incompatible `[compat] units=["MHI2"]` → error
  "not compatible"; corrupt file → error, nothing mounted rw; unknown `chosen` id → error;
  install from collection after `uploaded` → the "uploaded file" error.
- [ ] **Step 2:** run → FAIL.
- [ ] **Step 3:** move the body of `install` from `// `current` already names this version…` into
  `install_package`, taking `pkg.manifest.package.{name,version}` as name/version and loading
  `state`/`previous` itself. `install` keeps selection, download, sha check, name/version check,
  then calls `install_package`. `install_file` = `Package::read` → name check → compat check →
  compare with installed (Equal/Less as in `install`) → choose collection → `install_package`.
  `coll_name` for `uploaded` is `"Uploaded file"`.
- [ ] **Step 4:** `cargo test` → PASS (all existing install tests still pass unchanged).
- [ ] **Step 5:** commit `feat: install a package from a .mpk file`.

### Task 3: Online sources managed from the unit

**Files:** Modify `src/online.rs`; Test: `tests/sources.rs`

**Interfaces:**
- `online::SourceOrigin { Default, Settings, Added }`; `online::EffectiveSource { source: String, origin: SourceOrigin, enabled: bool }` (Serialize).
- `online::effective_sources(layout: &Layout) -> Vec<EffectiveSource>` — settings list (origin `Default` when equal to the built-in default list, else `Settings`) + `added`; `enabled=false` for `disabled` entries.
- `online::add_source(layout, source) -> Result<String>`, `online::remove_source(layout, source) -> Result<String>` (caller holds `/mnt/app` rw). Remove: `Added` → drop from `added`; others → push to `disabled`; delete cache files whose `Cached.source == source`.
- `fetch_all` iterates enabled effective sources.
- File `state/sources.json`: `{"added": [...], "disabled": [...]}`.

- [ ] **Step 1: failing tests:** default list shows `complex256-mibpkg-packages` as Default/enabled;
  add valid → appears as Added; add invalid (`http://x`, `github:a`, `github:a/b/c`, `$(id)`) →
  error from `index_url`; add duplicate → error "already"; remove Added → gone; remove Default →
  `enabled=false` and its cache file deleted; re-add a disabled one → re-enabled.
- [ ] **Steps 2–4:** run → FAIL; implement; `cargo test` → PASS.
- [ ] **Step 5:** commit `feat: online sources can be added and removed on the unit`.

### Task 4: New action verbs in the CLI

**Files:** Modify `src/cli.rs` (`action`, the `"gem"` arm, USAGE), create `src/web/settings.rs` (only the parts verbs need: `WebSettings`, `load`, `save`, `path`); Test: `tests/cli.rs` additions

**Interfaces (argv shapes the web layer builds in Task 9):**

| argv after `gem` | Result key | Does |
|---|---|---|
| `install-file <name> <id32> <coll\|->` | `<name>` | `install_file(.., layout.run("upload.<id>.mpk"), name, Some(coll))` |
| `config-set <name> <n> <id32>` | `<name>` | `config::import_file(layout, name, &m.configs[n].path, run("text.<id>"))` under rw; then `request_restart` for each service; deletes the text file |
| `source-add <source>` / `source-remove <source>` | `_` | Task 3 under rw; `source-add` then runs `online_check` |
| `web-settings <id32>` | `_` | read `run("text.<id>")` as JSON `WebSettings`, validate, save `home/web.toml` under rw, delete the text file |
| `web-reset` | `_` | delete `home/web.toml` under rw, `request_restart("mibpkg.web")` |
| `web-restart` | `_` | `request_restart("mibpkg.web")` |

Ids must match `^[0-9a-f]{32}$` (helper `is_id(&str) -> bool` in `web::settings`); `<n>` must parse as `usize` < `configs.len()`.

`web::settings::WebSettings { enabled: bool, port: u16, listen: Vec<String>, pin: Option<String>, interfaces: BTreeMap<String, Vec<String>> }` with serde defaults (`true`, `80`, `["hotspot","tailscale"]`, `None`, empty). `validate()`: listen ⊆ {hotspot, tailscale, any}, non-empty, `any` alone; port ≥ 1; pin None or `<32 hex>:<64 hex>`; interface names `[a-z0-9]{1,15}`. `load(layout) -> (WebSettings, Option<String /*warning*/>)` never fails (defaults + warning).

- [ ] **Step 1: failing tests** via `cli::run` with `--root`: `gem install-file` happy path prints
  `OK: installed demo 1.0.0` and records the result under `demo`; bad id (`../x`, `ZZ…`) → `FAILED:
  invalid id`; `gem config-set demo 0 <id>` writes `state/config/demo/<path>` and a restart request
  file `/tmp/mibpkg.restart.demo.demo`; `n` out of range → FAILED; `gem web-settings` with
  `{"listen":["any","hotspot"]}` → FAILED (any must be alone); valid → `web.toml` written;
  `gem web-reset` removes it and leaves `/tmp/mibpkg.restart.mibpkg.web`.
- [ ] **Steps 2–4:** FAIL → implement → `cargo test` PASS.
- [ ] **Step 5:** commit `feat: install-file, config-set, source, and web-settings action verbs`.

### Task 5: Web settings, PIN and randomness

**Files:** Extend `src/web/settings.rs`, create `src/web/mod.rs` (module declarations only); Test: `tests/web_settings.rs`

**Interfaces:** `random_id() -> String` (32 hex; `/dev/urandom`, else sha256 of time-nanos + pid + a process counter + stack address — logged once as "weak randomness"); `hash_pin(pin: &str) -> String`; `check_pin(stored: &str, pin: &str) -> bool` (constant-time compare of the hex digest); `valid_pin(pin) -> bool` (4–12 digits).

- [ ] Tests: defaults when file missing; warning + defaults when file is garbage; round trip; hash
  differs per call (salt) yet `check_pin` accepts; wrong pin rejected; `random_id` is 32 hex and
  differs across calls.
- [ ] FAIL → implement → PASS → commit `feat: web settings file, PIN hashing, random ids`.

### Task 6: HTTP layer

**Files:** `src/web/http.rs`; Test: `tests/web_http.rs`

**Interfaces:**
```rust
pub struct Request { pub method: String, pub path: String, pub query: Vec<(String, String)>,
                     pub headers: Vec<(String, String)>, pub body: Vec<u8> }
impl Request { pub fn header(&self, name: &str) -> Option<&str>; pub fn query(&self, k: &str) -> Option<&str> }
pub enum ReadError { TooLarge, BadRequest(String), Io(std::io::Error) }
pub fn read_request(stream: &mut impl BufRead /*+ Write for 100-continue*/, body_limit: impl Fn(&str, &str) -> u64) -> Result<Request, ReadError>
pub struct Response { pub status: u16, pub headers: Vec<(String, String)>, pub body: Vec<u8> }
impl Response { pub fn json(status: u16, v: &impl Serialize) -> Self; pub fn text(..); pub fn error(status, msg) -> Self /* {"error":msg} */ }
pub fn write_response(w: &mut impl Write, r: Response, gzip_ok: bool) -> io::Result<()>
```
Percent-decoding of path and query; header names case-insensitive; `Content-Length` required for
bodies, chunked → 400/411; `Expect: 100-continue` → write `HTTP/1.1 100 Continue\r\n\r\n` before
reading the body; gzip only for bodies ≥ 512 bytes of text/JSON/JS/CSS when `Accept-Encoding`
has `gzip`; always `Connection: close`, `Cache-Control: no-store` for `/api/`.

- [ ] Tests: simple GET; query decoding (`a=%20b&c`); header > 16 KB → TooLarge; body over limit →
  TooLarge before reading; missing CL with body method → 411; chunked → 400; 100-continue handled
  (a `Cursor` + `Vec<u8>` duplex helper); non-HTTP junk → BadRequest; gzip round-trip via flate2.
- [ ] FAIL → implement → PASS → commit `feat: minimal HTTP/1.1 request/response layer`.

### Task 7: Interface filter

**Files:** `src/web/net.rs`; fixtures `tests/fixtures/ifconfig-all-unit-guess.txt`, `ifconfig-all-mac.txt`; Test: `tests/web_net.rs`

**Interfaces:**
```rust
pub struct Iface { pub name: String, pub up: bool, pub v4: Vec<Ipv4Addr> }
pub fn parse_ifconfig_all(text: &str) -> Vec<Iface>
pub fn class_of(iface: &Iface, overrides: &BTreeMap<String, Vec<String>>) -> Option<&'static str> // "hotspot" | "tailscale" | None
pub struct Allowed { /* cache */ }
impl Allowed {
    pub fn new(settings: &WebSettings, probe: Box<dyn Fn() -> Option<String> + Send + Sync>) -> Self;
    pub fn permits(&self, local: IpAddr) -> bool; // loopback always; "any" always; refresh when > 10 s old
    pub fn urls(&self, port: u16) -> Vec<(String /*class*/, String /*url*/)>;
    pub fn interfaces_up(&self) -> Vec<(String, String /*addr*/)>;
}
```
Classes: overrides win (`interfaces = { hotspot = ["uap0"] }`); else `tun*` with an address in
`100.64.0.0/10` → tailscale; `lo*` → none (loopback handled separately); `ecm*`, `ppp*`, `rmnet*`,
`wwan*` → none (cellular); any other up interface with an RFC1918 IPv4 → hotspot. Probe default:
`run_limited("ifconfig", [], 3 s)`.

- [ ] Tests: parse Mac and unit fixtures; tun9 with 100.64.0.55 → tailscale; ecm0 with 10.x → none;
  `uap0` 10.255.1.1 → hotspot; override makes `en5` hotspot; `permits` refuses an ecm0 address,
  accepts loopback, accepts all under `any`; a probe that starts returning tun9 later is seen after
  the refresh interval (inject a clock: `Allowed::with_clock`).
- [ ] FAIL → implement → PASS → commit `feat: per-connection interface filter`.

### Task 8: Catalog and read API

**Files:** `src/catalog.rs`, `src/web/api.rs`; Test: `tests/web_api.rs`

**Interfaces:**
```rust
// catalog.rs
#[derive(Serialize)] pub struct Avail { pub collection: String, pub collection_name: String, pub origin: &'static str /*card|online*/, pub version: String, pub size: u64, pub description: String, pub compatible: bool, pub uses_scripts: bool, pub sha256: String }
#[derive(Serialize)] pub struct Svc { pub id: String, pub state: String, pub text: String }
#[derive(Serialize)] pub struct Inst { pub version: String, pub collection: String, pub previous: Option<String>, pub services: Vec<Svc>, pub uses_scripts: bool, pub missing_config: Vec<String>, pub last_result: Option<String>, pub configs: Vec<ConfigInfo> }
#[derive(Serialize)] pub struct ConfigInfo { pub index: usize, pub path: String, pub required: bool, pub exists: bool }
#[derive(Serialize)] pub struct PkgView { pub name: String, pub description: String, pub installed: Option<Inst>, pub available: Vec<Avail>, pub update: Option<String> }
pub fn build(layout: &Layout, unit: &UnitInfo) -> Vec<PkgView>
pub fn collections(layout: &Layout) -> Vec<Collection> // card (loaded) + online
// api.rs
pub fn status(app: &App) -> Response; pub fn packages(app: &App) -> Response;
pub fn log_tail(app: &App, id: &str, bytes: u64) -> Response; // ids: "mibpkg" -> run("log"); a known service id -> run("<id>.log")
pub fn config_get(app: &App, name: &str, n: usize) -> Response;
pub fn sources(app: &App) -> Response; pub fn settings(app: &App) -> Response;
```
`App` is defined in `src/web/mod.rs` (Task 11) — for this task create it with `layout`, `unit`,
`settings`, `sys: Box<dyn Sys + Send + Sync>`, `allowed: Allowed`, `port: u16`; later tasks add fields.

- [ ] Tests (fixture root, one installed + one card collection + one online cache): packages lists
  both with `update` set; `missing_config` filled; log ids reject `../x`, unknown ids → 404; tail
  returns at most `bytes`; config_get returns example when missing; status JSON has `version`,
  `busy=false`; settings never contains the pin hash.
- [ ] FAIL → implement → PASS → commit `feat: package catalog and read API`.

### Task 9: Jobs and actions

**Files:** `src/web/jobs.rs`, `src/web/actions.rs`; Test: `tests/web_actions.rs`

**Interfaces:**
```rust
pub trait Runner: Send + Sync { fn start(&self, argv: Vec<String>, job: Arc<Job>) -> io::Result<()>; }
pub struct ProcessRunner { pub exe: PathBuf, pub root: PathBuf } // argv: --root <root> gem <argv…>; stdout+stderr lines → job, also appended to run("log")
pub struct RecordingRunner { pub seen: Mutex<Vec<Vec<String>>> } // finishes jobs with "OK: recorded"
pub struct Job { pub id: String, pub label: String, /* Mutex<JobState{lines, done, ok, result, finished_at}> */ }
pub struct Jobs { /* running: Option<Arc<Job>>, done: Vec<Arc<Job>> */ }
impl Jobs { pub fn start(&self, runner: &dyn Runner, label: String, argv: Vec<String>) -> Result<Arc<Job>, Busy>; pub fn get(&self, id: &str) -> Option<Arc<Job>>; pub fn running(&self) -> Option<Arc<Job>>; }
pub fn job_json(job: &Job, since: usize) -> serde_json::Value // {id,label,lines,next,done,ok,result}

#[derive(Deserialize)] #[serde(tag = "action", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Action { Rescan, Check, UpdateAll, Install { package: String, collection: Option<String> }, Rollback { package: String }, Restart { package: String }, Remove { package: String, #[serde(default)] purge: bool } }
pub fn validate(a: &Action, known: &Known) -> Result<Vec<String /*argv after gem*/>, String>
pub struct Known { pub installed: BTreeSet<String>, pub available: BTreeSet<String>, pub collections: BTreeSet<String> }
pub fn lock_busy(layout: &Layout, sys: &dyn Sys) -> bool // lock file names a live pid
```
`result` = the last line starting with `OK:` or `FAILED:`, else `None`; `ok` = result starts with `OK:`.

- [ ] **Injection table test** — for every action variant and every string field, each of
  `[";reboot", "$(id)", "`id`", "a\nb", "--root", "-x", "../../etc", "x".repeat(5000), "ü", " carmqtt", "CARMQTT"]`
  → `validate` is `Err` and `RecordingRunner.seen` stays empty; JSON with an unknown action or extra
  field → parse error. Valid: `restart carmqtt` → `["restart","carmqtt"]`; install with collection
  `-` rejected for collection installs (only uploads use `-`).
- [ ] Jobs tests: second start while running → `Busy(job)`; `since` paging; 64 KB cap keeps the
  final result line; expiry after 10 min with an injected clock; `lock_busy` true for a lock file
  holding this test's own pid, false for a dead pid.
- [ ] `ProcessRunner` test using a fake exe (a shell script in the temp dir that echoes its args and
  `OK: done`) — argv arrives verbatim, including a `$(id)` literal inside one argument only when
  constructed directly (proves no shell).
- [ ] FAIL → implement → PASS → commit `feat: typed actions and jobs`.

### Task 10: Uploads

**Files:** `src/web/upload.rs`; Test: `tests/web_upload.rs`

**Interfaces:**
```rust
pub struct Uploads { /* Mutex<Option<Pending{id, path, created}>> */ }
impl Uploads { pub fn store(&self, layout: &Layout, body: &[u8]) -> io::Result<String /*id*/>; pub fn path(&self, id: &str) -> Option<PathBuf>; pub fn discard(&self, id: &str); pub fn sweep(&self, layout: &Layout) /* expiry + leftover run("upload.*") */ }
#[derive(Serialize)] pub struct Inspection { pub upload_id: String, pub name: Option<String>, pub version: Option<String>, pub description: String, pub size: u64, pub sha256: String, pub kind: Option<&'static str /*fresh|upgrade|same|older*/>, pub installed_version: Option<String>, pub installed_collection: Option<String>, pub compatible: bool, pub uses_scripts: bool, pub services: Vec<String>, pub problems: Vec<String>, pub candidates: Vec<Candidate>, pub preselected: Option<String>, pub ask: bool }
#[derive(Serialize)] pub struct Candidate { pub collection: String, pub name: String, pub version: String, pub matches_file: bool }
pub fn inspect(layout: &Layout, unit: &UnitInfo, colls: &[Collection], id: &str, path: &Path) -> Inspection
```
Preselection per spec (sha match → single → newest, official `complex256*` wins ties → `-`), `ask`
only when ≥ 2 candidates and none matches the file. `"-"` is the id for "None".

- [ ] Tests: all four preselection cases and `ask`; upgrade → `kind=upgrade`, candidates empty,
  `installed_collection` set; same/older → problems; incompatible → problems; not a gzip / an HTML
  body / empty body → a problem "not a mibpkg package", no panic; manifest name invalid → problem;
  store replaces a previous pending upload (old file removed); sweep removes expired + stray files.
- [ ] FAIL → implement → PASS → commit `feat: upload inspection and collection preselection`.

### Task 11: Auth, routes and the server

**Files:** `src/web/auth.rs`, `src/web/routes.rs`, `src/web/assets.rs` (stub serving a placeholder
`index.html` until Task 13), finish `src/web/mod.rs`, `src/cli.rs` (`web` command), `src/lib.rs`;
Test: `tests/web_server.rs`

**Interfaces:**
```rust
pub struct App { layout, unit, settings: WebSettings, sys, allowed: Allowed, port: u16, jobs: Jobs, uploads: Uploads, auth: Auth, runner: Box<dyn Runner>, started: Instant, exit_after_response: AtomicBool }
pub fn handle(app: &App, req: Request, local: IpAddr) -> Response // routes.rs: auth + checks + dispatch
pub fn serve(app: Arc<App>, listener: TcpListener) // accept loop, cap 6, per-conn thread, timeouts, filter
pub fn run(layout: Layout) -> Result<()> // settings → bind (80 → 8080 fallback) → write run("web.url") → serve
// auth.rs
pub struct Auth { /* sessions: Mutex<HashMap<token, last_seen>>, failures, locked_until */ }
impl Auth { pub fn login(&self, settings: &WebSettings, pin: &str, now: Instant) -> Result<String /*token*/, LoginError /*Wrong | LockedFor(Duration)*/>; pub fn check(&self, cookie_header: Option<&str>, now: Instant) -> bool; pub fn logout(&self, cookie_header: Option<&str>); }
pub fn host_ok(host: Option<&str>) -> bool
```
Route table exactly as the spec's HTTP API section. Writes: `X-Mibpkg: 1` else 403; `Host` check on
every request; PIN check on every `/api/` except `session` and `login` when a PIN is set. `PUT
/api/settings`: build `WebSettings` from body (+ `hash_pin` for a new pin; `clear_pin`), refuse
when the request's local address would no longer be permitted unless `confirm`, write JSON to
`run("text.<id>")`, run `web-settings <id>` synchronously (wait ≤ 30 s), respond, then set
`exit_after_response` (the connection thread calls `std::process::exit(0)` after flushing). `PUT
config` and `POST sources` similarly write the text file then start a job.

- [ ] Tests over a real `TcpListener` on `127.0.0.1:0` with `RecordingRunner`: GET `/` 200 HTML;
  POST without header → 403; bad Host `evil.example` → 403; `Host: 127.0.0.1:port` OK; with PIN
  set: `/api/status` → 401, login wrong ×5 → locked (429 with `retry_after`), right PIN → cookie
  flags present and status 200; action → 202 then job poll shows `OK: recorded`; second action
  while one runs (runner that never finishes) → 409 with job id; 7th concurrent connection → 503;
  upload → inspect JSON → install → recorded argv `install-file <name> <id> <coll>`.
- [ ] `mibpkg --root <tmp> web` boots against a temp root with `port = 0` support (`port = 0` →
  OS-assigned, URL file written) — test spawns the real binary (`env!("CARGO_BIN_EXE_mibpkg")`),
  reads `tmp/mibpkg.web.url`, GETs `/api/status`, kills it.
- [ ] FAIL → implement → PASS → commit `feat: mibpkg web server with PIN, routes and checks`.

### Task 12: Supervisor service, status and GEM screen

**Files:** Modify `src/supervisor.rs`, `src/cli.rs` (`print_status`), `src/gem.rs`; Test: `tests/supervisor.rs`, `tests/gem.rs` additions

- [ ] `Supervisor::reload` appends a built-in `BootEntry { package: "mibpkg", name: "web", exec: "@self", args: ["web"], restart: Always, .. }` when `web::settings::load(layout).0.enabled`. `start()` special-cases `exec == "@self"`: no `current_dir`, no copy; runs `layout.tmp_bin()` (tests: `Supervisor::with_self_exe(path)`) with `--root <root> web`, cwd `home()`. Its log is `run("mibpkg.web.log")`.
- [ ] `print_status` adds `web: <url or "not running">` from `run("web.url")`.
- [ ] GEM: main screen button-row "Web interface" → sub-screen `mibpkg_web` with: Live row for
  `service_key("mibpkg.web")`, text `URL: …` (from `web.url`), text `listen: hotspot, tailscale`,
  text `PIN: set|not set`, buttons `Restart web server` (`web-restart`), `Reset web settings`
  (`web-reset`).
- [ ] Tests: reload with default settings includes `mibpkg.web`; with `enabled = false` it doesn't;
  status JSON lists it; GEM screens include `mibpkg_web` with both buttons and script files
  `web-restart.sh`, `web-reset.sh`.
- [ ] commit `feat: supervisor runs the web server; GEM web interface screen`.

### Task 13: Front end — shell, core modules, Discover/Updates/Installed/Package/More

**Files:** `web/index.html`, `web/style.css`, `web/js/{app,api,router,dom,jobs,icons}.js`,
`web/js/views/{discover,updates,installed,package,more}.js`, `src/web/assets.rs` (real table),
`scripts/web-fixture.sh`

- [ ] `scripts/web-fixture.sh <dir>`: builds a root with an installed `carmqtt` (with a required
  config missing), an update on the card, an online cache with two more packages, a second
  collection sharing a name; prints the `mibpkg --root <dir> web` command (port 8099 via web.toml).
- [ ] Implement modules per the spec's Code section; App Store look per the approved mockup
  (`appstore.html`): letter icons, GET/UPDATE/OPEN pills, badge on Updates, facts strip, live log
  under a running row. Job polling every 500 ms; a job that ends with `result == null` after an
  update of `mibpkg` → "mibpkg is restarting…" then poll `/api/status` until the version changes,
  then `location.reload()`.
- [ ] Assets: `include_str!` each file; content types html/css/js; `MIBPKG_WEB_DIR` serves from disk.
- [ ] Verify with Playwright against the fixture: each tab renders, search filters, install from
  card shows the live log and ends ✓, package page actions work, screenshots saved.
- [ ] commit `feat: web front end — discover, updates, installed, package pages`.

### Task 14: Front end — upload, logs, config, sources, settings, login

**Files:** `web/js/views/{upload,logs,config,sources,settings,login}.js`, CSS sections

- [ ] Upload: picker/drop zone + progress (`XMLHttpRequest` upload progress), confirm page per
  `upload-v2.html` (collapsed "Updates from" unless `ask`), install log page.
- [ ] Logs: list (mibpkg, web, each service), tail with refresh + auto-scroll.
- [ ] Config: textarea with content/example, Save → job log, required/missing badge.
- [ ] Sources: list with origin and enabled, add form, remove.
- [ ] Settings: URLs, listen checkboxes (any greys others + warning), PIN change/remove, lock-out
  `confirm` dialog (in-page, not `window.confirm`), restart wait → reload at the new URL if needed.
- [ ] Login: PIN pad; `401` anywhere routes here and back.
- [ ] Playwright: upload a fixture `.mpk` → confirm → install → ✓; config edit; add/remove source;
  set and use a PIN; screenshots.
- [ ] commit `feat: web front end — upload, logs, config, sources, settings, login`.

### Task 15: Docs, version, cross-build, final checks

- [ ] README: web interface section (how to reach it, PIN, recovery). `docs/unit-facts.md`: a
  "Pending on-car checks (web)" list from the spec (+ `/dev/urandom` exists). Spec: log name
  `/tmp/mibpkg.mibpkg.web.log`, all flash writes via verbs (`web-settings`, `web-reset`).
- [ ] `Cargo.toml` version 0.4.0; `packaging` unchanged (single binary).
- [ ] `cargo test`, `cargo clippy` clean of new warnings; `scripts/build.sh` if Docker is available
  (report binary size delta).
- [ ] commit `chore: mibpkg 0.4.0 (web interface)`.

---

## M2 (after M1 is verified on the car — not part of this execution)

Slim GEM: `gem::screens` returns only the `mibpkg` page with the web status/URL/listen/PIN rows and
the buttons Restart web server, Reset web settings, Update mibpkg from SD card (`install mibpkg`);
`regenerate` removes the per-package `.esd` and scripts. Its own short plan once M1 facts are in.
