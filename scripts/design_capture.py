"""Capture each design option frame from docs/design.html into design/*.png."""

import pathlib

from playwright.sync_api import sync_playwright

ROOT = pathlib.Path(__file__).resolve().parent.parent
URL = (ROOT / "docs/design.html").as_uri()
OUT = ROOT / "design"
OUT.mkdir(exist_ok=True)

# oid -> output filename (role-based, primary refs first)
NAMES = {
    "4a": "1-connections.png",
    "4b": "2-workspace.png",
    "4c": "3-sql-editor.png",
    "3c": "4-modal-open-database.png",
    "3d": "5-modal-open-connection.png",
    "3b": "6-function-view.png",
    "3a": "7-sql-editor-t3.png",
    "2a": "8-welcome-t2.png",
    "2b": "9-workspace-empty.png",
    "2c": "10-grid-t2.png",
    "2d": "11-filter-edit-t2.png",
    "1a": "12-paper-t1.png",
}

with sync_playwright() as p:
    b = p.chromium.launch()
    pg = b.new_page(
        viewport={"width": 1900, "height": 1400}, device_scale_factor=2
    )
    pg.goto(URL)
    pg.wait_for_timeout(4000)
    for opt in pg.query_selector_all(".dv-opt"):
        oid_el = opt.query_selector(".dv-oid")
        oid = oid_el.text_content().strip() if oid_el else "?"
        name = NAMES.get(oid)
        if not name:
            continue
        card = opt.query_selector(".dv-card")
        if not card:
            continue
        card.scroll_into_view_if_needed()
        pg.wait_for_timeout(300)
        card.screenshot(path=str(OUT / name))
        box = card.bounding_box()
        print(f"{oid} -> {name}  {box['width']:.0f}x{box['height']:.0f}")
    b.close()
