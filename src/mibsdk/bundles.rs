//! The files the SDK install writes for the HMI: the bundle list
//! (`bundles_dsitracer_enabled.properties`) and `sdk.properties`. Port of the SDK repo's
//! `installer/gen_bundles.py`, plus `mibsdk-dev`'s `check_hmicar` guard.
//!
//! Every key in the bundle list other than the configurator and `Bundle.Startup.Autostart` is a
//! bundle definition to LSD, so `generate` touches exactly: the serviceDelegateConfigurator line,
//! the `Bundle.Startup.Autostart` value, the `HMICar=` value, and one appended SDK bundle line.
//! Everything else is copied byte for byte. SDK settings never go in the bundle list; they belong
//! in `sdk.properties`.
use crate::manifest::SdkSpec;
use anyhow::{bail, Result};

const CONFIGURATOR_KEY: &[u8] = b"de.audi.tghu.serviceDelegateConfigurator";
const AUTOSTART_KEY: &[u8] = b"Bundle.Startup.Autostart";
const HMICAR_KEY: &[u8] = b"HMICar";
/// DSITracer's bundle name: first in the autostart list while the tracer is activated.
const TRACER_BUNDLE: &[u8] = b"DSITracer";

/// The lines of `b` with their line breaks, split like Python's `bytes.splitlines(keepends=True)`:
/// after `\r\n`, `\n` or a lone `\r`.
fn split_lines(b: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < b.len() {
        let end = match b[i] {
            b'\r' if b.get(i + 1) == Some(&b'\n') => i + 2,
            b'\r' | b'\n' => i + 1,
            _ => {
                i += 1;
                continue;
            }
        };
        out.push(b[start..end].to_vec());
        start = end;
        i = end;
    }
    if start < b.len() {
        out.push(b[start..].to_vec());
    }
    out
}

/// `line` split into its body and its `\r\n` / `\n` line break (a lone `\r` stays in the body).
fn split_eol(line: &[u8]) -> (&[u8], &[u8]) {
    if line.ends_with(b"\r\n") {
        line.split_at(line.len() - 2)
    } else if line.ends_with(b"\n") {
        line.split_at(line.len() - 1)
    } else {
        (line, b"")
    }
}

fn skip_blanks(b: &[u8]) -> &[u8] {
    let n = b.iter().take_while(|c| matches!(c, b' ' | b'\t')).count();
    &b[n..]
}

/// What follows `<key>[ \t]*=` at the start of `line`, or `None` when it does not start so.
fn after_key_eq<'a>(line: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    skip_blanks(line.strip_prefix(key)?).strip_prefix(b"=")
}

/// `^(#+[ \t]*)?<configurator key>[ \t]*=`: an active or commented-out configurator line.
fn is_configurator(line: &[u8]) -> bool {
    let hashes = line.iter().take_while(|c| **c == b'#').count();
    let rest = if hashes > 0 { skip_blanks(&line[hashes..]) } else { line };
    after_key_eq(rest, CONFIGURATOR_KEY).is_some()
}

/// Python's `bytes.strip()`: ASCII whitespace off both ends.
fn strip_ascii(b: &[u8]) -> &[u8] {
    let ws = |c: &u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c);
    let start = b.iter().position(|c| !ws(c)).unwrap_or(b.len());
    let end = b.iter().rposition(|c| !ws(c)).map_or(start, |e| e + 1);
    &b[start..end]
}

/// `b` decoded as Latin-1 and stripped like Python's `str.strip()`, for an error message.
fn latin1_stripped(b: &[u8]) -> String {
    let s: String = b.iter().map(|&c| c as char).collect();
    s.trim_matches(|c: char| c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)).to_string()
}

