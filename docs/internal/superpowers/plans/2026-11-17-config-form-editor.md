# Config Form Editor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let an owner edit a package's `.toml` config file through a form (schema-driven when the package ships a schema, inferred otherwise) instead of only a raw textarea.

**Architecture:** A new `src/config_form.rs` parses and edits TOML with `toml_edit` (comments and order survive), describes a file as form fields, and applies validated edits. Two web endpoints expose it; saving reuses the existing `config-set` job. The React view renders the fields and keeps the textarea as a Raw tab.

**Tech Stack:** Rust (`toml` 0.8, `toml_edit` 0.22 already in `Cargo.toml`, `serde_json`), React 19 + shadcn components, Vitest + Testing Library.

**Spec:** `docs/internal/superpowers/specs/2026-11-17-config-form-editor-design.md`

## Global Constraints
- The form applies only to config paths ending `.toml`; every other file keeps the raw editor.
- Edits must preserve comments, key order and unknown keys.
- The server validates before starting a job: TOML parse first, then schema rules; a violation is HTTP 400 with per-field messages.
- `Manifest` keeps `#[serde(deny_unknown_fields)]`; `schema` is an optional `[[config]]` field naming a file in the package.
- Schema field `type` is exactly one of `string | int | float | bool | enum | list`.
- Saving still goes through `config-set` ("Save and restart"); the SD-card conflict flow is unchanged.
- Owner-facing phone UI: touch-friendly targets, busy states on buttons, works offline.
- Commits end with `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`; authorship stays `complex256` / `me@complex256.dev`.

## Review Focus
1. **Truncated read → data loss.** `config_get` reads at most `MAX_CONFIG` bytes; applying a form edit to a truncated file would overwrite the rest. A file over the cap, or not valid UTF-8, must open raw-only. (Task 5)
2. **CRLF files.** A file with `\r\n` line endings must not be rewritten with mixed endings after an edit. (Task 3)
3. **Dotted and quoted keys.** A key such as `"a.b" = 1` must not be addressed as `a` → `b`; it is shown as a raw field in inferred mode. (Task 2)
4. **Int vs float.** `8080.5` for an `int` field and `abc` for a number are field errors; a float `3` is written `3.0`. (Task 3)
5. **Conflict or unparseable file.** While a card conflict is pending, or when the file does not parse, there is no form; the user gets the compare view or the raw editor with the parse error. (Tasks 2, 6)

---

### Task 1: Schema model and parser

**Files:**
- Create: `src/config_form.rs`; add `pub mod config_form;` to `src/lib.rs`
- Test: `tests/config_form.rs`

**Interfaces:**
- Produces:
  - `pub enum FieldType { String, Int, Float, Bool, Enum, List }` (serde `rename_all = "lowercase"`)
  - `pub struct Field { pub key: String, pub label: Option<String>, pub help: Option<String>, pub ty: FieldType, pub default: Option<toml::Value>, pub min: Option<f64>, pub max: Option<f64>, pub choices: Vec<String>, pub of: Option<FieldType>, pub secret: bool, pub section: Option<String> }`
  - `pub struct Schema { pub fields: Vec<Field> }` with `pub fn Schema::parse(text: &str) -> anyhow::Result<Schema>`
- Schema file format (TOML, `deny_unknown_fields`): repeated `[[field]]` tables with the keys above; `type` is the TOML name of `ty`.

- [ ] **Step 1: Write failing tests** in `tests/config_form.rs`: `schema_parses_all_types` (one field of each type, assert the parsed fields), `schema_rejects_unknown_type` (`type = "date"` → error mentioning `date`), `schema_rejects_enum_without_choices`, `schema_rejects_default_of_wrong_type` (`type = "int"`, `default = "x"`), `schema_rejects_list_without_of_and_of_must_be_string_int_or_float`, `schema_rejects_duplicate_keys`, `schema_rejects_min_above_max`.
- [ ] **Step 2: Run** `cargo test --test config_form` — expect FAIL (module missing).
- [ ] **Step 3: Implement** the types and `Schema::parse` in `src/config_form.rs` (deserialize with `toml`, then validate: enum needs non-empty `choices`; list needs `of` in string/int/float; `default` must match `ty`; `min <= max`; keys unique and non-empty).
- [ ] **Step 4: Run** `cargo test --test config_form` — expect PASS.
- [ ] **Step 5: Commit** `feat: config form schema model and parser`.

### Task 2: Describe a file as form fields

**Files:**
- Modify: `src/config_form.rs`
- Test: `tests/config_form.rs`

**Interfaces:**
- Consumes: `Schema`, `Field`, `FieldType` (Task 1).
- Produces (all `#[derive(Serialize)]`):
  - `pub struct FormField { pub key: String, pub label: String, pub help: Option<String>, pub kind: String /* string|int|float|bool|enum|list|raw */, pub source: String /* schema|inferred */, pub value: serde_json::Value /* null = unset */, pub default: serde_json::Value, pub choices: Vec<String>, pub min: Option<f64>, pub max: Option<f64>, pub secret: bool, pub section: Option<String>, pub raw: Option<String> }`
  - `pub struct FormDoc { pub mode: String /* schema|inferred|raw */, pub fields: Vec<FormField>, pub reason: Option<String> }`
  - `pub fn describe(text: &str, schema: Option<&Schema>) -> FormDoc`

