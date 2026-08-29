# -*- coding: utf-8 -*-
"""检查 commitTerm 后 filter.tags 是否设置。"""
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
    page.keyboard.type("1girl,")
    page.wait_for_timeout(1200)
    state = page.evaluate(
        """() => {
            // 从 Pinia store 拿 filter（通过页面上的 Vue 实例不可行，改用行为判断：
            // 当前 grid 显示数量 + chips）
            const chips = [...document.querySelectorAll('.sf-chip')].map(c => c.textContent.trim());
            const cards = document.querySelectorAll('.image-card').length;
            const total = fetch('http://127.0.0.1:9178/api/v1/images?limit=1').then(r=>r.json()).then(d=>d.total);
            return Promise.resolve({ chips, cards });
        }"""
    )
    print("state:", state)
    # 手动再输入一次逗号（第二次触发），看是否生效
    page.click(".sf-input")
    page.keyboard.type("1girl,")
    page.wait_for_timeout(1200)
    total = page.evaluate("fetch('http://127.0.0.1:9178/api/v1/images?limit=1').then(r=>r.json()).then(d=>d.total)")
    chips2 = page.evaluate("[...document.querySelectorAll('.sf-chip')].map(c => c.textContent.trim())")
    print("after2 chips:", chips2, "total:", total)
    browser.close()
