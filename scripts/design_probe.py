"""Probe docs/design.html structure: list turns, options, frame sizes."""

import json
import sys

from playwright.sync_api import sync_playwright

URL = "file:///Users/badrusshoolehk/Documents/riset/database-management/docs/design.html"

JS = """
() => {
  const out = [];
  for (const sec of document.querySelectorAll('section.dv-turn')) {
    const turn = {
      id: sec.id,
      name: sec.querySelector('.dv-tname')?.textContent?.trim() ?? '',
      options: [],
    };
    for (const opt of sec.querySelectorAll('.dv-opts > *')) {
      const oid = opt.querySelector('.dv-oid')?.textContent?.trim()
        ?? opt.id ?? '';
      const label = opt.querySelector('.dv-olabel,.dv-oname')?.textContent?.trim() ?? '';
      // find the largest fixed-size child = the screen frame
      let frame = null;
      for (const el of opt.querySelectorAll('*')) {
        const r = el.getBoundingClientRect();
        if (r.width >= 600 && r.height >= 400) {
          if (!frame || r.width * r.height > frame.w * frame.h) {
            frame = { w: Math.round(r.width), h: Math.round(r.height),
                      cls: el.className.toString().slice(0, 60),
                      tag: el.tagName };
          }
        }
      }
      turn.options.push({ oid, label, cls: opt.className.toString().slice(0,60), frame });
    }
    // also note trailing dv-next commentary
    turn.next = sec.querySelector('.dv-next')?.textContent?.trim() ?? '';
    out.push(turn);
  }
  return out;
}
"""

with sync_playwright() as p:
    b = p.chromium.launch()
    pg = b.new_page(viewport={"width": 1800, "height": 1200})
    pg.goto(URL)
    pg.wait_for_timeout(4000)
    data = pg.evaluate(JS)
    print(json.dumps(data, indent=1, ensure_ascii=False))
    b.close()
