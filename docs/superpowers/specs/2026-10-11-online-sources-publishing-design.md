# mibpkg online sources + publisher tooling (M3 + M4, unsigned) — design

Status: design approved in brainstorming 2026-10-11; spec awaiting review.
Amends: [2026-09-30-online-updates-design.md](2026-09-30-online-updates-design.md) (milestones 3
and 4). Builds on: [2026-10-05-web-interface-design.md](2026-10-05-web-interface-design.md) (the
Sources screen and the `source-add` / `source-remove` verbs already exist).

## Goal

Cars discover and enable other publishers' collections, and a publisher goes from a package
directory to a published GitHub release with one command — all without signatures, which stay
deferred (milestone 2 of the online-updates spec, unchanged and still planned).

Success:
- The web UI's Sources screen lists sources from the community list and from the SD card; one tap
  enables one, and its packages appear on the next check.
- `mibpkg release . --repo me/tool --tag v1.2.0 --publish` builds, carries older versions forward,
  and creates the GitHub release; a car with that source enabled offers the update.
- `mibpkg verify github:me/tool` tells a publisher (and the list maintainer) whether a car would
  accept the source.
- carmqtt can move to its own collection id without an ssh step on the car.

## Decisions (from brainstorming)

- No signatures in this work. The list and `sources.toml` keep an optional `key` field, parsed and
  ignored, so signatures (M2) need no format change.
- mibpkg stays private. The community list lives in the existing public repo
  `complex256/mibpkg-packages` (its latest release carries `sources.json` next to the official
  collection's `index.json`).
- The online sources screen is the web UI's only; GEM gets nothing new (it is shrinking to a
  recovery page).
- The GitHub Action, Linux host binaries and list CI are written now but switched on only when
  mibpkg goes public (they need a public place to download the host tool from). The command-line
  tools are complete and tested against real GitHub now.
- First-time install of mibpkg onto a car is a separate design, required before going public
  (docs/ideas.md).

## Changes to the 2026-09-30 spec

| 2026-09-30 says | Now |
|---|---|
| List repo `complex256/mibpkg-sources`, signed by a list key | `sources.json` in `complex256/mibpkg-packages`' latest release, unsigned |
| List and `sources.toml` entries carry a `key`; pinning on Enable | `key` optional, ignored until M2 |
| mibpkg_online GEM screen | Web UI Sources screen only |
| `keygen`, automatic signing, `--no-sign`, key-rotation guard | Deferred with M2 |
| `release --from <source>`, old entries kept as absolute URLs | `release --repo --tag`: every entry is an absolute URL from the start |
| Collection ids unique per car | Unchanged, now enforced (see Collection ids) |

## Car side (M3)

### Source list

`sources.json` (format 1), an asset of the list repo's latest release:

```json
{ "format": 1,
  "sources": [
    { "name": "mibpkg", "description": "The package manager itself",
      "source": "github:complex256/mibpkg-packages" } ] }
```

- Entry fields: `name` (≤ 60 chars), `description` (≤ 200 chars, optional), `source` (must pass
  `online::index_url`), `key` (optional, ignored). Unknown fields are ignored (forward compatible);
  an unknown `format` or an invalid entry skips that entry with a reason, not the whole list.
- Where: `settings.toml` key `online_list`, default `github:complex256/mibpkg-packages`, resolved
  like a source but to the `sources.json` asset (`…/releases/latest/download/sources.json`, or an
  `https://…/sources.json` URL). `online_list = ""` turns the list off.
- When: during an online check, if the cached list is missing or older than 24 h (by the cache
  file's fetch time recorded inside it, in clock-offset-corrected seconds; when no offset is known
  the list is fetched every check). A failed fetch keeps the previous cache and adds
  `list: <error>` to the check's errors.
- Cache: `state/sources-list.json` (`{fetched_at, url, sources}`), rewritten only when changed.
  Size limit `MAX_INDEX`.
- Listed sources are *available*: nothing is fetched from them until enabled. Enable = the existing
  `source-add` verb; Disable = `source-remove`.

### SD card sources.toml

`/mibpkg/sources.toml` on any media root:

```toml
[[source]]
name        = "alice's tools"
description = "dashcam uploader"   # optional
source      = "github:alice/mib-tools"
# key       = "ed25519:…"          # optional; ignored until signatures
```

