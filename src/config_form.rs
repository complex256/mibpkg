//! Config form schema: describes a package's TOML config as editable form fields.
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldType {
    String,
    Int,
    Float,
    Bool,
    Enum,
    List,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub key: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub help: Option<String>,
    #[serde(rename = "type")]
    pub ty: FieldType,
    #[serde(default)]
    pub default: Option<toml::Value>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub choices: Vec<String>,
    #[serde(default)]
    pub of: Option<FieldType>,
    #[serde(default)]
    pub secret: bool,
    #[serde(default)]
    pub section: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schema {
    #[serde(default, rename = "field")]
    pub fields: Vec<Field>,
}

impl Schema {
    pub fn parse(text: &str) -> Result<Schema> {
        let schema: Schema = toml::from_str(text).context("parsing config schema")?;
        let mut seen = HashSet::new();
        for f in &schema.fields {
            if f.key.is_empty() {
                bail!("config schema: field with empty key");
            }
            if !seen.insert(f.key.as_str()) {
                bail!("config schema: duplicate key {:?}", f.key);
            }
            f.validate().with_context(|| format!("config schema: field {:?}", f.key))?;
        }
        Ok(schema)
    }
}

impl Field {
    fn validate(&self) -> Result<()> {
        if self.ty == FieldType::Enum && self.choices.is_empty() {
            bail!("enum needs non-empty choices");
        }
        if self.ty == FieldType::List {
            match self.of {
                Some(FieldType::String | FieldType::Int | FieldType::Float) => {}
                _ => bail!("list needs `of` = string, int or float"),
            }
        }
        if let (Some(min), Some(max)) = (self.min, self.max) {
            if min > max {
                bail!("min {min} is above max {max}");
            }
        }
        if let Some(d) = &self.default {
            if !self.default_matches(d) {
                bail!("default {d} does not match type {:?}", self.ty);
            }
        }
        Ok(())
    }

    fn default_matches(&self, d: &toml::Value) -> bool {
        use toml::Value as V;
        match (self.ty, d) {
            (FieldType::String, V::String(_)) => true,
            (FieldType::Int, V::Integer(_)) => true,
            (FieldType::Float, V::Float(_) | V::Integer(_)) => true,
            (FieldType::Bool, V::Boolean(_)) => true,
            (FieldType::Enum, V::String(s)) => self.choices.contains(s),
            (FieldType::List, V::Array(items)) => items.iter().all(|i| match (self.of, i) {
                (Some(FieldType::String), V::String(_)) => true,
                (Some(FieldType::Int), V::Integer(_)) => true,
                (Some(FieldType::Float), V::Float(_) | V::Integer(_)) => true,
                _ => false,
            }),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct FormField {
    pub key: String,
    pub label: String,
    pub help: Option<String>,
    /// string|int|float|bool|enum|list|raw
    pub kind: String,
    /// schema|inferred
    pub source: String,
    /// null = unset
    pub value: serde_json::Value,
    pub default: serde_json::Value,
    pub choices: Vec<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub secret: bool,
    pub section: Option<String>,
    pub raw: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct FormDoc {
    /// schema|inferred|raw
    pub mode: String,
    pub fields: Vec<FormField>,
    pub reason: Option<String>,
}

fn kind_name(t: FieldType) -> &'static str {
    match t {
        FieldType::String => "string",
        FieldType::Int => "int",
        FieldType::Float => "float",
        FieldType::Bool => "bool",
        FieldType::Enum => "enum",
        FieldType::List => "list",
    }
}

/// Convert a scalar or scalar-array TOML value to JSON; None for anything else.
fn value_json(v: &toml_edit::Value) -> Option<serde_json::Value> {
    use toml_edit::Value as V;
    match v {
        V::String(s) => Some(serde_json::json!(s.value())),
        V::Integer(i) => Some(serde_json::json!(*i.value())),
        V::Float(f) => Some(serde_json::json!(*f.value())),
        V::Boolean(b) => Some(serde_json::json!(*b.value())),
        V::Array(a) => a.iter().map(value_json).collect::<Option<Vec<_>>>().map(serde_json::Value::Array),
        _ => None,
    }
}

fn value_kind(v: &toml_edit::Value) -> Option<&'static str> {
    use toml_edit::Value as V;
    match v {
        V::String(_) => Some("string"),
        V::Integer(_) => Some("int"),
        V::Float(_) => Some("float"),
        V::Boolean(_) => Some("bool"),
        V::Array(a) if a.iter().all(|i| matches!(i, V::String(_) | V::Integer(_) | V::Float(_))) => Some("list"),
        _ => None,
    }
}

fn new_field(key: &str, label: &str, kind: &str, source: &str) -> FormField {
    FormField {
        key: key.to_string(),
        label: label.to_string(),
        help: None,
        kind: kind.to_string(),
        source: source.to_string(),
        value: serde_json::Value::Null,
        default: serde_json::Value::Null,
        choices: vec![],
        min: None,
        max: None,
        secret: false,
        section: None,
        raw: None,
    }
}

fn raw_field(key: &str, label: &str, section: Option<&str>, snippet: String) -> FormField {
    let mut f = new_field(key, label, "raw", "inferred");
    f.section = section.map(str::to_string);
    f.raw = Some(snippet.trim().to_string());
    f
}

fn infer_table(table: &toml_edit::Table, prefix: &str, out: &mut Vec<FormField>) {
    for (k, item) in table.iter() {
        let key = if prefix.is_empty() { k.to_string() } else { format!("{prefix}.{k}") };
        let section = if prefix.is_empty() { None } else { Some(prefix) };
        match item {
            toml_edit::Item::Table(t) if !k.contains('.') => infer_table(t, &key, out),
            toml_edit::Item::Value(v) if !k.contains('.') => match (value_kind(v), value_json(v)) {
                (Some(kind), Some(json)) => {
                    let mut f = new_field(&key, k, kind, "inferred");
                    f.value = json;
                    f.section = section.map(str::to_string);
                    out.push(f);
                }
                _ => out.push(raw_field(&key, k, section, v.to_string())),
            },
            other => out.push(raw_field(&key, k, section, other.to_string())),
        }
    }
}

fn lookup<'a>(doc: &'a toml_edit::DocumentMut, key: &str) -> Option<&'a toml_edit::Item> {
    let mut cur = doc.as_item();
    for part in key.split('.') {
        cur = cur.as_table_like()?.get(part)?;
    }
    Some(cur)
}

/// Describe a TOML file's text as editable form fields.
pub fn describe(text: &str, schema: Option<&Schema>) -> FormDoc {
    let doc = match text.parse::<toml_edit::DocumentMut>() {
        Ok(d) => d,
        Err(e) => {
            return FormDoc {
                mode: "raw".into(),
                fields: vec![],
                reason: Some(format!("could not parse as TOML: {e}")),
            }
        }
    };
    let Some(schema) = schema else {
        let mut fields = Vec::new();
        infer_table(doc.as_table(), "", &mut fields);
        return FormDoc { mode: "inferred".into(), fields, reason: None };
    };
    let fields = schema
        .fields
        .iter()
        .map(|sf| {
            let label = sf.label.as_deref().unwrap_or(&sf.key);
            let mut f = new_field(&sf.key, label, kind_name(sf.ty), "schema");
            f.help = sf.help.clone();
            f.choices = sf.choices.clone();
            f.min = sf.min;
            f.max = sf.max;
            f.secret = sf.secret;
            f.section = sf.section.clone();
            if let Some(d) = &sf.default {
                f.default = serde_json::to_value(d).unwrap_or(serde_json::Value::Null);
            }
            if let Some(toml_edit::Item::Value(v)) = lookup(&doc, &sf.key) {
                f.value = value_json(v).unwrap_or(serde_json::Value::Null);
            }
            f
        })
        .collect();
    FormDoc { mode: "schema".into(), fields, reason: None }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub key: String,
    /// `null` removes the key.
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct FieldError {
    pub key: String,
    pub message: String,
}

/// What an edit resolved to after validation.
enum Change {
    Remove,
    Set(toml_edit::Value),
}

/// Walk `key` through the document without creating anything. `Ok(None)` means
/// the key (or a parent table) does not exist. Errors on ambiguous dotted keys
/// and on parents that are not tables.
fn resolve<'a>(doc: &'a toml_edit::DocumentMut, key: &str) -> Result<Option<&'a toml_edit::Item>, String> {
    let parts: Vec<&str> = key.split('.').collect();
    if parts.iter().any(|p| p.is_empty()) {
        return Err("invalid key".into());
    }
    let mut cur = doc.as_item();
    for (i, part) in parts.iter().enumerate() {
        let Some(table) = cur.as_table_like() else {
            return Err(format!("`{}` is not a table", parts[..i].join(".")));
        };
        if parts.len() - i > 1 && table.contains_key(&parts[i..].join(".")) {
            return Err("key is ambiguous (a quoted key containing a dot exists); edit the file as text".into());
        }
        match table.get(part) {
            Some(item) => cur = item,
            None => return Ok(None),
        }
    }
    Ok(Some(cur))
}

