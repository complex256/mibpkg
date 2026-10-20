//! Line editing of `.properties` files the runtime reads (`grants.properties`,
//! `plugins.properties`): port of `mibsdk-dev`'s `GRANTS_PY`. Only the line whose key is exactly
//! the given key is touched; every other line is kept byte for byte.

/// The key of `line`: the trimmed text before the first `=`; `None` for a comment line (`#` after
/// leading whitespace) or a line without `=`.
fn key_of(line: &str) -> Option<&str> {
    if line.trim_start().starts_with('#') {
        return None;
    }
    line.split_once('=').map(|(k, _)| k.trim())
}

/// The lines of `text`, split on `\n` only (a `\r` stays with its line); a final newline does not
/// start another line.
fn lines(text: &str) -> Vec<&str> {
    let mut v: Vec<&str> = text.split('\n').collect();
    if v.last() == Some(&"") {
        v.pop();
    }
    v
}

fn join(out: &[&str]) -> String {
    if out.is_empty() {
        return String::new();
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// `text` with the first line keyed `key` replaced by `key=value` and any later ones dropped, or
/// with `key=value` appended when there is none.
pub fn set(text: &str, key: &str, value: &str) -> String {
    let new = format!("{key}={value}");
    let mut out = Vec::new();
    let mut done = false;
    for line in lines(text) {
        if key_of(line) == Some(key) {
            if !done {
                out.push(new.as_str());
                done = true;
            }
            continue;
        }
        out.push(line);
    }
    if !done {
        out.push(new.as_str());
    }
    join(&out)
}

/// `text` without the lines keyed `key`, and whether there were any.
pub fn drop(text: &str, key: &str) -> (String, bool) {
    let all = lines(text);
    let out: Vec<&str> = all.iter().copied().filter(|l| key_of(l) != Some(key)).collect();
    let dropped = out.len() != all.len();
    (join(&out), dropped)
}
