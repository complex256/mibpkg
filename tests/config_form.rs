use mibpkg::config_form::{describe, Field, FieldType, Schema};

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

fn kind_of<'a>(d: &'a mibpkg::config_form::FormDoc, key: &str) -> &'a mibpkg::config_form::FormField {
    d.fields.iter().find(|f| f.key == key).unwrap_or_else(|| panic!("no field {key}"))
}

#[test]
fn infers_scalar_kinds() {
    let d = describe(
        "top = true\n[net]\nport = 80\nratio = 1.5\nname = \"x\"\nhosts = [\"a\", \"b\"]\n",
        None,
    );
    assert_eq!(d.mode, "inferred");
    assert_eq!(kind_of(&d, "top").kind, "bool");
    assert_eq!(kind_of(&d, "top").section, None);
    let p = kind_of(&d, "net.port");
    assert_eq!(p.kind, "int");
    assert_eq!(p.section.as_deref(), Some("net"));
    assert_eq!(p.source, "inferred");
    assert_eq!(p.value, serde_json::json!(80));
    assert_eq!(kind_of(&d, "net.ratio").kind, "float");
    assert_eq!(kind_of(&d, "net.name").kind, "string");
    let h = kind_of(&d, "net.hosts");
    assert_eq!(h.kind, "list");
    assert_eq!(h.value, serde_json::json!(["a", "b"]));
}

#[test]
fn inferred_complex_values_are_raw() {
    let d = describe(
        "t = { a = 1 }\nwhen = 1979-05-27T07:32:00Z\n[[items]]\nn = 1\n",
        None,
    );
    for k in ["t", "when", "items"] {
        let f = kind_of(&d, k);
        assert_eq!(f.kind, "raw", "{k}");
        assert!(f.raw.is_some(), "{k}");
    }
    assert!(kind_of(&d, "t").raw.as_deref().unwrap().contains("a = 1"));
    assert!(kind_of(&d, "when").raw.as_deref().unwrap().contains("1979"));
}

#[test]
fn dotted_quoted_key_is_raw_field() {
    let d = describe("\"a.b\" = 1\n", None);
    assert_eq!(d.fields.len(), 1);
    assert_eq!(d.fields[0].kind, "raw");
    assert_eq!(d.fields[0].key, "a.b");
}

const SCHEMA: &str = "[[field]]\nkey = \"port\"\ntype = \"int\"\ndefault = 8080\n[[field]]\nkey = \"net.mode\"\ntype = \"enum\"\nchoices = [\"a\", \"b\"]\n";

#[test]
fn schema_fields_listed_even_when_missing() {
    let s = Schema::parse(SCHEMA).unwrap();
    let d = describe("", Some(&s));
    assert_eq!(d.mode, "schema");
    let p = kind_of(&d, "port");
    assert_eq!(p.source, "schema");
    assert!(p.value.is_null());
    assert_eq!(p.default, serde_json::json!(8080));
    assert_eq!(kind_of(&d, "net.mode").kind, "enum");
    assert_eq!(kind_of(&d, "net.mode").choices, vec!["a", "b"]);
}

#[test]
fn schema_mode_keeps_unlisted_keys_out_of_form() {
    let s = Schema::parse(SCHEMA).unwrap();
    let d = describe("port = 9\nother = 1\n[net]\nmode = \"b\"\n", Some(&s));
    assert_eq!(d.mode, "schema");
    assert_eq!(d.fields.len(), 2);
    assert_eq!(kind_of(&d, "port").value, serde_json::json!(9));
    assert_eq!(kind_of(&d, "net.mode").value, serde_json::json!("b"));
}

#[test]
fn unparseable_file_is_raw_mode() {
    let d = describe("this is = = not toml", None);
    assert_eq!(d.mode, "raw");
    assert!(d.fields.is_empty());
    assert!(d.reason.is_some());
    let s = Schema::parse(SCHEMA).unwrap();
    assert_eq!(describe("[", Some(&s)).mode, "raw");
}

#[test]
fn empty_text_with_schema_lists_fields() {
    let s = Schema::parse(SCHEMA).unwrap();
    assert_eq!(describe("", Some(&s)).fields.len(), 2);
    assert_eq!(describe("", None).mode, "inferred");
}
