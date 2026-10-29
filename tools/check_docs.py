#!/usr/bin/env python3
"""Checks the public documentation against the repository.

Run from the repository root: python3 tools/check_docs.py
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PUBLIC = [ROOT / "README.md", ROOT / "INSTALL.md", *sorted((ROOT / "docs").rglob("*.md"))]
PUBLIC = [p for p in PUBLIC if "internal" not in p.parts]
BRANDS = re.compile(
    r"\b(?:Audi|Volkswagen|VW|Porsche|Škoda|Skoda|Bentley|Lamborghini|Bugatti|SEAT)\b",
    re.IGNORECASE,
)
failures = []


def fail(message):
    failures.append(message)


def text(path):
    return path.read_text(encoding="utf-8")


def check_version():
    cargo = text(ROOT / "Cargo.toml")
    m = re.search(r'^version\s*=\s*"([^"]+)"', cargo, re.MULTILINE)
    if not m:
        fail("Cargo.toml: version is missing")
        return
    version = m.group(1)
    for page in [ROOT / "README.md", ROOT / "docs/index.md"]:
        if version not in text(page):
            fail(f"{page.relative_to(ROOT)}: does not mention the current version {version}")


def check_brands():
    for page in PUBLIC:
        for m in BRANDS.finditer(text(page)):
            line = text(page).count("\n", 0, m.start()) + 1
            fail(f"{page.relative_to(ROOT)}:{line}: brand name '{m.group(0)}' is not allowed in public docs")


def check_images():
    image = re.compile(r"!\[[^\]]*\]\(([^)]+)\)")
    for page in PUBLIC:
        for m in image.finditer(text(page)):
            target = m.group(1).strip().split()[0]
            if target.startswith(("http://", "https://", "/")):
                continue
            path = (page.parent / target).resolve()
            if not path.is_file():
                fail(f"{page.relative_to(ROOT)}: missing image {target}")


def check_links():
    link = re.compile(r"(?<!!)\[[^\]]*\]\(([^)]+)\)")
    for page in PUBLIC:
        for m in link.finditer(text(page)):
            target = m.group(1).strip().split()[0]
            if target.startswith(("http://", "https://", "#", "mailto:")):
                continue
            path = (page.parent / target).resolve()
            if not path.exists():
                fail(f"{page.relative_to(ROOT)}: missing link target {target}")
            elif "internal" in path.parts and "contributing" not in page.parts:
                fail(f"{page.relative_to(ROOT)}: public page links to internal docs ({target})")


def main():
    check_version()
    check_brands()
    check_images()
    check_links()
    if failures:
        print("documentation check failed:")
        for failure in failures:
            print("  - " + failure)
        return 1
    print(f"documentation check passed ({len(PUBLIC)} public pages)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