- [ ] **Step 1: Write failing tests**: `infers_scalar_kinds` (bool/int/float/string/array of scalars → kinds, `section` = table name, `mode == "inferred"`); `inferred_complex_values_are_raw` (array of tables, inline table, datetime → `kind == "raw"`, `raw` holds the TOML snippet); `dotted_quoted_key_is_raw_field` (`"a.b" = 1` → one `raw` field, never split); `schema_fields_listed_even_when_missing` (schema field absent from file → `value` null, `default` set, `source == "schema"`); `schema_mode_keeps_unlisted_keys_out_of_form` (unlisted keys are not fields but are not an error); `unparseable_file_is_raw_mode` (`mode == "raw"`, `reason` contains the parse error); `empty_text_with_schema_lists_fields`.
- [ ] **Step 2: Run** — expect FAIL.
- [ ] **Step 3: Implement `describe`** using `toml_edit::DocumentMut`; walk tables depth-first for inference, joining table names with `.` into `key` and using the table name as `section`.
- [ ] **Step 4: Run** — expect PASS.
- [ ] **Step 5: Commit** `feat: describe TOML config files as form fields`.

### Task 3: Apply edits with validation

**Files:**
- Modify: `src/config_form.rs`
- Test: `tests/config_form.rs`

**Interfaces:**
- Consumes: `Schema`, `Field` (Task 1).
- Produces:
  - `pub struct Edit { pub key: String, pub value: serde_json::Value }` (`Deserialize`, `deny_unknown_fields`); `Null` removes the key
  - `pub struct FieldError { pub key: String, pub message: String }` (`Serialize`)
  - `pub fn apply(text: &str, edits: &[Edit], schema: Option<&Schema>) -> Result<String, Vec<FieldError>>`

- [ ] **Step 1: Write failing tests**: `edit_preserves_comments_order_and_unknown_keys`; `edit_creates_missing_table` (`server.port` into a file without `[server]`); `null_removes_key`; `crlf_file_stays_crlf` (no bare `\n` after an edit); `int_field_rejects_fraction`; `int_out_of_range_for_min_max`; `enum_rejects_value_outside_choices`; `float_written_with_decimal_point` (3 → `3.0`); `list_edit_replaces_array_with_typed_items`; `edit_conflicting_with_existing_non_table_parent_errors` (`a = 1` then edit `a.b`); `all_field_errors_returned_together`; `unparseable_input_returns_error_for_empty_key`; `existing_out_of_range_value_not_touched_is_not_an_error`.
- [ ] **Step 2: Run** — expect FAIL.
- [ ] **Step 3: Implement `apply`**: parse with `toml_edit::DocumentMut`; for each edit, look up the schema field (or infer the type from the existing value; unknown key with no schema → string/number/bool from the JSON type); validate type, range and choices; set or remove, preserving decor. Detect line ending (`\r\n` anywhere in input) and normalise inserted newlines to it. Collect all `FieldError`s before returning `Err`.
- [ ] **Step 4: Run** — expect PASS.
- [ ] **Step 5: Commit** `feat: apply validated form edits to TOML config`.

### Task 4: Manifest `schema` field and package validation

**Files:**
- Modify: `src/manifest.rs` (`ConfigEntry`, `Manifest::validate`), `src/archive.rs` (`pack`, `Package::read`)
- Test: `tests/manifest.rs`, `tests/config_form.rs`

**Interfaces:**
- Consumes: `Schema::parse` (Task 1).
- Produces: `ConfigEntry.schema: Option<String>` (`#[serde(default)]`).

- [ ] **Step 1: Write failing tests**: in `tests/manifest.rs` — `config_schema_parses`, `config_schema_must_be_safe_relative_path`, `config_schema_requires_toml_config_path` (a `schema` on `foo.json` → error); in `tests/config_form.rs` — `pack_rejects_missing_schema_file` and `pack_rejects_invalid_schema_file` (using `common::pkg_src` and `archive::pack`).
- [ ] **Step 2: Run** `cargo test --test manifest --test config_form` — expect FAIL.
- [ ] **Step 3: Implement**: add `schema` to `ConfigEntry`; in `Manifest::validate` apply `safe_rel` and require the config `path` to end `.toml`; in `pack` add the schema file to `needed` and parse it with `Schema::parse`; in `Package::read` parse the schema when present so `mibpkg verify` and installs fail on a broken schema.
- [ ] **Step 4: Run** `cargo test` — expect PASS.
- [ ] **Step 5: Commit** `feat: optional schema on [[config]]`.

### Task 5: Web API

