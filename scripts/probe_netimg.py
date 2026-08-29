# -*- coding: utf-8 -*-
"""对比原图网络图加载排查：拿 file_url，测 WebView2 内加载。"""
import json

from playwright.sync_api import sync_playwright

CDP = "http://127.0.0.1:9223"
BACKEND = "http://127.0.0.1:9178"

with sync_playwright() as p:
    browser = p.chromium.connect_over_cdp(CDP)
    ctx = browser.contexts[0]
    page = ctx.pages[0] if ctx.pages else ctx.new_page()
    # 直接调 source-info API
    info = page.evaluate("fetch('http://127.0.0.1:9178/api/v1/images/85/source-info').then(r=>r.json())")
    print("source-info:", json.dumps(info, ensure_ascii=False)[:400])

    fu = info["info"].get("file_url")
    if fu:
        # 测 WebView2 内能否加载该 URL（fetch 无 referer）
        try:
            r = page.evaluate(
                "fetch('%s', {headers:{'Referer':'https://yande.re/'}}).then(r=>({s:r.status, len:r.headers.get('content-length')}))" % fu
            )
            print("fetch with referer:", r)
        except Exception as e:
            print("fetch err:", e)
        # 无 referer
        try:
            r2 = page.evaluate("fetch('%s').then(r=>({s:r.status, len:r.headers.get('content-length')}))" % fu)
            print("fetch no-referer:", r2)
        except Exception as e:
            print("fetch no-ref err:", e)
        # 直接 new Image 测试（模拟 <img>）
        r3 = page.evaluate(
            """new Promise(res => {
                const i = new Image();
                i.onload = () => res({ok: true, w: i.naturalWidth});
                i.onerror = () => res({ok: false});
                i.src = arguments[0];
            })""",
            fu,
        )
        print("img load:", r3)
    browser.close()
