# mibpkg start conditions (boot phases) — design

Status: design approved in brainstorming 2026-09-26; spec awaiting review.
Builds on: [2026-09-26-mibpkg-design.md](2026-09-26-mibpkg-design.md). Backlog: [docs/ideas.md](../../ideas.md).

## Goal

Let a package say *when* each of its services may start, so services that need something
(an internet connection, the tailnet, the HMI) do not start before it exists.

Motivating evidence (first managed boot on the MHI2Q, 2026-09-26, docs/unit-facts.md): the
supervisor starts carmqtt ~49 s after boot, but the route to its MQTT broker (over Tailscale)
only works at ~110-170 s; on one boot Tailscale never finished coming up. carmqtt spent minutes
failing to connect.

Success: carmqtt with `after = ["tailnet"]` shows `waiting: tailnet` until the tailnet route
exists, then connects on its first attempt; on a boot where Tailscale is stuck it stays visibly
waiting (or starts after its opt-in timeout).

## Scope

**In:** per-service start conditions `after = [...]`; built-in aliases `internet`, `tailnet`,
`hmi-ready`; raw checks `file:`, `iface:`, `route:`; aliases overridable in `settings.toml`;
opt-in `after_timeout`; `waiting` service state in status and GEM; carmqtt manifest uses it.

**Out (recorded in docs/ideas.md):**
- `phase = "early"` (a second, earlier startup.sh hook so the supervisor starts before the HMI /
  navigation). No concrete use yet; `phase` stays reserved for it.
- `service:<pkg>.<svc>` conditions (service dependencies with health checks) — fits the same
  syntax later.
- Stopping a running service when a condition goes away.

## Terms

- **phase** (existing field): which boot hook starts the supervisor. Only `default` exists.
  Unchanged by this design.
- **condition**: a check that must hold before a service is started. A service's `after` list
  must hold *all* together.

## Manifest

```toml
[[service]]
name          = "carmqtt"
exec          = "@pkg/bin/carmqtt"
args          = ["--config", "@config/carmqtt.toml"]
after         = ["tailnet"]   # optional; default [] = start immediately (v1 behaviour)
after_timeout = "5m"          # optional; <n>s | <n>m | <n>h; omitted = wait forever
```

Validation at build and install time:
- Each `after` entry is a known alias (`internet`, `tailnet`, `hmi-ready`) or a raw check
  (`file:<absolute path>`, `iface:<name>`, `route:<dest>` or `route:<dest>@<iface prefix>`).
  Anything else (including future `service:` entries) is refused with
  "condition \"...\" requires a newer mibpkg" — same rule as unknown phases.
- `file:` paths must be absolute; `iface:` names `[a-z0-9]{1,15}`; `route:` dest non-empty.
- `after_timeout` must parse to 1 s .. 24 h; it is an error to give it with an empty `after`.

An older mibpkg (0.1.0, `deny_unknown_fields`) refuses a manifest with `after` as an unknown
field: safe, less friendly. The official collection ships the mibpkg update before any package
that uses `after`.

## boot.d

`BootEntry` gains `after: Vec<String>` (default empty) and `after_timeout_secs: Option<u64>`
(default none), both `#[serde(default)]`, so existing boot.d files load unchanged. Aliases stay
unresolved in boot.d; the supervisor resolves them with the current settings each check, so a
settings override takes effect without reinstalling packages.

## Checks

Raw checks (the only things that touch the system):

| Check | Holds when | Source |
|---|---|---|
| `file:<path>` | the path exists | `stat` |
| `iface:<name>` | interface is UP and RUNNING and has an `inet` address | `ifconfig <name>` output |
| `route:<dest>[@<iface>]` | routing table has destination `<dest>`, and (if given) its interface name starts with `<iface>` | `/mnt/app/armle/usr/bin/netstat -rn` output (not on PATH) |

Built-in aliases, defaults in `Settings` and overridable in `settings.toml` under `[conditions]`:

```toml
[conditions]
internet  = "route:default"
tailnet   = "route:100.64@tun"          # exact dest spelling set from real QNX netstat output
hmi-ready = "file:/tmp/smFlagStartupDone"  # chosen by on-car timing, see Verification
```

An alias maps to exactly one raw check (a list of raw checks is not needed yet). Settings may
override an alias but not add new alias names (unknown names stay "requires a newer mibpkg", so
packages stay portable across units).

Evidence for the defaults (firmware dump P5087 + unit):
- `internet`: a default route exists for SIM data (`ecm0`, ppp) and Wi-Fi client alike.
  `/tmp/ppp_connected` is maintained for SIM data only (created by connectionmanager; removed by
  `/etc/ppp/ip-down`, `/eso/telephone/ecm-down`, `/etc/wpa_action.sh`), so the route is the
  better general signal.
