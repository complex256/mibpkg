# mibpkg online updates (v2) — design

Status: design approved in brainstorming 2026-09-30; spec awaiting review.
Builds on: [2026-09-26-mibpkg-design.md](2026-09-26-mibpkg-design.md),
[2026-09-26-start-conditions-design.md](2026-09-26-start-conditions-design.md).
Evidence: [spikes/README.md](../../../spikes/README.md), [docs/unit-facts.md](../../unit-facts.md).

## Goal

Cars with an internet connection find package updates themselves, and anyone can publish their
own packages with no central repository or hosting run by the mibpkg owner. The owner's only
ongoing role is merging pull requests to a community list.

Success:
- A publisher goes from nothing to a signed online collection with `mibpkg keygen --github`,
  one workflow file, and a tag push.
- A car with internet shows "Update to X available (online)" in GEM, installs it on a tap, and
  refuses a collection whose signing key changed unless the user explicitly trusts the new key.
- Cars without internet, and SD installs, behave exactly as today.

## Decisions (from brainstorming)

- Hosting: plain HTTPS files (approach A). A source is `github:<owner>/<repo>` (= that repo's
  latest release) or any `https://…/index.json`. No GitHub API, no registry protocols.
- Discovery: both a community list repo (tap-to-enable in GEM) and an SD `sources.toml`.
- Updates: the car checks in the background; nothing is downloaded or installed without a tap.
- Trust: publisher ed25519 signatures with trust on first use (key pinned when a source is enabled).
- Signing must be frictionless for developers and CI.

## Sources

Every publisher, including the mibpkg owner, publishes the same way: a repo's own releases are its
source (an `index.json` plus its `.mpk` files as release assets). There is no central package
repository or central index of packages; the only central piece is the community *list of
sources* below, which changes only when a new publisher joins. Each release updates only the
publisher's own repo, so no one else has to merge anything.

- The official source is mibpkg's own source repo, `github:complex256/mibpkg` (its releases also
  carry the SD bundle zip). carmqtt and other owner packages are separate sources from their own
  repos, published when the owner chooses to.
- Transition (Milestone 1): until the source repo is public, the built-in default is the
  release-only repo `github:complex256/mibpkg-packages` (release v1 = mibpkg 0.3.0). The first
  mibpkg release after the source repo goes public changes the default to `github:complex256/mibpkg`
  and is also published to `mibpkg-packages`, so 0.3.0 cars move over; `mibpkg-packages` is then
  retired (deleted, or renamed to be the community list).

Source strings:

| Form | Index URL |
|---|---|
| `github:<owner>/<repo>` | `https://github.com/<owner>/<repo>/releases/latest/download/index.json` |
| `https://…/index.json` | as given |

An index entry's `file` is either relative (resolved against the index URL, e.g. the release
download directory) or an absolute `https://` URL (used for older versions kept in earlier
releases). `http://` and other schemes are refused.

### Community list

A public GitHub repo (`github:complex256/mibpkg-sources`, possibly the renamed `mibpkg-packages`, built in as the default list, overridable
in settings.toml `online.list`) whose latest release carries `sources.json`:

```json
{ "format": 1,
  "sources": [
    { "name": "complex256 packages", "description": "carmqtt, mibpkg",
      "source": "github:complex256/mibpkg",
      "key": "ed25519:<base64 public key>" } ] }
```

Publishers add themselves by pull request. The list repo's CI fetches each entry and runs
`mibpkg verify` against the listed key. The list is itself signed with the list maintainer's key,
built into mibpkg (a compromised list repo cannot push entries to cars).

### SD sources file

`/mibpkg/sources.toml` on a card, imported on Rescan:

```toml
[[source]]
name   = "alice's tools"
source = "github:alice/mib-tools"
key    = "ed25519:<base64>"   # optional; if absent, pinned on the first successful fetch
```

### Enabling

Sources from the list and from `sources.toml` appear on a GEM **mibpkg_online** screen with an
**Enable** button each; nothing is fetched from a source until it is enabled. The official
collection is enabled by default. Enabled sources are stored in `state/installed.json`
(`online.sources[]`: source, name, pinned key, enabled-at).

## Trust

- Keys: ed25519. Public key text `ed25519:<base64 32 bytes>`; secret key text
  `mibpkg-secret:<base64 32-byte seed>` (one line, pasteable into a CI secret).
- `index.json` gets its reserved `signatures` array filled:
  `[{ "alg": "ed25519", "key": "ed25519:…", "sig": "<base64>" }]`. The signature covers the
  canonical bytes of the index with `signatures` removed (serde_json serialization of the index
  struct with `signatures: []`, keys in struct order). Package integrity follows from the per-entry
  sha256 inside the signed index.
- Pinning (trust on first use): on Enable, the key from the list entry, else from `sources.toml`,
  else the key of the first successfully fetched and self-consistent signed index, is pinned.
- Every fetched index must carry a valid signature by the pinned key. Otherwise the source shows
  `key changed - not trusted` (or `unsigned`) on its GEM screen, none of its packages are offered,
  and only an explicit **Trust new key** button re-pins (showing old and new key fingerprints).
- SD collections: unsigned is allowed as today, unless the collection id is pinned by an enabled
  online source, in which case the SD copy must verify against that key.
