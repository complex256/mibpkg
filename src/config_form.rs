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