- Read whenever card collections are scanned (Rescan, page loads that scan media). Not copied to
  flash: available only while the card is in. Invalid entries are skipped with a reason shown on
  the Sources screen.
- Shown as available with origin "from card"; Enable adds it like a list entry, after which it no
  longer depends on the card.

### Available vs enabled

`online::available(layout) -> Vec<AvailableSource>`: list entries and card entries, minus sources
already in the effective list, deduplicated by `source` (card wins for the name/description, so a
card can label a source the list does not have). Each carries `origin` (`list` | `card`).

### Collection ids

- Only one enabled source may own a collection id. The owner is the source that cached it first
  (the cache file `state/online/<id>.json` records its `source`); a later enabled source whose
  index uses that id is refused with `<source>: collection <id> is already provided by <owner>`,
  its index is not cached, and the Sources screen shows the conflict.
- Removing a source drops its cache (existing), which frees the id.
- A card collection and an online collection with the same id remain one collection (unchanged).

### Package moved

When an installed package's recorded collection is on no card and in no online cache:
- if exactly one present collection offers that package name, it is the package's collection for
  updates (GEM Update button, web Update, `update-all`, `pick_collection`), and a successful install
  records the new id;
- if several do, no automatic choice: the web package page shows one Update button per collection
  (`Update to X from <id>`), and `install <pkg>` without a collection fails with the list of ids.

The existing "installed from collection A; remove it before installing it from B" guard still
applies whenever A is present.

### Web UI

- `GET /api/sources` gains `available: [{name, description, source, origin}]` and, per enabled
  source, `collection` (id), `packages` (count) and `conflict` (text or null); plus
  `list: {url, fetched_at, error}`.
- Sources screen: Enabled section (existing rows + collection id, package count, conflict text);
  Available section (name, description, source, origin badge, Enable → `source-add` job); the
  existing add-by-text field stays.

## Publisher tooling (M4, host)

### mibpkg release

```
mibpkg release <pkg-dir|file.mpk>... --repo <owner/repo> --tag <tag>
               [--id ID --name NAME [--new-id]] [--keep N] [--out DIR] [--asset FILE]... [--publish]
```

1. Builds each package directory with the existing `build` (a `.mpk` argument is taken as is).
2. Fetches the previous index from `github:<owner>/<repo>` (host fetch: the same `Fetcher` trait,
   implemented on the host with system libcurl like the host tests). HTTP 404 = first release.
3. Collection: `--id`/`--name`, else the previous index's; a first release without `--id` fails.
   A `--id` that differs from the previous index's fails (every car would treat it as a different
   collection) unless `--new-id` is given.
4. New entries: `file` = `https://github.com/<owner>/<repo>/releases/download/<tag>/<file>`.
   Previous entries are kept as they are (already absolute; relative ones from the 0.3.x era are
   made absolute against the previous release's download directory, read from the 302 Location of
   the `latest/download/index.json` request).
5. Same name and version already in the previous index: identical sha256 → kept once; different →
   fails ("version reused with different contents"; bump the version).
6. Keeps the newest `--keep` (default 3) versions per package name by semver.
7. Writes `<out>/index.json` (flat) + the new `.mpk` files + each `--asset` (default out dir
   `out/release-<tag>`), then prints a summary (added, kept, dropped).
8. `--publish`: `gh release create <tag> --repo <owner>/<repo> --title <tag> --notes <summary>
   <out>/*`. Fails clearly if `gh` is missing or not logged in (`GH_TOKEN` in CI). After creating,
   re-fetches `latest/download/index.json` for up to 60 s until it serves the new index (GitHub's
   redirect lags ~20 s) and reports when cars will see it.

`scripts/publish-official.sh <tag> <mpk>...` becomes `mibpkg release … --repo
complex256/mibpkg-packages --tag <tag> --asset packaging/sources.json --publish`. `sources.json`
for the list lives in this repo at `packaging/sources.json`, so it is versioned and reviewed.

### mibpkg verify

```
mibpkg verify <source|index.json path|dir> [--latest] [--unit mhi2q]
```