- Collection ids are unique per car: an enabled source that publishes an id already owned by
  another enabled source is refused. A publisher with several repos uses one id per repo: the
  official source is `complex256-mibpkg` (renamed from `complex256` on 2026-09-30 while the
  owner's car was the only install; that car moved over with one `mibpkg install mibpkg
  complex256-mibpkg` over ssh), carmqtt will be `complex256-carmqtt`. (Possible relaxation, not planned: sources signed
  by the same pinned key may share an id.)

## Car side

### Network time

- A single SNTP query (UDP 123) to `time.cloudflare.com`, then `pool.ntp.org`, gives
  `offset = network time - system time`, stored in `/tmp/mibpkg.clock_offset`. mibpkg never relies
  on other packages' files (Tailscale is optional and will be its own package), so it always gets
  time through its own query.
- The system clock is never changed. The offset is applied only to TLS certificate checks.

### Fetching

- New `fetch` module: dlopen `libcurl.so.2` (`/mnt/app/armle/usr/lib`), hand-written FFI;
  `CURLOPT_SSL_CTX_FUNCTION` → `SSL_CTX_get0_param` → `X509_VERIFY_PARAM_set_time(now + offset)`
  (verified on the unit, spikes/README.md). Redirects followed; User-Agent `mibpkg/<version>`.
- CA bundle `ca.pem` ships inside the mibpkg package (roots for GitHub, Let's Encrypt, Google Trust
  Services, Cloudflare), so updating mibpkg updates the roots.
- Limits: 30 s per request; index ≤ 1 MB; package ≤ 50 MB. Downloads go to `/tmp` (RAM).
- A `Fetcher` trait so everything above it is testable with a fake.

### Checks

- The supervisor spawns `mibpkg online-check` as a child process (never blocking its loop) once
  per boot when the `internet` condition holds and a clock offset is available, then every 6 h
  while running. GEM **Check for updates** runs the same check on demand.
- A check fetches the community list (if its cache is older than 24 h) and each enabled source's
  index, verifies signatures, and caches verified indexes in `state/online/<collection-id>.json`
  (rewritten only when changed, flash).
- Result published live (persistence key) for the main screen: `online: checked at <n> s,
  <k> update(s)`, or `online: no internet` / `no clock yet` / `<source>: key changed`.

### Installing

- Online collections load like SD collections (`Collection` with a base URL). If an SD card and an
  online source offer the same collection, the newest compatible version is used.
- Install/update of an online package (on a tap): download the `.mpk` to `/tmp`, check size and
  sha256 against the signed index, then the existing install path (commit point, rollback, config,
  migration). The download is deleted afterwards. Progress lines include `downloading <name> <ver>`.

### GEM

- Main screen: live `online:` row; **Check for updates** button; entry to **mibpkg_online**.
- mibpkg_online: one sub-screen per source: name, source, key fingerprint, state
  (enabled / available / key changed / unsigned / error), **Enable**, **Trust new key**,
  **Disable**.
- Package screens: `online [<id>]: <version>` lines next to card versions; Install/Update buttons
  as today.

## Publisher tooling (host)

Frictionless and CI-friendly:

- `mibpkg keygen` — writes the secret to `~/.config/mibpkg/signing.key` (0600), prints the public
  key. `--github <owner/repo>` also stores the secret as the repo secret `MIBPKG_SIGNING_KEY` via
  the `gh` CLI when available, and prints the public key for the list entry/README.
- Signing is automatic: `mibpkg index` and `mibpkg release` sign whenever a key is available —
  `MIBPKG_SIGNING_KEY` (env, secret text), else `MIBPKG_SIGNING_KEY_FILE`, else
  `~/.config/mibpkg/signing.key`. `--no-sign` opts out.
- `mibpkg release <pkg-dir>... --from <source> [--out out/]` — builds packages, fetches the
  previous release's index from `<source>` and keeps its entries as absolute URLs (old versions
  stay installable), adds the new ones, signs, writes `out/` (index.json + new .mpk files) ready to
  attach to a release.
- Key-rotation guard: `release`/`index` refuse when the previous index was signed by a different
  key (every car would show "key changed"), unless `--rotate-key`, which prints what users will see.
- `mibpkg verify <dir|url|source> [--key ed25519:…]` — checks exactly what a car would accept
  (signature, pinned key, sha256 of every reachable package, manifests).
- GitHub Action `complex256/mibpkg/publish` (in this repo): on tag push, installs the host mibpkg,
  runs `mibpkg release … --from github:${{ github.repository }}` with `MIBPKG_SIGNING_KEY`, and
  uploads `out/*` to the release. A publisher's workflow is a few lines.
- Community list repo: `sources.json` + CI running `mibpkg verify <source> --key <key>` per entry,
  and signing the list on merge with the list key (kept as that repo's secret).

## Feasibility gates (first tasks of the plan)

1. A pure-Rust ed25519 verifier builds for `armv7-unknown-nto-qnx650` without the `libc` crate
   (verification needs no randomness). Fallback: ECDSA P-256 via the unit's libcrypto 1.0.2.
2. `https://github.com/<o>/<r>/releases/latest/download/index.json` fetched from the car through
   libcurl (redirects to the release CDN; not the rate-limited API).
3. SNTP over UDP from the car (the unit runs pf).

## Testing

- Host, with fakes (`Fetcher`, `SystemView`, `Sys`): source parsing and URL resolution; signing and
  canonical bytes; pinning, key change, Trust new key; list and sources.toml import; id conflicts;
  index merge in `release`; rotation guard; check scheduling; download-then-install incl. sha
  mismatch; GEM rows.
- Real fetcher on the host against a local HTTP server via macOS's libcurl (same dlopen path).
- On the car: enable a test source, see the update, tap Update; swap the key and see refusal.

## Milestones (each shippable)

1. Time + fetch: official collection online (Check for updates, Update from the internet).
2. Signatures + trust on first use.
3. Sources: community list, SD sources.toml, mibpkg_online screen.
4. Publisher tooling: keygen/sign/release/verify, GitHub Action, community list repo.

## Out of scope

Automatic installs, background package pre-download, multiple community lists, key revocation
lists, delta updates.
