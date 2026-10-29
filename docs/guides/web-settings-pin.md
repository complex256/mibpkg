# Web settings and PIN

Open **More → Web settings**.

## Listen addresses

By default the web interface answers on:

- **hotspot** — the car's Wi-Fi access point;
- **tailnet** — Tailscale.

It does not answer on the SIM data link. You can enable or disable each class.

## Port

The default is **80**. If you change it, the page shows the new URL after the web server restarts. Port
`8080` is used by a stock process and is not a good choice.

## Hotspot interface

mibpkg recognises the hotspot interface by name (`uap*`). If your unit names it differently, choose the
interface in **Web settings**. A wrong interface can make the web page unreachable from the hotspot.

## PIN

A PIN is **4 to 12 digits**. Set it under **Web settings**. It is required after every web-server
restart, including a self-update.

Forgot the PIN? **GEM → mibpkg → Remove PIN**.

Locked out by a wrong listen/port setting? **GEM → mibpkg → Reset web settings**. This deletes
`web.toml`, restores defaults and restarts the web server.

## Firewall

The web server asks mibpkg to add a pass rule for its port on the hotspot interface. The rule is written
to `pf.conf` on flash and takes effect on the next boot. mibpkg never reloads `pf.conf` at runtime: doing
so resets the stock anchors and can cut off the hotspot's internet.

If the page is unreachable after changing the port, reboot once so the firewall rule is active.

## Confirmation

Saving settings that would make the current connection unable to reach the page returns a
**needs_confirm** warning. Confirm the save only if you have another way in (the car's screen or
Tailscale).

## Related

- [Web interface](web-interface.md)
- [Recovery and DISABLE](recovery-disable.md)
- [Web API](../reference/web-api.md)
