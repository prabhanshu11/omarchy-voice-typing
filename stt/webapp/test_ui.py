# /// script
# dependencies = ["playwright==1.57.0", "pillow"]
# requires-python = ">=3.11"
# ///
"""Headless check of the voice-labels page (never touches the user's browser).

  uv run stt/webapp/test_ui.py http://127.0.0.1:18771 OUTDIR
Point it at a test instance started with a throwaway STT_HOME. Screenshots get
PNG tEXt provenance (source, pipeline, created).
"""
import sys
from datetime import datetime
from pathlib import Path

from PIL import Image, PngImagePlugin
from playwright.sync_api import sync_playwright

url, out = sys.argv[1], Path(sys.argv[2])
out.mkdir(parents=True, exist_ok=True)


def shot(page, name, **kw):
    p = out / f"{name}.png"
    page.screenshot(path=str(p), **kw)
    meta = PngImagePlugin.PngInfo()
    meta.add_text("source", f"{url} (voice-labels web app, headless Chromium)")
    meta.add_text("pipeline", "omarchy-voice-typing/stt/webapp/test_ui.py")
    meta.add_text("created", datetime.now().isoformat(timespec="seconds"))
    Image.open(p).save(p, pnginfo=meta)
    print("saved", p)


with sync_playwright() as pw:
    b = pw.chromium.launch()
    page = b.new_page(viewport={"width": 1280, "height": 900}, has_touch=True)
    errors = []
    page.on("pageerror", lambda e: errors.append(str(e)))
    page.goto(url)
    page.wait_for_selector(".card .w")
    shot(page, "01-list")
    card = page.locator(".card").nth(1)
    card.locator(".w").nth(1).tap()
    page.wait_for_selector("#sheet.open")
    page.locator("#growR").tap()
    page.fill("#fix", "really am")
    page.check("#addVocab")
    shot(page, "02-sheet")
    page.locator("#save").tap()
    page.wait_for_timeout(600)
    shot(page, "03-saved")
    page.locator("#tabVocab").tap()
    page.wait_for_selector(".vchip")
    shot(page, "04-words")
    small = b.new_page(viewport={"width": 420, "height": 860}, has_touch=True, is_mobile=True)
    small.goto(url)
    small.wait_for_selector(".card .w")
    shot(small, "05-narrow")
    b.close()
    print("page errors:", errors or "none")
