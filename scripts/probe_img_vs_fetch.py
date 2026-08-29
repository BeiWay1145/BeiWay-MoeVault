# -*- coding: utf-8 -*-
"""对比：页面内 new Image() vs fetch 加载代理 URL。"""
import json
import time

from playwright.sync_api import sync_playwright

CDP = "http://127.0.0.1:9223"
BACKEND = "http://127.0.0.1:9178"

with sync_playwright() as p:
    browser = p.chromium.connect_over_cdp(CDP)
    ctx = browser.contexts[0]
    page = ctx.pages[0] if ctx.pages else ctx.new_page()
    page.goto(BACKEND + "/library/85", wait_until="networkidle")
    page.wait_for_timeout(1500)
    page.evaluate("""() => { const b=[...document.querySelectorAll('.el-button')].find(e=>e.textContent.includes('对比原图')); if(b)b.click(); }""")
    time.sleep(2)
    # 拿到代理 URL
    url = page.evaluate(
        """() => {
            const dlg = [...document.querySelectorAll('.el-dialog')].find(d => d.textContent.includes('对比原图'));
            const img = dlg?.querySelector('.compare-img-native');
            return img ? img.src : null;
        }"""
    )
    print("proxy url:", url)
    # new Image() 加载
    r1 = page.evaluate(
        """new Promise(res => {
            const i = new Image();
            i.onload = () => res({ok: true, w: i.naturalWidth, h: i.naturalHeight});
            i.onerror = () => res({ok: false});
            i.src = arguments[0];
        })""",
        url,
    )
    print("new Image():", r1)
    # fetch 加载（blob 大小）
    r2 = page.evaluate(
        """fetch(arguments[0]).then(r => r.blob()).then(b => ({ok: true, size: b.size, type: b.type})).catch(e => ({ok: false, err: String(e)}))""",
        url,
    )
    print("fetch:", r2)
    browser.close()
