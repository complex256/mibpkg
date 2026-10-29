# Custom-script install

If the unit already runs M.I.B., you can install mibpkg with its script runner instead of the
engineering-menu update. The card image is the same; one card works for both methods.

## Steps

1. Unzip `mibpkg-sd-<version>.zip` onto the M.I.B. SD card.
2. Insert the card.
3. Open **GEM → M.I.B. → Advanced Settings → Run Custom Script**.
4. Wait for the script to finish.
5. Open **GEM → mibpkg**, or the web interface.

The script installs the payload, starts the supervisor and adds the GEM entry. It does not need the
signed engineering-menu header.

## Which method should I use?

| | Engineering menu | Custom script |
|---|---|---|
| Needs M.I.B. | No | Yes |
| Needs a signed header | Yes, for the update menu to accept the entry | No |
| Installs the same payload | Yes | Yes |
| Recovery | GEM → mibpkg | GEM → mibpkg |

The engineering-menu method is the normal path for a new unit. Use the custom-script method if M.I.B.
is already set up and you prefer one tap.

## Next

- [First use](first-use.md)
- [Install methods](../explanation/install-methods.md)
