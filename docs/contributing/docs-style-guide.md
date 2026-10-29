# Documentation style guide

## Voice

- Second person: **you** install a package.
- Active voice, present tense.
- Short sentences and paragraphs.
- Explain *why* only when it changes what the reader should do.
- Do not address an AI, an agent or a future maintainer in public pages.

## Structure

- Every page answers one question.
- Prerequisites near the top.
- A runnable command or example before the long explanation.
- End with a **Related** list, not a summary.
- Exact fields, limits and commands belong in **Reference**; guides explain the shape.

| Type | Use it for |
|---|---|
| Tutorial | A first complete success |
| Guide | A task an experienced reader dips into |
| Reference | Exact names, fields, defaults and errors |
| Explanation | Architecture, trade-offs and why |

## Code

- Every command must run as written from the stated directory.
- Use `sh` for shell blocks and `toml` for manifests.
- Never invent a command or a field. Check `src/cli.rs` and `src/manifest.rs`.
- Keep example manifests short; link to real packages for the full version.

## Formatting

- Use Material admonitions: `note`, `tip`, `warning`, `danger`.
- Use tables for reference data.
- Use Mermaid for flows and architecture.
- Use repo-relative links for files and full URLs for the published site.

## What not to write

- OEM vehicle-brand names in public docs.
- Internal paths, plan names or task numbers.
- Machine-specific paths, IP addresses or personal data.
- The future as if it were shipped.

## Brand names

Public docs keep platform codes (`MIB2`, `MHI2`, `MHI2Q`) and third-party tool names (`M.I.B.`, `GEM`).
They do not name vehicle manufacturers. `tools/check_docs.py` enforces the rule.

## Word budgets

| Page | Target |
|---|---|
| `README.md` | under 1,000 words |
| Getting-started page | 300–800 words plus code |
| Guide | under 1,500 words |
| Reference | as long as the facts need |
