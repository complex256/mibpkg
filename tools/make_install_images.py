#!/usr/bin/env python3
"""Generate the annotated SD-format step illustrations used by the install guide."""
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "docs" / "install-images"
BG = "#f4f5f7"
CARD = "#ffffff"
INK = "#1d2433"
MUTED = "#6b7280"
ACCENT = "#2563eb"
ACCENT_SOFT = "#dbeafe"
WARN = "#b45309"


def svg(body: str, width=820, height=500) -> str:
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" role="img">
  <rect width="{width}" height="{height}" fill="{BG}"/>
  {body}
</svg>
'''


def window(x, y, w, h, title):
    return f'''
  <rect x="{x}" y="{y}" width="{w}" height="{h}" rx="10" fill="{CARD}" stroke="#d1d5db"/>
  <rect x="{x}" y="{y}" width="{w}" height="38" rx="10" fill="#e5e7eb"/>
  <rect x="{x}" y="{y+28}" width="{w}" height="10" fill="#e5e7eb"/>
  <circle cx="{x+18}" cy="{y+19}" r="5" fill="#ef4444"/>
  <circle cx="{x+34}" cy="{y+19}" r="5" fill="#f59e0b"/>
  <circle cx="{x+50}" cy="{y+19}" r="5" fill="#10b981"/>
  <text x="{x+w/2}" y="{y+24}" text-anchor="middle" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="14" font-weight="600" fill="{INK}">{title}</text>
'''


def callout(x, y, n, text, w=300):
    return f'''
  <circle cx="{x}" cy="{y}" r="16" fill="{ACCENT}"/>
  <text x="{x}" y="{y+5}" text-anchor="middle" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="15" font-weight="700" fill="#ffffff">{n}</text>
  <rect x="{x+24}" y="{y-16}" width="{w}" height="32" rx="16" fill="{ACCENT_SOFT}"/>
  <text x="{x+38}" y="{y+5}" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="14" fill="{INK}">{text}</text>
'''


def row(x, y, w, label, selected=False, muted=False):
    fill = ACCENT_SOFT if selected else "transparent"
    color = INK if not muted else MUTED
    return f'''
  <rect x="{x}" y="{y}" width="{w}" height="30" rx="6" fill="{fill}"/>
  <text x="{x+12}" y="{y+20}" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="14" fill="{color}">{label}</text>
'''


def field(x, y, w, label, value, highlight=False):
    border = ACCENT if highlight else "#cbd5e1"
    return f'''
  <text x="{x}" y="{y-6}" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="12" fill="{MUTED}">{label}</text>
  <rect x="{x}" y="{y}" width="{w}" height="34" rx="6" fill="#ffffff" stroke="{border}" stroke-width="{2 if highlight else 1}"/>
  <text x="{x+10}" y="{y+22}" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="14" fill="{INK}">{value}</text>
'''


def macos_view():
    b = window(90, 50, 640, 400, "Disk Utility")
    b += f'''
  <rect x="90" y="88" width="150" height="362" fill="#f8fafc"/>
  <text x="108" y="116" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="12" fill="{MUTED}">DEVICES</text>
  {row(102, 126, 126, "Macintosh HD", muted=True)}
  {row(102, 160, 126, "SD Card", muted=True)}
  <rect x="240" y="88" width="490" height="362" fill="#ffffff"/>
  <text x="270" y="150" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="22" font-weight="600" fill="{INK}">Disk Utility</text>
  <text x="270" y="180" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="14" fill="{MUTED}">Turn on View → Show All Devices so the card itself appears.</text>
  <rect x="270" y="205" width="430" height="42" rx="8" fill="#f8fafc" stroke="#cbd5e1"/>
  <text x="286" y="231" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="14" fill="{INK}">View</text>
  <text x="330" y="231" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="14" fill="{MUTED}">Show All Devices</text>
  <rect x="320" y="208" width="160" height="36" rx="6" fill="{ACCENT_SOFT}" stroke="{ACCENT}"/>
  <text x="330" y="231" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="14" fill="{INK}">Show All Devices</text>
  {callout(510, 250, "1", "Turn on Show All Devices", 260)}
  <text x="270" y="310" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="13" fill="{MUTED}">The card then shows as a top-level device, not only as a volume.</text>
