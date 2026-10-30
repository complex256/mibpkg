//! Optional package dependency metadata in `requires.toml`.
//!
//! It is a sidecar file, not a `[plugin]` field: older mibpkg releases reject unknown manifest
//! fields, and must still install a package that carries a newer dependency declaration. New
//! mibpkg reads the file and enforces it; old mibpkg simply ignores it.
//!
//! ```toml
//! [mibsdk]
//! min = "0.6.3"
//! ```
use crate::mibsdk::icons::Source;
use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Requires {
    #[serde(default)]
    pub mibsdk: Option<Mibsdk>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Mibsdk {
    pub min: String,
}

impl Requires {
    pub fn parse(text: &str) -> Result<Requires> {
        let r: Requires = toml::from_str(text).context("invalid requires.toml")?;
        if let Some(m) = &r.mibsdk {
            semver::Version::parse(&m.min)
                .with_context(|| format!("[mibsdk] min {:?} is not semver (like 0.6.3)", m.min))?;
        }
        Ok(r)
    }

    pub fn min_sdk(&self) -> Option<&str> {
        self.mibsdk.as_ref().map(|m| m.min.as_str())
    }
}

/// Read `requires.toml` from an unpacked package directory or an in-memory package.
/// Missing means no requirements.
pub fn read(src: Source) -> Result<Requires> {
    let bytes = match src {
        Source::Dir(d) => match std::fs::read(d.join("requires.toml")) {
            Ok(b) => Some(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e).with_context(|| format!("reading {}/requires.toml", d.display())),
        },
        Source::Package(p) => p.file("requires.toml").map(<[u8]>::to_vec),
    };
    match bytes {
        Some(b) => {
            let text = std::str::from_utf8(&b).context("requires.toml is not UTF-8")?;
            Requires::parse(text)
        }
        None => Ok(Requires::default()),
    }
}

/// Read `requires.toml` from an installed version directory.
pub fn read_dir(dir: &std::path::Path) -> Result<Requires> {
    read(Source::Dir(dir))
}
