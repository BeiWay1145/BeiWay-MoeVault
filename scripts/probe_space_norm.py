# -*- coding: utf-8 -*-
"""确认前端空格输入归一化：搜索框输入 'looking at viewer,' → chip + 筛选命中。"""
from playwright.sync_api import sync_playwright

CDP = "http://127.0.0.1:9223"
BACKEND = "http://127.0.0.1:9178"

with sync_playwright() as p:
    browser = p.chromium.connect_over_cdp(CDP)
    ctx = browser.contexts[0]
    page = ctx.pages[0] if ctx.pages else ctx.new_page()
    page.goto(BACKEND + "/library", wait_until="networkidle")
    page.wait_for_timeout(1500)
    page.click(".sf-input")
    page.keyboard.type("looking at viewer,")
    page.wait_for_timeout(1500)
    chips = page.evaluate("[...document.querySelectorAll('.sf-chip')].map(c => c.textContent.trim())")
    cards = page.evaluate("document.querySelectorAll('.image-card').length")
    total = page.evaluate("fetch('http://127.0.0.1:9178/api/v1/images?limit=1').then(r=>r.json()).then(d=>d.total)")
    print("chips:", chips, "cards:", cards, "filtered_total:", total)
    ok = len(chips) == 1 and "looking at viewer" in chips[0] and total >= 1
    print("PASS" if ok else "FAIL", "| 空格输入归一化筛选命中")
    browser.close()
