# -*- coding: utf-8 -*-
"""对比原图代理加载：长等待 + 轮询确认图片真正加载。"""
import json
import sys
import time

from playwright.sync_api import sync_playwright

CDP = "http://127.0.0.1:9223"
BACKEND = "http://127.0.0.1:9178"

with sync_playwright() as p:
    browser = p.chromium.connect_over_cdp(CDP)
    ctx = browser.contexts[0]
    page = ctx.pages[0] if ctx.pages else ctx.new_page()
    page.goto(BACKEND + "/library/85", wait_until="networkidle")
    page.wait_for_timeout(2000)
    page.evaluate("""() => { const b=[...document.querySelectorAll('.el-button')].find(e=>e.textContent.includes('对比原图')); if(b)b.click(); }""")
    # 轮询最多 25 秒等图片加载
    ok = False
    detail = None
    for _ in range(25):
        time.sleep(1)
        detail = page.evaluate(
            """() => {
                const dlg = [...document.querySelectorAll('.el-dialog')].find(d => d.textContent.includes('对比原图'));
                if (!dlg) return null;
                const sides = dlg.querySelectorAll('.compare-side');
                const right = sides[sides.length - 1];
                const img = right?.querySelector('img');
                const errSlot = right?.querySelector('.placeholder-name');
                return {
                    imgLoaded: img ? (img.complete && img.naturalWidth > 0) : false,
                    naturalW: img ? img.naturalWidth : 0,
                    errText: errSlot ? errSlot.textContent.trim() : '',
                };
            }"""
        )
        if detail and detail.get("imgLoaded"):
            ok = True
            break
    print("最终状态:", json.dumps(detail, ensure_ascii=False))
    print("PASS" if ok else "FAIL", "| 网络图经代理加载成功")
    browser.close()
    sys.exit(0 if ok else 1)
