# Test

## Rust

```sh
cargo test
cargo fmt --check
cargo clippy
```

Host tests map every unit path under a temporary root. They cover:

- manifest parsing and validation;
- package build, read and extract;
- install, update, rollback and remove;
- plugin checks, approval and grants;
- SDK package handling;
- supervisor conditions;
- online index, release and verify;
- the web API and upload flow.

## Web

The front end uses Node's built-in test runner:

```sh
node --test web/tests/*.test.mjs
```

## Package examples

Build a package and verify it:

```sh
cargo run --release -- build path/to/package out
cargo run --release -- verify out
```

For a combined product package, check the SDK plugin half too:

```sh
cargo run --release -- verify out --unit MHI2Q
```

## Docs

```sh
python3 -m pip install -r docs/requirements.txt
python3 tools/check_docs.py
mkdocs build --strict
```

## Before a release

```sh
cargo test
node --test web/tests/*.test.mjs
python3 tools/check_docs.py
mkdocs build --strict
```

## Related

- [Build](build.md)
- [Release](release.md)
