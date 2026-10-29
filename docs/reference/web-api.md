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
| `GET` | `/api/packages/<name>/config/<n>` | Read config `n` |
| `PUT` | `/api/packages/<name>/config/<n>` | Write config `n` (UTF-8 text body) |
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

## Uploads

| Method | Path | Meaning |
|---|---|---|
| `POST` | `/api/uploads` | Stream a `.mpk`; returns an upload id |
| `POST` | `/api/uploads/<id>/install` | `{collection, approve[]}` |
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
