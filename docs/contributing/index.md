# Contributing

mibpkg is a Rust binary, an on-unit web app and a small set of shell/Python/Gradle tools around it.

## Good places to start

- A package recipe or example that shows a pattern the docs do not.
- A web interface fix or a JSON API improvement.
- A test that reproduces a package or supervisor bug.
- A documentation fix.

## Ground rules

- The unit has a small, old QNX userland. Do not assume `printf`, `which`, `cmp`, `od`, `uniq` or
  `mkfifo`.
- `/tmp` is `/dev/shmem`: files only, no directories.
- `/mnt/app` is normally read-only; follow the remount holder protocol.
- A package runs as root. Do not add a feature that hides what a package does.
- Keep the package format and the web API backward compatible unless you bump the version and document
  the change.

## Workflow

1. Open an issue or comment on one.
2. Branch from `main`.
3. Add tests.
4. Run the gates in [Test](test.md).
5. Update the docs and `CHANGELOG.md` in the same change.
6. Open a pull request.

## License

By contributing you agree that your changes are licensed under MIT.

## Related

- [Build](build.md)
- [Test](test.md)
- [Release](release.md)
- [Docs style guide](docs-style-guide.md)
