# -*- coding: utf-8 -*-
"""侦察主目录页面 DOM 结构。"""
from playwright.sync_api import sync_playwright

BACKEND = "http://127.0.0.1:9178"

with sync_playwright() as p:
    browser = p.chromium.connect_over_cdp("http://127.0.0.1:9223")
    ctx = browser.contexts[0]
    page = ctx.pages[0] if ctx.pages else ctx.new_page()
    page.goto(BACKEND + "/imports", wait_until="networkidle")
    page.wait_for_timeout(2500)
    info = page.evaluate(
        """() => ({
            url: location.pathname,
            dirImages: document.querySelectorAll('.dir-images').length,
            dayGroups: document.querySelectorAll('.day-group').length,
            cards: document.querySelectorAll('.image-card').length,
            cells: document.querySelectorAll('.dir-cell').length,
            empty: !!document.querySelector('.el-empty'),
            sample: document.querySelector('.imports-page')?.innerHTML.slice(0, 600) ?? null,
        })"""
    )
    import json
    print(json.dumps(info, ensure_ascii=False, indent=1)[:1200])
    browser.close()
