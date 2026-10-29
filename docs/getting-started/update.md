# Update mibpkg

## From the web interface

Open **Updates**. When a newer mibpkg is available, its row shows **Update to `<version>`**. Tap it.
The server restarts on the new version; the page reloads and asks for the PIN again if one is set.

The update comes from the official collection, or from a card if the card carries a newer version.

## From a card

1. Copy the new `mibpkg-sd-<version>.zip` contents to the card.
2. Open **More → Rescan** (or **Updates**).
3. Install the newer mibpkg from **Updates**.

## Check for online updates

The supervisor checks online sources periodically when the car has internet. You can also open
**Updates** and refresh. **More → Sources** shows the enabled collections.

## If the update fails

- The old version keeps running until the new one is committed.
- `mibpkg status` and **More → Health** show the installed version and the supervisor's version.
- If the web server does not come back, use **GEM → mibpkg → Reset web settings** or reboot.

See [Safety and recovery](../explanation/safety-and-recovery.md).
