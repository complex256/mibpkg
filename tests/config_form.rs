use mibpkg::config_form::{apply, describe, Edit, Field, FieldType, Schema};
use serde_json::json;

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

// ---- apply ----

fn e(key: &str, value: serde_json::Value) -> Edit {
    Edit { key: key.to_string(), value }
}

fn sch(text: &str) -> Schema {
    Schema::parse(text).unwrap()
}

#[test]
fn edit_preserves_comments_order_and_unknown_keys() {
    let text = "# top comment\nz = 1 # trailing\nname = \"old\" # keep me\n\n# section\n[net]\nunknown = [1, 2]\nport = 80\n";
    let out = apply(text, &[e("name", json!("new")), e("net.port", json!(81))], None).unwrap();
    assert_eq!(
        out,
        "# top comment\nz = 1 # trailing\nname = \"new\" # keep me\n\n# section\n[net]\nunknown = [1, 2]\nport = 81\n"
    );
}

#[test]
fn edit_creates_missing_table() {
    let out = apply("a = 1\n", &[e("server.port", json!(8080))], None).unwrap();
    assert!(out.starts_with("a = 1\n"));
    assert!(out.contains("[server]\nport = 8080"), "{out}");
    let doc: toml::Value = toml::from_str(&out).unwrap();
    assert_eq!(doc["server"]["port"].as_integer(), Some(8080));
}

#[test]
fn null_removes_key() {
    let out = apply("a = 1\nb = 2\n", &[e("a", json!(null)), e("missing", json!(null))], None).unwrap();
    assert_eq!(out, "b = 2\n");
}

#[test]
fn crlf_file_stays_crlf() {
    let text = "# c\r\na = 1\r\n[t]\r\nb = 2\r\n";
    let out = apply(text, &[e("a", json!(5)), e("new.x", json!("y")), e("t.b", json!(null))], None).unwrap();
    assert!(out.contains("\r\n"));
    assert!(!out.replace("\r\n", "").contains('\n'), "{out:?}");
}

#[test]
fn int_field_rejects_fraction() {
    let s = sch("[[field]]\nkey = \"n\"\ntype = \"int\"\n");
    let errs = apply("n = 1\n", &[e("n", json!(2.5))], Some(&s)).unwrap_err();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].key, "n");
}

#[test]
fn int_out_of_range_for_min_max() {
    let s = sch("[[field]]\nkey = \"n\"\ntype = \"int\"\nmin = 1\nmax = 10\n");
    assert!(apply("n = 1\n", &[e("n", json!(0))], Some(&s)).is_err());
    assert!(apply("n = 1\n", &[e("n", json!(11))], Some(&s)).is_err());
    assert_eq!(apply("n = 1\n", &[e("n", json!(10))], Some(&s)).unwrap(), "n = 10\n");
}

#[test]
fn enum_rejects_value_outside_choices() {
    let s = sch("[[field]]\nkey = \"m\"\ntype = \"enum\"\nchoices = [\"a\", \"b\"]\n");
    assert!(apply("m = \"a\"\n", &[e("m", json!("c"))], Some(&s)).is_err());
    assert_eq!(apply("m = \"a\"\n", &[e("m", json!("b"))], Some(&s)).unwrap(), "m = \"b\"\n");
}

#[test]
fn float_written_with_decimal_point() {
    let s = sch("[[field]]\nkey = \"r\"\ntype = \"float\"\n");
    assert_eq!(apply("r = 0.5\n", &[e("r", json!(3))], Some(&s)).unwrap(), "r = 3.0\n");
    // inferred from the existing float
    assert_eq!(apply("r = 0.5\n", &[e("r", json!(4))], None).unwrap(), "r = 4.0\n");
    // new key: JSON integer stays integer, float keeps decimal
    let out = apply("", &[e("i", json!(3)), e("f", json!(1.5))], None).unwrap();
    assert!(out.contains("i = 3\n") && out.contains("f = 1.5"), "{out}");
}

