# Config forms and schemas

For a config file that ends in `.toml`, the web interface shows two tabs: **Form** and **Raw**. Raw is
the plain text editor. Form shows one control per setting: a switch for yes/no, a text box with a
numeric keypad for numbers, a drop-down for choices, and so on. Saving from either tab uses the same **Save and restart**
button.

A package author can ship a **schema** that tells the form what each setting is called, what it is for,
and what values are allowed. Without one, the form works out its controls from the file itself.

## What is preserved

Saving from the form changes only the settings you edited. Comments, the order of the file, blank lines
and any keys the form does not know about stay exactly as they were. Clearing a setting removes that key
from the file.

## Writing a schema

Add `schema` to the `[[config]]` entry in `manifest.toml`. The config `path` must end in `.toml`.

```toml
[[config]]
path = "broker.toml"
example = "broker.toml.example"
schema = "broker.schema.toml"
```

The schema file only has to be in the package folder; `mibpkg build` packs it with the rest. You do not
need to list it under `[[file]]`.

The schema file ships in the package, next to the example. It is a TOML file with one `[[field]]` table
per setting:

```toml
[[field]]
key = "host"
label = "Broker address"
help = "Host name or IP address of the MQTT broker."
type = "string"
default = "homeassistant.local"
section = "Connection"

[[field]]
key = "port"
label = "Port"
type = "int"
min = 1
max = 65535
default = 1883
section = "Connection"

[[field]]
key = "password"
label = "Password"
type = "string"
secret = true
section = "Connection"

[[field]]
key = "tls"
label = "Use TLS"
type = "bool"
default = false
section = "Connection"

[[field]]
key = "publish.interval"
label = "Publish every (seconds)"
type = "float"
min = 0.5
max = 60
default = 2.5
section = "Publishing"

[[field]]
key = "publish.qos"
label = "Quality of service"
type = "enum"
choices = ["at-most-once", "at-least-once", "exactly-once"]
default = "at-least-once"
section = "Publishing"

[[field]]
key = "publish.pids"
label = "Data points (PIDs)"
type = "list"
of = "int"
min = 0
default = [12, 13]
section = "Publishing"
```

### Field keys

| Key | Meaning |
|---|---|
| `key` | Required. The setting's name in the config file. Use dots for a table: `publish.interval` is `interval` inside `[publish]`. A key is a dotted path with no empty parts (`a..b`, `.a` and `a.` are rejected). Unique within the schema |
| `type` | Required. `string`, `int`, `float`, `bool`, `enum` or `list` |
| `label` | The name shown in the form. Defaults to the key |
| `help` | A line of explanation under the control |
| `default` | Shown as the default while the key is not in the file; it is not written to the file unless you set a value. It must fit the type |
| `min`, `max` | Limits for `int`, `float`, and for each item of a `list`. `min` may not be above `max` |
| `choices` | Required for `enum`: the allowed values, as text. Not empty |
| `of` | Required for `list`: the type of each item, `string`, `int` or `float` |
| `secret` | `true` hides the value in the form, like a password box |
| `section` | A heading to group fields under. Fields with the same `section` appear together |

### Field types

| Type | Control | Rules |
|---|---|---|
| `string` | Text box | Any text. Clearing the box removes the key from the file |
| `int` | Text box with a numeric keypad | A whole number, within `min` and `max` if set |
| `float` | Text box with a numeric keypad | Any number, within `min` and `max` if set |
| `bool` | Switch | `true` or `false` |
| `enum` | Drop-down | One of `choices` |
| `list` | Text box, one item per line | A list of `string`, `int` or `float` items, set by `of`; no lists of lists |

Only keys listed in the schema get a control; everything else in
the file is kept untouched.

### Checks

`mibpkg build` and `mibpkg verify` reject a package whose schema file is missing from the package or does
not parse. A schema is rejected for an unknown type or key, a repeated `key`, an `enum` without
`choices`, a `list` without a valid `of`, a `min` above `max`, or a `default` that does not fit its type.
Fix these before publishing.

When you save, the server checks every edited value against the schema. A value that is out of range or
the wrong type is refused, the file is not changed, and the form shows the message under that field.

## Without a schema

If the package has no schema, the form is built from the file as it stands:

- yes/no values become switches, numbers become number boxes, text becomes a text box;
- a list of text or numbers becomes a text box with one item per line;
- a table becomes a section;
- arrays of tables, inline tables and dates are shown as read-only text; change them in the Raw tab.

Labels are the key names, and there are no limits beyond keeping each value's type. A setting that is not
in the file yet cannot be added this way; ship a schema, or use Raw. A value is only hidden when a schema says
`secret = true`.

## When the form is not offered

Only the Raw editor is shown, with no Form tab, when:

- the config path does not end in `.toml`;
- the card holds a different copy of the file and the conflict is not resolved yet. The conflict
  warning appears; choose to keep the car's copy or use the card's first, see
  [Config, logs and restart](config-logs-restart.md).

The Raw editor also opens, and says why, when the form cannot be built:

- the file is not valid TOML (the parse error is shown);
- the file is over 64 KiB, is not valid UTF-8, or cannot be read.

The form also refuses a change that would make the file larger than 64 KiB, because the editor could
not open it again.

If the config file does not exist yet, the form starts from the package's example. **Save and restart**
is available straight away, so an example that is already right can be saved as it is.

Raw always works. While you are editing, the other tab keeps what you typed, but a save uses only the tab
you are on. After a save, both tabs show the saved file, so unsaved edits in the other tab are dropped.
