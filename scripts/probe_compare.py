# -*- coding: utf-8 -*-
"""对比原图弹窗 DOM 检查。"""
import json

from playwright.sync_api import sync_playwright

CDP = "http://127.0.0.1:9223"
BACKEND = "http://127.0.0.1:9178"

with sync_playwright() as p:
    browser = p.chromium.connect_over_cdp(CDP)
    ctx = browser.contexts[0]
    page = ctx.pages[0] if ctx.pages else ctx.new_page()
    page.goto(BACKEND + "/library/85", wait_until="networkidle")
    page.wait_for_timeout(2000)
    # 直接调 openCompare 的等价操作：点击对比原图按钮（在基本信息里）
    clicked = page.evaluate(
        """() => {
            const btns = [...document.querySelectorAll('.el-button')];
            const b = btns.find(e => e.textContent.includes('对比原图'));
            if (b) { b.click(); return true; }
            return false;
        }"""
    )
    print("clicked:", clicked)
    page.wait_for_timeout(3000)
    info = page.evaluate(
        """() => {
            const dlg = [...document.querySelectorAll('.el-dialog')].find(d => d.textContent.includes('对比原图'));
            if (!dlg) return { found: false };
            return {
                found: true,
                empties: [...dlg.querySelectorAll('.el-empty__description')].map(e => e.textContent.trim()),
                body: dlg.textContent.slice(0, 200),
            };
        }"""
    )
    print(json.dumps(info, ensure_ascii=False))
    browser.close()
