#!/usr/bin/env python3
"""Draw the "Get Warden on your phone" QR code as a static SVG for the sign-in window.

The app ships this file; it does not make QR codes at run time and needs no library or network
for it. The URL is read from GET_WARDEN_URL in src-tauri/src/commands.rs, so the QR code and the
"getwarden.app" link always go to the same place.

Regenerate (from desktop/, needs `brew install qrencode`):

    python3 scripts/make-qr.py

Look: rounded modules, rounded finder "eyes", ink on white (the white tile is drawn by the CSS
in both themes, so the code stays scannable in dark mode), the mochi in the middle. Error
correction is H (30% of the code can be lost), and the mochi covers well under 20% of it.
"""

import base64
import re
import subprocess
from pathlib import Path

DESKTOP = Path(__file__).resolve().parent.parent
COMMANDS_RS = DESKTOP / "src-tauri/src/commands.rs"
MOCHI = DESKTOP / "ui/public/mochi/idle.webp"
# The app's copy, and the approved mockup's copy (mockups/desktop/index.html).
OUTS = [DESKTOP / "ui/public/get-warden-qr.svg", DESKTOP.parent / "mockups/shared/get-warden-qr.svg"]

INK = "#1D1A1F"  # --ink of the light theme (mockups/shared/tokens.css)
LOGO_MODULES = 9  # side of the square cleared for the mochi: 81 of 841 modules = 9.6%
# The mochi fills only the middle of the 512 px picture; this square (x, y, side) holds it all.
MOCHI_BOX = (100, 92, 312)


def get_warden_url() -> str:
    match = re.search(r'const GET_WARDEN_URL: &str = "([^"]+)";', COMMANDS_RS.read_text())
    if not match:
        raise SystemExit(f"GET_WARDEN_URL not found in {COMMANDS_RS}")
    return match.group(1)


def qr_matrix(text: str) -> list[list[bool]]:
    """Rows of modules, True = dark. qrencode's ASCII output draws each module as two chars."""
    ascii_art = subprocess.run(
        ["qrencode", "-t", "ASCII", "-l", "H", "-m", "0", text],
        check=True, capture_output=True, text=True,
    ).stdout
    return [[line[x] == "#" for x in range(0, len(line), 2)] for line in ascii_art.splitlines() if line]


def in_finder(x: int, y: int, size: int) -> bool:
    """The three 7x7 corner squares; they are drawn as rounded eyes instead of modules."""
    return ((x < 7 or x >= size - 7) and y < 7) or (x < 7 and y >= size - 7)


def in_logo(x: int, y: int, size: int) -> bool:
    start = (size - LOGO_MODULES) // 2
    return start <= x < start + LOGO_MODULES and start <= y < start + LOGO_MODULES


def eye(x: int, y: int) -> str:
    # Outer ring: a 7x7 rounded square drawn as a 1-module stroke; centre: a 3x3 rounded square.
    return (
        f'<rect x="{x + 0.5}" y="{y + 0.5}" width="6" height="6" rx="2" fill="none" stroke="{INK}"/>'
        f'<rect x="{x + 2}" y="{y + 2}" width="3" height="3" rx="1"/>'
    )


def svg(matrix: list[list[bool]]) -> str:
    size = len(matrix)
    modules = [
        f'<rect x="{x + 0.06}" y="{y + 0.06}" width=".88" height=".88" rx=".34"/>'
        for y, row in enumerate(matrix)
        for x, dark in enumerate(row)
        if dark and not in_finder(x, y, size) and not in_logo(x, y, size)
    ]
    eyes = eye(0, 0) + eye(size - 7, 0) + eye(0, size - 7)
    logo_side = LOGO_MODULES - 1
    logo_at = (size - logo_side) / 2
    box_x, box_y, box_side = MOCHI_BOX
    mochi = base64.b64encode(MOCHI.read_bytes()).decode()
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size} {size}" fill="{INK}">'
        f"<!-- Made by scripts/make-qr.py. Do not edit by hand. -->"
        + "".join(modules)
        + eyes
        + f'<svg x="{logo_at}" y="{logo_at}" width="{logo_side}" height="{logo_side}" '
        f'viewBox="{box_x} {box_y} {box_side} {box_side}">'
        f'<image width="512" height="512" href="data:image/webp;base64,{mochi}"/></svg>'
        "</svg>\n"
    )


def main() -> None:
    url = get_warden_url()
    drawing = svg(qr_matrix(url))
    for out in OUTS:
        out.write_text(drawing)
        print(f"wrote {out} for {url}")


if __name__ == "__main__":
    main()
