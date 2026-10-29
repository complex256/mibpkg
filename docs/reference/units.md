# Units

## Supported

| | |
|---|---|
| Model | **MIB2 High**, `MHI2` or `MHI2Q` |
| OS | QNX 6.5 |
| Architecture | ARMv7 |
| Shell | `/bin/ksh` as `/bin/sh` |

The unit model is read from the unit, not from the package. `mibpkg status` and the web health page show
it.

## Compatibility

A package can restrict itself:

```toml
[compat]
units = ["MHI2Q"]
firmware = ["HMI_AU_MIB2_HQC_NAR_G22_C10_180803"]
```

- `units` matches the model string (`MHI2`, `MHI2Q`).
- `firmware` matches the HMI build string.
- Omit a list to allow everything.

mibpkg refuses an incompatible package with a message naming the unit. `mibpkg verify --unit MHI2Q`
checks a collection against a unit before cars see it.

## Not supported

- Other MIB2 variants.
- MIB2 Standard, MIB3 and other platforms.
- Other head-unit families.

The installer and boot hook are specific to the MIB2 High.

## Related

- [manifest.toml](manifest.md)
- [Requirements](../getting-started/requirements.md)
- [Collection index](collection-index.md)
