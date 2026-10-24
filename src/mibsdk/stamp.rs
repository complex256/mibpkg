//! Check stamps (M7): only jars the SDK build checked may be installed. A port of mibsdk-dev's
//! `STAMP_PY`, reading what build-logic's `CheckStamp` writes (format 1): ISO-8859-1, `\n` line
//! ends, `format=1`, `kind=plugin|unit`, `checks=<list>`, sorted attributes (`plugin.id`,
//! `plugin.unrestricted` for a plugin), then one `sha256.<jar file name>=<hex>` line per jar.
//! A unit stamp covers all three SDK jars. The stamp is hashed, not signed: it guards against
//! accident, not an adversary.
use crate::archive::sha256_file;
use anyhow::{bail, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StampKind {
    Plugin,
    Unit,
}

impl StampKind {
    pub fn as_str(self) -> &'static str {
        match self {
            StampKind::Plugin => "plugin",
            StampKind::Unit => "unit",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    /// `plugin.unrestricted` of a plugin stamp; None for a unit stamp.
    pub unrestricted: Option<bool>,
}

impl Verified {
    /// The warning to show for plugin `id`, if it declared `platform.unrestricted`.
    pub fn warning(&self, id: &str) -> Option<String> {
        (self.unrestricted == Some(true)).then(|| unrestricted_warning(id))
    }
}

pub fn unrestricted_warning(id: &str) -> String {
    format!("WARNING: {id} declares platform.unrestricted: it may use reflection, exec or exit; the SDK cannot contain it")
}

/// The refusal of plugin `id`'s jar for reason `why` (the text of a `verify` error).
pub fn plugin_refusal(id: &str, why: &str) -> String {
    format!("{id}.jar has no valid check stamp ({why}); only jars built by the mibsdk.plugin convention can be installed: ./gradlew :<project>:mibPluginJar (M7)")
}

/// The refusal of the SDK package's jars for reason `why`.
pub fn sdk_refusal(why: &str) -> String {
    format!("the mibsdk package's jars are not a checked build ({why}); build it with ./gradlew :dist:mibpkgPackage; nothing changed (M7)")
}

/// Check `stamp_file` against `jars` (each by its file name). The error's text is exactly the
/// reason: `missing`, `format`, `kind`, `id <got|None> != <want>`, `no hash for <name>` or
/// `hash mismatch for <name>`. Unknown keys are ignored.
///
/// Caller contract: verify and install from the same mibpkg-owned staged copy (the jars checked
/// here are the bytes `put_file` writes); a unit stamp is verified against ALL `[sdk] jars`
/// (a jar left out is never checked); a plugin stamp against its one jar with `Some(id)`.
pub fn verify(stamp_file: &Path, kind: StampKind, expected_id: Option<&str>, jars: &[&Path]) -> Result<Verified> {
    let raw = std::fs::read(stamp_file).ok();
    check(raw.as_deref(), kind, expected_id, jars.len(), &mut |i| {
        let name = jars[i].file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        (name, sha256_file(jars[i]).ok().map(|(h, _)| h))
    })
}

/// `verify` against in-memory bytes: `stamp` (None: there is none) and each jar as
/// `(file name, bytes)`. For a snapshot taken once, so the bytes checked are the bytes written.
pub fn verify_bytes(stamp: Option<&[u8]>, kind: StampKind, expected_id: Option<&str>, jars: &[(&str, &[u8])]) -> Result<Verified> {
    check(stamp, kind, expected_id, jars.len(), &mut |i| {
        let mut h = Sha256::new();
        h.update(jars[i].1);
        (jars[i].0.to_string(), Some(h.finalize().iter().map(|b| format!("{b:02x}")).collect()))
    })
}

/// The rules of `verify`; `jar(i)` gives jar `i`'s file name and sha256 (None: unreadable).
fn check(raw: Option<&[u8]>, kind: StampKind, expected_id: Option<&str>, n_jars: usize, jar: &mut dyn FnMut(usize) -> (String, Option<String>)) -> Result<Verified> {
    let Some(raw) = raw else { bail!("missing") };
    // ISO-8859-1: every byte is the code point of the same value.
    let text: String = raw.iter().map(|&b| b as char).collect();
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    let mut attrs: HashMap<&str, &str> = HashMap::new();
    for line in &lines {
        let Some((k, v)) = line.split_once('=') else { bail!("format") };
        if attrs.insert(k, v).is_some() {
            bail!("format");
        }
    }
    if lines.first() != Some(&"format=1") || attrs.get("checks").is_none_or(|c| c.is_empty()) {
        bail!("format");
    }
    if attrs.get("kind").copied() != Some(kind.as_str()) {
        bail!("kind");
    }
    let mut unrestricted = None;
    if kind == StampKind::Plugin {
        let got = attrs.get("plugin.id").copied();
        if got != expected_id {
            bail!("id {} != {}", got.unwrap_or("None"), expected_id.unwrap_or("None"));
        }
        unrestricted = match attrs.get("plugin.unrestricted").copied() {
            Some("true") => Some(true),
            Some("false") => Some(false),
            _ => bail!("format"),
        };
    }
    for i in 0..n_jars {
        let (name, got) = jar(i);
        let Some(want) = attrs.get(format!("sha256.{name}").as_str()).copied() else {
            bail!("no hash for {name}")
        };
        let Some(got) = got else { bail!("missing") };
        if got != want {
            bail!("hash mismatch for {name}");
        }
    }
    Ok(Verified { unrestricted })
}
