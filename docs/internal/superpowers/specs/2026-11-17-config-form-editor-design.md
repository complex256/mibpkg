# Config form editor — design

## Context
mibpkg lets an owner edit a package's `[[config]]` files from the phone UI, but only as a raw `<textarea>`
(`web/src/views/config.tsx`). Saving is an unvalidated text `PUT /api/packages/:name/config/:n` that starts a
`config-set` job. Raw TOML on a phone is error-prone, and a bad edit can stop a service that restarts on save.

Goal: a form editor for `.toml` config files. It uses a package-supplied schema when there is one, and otherwise
a best-effort mode inferred from the file. It stays owner-facing and works fully offline on the unit.

Non-goals: authoring `manifest.toml` (publisher tooling, outside the phone UI's remit per PRODUCT.md); non-TOML
config files (they keep the raw editor); changing the SD-card conflict flow or the `config-set` job.
Anti-goal: an edit that loses the user's comments, ordering or unknown keys.

## Approaches considered
1. **Browser-side parsing and editing.** Little server work, but comments are easily lost, validation is split
   from the car, and the served bundle grows.
2. **Server-side with `toml_edit`, thin UI (chosen).** Comments and ordering survive, there is one validator
   that `mibpkg verify` can reuse, and the bundle stays small. Cost: a new dependency, which must cross-compile
   for QNX ARMv7.
3. **Hybrid.** Has the costs of both.

## Design

### Schema mode
- New optional `schema = "<file>"` on `[[config]]` (`ConfigEntry` in `src/manifest.rs`), a file shipped in the
  package. (Builder's-call: the field lives on `[[config]]`.)
- The schema is a small TOML file of our own, matching `manifest.toml`. Each field has:
  - `key` (dotted path), `label`, `help`
  - `type`: string | int | float | bool | enum | list
  - `default`, `min`, `max`, `choices`, `secret`
  - `section` (optional grouping)
- Validate the schema at install and in `mibpkg verify`: unknown types and a `default` that doesn't fit its
  type are errors, in the existing `deny_unknown_fields` style.

### Fallback mode (no schema)
Infer fields from the parsed file:
- bool → switch, int/float → number input, string → text box, array of scalars → list
- table → section
- array of tables, inline tables and datetimes → read-only raw TOML snippet
- a file that doesn't parse → raw editor only, with the parse error shown

### Both modes
- A Form/Raw toggle stays available. Unknown keys and comments are kept.
- Saving still goes through the existing `config-set` job ("Save and restart").
- The server validates the result against the schema (if any) and as TOML before the job starts. A violation is a
  400 with a field-level message.
- The form is offered only for paths ending `.toml`.

## Work breakdown
1. Spike: `toml_edit` cross-compiles for QNX ARMv7 (`scripts/build.sh`); check binary-size delta. Gate.
2. `src/config_form.rs`: `Schema` parse/validate, `describe(text, schema)`, `apply(text, edits, schema)`.
3. Manifest: `schema` on `ConfigEntry`, validation, bundled-file check in `archive.rs`.
4. API: `form` payload on `config_get` (`src/web/api.rs`); `POST /api/packages/:name/config/:n/form`
   (`src/web/routes.rs`) feeding the existing `config_set` path.
5. UI: form renderer beside `web/src/views/config.tsx` with the existing shadcn components; textarea stays as Raw.
6. Docs: `docs/reference/manifest.md`, `docs/reference/web-api.md`, a schema authoring guide, CHANGELOG.
7. Tests: comment-preserving round trip, schema errors, inference per type, non-TOML paths, API tests, Vitest UI
   tests, fixture package with a schema.

## Verification
`cargo test`/`clippy`; `npm test`/`npm run build` in `web/`; `scripts/web-fixture.sh` exercised at phone width
(comments survive save, field-level errors, raw toggle round-trips); QNX build with `toml_edit`;
`ripwire --quality-delta` then `--test-gate`.

## Left to the builder
Widget details, error wording, section collapse defaults, whether `secret` fields mask in the Raw view.
