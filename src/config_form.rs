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
