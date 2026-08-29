# -*- coding: utf-8 -*-
"""回收站页面诊断：截图 + DOM 检查。"""
import sys

from playwright.sync_api import sync_playwright

CDP = "http://127.0.0.1:9223"
BACKEND = "http://127.0.0.1:9178"

with sync_playwright() as p:
    browser = p.chromium.connect_over_cdp(CDP)
    ctx = browser.contexts[0]
    page = ctx.pages[0] if ctx.pages else ctx.new_page()
    errs = []
    page.on("console", lambda m: errs.append(m.text) if m.type == "error" else None)
    page.goto(BACKEND + "/trash", wait_until="networkidle")
    page.wait_for_timeout(2500)
    info = page.evaluate(
        """() => ({
            rows: document.querySelectorAll('.el-table__row').length,
            imgs: [...document.querySelectorAll('.el-table__row .el-image')].map(e => ({
                src: e.querySelector('img')?.src ?? null,
                broken: e.querySelector('img')?.complete === false,
                errText: e.textContent.trim().slice(0, 30),
            })),
            tableHtml: document.querySelector('.el-table')?.innerHTML.slice(0, 500) ?? null,
        })"""
    )
    import json
    print(json.dumps(info, ensure_ascii=False, indent=1)[:1500])
    page.screenshot(path=r"D:\Code\Reasonix_Projects\image\scripts\trash_check.png", full_page=True)
    print("console errors:", errs[:5])
    browser.close()