fn existing_type(item: Option<&toml_edit::Item>) -> Result<Option<(FieldType, Option<FieldType>)>, String> {
    let Some(item) = item else { return Ok(None) };
    let toml_edit::Item::Value(v) = item else {
        return Err("existing value is a table and cannot be edited as a field".into());
    };
    use toml_edit::Value as V;
    let elem = |a: &toml_edit::Array| match a.iter().next() {
        Some(V::Integer(_)) => Some(FieldType::Int),
        Some(V::Float(_)) => Some(FieldType::Float),
        _ => Some(FieldType::String),
    };
    match v {
        V::String(_) => Ok(Some((FieldType::String, None))),
        V::Integer(_) => Ok(Some((FieldType::Int, None))),
        V::Float(_) => Ok(Some((FieldType::Float, None))),
        V::Boolean(_) => Ok(Some((FieldType::Bool, None))),
        V::Array(a) if a.iter().all(|i| matches!(i, V::String(_) | V::Integer(_) | V::Float(_))) => {
            Ok(Some((FieldType::List, elem(a))))
        }
        _ => Err("existing value has a type that cannot be edited as a field".into()),
    }
}

fn infer_json_type(v: &serde_json::Value) -> Result<(FieldType, Option<FieldType>), String> {
    use serde_json::Value as J;
    match v {
        J::String(_) => Ok((FieldType::String, None)),
        J::Bool(_) => Ok((FieldType::Bool, None)),
        J::Number(n) if n.is_i64() => Ok((FieldType::Int, None)),
        J::Number(_) => Ok((FieldType::Float, None)),
        J::Array(items) => {
            let of = if items.iter().all(|i| i.is_string()) {
                FieldType::String
            } else if items.iter().all(|i| i.is_i64()) {
                FieldType::Int
            } else if items.iter().all(|i| i.is_number()) {
                FieldType::Float
            } else {
                return Err("list items must be all strings or all numbers".into());
            };
            Ok((FieldType::List, Some(of)))
        }
        _ => Err("unsupported value type".into()),
    }
}