/// The bundle list for an SDK install: `stock` with the configurator set to `sdk.configurator`,
/// the SDK bundle (and DSITracer when `tracer_active`) first in `Bundle.Startup.Autostart`,
/// `HMICar=` pointed at `sdk.hmicar` and `sdk.bundle` appended when no line defines that bundle.
/// Each changed line keeps its own line break. The output is a fixed point: generating from it
/// again gives the same bytes.
pub fn generate(stock: &[u8], sdk: &SdkSpec, tracer_active: bool) -> Result<Vec<u8>> {
    let bundle_line = sdk.bundle.as_bytes();
    let bundle_name = match sdk.bundle.split_once('=') {
        Some((name, _)) if !name.is_empty() => name.as_bytes(),
        _ => bail!("[sdk] bundle {:?} is not a <name>=<activator> line", sdk.bundle),
    };
    let mut lines = split_lines(stock);

    let cfg: Vec<usize> = (0..lines.len()).filter(|&i| is_configurator(&lines[i])).collect();
    let active: Vec<usize> = cfg.iter().copied().filter(|&i| !lines[i].starts_with(b"#")).collect();
    if cfg.is_empty() {
        bail!("no serviceDelegateConfigurator line in the stock file");
    }
    if active.len() > 1 {
        bail!(
            "{} active serviceDelegateConfigurator lines in the stock file (the last would win); refusing to guess",
            active.len()
        );
    }
    let target = active.first().copied().unwrap_or(cfg[0]);
    let (body, eol) = split_eol(&lines[target]);
    refuse_bare_cr(body, CONFIGURATOR_KEY)?;
    let eol = eol.to_vec();
    lines[target] = [CONFIGURATOR_KEY, b"=", sdk.configurator.as_bytes(), &eol].concat();

    let auto: Vec<usize> =
        (0..lines.len()).filter(|&i| after_key_eq(split_eol(&lines[i]).0, AUTOSTART_KEY).is_some()).collect();
    if auto.len() != 1 {
        bail!("expected exactly one Bundle.Startup.Autostart line, found {}", auto.len());
    }
    let (body, eol) = split_eol(&lines[auto[0]]);
    refuse_bare_cr(body, AUTOSTART_KEY)?;
    let value = skip_blanks(after_key_eq(body, AUTOSTART_KEY).unwrap_or_default());
    let prefix = &body[..body.len() - value.len()];
    let rest = value
        .split(|c| *c == b',')
        .map(strip_ascii)
        .filter(|x| !x.is_empty() && *x != TRACER_BUNDLE && *x != bundle_name);
    let head: &[&[u8]] = if tracer_active { &[TRACER_BUNDLE, bundle_name] } else { &[bundle_name] };
    let list: Vec<&[u8]> = head.iter().copied().chain(rest).collect();
    lines[auto[0]] = [prefix, &list.join(&b","[..]), eol].concat();

    let hmi: Vec<usize> =
        (0..lines.len()).filter(|&i| after_key_eq(split_eol(&lines[i]).0, HMICAR_KEY).is_some()).collect();
    if hmi.len() != 1 {
        bail!("expected exactly one HMICar= line, found {}", hmi.len());
    }
    // The key as the HMI's Properties.load reads it must name that same line and no other.
    let props = hmicar_lines(&lines);
    if props != hmi {
        let n = if props.is_empty() { 0 } else { props.len() + usize::from(!props.contains(&hmi[0])) };
        bail!("expected exactly one HMICar= line, found {n}");
    }
    let (body, eol) = split_eol(&lines[hmi[0]]);
    refuse_bare_cr(body, HMICAR_KEY)?;
    // Exact bytes; the SDK's own value is accepted too, so generate() is a fixed point.
    let accepted = [&sdk.hmicar_stock, &sdk.hmicar].map(|v| [HMICAR_KEY, b"=", v.as_bytes()].concat());
    if !accepted.iter().any(|a| a == body) {
        let value = body.splitn(2, |c| *c == b'=').nth(1).unwrap_or_default();
        bail!("HMICar is not stock ({}); refusing", latin1_stripped(value));
    }
    lines[hmi[0]] = [&accepted[1], eol].concat();

    if !lines.iter().any(|l| after_key_eq(l, bundle_name).is_some()) {
        let eol: &[u8] = if lines.last().is_some_and(|l| l.ends_with(b"\r\n")) { b"\r\n" } else { b"\n" };
        if let Some(last) = lines.last_mut() {
            if !last.ends_with(b"\n") {
                last.extend_from_slice(eol);
            }
        }
        lines.push([bundle_line, eol].concat());
    }
    Ok(lines.concat())
}

/// `sdk.properties`: `chain.configurator=<tracer configurator>` while the tracer is activated
/// (the SDK's configurator chains it), then each `[sdk] properties` line.
pub fn sdk_properties(sdk: &SdkSpec, tracer_active: bool) -> Vec<u8> {
    let mut out = Vec::new();
    if tracer_active {
        out.extend_from_slice(format!("chain.configurator={}\n", sdk.tracer_configurator).as_bytes());
    }
    for line in &sdk.properties {
        out.extend_from_slice(line.as_bytes());
        out.push(b'\n');
    }
    out
}