**Files:**
- Modify: `src/web/api.rs` (`config_get`), `src/web/routes.rs` (route table, new `config_form`)
- Test: `tests/config_form.rs` (or the existing web API test location found via `grep -ln config_get tests`)

**Interfaces:**
- Consumes: `describe`, `apply`, `Edit`, `FieldError`, `Schema`, `ConfigEntry.schema`.
- Produces:
  - `GET /api/packages/:name/config/:n` gains `"form": FormDoc | null` (null for non-`.toml` paths)
  - `POST /api/packages/:name/config/:n/form` body `{"edits":[{"key":..,"value":..}]}`; 202 `{job}` on success (same reply as the `PUT`); 400 `{"error": "...", "fields": [{"key","message"}]}` on validation failure
  - private helper `fn read_config_text(app: &App, m: &Manifest, c: &ConfigEntry, name: &str) -> Option<String>` (None when the file exceeds `MAX_CONFIG` or is not UTF-8) shared by both handlers

- [ ] **Step 1: Write failing tests**: `get_includes_form_for_toml`, `get_form_is_null_for_non_toml`, `get_form_is_raw_mode_over_size_cap` (Review Focus 1), `get_form_is_raw_mode_for_non_utf8`, `post_form_starts_job_and_saves_edited_text`, `post_form_validation_error_is_400_with_fields`, `post_form_refused_while_card_conflict_pending` (409), `post_form_unknown_package_404`.
- [ ] **Step 2: Run** — expect FAIL.
- [ ] **Step 3: Implement**: extract `read_config_text` from `config_get`; load the schema from `current_dir(&app.layout, name)` joined with `c.schema`; in the POST handler build the text, call `apply`, then reuse the `config_set` path (`text_file` + `start_job`).
- [ ] **Step 4: Run** `cargo test` — expect PASS.
- [ ] **Step 5: Commit** `feat: web API for the config form`.

### Task 6: Form UI

**Files:**
- Create: `web/src/components/config-form.tsx`
- Modify: `web/src/views/config.tsx`, `web/src/lib/types.ts`
- Test: `web/tests/config-form.test.tsx` (helpers in `web/tests/helpers.tsx`)

**Interfaces:**
- Consumes: the `form` payload and POST endpoint (Task 5).
- Produces: `export interface FormField` / `FormDoc` in `types.ts` mirroring Task 2; `ConfigDetail.form: FormDoc | null`; `export function ConfigForm({ doc, onSubmit, saving, errors }: { doc: FormDoc; onSubmit: (edits: {key: string; value: unknown}[]) => void; saving: boolean; errors: Record<string, string> })`.

- [ ] **Step 1: Write failing tests**: `renders_switch_input_and_select_for_kinds`; `submits_only_changed_fields`; `empty_optional_field_sends_null`; `shows_field_error_from_400`; `secret_field_is_password_input`; `raw_tab_shows_textarea_and_round_trips`; `no_form_tab_when_form_is_null_or_raw_mode` (shows `reason`); `no_form_tab_during_card_conflict` (Review Focus 5).
- [ ] **Step 2: Run** `npm test` in `web/` — expect FAIL.
- [ ] **Step 3: Implement** `ConfigForm` with the existing `Switch`, `Input`, `Label`, `Collapsible` per section; add Form/Raw tabs to `ConfigView` (default Form when `form.mode !== "raw"` and no conflict); submit through `api('POST', .../form, {edits})`, parse the 400 `fields` into `errors`, and reuse the existing `JobConsole`.
- [ ] **Step 4: Run** `npm test && npm run build` in `web/` — expect PASS.
- [ ] **Step 5: Commit** `feat: form editor for TOML config files`.

### Task 7: Fixture, docs, release gates

**Files:**
- Modify: `scripts/web-fixture.sh`, `docs/reference/manifest.md` (`[[config]]` table: `schema`), `docs/reference/web-api.md`, `CHANGELOG.md`
- Create: `docs/guides/config-schema.md` (schema authoring guide; add to `mkdocs.yml` nav)

- [ ] **Step 1:** Extend `scripts/web-fixture.sh` with one package that ships a schema (all six types, one `secret`) and keep `gps-logger` (no schema) for fallback mode.
- [ ] **Step 2:** Write the docs: the `schema` row, the schema file format with one complete example, the endpoints, and a short note that unlisted keys and comments are preserved.
- [ ] **Step 3:** Run `cargo test`, `cargo clippy`, `npm test`, `npm run build`; run the fixture and check both modes at phone width in a browser (comments survive a save; a bad value shows a field error; Raw round-trips).
- [ ] **Step 4:** Run `scripts/build.sh`; compare the binary against the 3,238,978-byte baseline and record the delta in the CHANGELOG entry.
- [ ] **Step 5:** Run `ripwire . --quality-delta`, then `ripwire . --test-gate`; fix what they flag.
- [ ] **Step 6: Commit** `docs: config schema guide and changelog`.