#[test]
fn list_edit_replaces_array_with_typed_items() {
    let s = sch("[[field]]\nkey = \"t\"\ntype = \"list\"\nof = \"int\"\n");
    assert_eq!(apply("t = [1]\n", &[e("t", json!([1, 2, 3]))], Some(&s)).unwrap(), "t = [1, 2, 3]\n");
    assert!(apply("t = [1]\n", &[e("t", json!([1, "x"]))], Some(&s)).is_err());
    let sf = sch("[[field]]\nkey = \"t\"\ntype = \"list\"\nof = \"float\"\n");
    assert_eq!(apply("t = [1.5]\n", &[e("t", json!([1, 2]))], Some(&sf)).unwrap(), "t = [1.0, 2.0]\n");
}

#[test]
fn edit_conflicting_with_existing_non_table_parent_errors() {
    let errs = apply("a = 1\n", &[e("a.b", json!(2))], None).unwrap_err();
    assert_eq!(errs[0].key, "a.b");
}

#[test]
fn quoted_dotted_key_is_refused_not_split() {
    let errs = apply("\"a.b\" = 1\n", &[e("a.b", json!(2))], None).unwrap_err();
    assert_eq!(errs[0].key, "a.b");
    let errs = apply("[t]\n\"a.b\" = 1\n", &[e("t.a.b", json!(2))], None).unwrap_err();
    assert_eq!(errs[0].key, "t.a.b");
}

#[test]
fn all_field_errors_returned_together() {
    let s = sch("[[field]]\nkey = \"n\"\ntype = \"int\"\n[[field]]\nkey = \"b\"\ntype = \"bool\"\n");
    let errs = apply("", &[e("n", json!("x")), e("b", json!(1)), e("ok", json!(1))], Some(&s)).unwrap_err();
    let keys: Vec<_> = errs.iter().map(|x| x.key.as_str()).collect();
    assert_eq!(keys, ["n", "b"]);
}

#[test]
fn unparseable_input_returns_error_for_empty_key() {
    let errs = apply("a = = 1", &[e("a", json!(1))], None).unwrap_err();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].key, "");
}

#[test]
fn existing_out_of_range_value_not_touched_is_not_an_error() {
    let s = sch("[[field]]\nkey = \"n\"\ntype = \"int\"\nmax = 10\n[[field]]\nkey = \"m\"\ntype = \"int\"\n");
    let out = apply("n = 99\nm = 1\n", &[e("m", json!(2))], Some(&s)).unwrap();
    assert_eq!(out, "n = 99\nm = 2\n");
}

#[test]
fn edit_keeps_comment_above_key_and_trailing_comment() {
    let out = apply("[net]\n# the port\nport = 80 # tail\n", &[e("net.port", json!(81))], None).unwrap();
    assert_eq!(out, "[net]\n# the port\nport = 81 # tail\n");
    let out = apply("# lead\nname = \"a\" # t\n", &[e("name", json!("b"))], None).unwrap();
    assert_eq!(out, "# lead\nname = \"b\" # t\n");
}

#[test]
fn null_on_table_or_array_of_tables_is_refused() {
    let text = "a = 1\n[server]\nport = 80\nhost = \"h\"\n";
    let errs = apply(text, &[e("server", json!(null))], None).unwrap_err();
    assert_eq!(errs[0].key, "server");
    let errs = apply("[[s]]\nx = 1\n", &[e("s", json!(null))], None).unwrap_err();
    assert_eq!(errs[0].key, "s");
}

#[test]
fn schema_field_cannot_overwrite_a_table() {
    let s = sch("[[field]]\nkey = \"server\"\ntype = \"string\"\n");
    let errs = apply("[server]\nport = 80\n", &[e("server", json!("x"))], Some(&s)).unwrap_err();
    assert_eq!(errs[0].key, "server");
    let errs = apply("[[server]]\nport = 80\n", &[e("server", json!("x"))], Some(&s)).unwrap_err();
    assert_eq!(errs[0].key, "server");
}