Checks what a car would accept and reports every problem (not just the first), exit 1 if any:
index format and fields (`Collection::from_online` rules); every entry's URL is https; download
each entry (or only the newest per name with `--latest`), size and sha256 match; the package
unpacks; its manifest parses and its name/version match the entry; scripts flagged consistently
(`uses_scripts`). `--unit` additionally reports which entries are incompatible with that unit.
Works on a local release dir (`out/release-<tag>`) before publishing.

### mibpkg new

```
mibpkg new <name> [--dir DIR]
```

Creates `<dir or name>/` with a commented `manifest.toml` (one service, `@pkg/bin/<name>`),
`bin/.keep`, `README.md` (build, `mibpkg release`, `mibpkg verify`), and
`.github/workflows/mibpkg-release.yml`. Refuses an existing non-empty directory. The name must
pass the manifest name rules.

### Written now, switched on when public

- `action.yml` (composite action at the mibpkg repo root): downloads the Linux host mibpkg from
  the mibpkg repo's release, runs `mibpkg release … --publish` with the workflow's tag.
- The workflow `mibpkg new` writes uses that action on `push: tags: ['v*']`.
- Linux x86_64 host binary built and attached to mibpkg releases (CI job in the mibpkg repo).
- `mibpkg-packages` CI: on a pull request changing `sources.json`, run `mibpkg verify --latest` on
  each new or changed entry.
- Until then: list entries are verified by hand with `mibpkg verify`; developers publish from a
  laptop with `--publish`.

## Code structure

- `src/online.rs`: list fetch/cache, `sources.toml` reading, `available()`, collection-id ownership
  in `store`. If it grows past ~400 lines, split the list into `src/sources.rs`.
- `src/install.rs`: package-moved fallback in `pick_collection` / `updates_available`.
- `src/web/api.rs`, front end Sources screen: available section, conflicts, list status.
- New `src/publish.rs`: `release` and `verify` (pure functions over a `Fetcher` and a `Publisher`
  trait wrapping `gh`, so tests run with fakes); `new` scaffolding in `src/scaffold.rs`.
- `src/cli.rs`: `release`, `verify`, `new` commands; USAGE.
- Host fetcher: `CurlFetcher` with the system libcurl (already used by host tests), CA from the
  system.
- `packaging/sources.json`; `scripts/publish-official.sh` rewritten.

## Errors and robustness

- The list and `sources.toml` are untrusted input: size-capped, every string length-capped, entries
  validated individually; one bad entry never hides the rest.
- An unreachable list never blocks the check of enabled sources.
- `release` never publishes a partial release: everything is built and validated into `out/` first;
  `gh` runs last. A failed `gh release create` leaves `out/` for a retry.
- `verify` streams downloads to a temp dir and cleans up.

## Testing

Host, with fakes (`MemoryFetcher`, fake `Publisher`, fixture media):
- List: parse, bad entries skipped with reasons, unknown format, 24 h cache rule, failed fetch
  keeps cache, `online_list = ""`.
- `sources.toml`: parse, invalid entries, available-list merge and dedup, enable via `source-add`.
- Collection ids: second source with the same id refused and reported; removal frees the id.
- Package moved: single new collection offers updates and install records it; several → no
  automatic choice; recorded collection present → old guard unchanged.
- `release`: first release, carry forward, keep N, version reuse refused, id change refused,
  relative → absolute conversion, `--publish` calls the publisher with exactly the out files,
  nothing published on a build error.
- `verify`: good source passes; sha/size mismatch, manifest mismatch, http URL, broken archive each
  reported.
- `new`: scaffold builds with `mibpkg build`; refuses non-empty dirs.
- Web API: `/api/sources` shapes.

Real GitHub (host, before the car): publish the official collection with the new `release`
(mibpkg 0.6.x + `sources.json`) to `mibpkg-packages`; `mibpkg verify github:complex256/mibpkg-packages`.

On the car: the list appears on the Sources screen; a `sources.toml` card entry appears and
enables; the package-moved path with an SD card whose only carmqtt collection has the id
`complex256-carmqtt` (the old `complex256` folder removed from the card): the web UI offers the
update from the new id and installing records it. carmqtt is never published online for this.

## Out of scope

Signatures (M2), going public (first-time install, scrub, license, README, switching on the
Action/CI), list curation beyond hand verification, multiple lists, mirrors.
