# -*- coding: utf-8 -*-
"""精确验证：导入完成后新图是否真的出现在可见网格（按 .name 元素匹配）。"""
import sys
import time

from playwright.sync_api import sync_playwright

CDP = "http://127.0.0.1:9223"
BACKEND = "http://127.0.0.1:9178"

with sync_playwright() as p:
    browser = p.chromium.connect_over_cdp(CDP)
    ctx = browser.contexts[0]
    page = ctx.pages[0] if ctx.pages else ctx.new_page()
    page.goto(BACKEND + "/imports", wait_until="networkidle")
    page.wait_for_timeout(2000)
    info = page.evaluate(
        """() => {
            const names = [...document.querySelectorAll('.dir-images .image-card .name')].map(e => e.textContent.trim());
            const counts = [...document.querySelectorAll('.dir-header .dir-count')].map(e => e.textContent.trim());
            return { names, counts, total: names.length };
        }"""
    )
    found = [n for n in info["names"] if "moerepro" in n]
    print("组计数:", info["counts"])
    print("可见卡片名（含 moerepro 的）:", found)
    print("总可见卡片:", info["total"])
    browser.close()
