# Install

This is the engineering-menu method. It needs no other software on the unit and is the recommended
path.

## 1. Format the SD card

The unit reads cards partitioned with the **Master Boot Record (MBR)** scheme. A card formatted with
GUID/GPT makes the SD-card option in the update menu greyed out.

=== "macOS"

    1. Open **Disk Utility** and turn on **View → Show All Devices**. The card then appears as a
       top-level device, not only as a volume.

        ![Disk Utility: View → Show All Devices](../install-images/sd-macos-1-view.svg)

    2. Select the **whole SD card device** (the top-level entry), not the volume under it.

        ![Disk Utility: select the SD card device](../install-images/sd-macos-2-select.svg)

    3. Click **Erase**, choose **MS-DOS (FAT)** and **Master Boot Record**, then click **Erase**.

        ![Disk Utility: Erase with MS-DOS (FAT) and Master Boot Record](../install-images/sd-macos-3-erase.svg)

=== "Windows"

    1. Open **SD Memory Card Formatter**, select the SD card (not a hard drive) and click **Format**.

        ![SD Card Formatter: select the card](../install-images/sd-windows-1-select.svg)

    2. Choose **FAT32** and **Master Boot Record (MBR)**, then start.

        ![Format options: FAT32 on MBR](../install-images/sd-windows-2-format.svg)

    If you use `diskpart` instead, run `list disk`, `select disk <n>`, `clean`, `create partition
    primary`, `format fs=fat32 quick`, and make sure the disk is MBR. Do not choose GPT/GUID.

## 2. Copy the image

Unzip `mibpkg-sd-<version>.zip` and copy its **contents** to the **root** of the card, so
`metainfo2.txt` sits at the top level next to `mibpkg/` and `mod/`.

```text
/
├── metainfo2.txt
├── _Swdlautorun.txt
├── mibpkg/
└── mod/
```

Eject the card safely.


## 3. Insert the card in SD1

Put it in the unit's **SD card 1** slot.


## 4. Open the engineering menu

Open the engineering (Red) menu and choose **Software Update**. Select **SD card 1**.

![SW Update media selection](../install-images/04-sw-update.jpg)

## 5. Start the update

Choose **Standard**. Scroll to the bottom of the list: **`mibpkg`** is marked **`[Y]`**. The stock
rows show `[N/A]`; that is normal. Select **Start update**.

!!! note "Building your own card"
    The engineering menu accepts only a card with a genuine signed `metainfo2` header. The release
    image includes one. If you build a card with `scripts/bundle.sh`, pass
    `--signed-header <file>`; without it, use the custom-script method.

![Update type: Standard](../install-images/05a-update-type.jpg)

![mibpkg listed as installable](../install-images/05-mibpkg-listed.jpg)

## 6. Wait for the reboots

The install runs and the unit restarts a few times. Do not pull the card or cut power.

![Install running](../install-images/06-installing.jpg)

## 7. Open mibpkg

The main interface is the web page served by the car. Join the unit's Wi-Fi hotspot and open
**GEM → mibpkg** for the address. See [First use](first-use.md).

The in-car **GEM → mibpkg** screen is the backup interface: it shows the address and has recovery
buttons for a forgotten PIN, broken web settings, and removing mibpkg.

![GEM Version information](../install-images/08-version-info.jpg)

## If the SD-card option is greyed out

The card is not MBR/FAT32. Reformat it and try again.

## Next

- [First use](first-use.md)
- [Custom-script install](install-custom-script.md)