fn check_range(n: f64, min: Option<f64>, max: Option<f64>) -> Result<(), String> {
    if let Some(min) = min {
        if n < min {
            return Err(format!("must be at least {min}"));
        }
    }
    if let Some(max) = max {
        if n > max {
            return Err(format!("must be at most {max}"));
        }
    }
    Ok(())
}

fn as_int(v: &serde_json::Value) -> Option<i64> {
    if let Some(i) = v.as_i64() {
        return Some(i);
    }
    let f = v.as_f64()?;
    (f.fract() == 0.0 && f >= -9.2e18 && f <= 9.2e18).then_some(f as i64)
}

fn scalar_value(
    ty: FieldType,
    v: &serde_json::Value,
    min: Option<f64>,
    max: Option<f64>,
    choices: &[String],
) -> Result<toml_edit::Value, String> {
    match ty {
        FieldType::String => v.as_str().map(toml_edit::Value::from).ok_or_else(|| "expected text".to_string()),
        FieldType::Bool => v.as_bool().map(toml_edit::Value::from).ok_or_else(|| "expected true or false".to_string()),
        FieldType::Int => {
            let i = as_int(v).ok_or_else(|| "expected a whole number".to_string())?;
            check_range(i as f64, min, max)?;
            Ok(toml_edit::Value::from(i))
        }
        FieldType::Float => {
            let f = v.as_f64().ok_or_else(|| "expected a number".to_string())?;
            check_range(f, min, max)?;
            Ok(toml_edit::Value::from(f))
        }
        FieldType::Enum => {
            let s = v.as_str().ok_or_else(|| "expected text".to_string())?;
            if choices.iter().any(|c| c == s) {
                Ok(toml_edit::Value::from(s))
            } else {
                Err(format!("must be one of: {}", choices.join(", ")))
            }
        }
        FieldType::List => Err("nested lists are not supported".into()),
    }
}

