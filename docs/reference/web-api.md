# Web API

The web interface is a static page over a JSON API on the car. This page documents the API for
contributors and for anyone building another client.

## Session

| Method | Path | Meaning |
|---|---|---|
| `GET` | `/api/session` | `{pin_required, authed}` |
| `POST` | `/api/login` | `{pin}`; sets a session cookie |
| `POST` | `/api/logout` | Clears the session |

A wrong PIN returns 401. Too many wrong PINs returns 429 with `retry_after`.

## Status and packages

| Method | Path | Meaning |
|---|---|---|
| `GET` | `/api/status` | Versions, health, current job |
| `GET` | `/api/packages` | Installed and available packages |
| `GET` | `/api/packages/<name>/config/<n>` | Read config `n`; for a `.toml` config also its `form` |
| `PUT` | `/api/packages/<name>/config/<n>` | Write config `n` (UTF-8 text body) |
| `GET` | `/api/packages/<name>/page` | The store page as `text/markdown`: the installed copy, or with `?collection=<id>&version=<v>` that version from its collection, checked against the index |
| `POST` | `/api/packages/<name>/config/<n>/form` | Change keys of a `.toml` config from the form |
| `GET` | `/api/sources` | Enabled and available sources |
| `POST` | `/api/sources` | Add `{source}` |
| `DELETE` | `/api/sources` | Remove `{source}` |
| `GET` | `/api/settings` | Web settings |
| `PUT` | `/api/settings` | Save web settings |
| `GET` | `/api/wifi` | The Wi-Fi uplink: saved SSID, on/off, live state; no password |
| `PUT` | `/api/wifi` | Write `wifi.toml` (the same file the car page edits) |
| `GET` | `/api/logs` | Known logs |
| `GET` | `/api/logs/<id>?bytes=N` | Tail a log |
| `POST` | `/api/actions` | Start an action job |
| `GET` | `/api/jobs/<id>?since=N` | Job state and output |

## Jobs

Actions run as jobs. `POST /api/actions` returns **202** with `{job}` or **409** with
`{busy, job, error}` when another action is running. Poll the job until `ok` or `error`.

## Config forms

`GET /api/packages/<name>/config/<n>` returns `path`, `required`, `exists`, `content`, `example` and
`card` (the card's copy while it is in conflict, else `null`). For a path ending `.toml` it also returns
`form`; for any other path `form` is `null`.

```json
{
  "mode": "schema",
  "reason": null,
  "fields": [
    {"key": "port", "label": "Port", "help": null, "kind": "int", "source": "schema",
     "value": 1883, "default": 1883, "choices": [], "min": 1, "max": 65535,
     "secret": false, "section": "Connection", "raw": null, "item_kind": null}
  ]
}
```

- `mode` is `schema` (the package ships a schema), `inferred` (no schema; fields are read from the
  file) or `raw` (the form cannot be used; `reason` says why and `fields` is empty).
- `kind` is `string`, `int`, `float`, `bool`, `enum`, `list` or `raw`. A `raw` field is a value the form
  cannot edit (a table array, an inline table, a date); `raw` holds its TOML text.
- `value` is `null` when the key is not set in the file. `item_kind` is the list item type for lists.
- The file text is the config, or its example while the config does not exist yet.

`POST /api/packages/<name>/config/<n>/form` takes `{"edits": [{"key": "port", "value": 1884}]}`.
A `null` value removes the key. An empty `edits` list is valid: if the config file does not exist yet, it saves the example as it is. Keys use dots for tables (`publish.interval`). The server applies the
edits to the file, keeping comments, order and keys it does not know, checks them against the schema
(or, without one, against the type already in the file), and then saves the result as the usual
"Save and restart" job.

| Status | Body | Meaning |
|---|---|---|
| 202 | `{job}` | Saved; poll the job |
| 400 | `{error, fields: [{key, message}]}` | An edit is invalid, or the file is not valid TOML; `fields` names each bad key. Nothing is written |
| 404 | `{error}` | Unknown package or config |
| 400 | `{error}` | The body is not valid JSON or has unknown fields |
| 409 | `{error}` | The card has a conflicting copy (resolve it first), the file is over 64 KiB, is not UTF-8 or cannot be read, or the result would be over 64 KiB ("this change would make the file larger than the editor can safely open") |
| 409 | `{busy, job, error}` | Another job is running |

A config whose path does not end `.toml` is refused with 400.

## Uploads

| Method | Path | Meaning |
|---|---|---|
| `POST` | `/api/uploads` | Stream a `.mpk`; returns an upload id |
| `POST` | `/api/uploads/<id>/install` | `{collection, approve[]}` |
| `GET` | `/api/uploads/<id>/page` | The uploaded package's store page |
| `DELETE` | `/api/uploads/<id>` | Discard |

An upload is inspected before install: name, version, kind, compatibility, services, permissions and
`needs_approval`. Uploads expire after 10 minutes.

## Settings body

```json
{
  "listen": ["hotspot", "tailnet"],
  "port": 80,
  "pin": "1234",
  "clear_pin": false,
  "confirm": false,
  "hotspot_interface": ""
}
```

If the new settings would make the current connection unable to reach the page, the API returns **409**
with `needs_confirm`. Send `confirm: true` to save anyway.

## Wi-Fi body

```json
{
  "enabled": true,
  "ssid": "my-phone",
  "password": "secret",
  "dhcp": true,
  "ip": "",
  "netmask": "",
  "gateway": "",
  "dns": ""
}
```

Omit `password` (or send `null`) to keep the saved one; send `""` to clear it. Omitted address fields
keep their saved values. With `"dhcp": false`, `ip` and `netmask` are required and may not be empty.
The reply has `configured`, `enabled`, `ssid`, `password_set`, `dhcp`, `ip`, `netmask`, `gateway`,
`dns`, `state`, `active_ssid`, `ipv4`, `live_netmask`, `live_gateway`, `live_dns`, `error` and
`connected`. The password is never returned.

## Errors

| Code | Meaning |
|---|---|
| 400 | Bad request or invalid package |
| 401 | Wrong PIN |
| 404 | Unknown package, job, upload or config |
| 409 | Busy, or settings need confirmation |
| 429 | Too many wrong PINs |
| 500 | Server-side failure |

All errors are JSON with an `error` field.

## Related

- [Web interface](../guides/web-interface.md)
- [Web settings and PIN](../guides/web-settings-pin.md)