- `tailnet`: on the stuck-Tailscale boot, `tun9` already had `100.64.0.55` but tailscaled
  stopped before configuring routes; "interface has an address" would have been wrong, "route to
  the tailnet range via tun" is right. Fallback if the route check proves unreliable: a marker
  file written by `tailscale.sh` after `tailscale up` succeeds (owned by the mib2q-tailscale work).
- `hmi-ready`: candidates `/tmp/smFlagStartupDone` (smartphone_integrator.json),
  `/tmp/startupFinishedFlag` (medialauncher.json), `/tmp/MMX-startup-finished`; pick by measuring
  appearance time after boot.

Evaluation cost: checks run once per supervisor tick (1 s) and only for services in `waiting`.
Each tick evaluates each distinct raw check at most once (shared across waiting services);
`netstat -rn` and `ifconfig` are spawned at most once per tick each.

## Supervisor behaviour

New state `waiting` (status text `waiting: <unmet conditions>`, e.g. `waiting: tailnet`).

- Before any start of a service — first start, restart after a crash, restart requested by
  the user, restart after reload — it enters `waiting` if its `after` list is not all met; it
  starts on the first tick where all hold.
- The wait clock starts when the service enters `waiting`. With `after_timeout`, when the clock
  exceeds it, the service starts anyway and keeps a note shown in status:
  `started without <unmet> after <timeout>`. The note clears on the next normal start.
- Conditions are not watched after start: a running service is never stopped because a condition
  went away.
- Order with existing checks: `after` conditions are checked before required-config
  (`needs-config`); a service that is waiting does not count toward the crash-loop window.
- A raw check that cannot be evaluated (e.g. `netstat` missing, unparsable output) counts as not
  met, and the status shows the reason (`waiting: tailnet (netstat: <error>)`), so a broken check
  is visible rather than silently skipped.

## Status and GEM

- `SvcStatus` gains `waiting_for: Vec<String>` (unmet, as written in `after`), `waiting_secs:
  Option<u64>`, and `note: Option<String>` (the timeout note). `mibpkg status` prints
  `carmqtt.carmqtt: waiting: tailnet (2m10s)`.
- The GEM package screen line shows the same text (a snapshot, like all GEM status).

## Code structure

- New `src/conditions.rs`: `Condition` enum (`File(PathBuf)`, `Iface(String)`, `Route { dest,
  iface: Option<String> }`), `parse(&str) -> Result<Condition>`, alias resolution against
  `Settings`, `SystemView` trait (`file_exists`, `iface_up`, `routes`) with the real
  implementation (spawns `ifconfig` / `netstat`, parses their output) and a per-tick cache,
  `unmet(after: &[String], view) -> Vec<String>`.
- `manifest.rs`: `after`, `after_timeout` fields + validation via `conditions::parse`.
- `bootd.rs`: carry the two fields.
- `paths.rs` `Settings`: `conditions` table with the three alias defaults.
- `supervisor.rs`: `Waiting` state, wait clock, timeout, status fields; uses a `SystemView`
  passed in (real on the unit, fake in tests).
- `status.rs`, `gem.rs`, `cli.rs`: show the new state.
- carmqtt `packaging/manifest.toml.in`: `after = ["tailnet"]`, `after_timeout = "5m"`.

## Testing

Host:
- Parser: every raw form, aliases, unknown forms → "requires a newer mibpkg", bad timeouts.
- Settings: alias override applied; unknown alias names rejected.
- `netstat -rn` / `ifconfig` parsers against output captured from the unit (fixtures committed
  under `tests/fixtures/`), including the stuck-Tailscale case.
- Evaluator with a fake `SystemView`.
- Supervisor with real `file:` conditions: waits then starts when the file appears; timeout
  starts anyway with the note; a crash restart waits again; waiting does not consume the
  crash-loop budget.

On the unit:
- Capture `netstat -rn` (Tailscale up and stuck), `ifconfig` for `tun9`/`ecm0`/missing
  interface, and the appearance order/time of the three HMI markers after boot; set the alias
  defaults from them.
- carmqtt with `after = ["tailnet"]`: first MQTT connect attempt after boot succeeds; with
  Tailscale stuck, status shows `waiting: tailnet`, then the 5 m timeout starts it.

## Verification items (before the defaults are final)

1. `netstat -rn` format on QNX 6.5: how `default`, `100.64.0.0/10`-style and per-peer tailnet
   routes and interface names appear.
2. `ifconfig <missing>` exit status and output.
3. Which HMI marker appears last / most meaningfully, and when (seconds after boot).