fn build_value(
    ty: FieldType,
    of: Option<FieldType>,
    v: &serde_json::Value,
    min: Option<f64>,
    max: Option<f64>,
    choices: &[String],
) -> Result<toml_edit::Value, String> {
    if ty != FieldType::List {
        return scalar_value(ty, v, min, max, choices);
    }
    let items = v.as_array().ok_or_else(|| "expected a list".to_string())?;
    let of = of.unwrap_or(FieldType::String);
    let mut arr = toml_edit::Array::new();
    for (i, item) in items.iter().enumerate() {
        let val = scalar_value(of, item, None, None, &[]).map_err(|m| format!("item {}: {m}", i + 1))?;
        arr.push(val);
    }
    Ok(toml_edit::Value::Array(arr))
}

fn plan_edit(
    doc: &toml_edit::DocumentMut,
    edit: &Edit,
    schema: Option<&Schema>,
) -> Result<Change, String> {
    if edit.key.is_empty() {
        return Err("empty key".into());
    }
    let existing = resolve(doc, &edit.key)?;
    if edit.value.is_null() {
        return Ok(Change::Remove);
    }
    let field = schema.and_then(|s| s.fields.iter().find(|f| f.key == edit.key));
    if let Some(f) = field {
        return build_value(f.ty, f.of, &edit.value, f.min, f.max, &f.choices).map(Change::Set);
    }
    let (ty, of) = match existing_type(existing)? {
        Some((ty, of)) => (ty, of),
        None => infer_json_type(&edit.value)?,
    };
    // A JSON integer edit to an existing float stays a float; a fraction into
    // an existing int is rejected by the Int check.
    build_value(ty, of, &edit.value, None, None, &[]).map(Change::Set)
}

fn apply_change(doc: &mut toml_edit::DocumentMut, key: &str, change: Change) -> Result<(), String> {
    use toml_edit::{InlineTable, Item, Table, Value};
    let parts: Vec<&str> = key.split('.').collect();
    let (leaf, parents) = parts.split_last().expect("key checked non-empty");
    let mut cur: &mut Item = doc.as_item_mut();
    for part in parents {
        let inline = matches!(cur, Item::Value(Value::InlineTable(_)));
        let Some(t) = cur.as_table_like_mut() else {
            return Err(format!("`{part}` has a non-table parent"));
        };
        if t.get(part).is_none() {
            if matches!(change, Change::Remove) {
                return Ok(());
            }
            let new = if inline {
                Item::Value(Value::InlineTable(InlineTable::new()))
            } else {
                let mut tb = Table::new();
                tb.set_implicit(true);
                Item::Table(tb)
            };
            t.insert(part, new);
        }
        cur = t.get_mut(part).expect("just ensured");
    }
    let Some(t) = cur.as_table_like_mut() else {
        return Err("parent is not a table".into());
    };
    match change {
        Change::Remove => {
            t.remove(leaf);
        }
        Change::Set(mut v) => {
            if let Some(Item::Value(old)) = t.get(leaf) {
                *v.decor_mut() = old.decor().clone();
            }
            t.insert(leaf, Item::Value(v));
        }
    }
    Ok(())
}

/// Apply form edits to TOML text, preserving comments, key order and unknown keys.
/// Returns every field error together when any edit is invalid.
pub fn apply(text: &str, edits: &[Edit], schema: Option<&Schema>) -> Result<String, Vec<FieldError>> {
    let mut doc = text.parse::<toml_edit::DocumentMut>().map_err(|e| {
        vec![FieldError { key: String::new(), message: format!("could not parse as TOML: {e}") }]
    })?;
    let mut errors = Vec::new();
    for edit in edits {
        let result = plan_edit(&doc, edit, schema).and_then(|c| apply_change(&mut doc, &edit.key, c));
        if let Err(message) = result {
            errors.push(FieldError { key: edit.key.clone(), message });
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let out = doc.to_string();
    if text.contains("\r\n") {
        Ok(out.replace("\r\n", "\n").replace('\n', "\r\n"))
    } else {
        Ok(out)
    }
}
