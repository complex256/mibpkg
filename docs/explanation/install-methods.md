# Install methods

One card image installs through two paths.

## Engineering menu (SWDL)

The update menu reads `metainfo2.txt` at the card root. It runs the installer payload, which:

1. copies mibpkg to `/mnt/app/root/mibpkg`;
2. runs `mibpkg bootstrap`;
3. adds the boot hook;
4. enables the GEM entry;
5. applies the GEM patch.

The menu requires a genuine, manufacturer-signed `metainfo2` header. mibpkg cannot ship one; the release
image includes the rest. A card without the header still works through the custom-script method.

## Custom script

The card also carries a `mod/` script. M.I.B.'s **Run Custom Script** runs it. The script installs the same
payload and calls the same `mibpkg bootstrap`; it does not need the signed header.

## One card

The release image carries both paths:

```text
/
├── metainfo2.txt
├── _Swdlautorun.txt
├── mibpkg/
└── mod/
```

`mibpkg swdl-card` assembles it. `--no-mib` emits a pure SWDL card.

## Which path is used

The unit picks the path you start:

- **Engineering menu → Update** uses SWDL;
- **M.I.B. → Run Custom Script** uses the script.

They do not run together.

## Uninstall

Both paths install the same files, so either recovery path works:

- **GEM → mibpkg → Remove mibpkg**;
- `mibpkg uninstall-self`;
- reinstall from the card.

Uninstall restores the stock files mibpkg backed up.

## Related

- [Install](../getting-started/install.md)
- [Custom-script install](../getting-started/install-custom-script.md)
- [Safety and recovery](safety-and-recovery.md)
