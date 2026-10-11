//! `mibpkg new <name>`: a package directory a developer can build, release and verify.
use crate::manifest::{valid_name, Manifest};
use anyhow::{bail, Result};
use std::fs;
use std::path::{Path, PathBuf};

fn manifest(name: &str) -> String {
    format!(
        r#"# mibpkg package manifest. Reference: the mibpkg README ("Package format").
[package]
name = "{name}"
version = "0.1.0"            # bump for every release (semver); a version is never reused
description = "What {name} does, in one line"

# Units it runs on; leave out to allow every unit.
# [compat]
# units = ["MHI2Q"]

# Files copied into the package directory on the car (@pkg).
[[file]]
src = "bin/{name}"
mode = "755"

# A long-running program the supervisor starts at boot and restarts if it exits.
[[service]]
name = "{name}"
exec = "@pkg/bin/{name}"
# args  = ["--config", "@config/{name}.toml"]
# after = ["internet"]       # wait for: internet, tailnet, hmi-ready, file:/path, iface:<name>, route:<dest>
"#
    )
}

fn workflow() -> &'static str {
    r#"# Publishes a release when you push a tag like v1.2.0:
#   git tag v1.2.0 && git push --tags
# Cars that have this repo as a source (github:<owner>/<repo>) offer the update.
name: mibpkg release
on:
  push:
    tags: ['v*']
permissions:
  contents: write
jobs:
  release:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      # Put your build here: the package directory must contain bin/<name> before releasing.
      - uses: complex256/mibpkg@v1
        with:
          packages: .
"#
}

fn readme(name: &str) -> String {
    format!(
        "# {name}\n\nA [mibpkg](https://github.com/complex256/mibpkg) package for MIB2 High head units.\n\n\
         ## Release\n\n```sh\nmibpkg release . --repo <owner>/{name} --tag v0.1.0 --id <your-collection-id> --name \"<your name>'s packages\" --publish\n```\n\n\
         Later releases need only `--repo` and `--tag` (or push a `v*` tag: see `.github/workflows/mibpkg-release.yml`).\n\n\
         ## Check what cars will see\n\n```sh\nmibpkg verify github:<owner>/{name}\n```\n\n\
         Cars add the source `github:<owner>/{name}` in the web interface (More → Sources).\n"
    )
}

/// Write a new package skeleton into `dir` (created; must be empty if it exists).
pub fn new_package(dir: &Path, name: &str) -> Result<Vec<PathBuf>> {
    if !valid_name(name) {
        bail!("package name {name:?} must be 1-40 of [a-z0-9-]");
    }
    if dir.exists() && fs::read_dir(dir)?.next().is_some() {
        bail!("{} is not empty", dir.display());
    }
    let m = manifest(name);
    Manifest::parse(&m)?; // the template must always be valid
    let files: Vec<(PathBuf, String)> = vec![
        (dir.join("manifest.toml"), m),
        (dir.join("bin/.keep"), String::new()),
        (dir.join("README.md"), readme(name)),
        (dir.join(".github/workflows/mibpkg-release.yml"), workflow().to_string()),
    ];
    for (p, text) in &files {
        fs::create_dir_all(p.parent().unwrap())?;
        fs::write(p, text)?;
    }
    Ok(files.into_iter().map(|(p, _)| p).collect())
}