'''
    return svg(b)


def macos_select():
    b = window(90, 50, 640, 400, "Disk Utility")
    b += f'''
  <rect x="90" y="88" width="190" height="362" fill="#f8fafc"/>
  <text x="108" y="116" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="12" fill="{MUTED}">DEVICES</text>
  {row(102, 126, 160, "Macintosh HD", muted=True)}
  {row(102, 160, 160, "SD Card (device)", selected=True)}
  <text x="118" y="196" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="12" fill="{MUTED}">↳ SD Card volume</text>
  <rect x="280" y="88" width="450" height="362" fill="#ffffff"/>
  <text x="310" y="150" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="22" font-weight="600" fill="{INK}">SD Card</text>
  <text x="310" y="180" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="14" fill="{MUTED}">Select the whole device, not the volume below it.</text>
  <rect x="310" y="215" width="380" height="70" rx="8" fill="#f8fafc" stroke="#cbd5e1"/>
  <text x="326" y="243" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="14" fill="{INK}">Device: SD Card</text>
  <text x="326" y="266" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="13" fill="{MUTED}">Capacity 32 GB · scheme Master Boot Record</text>
  {callout(500, 320, "2", "Select the top-level device", 270)}
'''
    return svg(b)


def macos_erase():
    b = window(120, 40, 580, 420, "Disk Utility")
    b += f'''
  <rect x="120" y="78" width="580" height="382" fill="#ffffff"/>
  <text x="150" y="120" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="18" font-weight="600" fill="{INK}">Erase “SD Card”</text>
  {field(150, 155, 500, "Name", "SD Card")}
  {field(150, 220, 500, "Format", "MS-DOS (FAT)")}
  {field(150, 285, 500, "Scheme", "Master Boot Record", True)}
  <rect x="150" y="345" width="130" height="36" rx="7" fill="{ACCENT}"/>
  <text x="215" y="368" text-anchor="middle" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="14" font-weight="600" fill="#ffffff">Erase</text>
  <rect x="295" y="345" width="120" height="36" rx="7" fill="#e5e7eb"/>
  <text x="355" y="368" text-anchor="middle" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="14" fill="{INK}">Cancel</text>
  {callout(140, 410, "3", "FAT + Master Boot Record, then Erase", 320)}
'''
    return svg(b)


def windows_select():
    b = window(80, 40, 660, 420, "SD Card Formatter")
    b += f'''
  <rect x="80" y="78" width="660" height="382" fill="#ffffff"/>
  <text x="110" y="120" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="18" font-weight="600" fill="{INK}">Select card</text>
  <text x="110" y="148" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="13" fill="{MUTED}">Choose the SD card, not a hard drive.</text>
  <rect x="110" y="170" width="590" height="150" rx="8" fill="#f8fafc" stroke="#cbd5e1"/>
  <text x="130" y="198" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="12" fill="{MUTED}">DRIVE</text>
  <text x="130" y="226" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="15" fill="{INK}">E:  SD Card  32 GB</text>
  <text x="130" y="252" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="13" fill="{MUTED}">Capacity: 32 GB · Removable: yes</text>
  <rect x="130" y="270" width="240" height="32" rx="6" fill="{ACCENT_SOFT}" stroke="{ACCENT}"/>
  <text x="142" y="292" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="14" fill="{INK}">Selected: E: SD Card</text>
  {callout(500, 230, "1", "Select the SD card", 220)}
  <rect x="110" y="355" width="180" height="38" rx="7" fill="{ACCENT}"/>
  <text x="200" y="380" text-anchor="middle" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="14" font-weight="600" fill="#ffffff">Format</text>
'''
    return svg(b)


def windows_format():
    b = window(80, 40, 660, 430, "Format")
    b += f'''
  <rect x="80" y="78" width="660" height="392" fill="#ffffff"/>
  <text x="110" y="120" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="18" font-weight="600" fill="{INK}">Format options</text>
  {field(110, 155, 250, "File system", "FAT32")}
  {field(390, 155, 250, "Allocation unit size", "Default")}
  {field(110, 230, 250, "Volume label", "SD Card")}
  <text x="110" y="305" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="14" fill="{INK}">Partition scheme</text>
  <rect x="110" y="318" width="250" height="36" rx="6" fill="#ffffff" stroke="{ACCENT}" stroke-width="2"/>
  <text x="122" y="342" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="14" fill="{INK}">Master Boot Record (MBR)</text>
  <text x="390" y="342" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="13" fill="{MUTED}">Do not choose GPT/GUID.</text>
  <text x="110" y="390" font-family="system-ui,-apple-system,Segoe UI,sans-serif" font-size="13" fill="{WARN}">A GUID card makes the SD-card option in the update menu greyed out.</text>
  {callout(390, 235, "2", "FAT32 on MBR, then Start", 260)}
'''
    return svg(b)


files = {
    "sd-macos-1-view.svg": macos_view(),
    "sd-macos-2-select.svg": macos_select(),
    "sd-macos-3-erase.svg": macos_erase(),
    "sd-windows-1-select.svg": windows_select(),
    "sd-windows-2-format.svg": windows_format(),
}
OUT.mkdir(parents=True, exist_ok=True)
for name, content in files.items():
    (OUT / name).write_text(content, encoding="utf-8")
    print(OUT / name)
