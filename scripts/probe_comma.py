# -*- coding: utf-8 -*-
"""逗号提交排查：输入 1girl, → 是否出 chip + 筛选生效。"""
from playwright.sync_api import sync_playwright

CDP = "http://127.0.0.1:9223"
BACKEND = "http://127.0.0.1:9178"

with sync_playwright() as p:
    browser = p.chromium.connect_over_cdp(CDP)
    ctx = browser.contexts[0]
    page = ctx.pages[0] if ctx.pages else ctx.new_page()
    errs = []
    page.on("console", lambda m: errs.append(m.text) if m.type in ("error", "warning") else None)
    page.goto(BACKEND + "/library", wait_until="networkidle")
    page.wait_for_timeout(1500)

    page.click(".sf-input")
    page.keyboard.type("1girl,")
    page.wait_for_timeout(1200)

    chips = page.evaluate("[...document.querySelectorAll('.sf-chip')].map(c => c.textContent.trim())")
    kw = page.evaluate("document.querySelector('.sf-input')?.value || ''")
    total = page.evaluate("fetch('http://127.0.0.1:9178/api/v1/images?limit=1').then(r=>r.json()).then(d=>d.total)")
    print("chips:", chips, "kw:", repr(kw), "total:", total)
    print("errors:", errs[:3])
    browser.close()
