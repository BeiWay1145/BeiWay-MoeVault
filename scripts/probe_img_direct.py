# -*- coding: utf-8 -*-
"""直接 new Image() 加载代理 URL。"""
from playwright.sync_api import sync_playwright

CDP = "http://127.0.0.1:9223"
BACKEND = "http://127.0.0.1:9178"

with sync_playwright() as p:
    browser = p.chromium.connect_over_cdp(CDP)
    ctx = browser.contexts[0]
    page = ctx.pages[0] if ctx.pages else ctx.new_page()
    page.goto(BACKEND + "/library/85", wait_until="networkidle")
    page.wait_for_timeout(1500)
    fu = "https://files.yande.re/image/11d7cf4c2e1ded6a89ff1d2ac0427f24/yande.re%201239119%20bikini%20blue_archive%20breasts%20cosplay%20kiryuu_kikyou%20nipples%20panty_pull%20pussy%20shigure%20swimsuits%20tail%20thong%20tsukatsuki_rio%20uncensored%20wet.png"
    proxy = f"{BACKEND}/api/v1/proxy-image?url={fu.replace('&', '%26').replace('%', '%25')}"
    print("proxy:", proxy[:80])
    r = page.evaluate(
        """new Promise(res => {
            const i = new Image();
            const t = setTimeout(() => res({ok: false, timeout: true}), 20000);
            i.onload = () => { clearTimeout(t); res({ok: true, w: i.naturalWidth, h: i.naturalHeight}); };
            i.onerror = () => { clearTimeout(t); res({ok: false}); };
            i.src = arguments[0];
        })""",
        proxy,
    )
    print("new Image():", r)
    browser.close()
