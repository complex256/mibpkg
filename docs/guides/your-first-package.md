# Your first package

This tutorial builds a small package with one service, packs it, installs it, and publishes a collection
cars can update from.

## 1. Scaffold

```sh
mibpkg new hello
cd hello
```

The generated tree:

```text
hello/
├── manifest.toml
├── bin/hello                  executable sample service
├── README.md
├── .gitignore
└── .github/workflows/mibpkg-release.yml
```

`manifest.toml` is already valid:

```toml
[package]
name = "hello"
version = "0.1.0"
description = "What hello does, in one line"

[[file]]
src = "bin/hello"
mode = "755"

[[service]]
name = "hello"
exec = "@pkg/bin/hello"
```

`bin/hello` is a shell loop. Replace it with your program, or point `exec` at another file in `bin/`.

## 2. Build

```sh
mibpkg build . out
```

Output:

```text
out/hello-0.1.0.mpk
```

The `.mpk` is a gzip'd tar with `manifest.toml` and the payload. Inspect it with:

```sh
tar tzf out/hello-0.1.0.mpk
```

## 3. Install it on the unit

Copy the `.mpk` to the card, or upload it from a phone:

1. Open the web interface.
2. **More → Install from file**.
3. Choose `hello-0.1.0.mpk`.
4. Review the package and install.

The package page shows the service state and its logs. The service starts after install and is
supervised at boot.

## 4. Add a config

```toml
[[file]]
src = "hello.conf.example"

[[config]]
path = "hello.conf"
example = "hello.conf.example"
required = true
```

A required config makes the package page show **needs config** until you create
`mibpkg/config/hello/hello.conf` on the card and rescan.

## 5. Add a start condition

```toml
[[service]]
name = "hello"
exec = "@pkg/bin/hello"
after = ["internet"]
after_timeout = "5m"
```

The supervisor waits for the condition, or starts anyway after 5 minutes. Aliases and raw checks are in
[Service conditions](../reference/service-conditions.md).

## 6. Package a native program with an SDK plugin

A product can carry both a native service and an SDK status plugin in one `.mpk`:

```toml
[plugin]
id = "hello-status"
jar = "hello-status.jar"
api = 6
permissions = ["ui.slot.statusbar"]
icons = ["hello-20"]
```

The package needs `hello-status.jar`, its `.stamp` and `icons/hello-20.png` at the root. See
[Real packages](real-packages.md).

## 7. Publish

Create a repository and run:

```sh
mibpkg release . \
  --repo <owner>/hello \
  --tag v0.1.0 \
  --id my-packages \
  --name "My packages" \
  --publish
```

This builds the package, writes `index.json`, creates the GitHub release, uploads the files and waits for
the index to become live. See [Publishing a collection](publishing-a-collection.md).

Cars add:

```text
github:<owner>/hello
```

in **More → Sources**.

## 8. Check what cars see

```sh
mibpkg verify github:<owner>/hello
```

## Related

- [manifest.toml](../reference/manifest.md)
- [Package layout](../reference/package-layout.md)
- [GitHub Action](github-action.md)