/// Port of `mibsdk-dev`'s `check_hmicar`, counting the key as `java.util.Properties` reads it:
/// `text` must define the key `HMICar` exactly once, in any spelling (`hmicar_lines`), and that
/// line (without its `\r\n` / `\n`) must equal one of the `allowed` full lines byte for byte.
/// Properties keeps the last of several, so any extra spelling refuses. `what` names the file in
/// the refusal.
pub fn check_hmicar(text: &[u8], what: &str, allowed: &[&str]) -> Result<()> {
    let lines = split_lines(text);
    let hits = hmicar_lines(&lines);
    let got = hits.last().map(|&i| split_eol(&lines[i]).0);
    if hits.len() != 1 || !allowed.iter().any(|a| Some(a.as_bytes()) == got) {
        let shown = got.map_or_else(|| "no HMICar= line".to_string(), latin1);
        bail!("HMICar is not stock in {what} ({shown}); refusing; nothing changed");
    }
    Ok(())
}

/// Refuses a line about to be rewritten whose break is a lone `\r`: `split_eol` keeps that `\r`
/// in the body, so the rewritten line would run into the next one.
fn refuse_bare_cr(body: &[u8], key: &[u8]) -> Result<()> {
    if body.ends_with(b"\r") {
        bail!(
            "the {} line ends in a bare CR; rewriting it would join it to the next line; refusing",
            latin1(key)
        );
    }
    Ok(())
}

/// `b` decoded as Latin-1 (the encoding `Properties.load` reads).
fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

/// `Properties.load`'s blanks: space, tab and form feed.
fn is_props_blank(c: &u8) -> bool {
    matches!(c, b' ' | b'\t' | 0x0c)
}

/// The indexes (into `split_lines`) of the lines that start a logical line whose key
/// `Properties.load` reads as `HMICar`: leading blanks skipped; `#` / `!` comment lines skipped
/// (they never continue); a line ending in an odd number of backslashes continues onto the next,
/// which is then part of its value (or key), never a key of its own.
fn hmicar_lines(lines: &[Vec<u8>]) -> Vec<usize> {
    let mut hits = Vec::new();
    let mut open: Option<(usize, Vec<u8>)> = None;
    for (i, line) in lines.iter().enumerate() {
        let body = split_eol(line).0;
        let body = body.strip_suffix(b"\r").unwrap_or(body);
        let text = &body[body.iter().take_while(|c| is_props_blank(c)).count()..];
        let (start, mut logical) = match open.take() {
            Some(cont) => cont,
            None if text.is_empty() || text[0] == b'#' || text[0] == b'!' => continue,
            None => (i, Vec::new()),
        };
        let backslashes = text.iter().rev().take_while(|c| **c == b'\\').count();
        if backslashes % 2 == 1 {
            logical.extend_from_slice(&text[..text.len() - 1]);
            open = Some((start, logical));
            continue;
        }
        logical.extend_from_slice(text);
        if properties_key(&logical).as_deref() == Some("HMICar") {
            hits.push(start);
        }
    }
    if let Some((start, logical)) = open {
        if properties_key(&logical).as_deref() == Some("HMICar") {
            hits.push(start);
        }
    }
    hits
}

/// The key of a logical line as `Properties.load` reads it: up to the first unescaped `=`, `:`
/// or blank, with `\uXXXX`, `\t`, `\n`, `\r`, `\f` and `\<c>` escapes resolved. `None` for a
/// malformed `\u` escape.
fn properties_key(logical: &[u8]) -> Option<String> {
    let mut key = String::new();
    let mut i = 0;
    while let Some(&c) = logical.get(i) {
        i += 1;
        match c {
            b'=' | b':' => break,
            _ if is_props_blank(&c) => break,
            b'\\' => {
                let Some(&e) = logical.get(i) else { break };
                i += 1;
                key.push(match e {
                    b'u' => {
                        let hex = logical.get(i..i + 4).filter(|h| h.iter().all(u8::is_ascii_hexdigit))?;
                        i += 4;
                        char::from_u32(u32::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?)?
                    }
                    b't' => '\t',
                    b'n' => '\n',
                    b'r' => '\r',
                    b'f' => '\x0c',
                    other => other as char,
                });
            }
            _ => key.push(c as char),
        }
    }
    Some(key)
}
