use mibpkg::config_form::{Field, FieldType, Schema};

fn err(text: &str) -> String {
    format!("{:#}", Schema::parse(text).unwrap_err())
}

#[test]
fn schema_parses_all_types() {
    let s = Schema::parse(
        r#"
[[field]]
key = "name"
label = "Name"
help = "Display name"
type = "string"
default = "x"
secret = true
section = "General"

[[field]]
key = "count"
type = "int"
min = 1
max = 10
default = 5

[[field]]
key = "ratio"
type = "float"
default = 0.5

[[field]]
key = "on"
type = "bool"
default = true

[[field]]
key = "mode"
type = "enum"
choices = ["a", "b"]
default = "a"

[[field]]
key = "tags"
type = "list"
of = "string"
default = ["x", "y"]
"#,
    )
    .unwrap();
    assert_eq!(s.fields.len(), 6);
    let f: &Field = &s.fields[0];
    assert_eq!(f.key, "name");
    assert_eq!(f.label.as_deref(), Some("Name"));
    assert_eq!(f.help.as_deref(), Some("Display name"));
    assert_eq!(f.ty, FieldType::String);
    assert_eq!(f.default, Some(toml::Value::String("x".into())));
    assert!(f.secret);
    assert_eq!(f.section.as_deref(), Some("General"));
    assert_eq!(s.fields[1].ty, FieldType::Int);
    assert_eq!(s.fields[1].min, Some(1.0));
    assert_eq!(s.fields[1].max, Some(10.0));
    assert_eq!(s.fields[2].ty, FieldType::Float);
    assert_eq!(s.fields[3].ty, FieldType::Bool);
    assert!(!s.fields[3].secret);
    assert_eq!(s.fields[4].ty, FieldType::Enum);
    assert_eq!(s.fields[4].choices, vec!["a", "b"]);
    assert_eq!(s.fields[5].ty, FieldType::List);
    assert_eq!(s.fields[5].of, Some(FieldType::String));
}

#[test]
fn schema_rejects_unknown_type() {
    let e = err("[[field]]\nkey = \"d\"\ntype = \"date\"\n");
    assert!(e.contains("date"), "{e}");
}

#[test]
fn schema_rejects_enum_without_choices() {
    let e = err("[[field]]\nkey = \"m\"\ntype = \"enum\"\n");
    assert!(e.contains("choices"), "{e}");
}

#[test]
fn schema_rejects_default_of_wrong_type() {
    let e = err("[[field]]\nkey = \"n\"\ntype = \"int\"\ndefault = \"x\"\n");
    assert!(e.contains("default"), "{e}");
}

#[test]
fn schema_rejects_list_without_of_and_of_must_be_string_int_or_float() {
    let e = err("[[field]]\nkey = \"l\"\ntype = \"list\"\n");
    assert!(e.contains("of"), "{e}");
    let e = err("[[field]]\nkey = \"l\"\ntype = \"list\"\nof = \"bool\"\n");
    assert!(e.contains("of"), "{e}");
}

#[test]
fn schema_rejects_duplicate_keys() {
    let e = err("[[field]]\nkey = \"a\"\ntype = \"bool\"\n[[field]]\nkey = \"a\"\ntype = \"bool\"\n");
    assert!(e.contains("duplicate"), "{e}");
}

#[test]
fn schema_rejects_min_above_max() {
    let e = err("[[field]]\nkey = \"n\"\ntype = \"int\"\nmin = 5\nmax = 1\n");
    assert!(e.contains("min"), "{e}");
}
