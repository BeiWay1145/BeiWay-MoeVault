# -*- coding: utf-8 -*-
"""用页面真实代理 URL 测 new Image() vs fetch。"""
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
    res = page.evaluate(
        """async () => {
            const dlg = [...document.querySelectorAll('.el-dialog')].find(d => d.textContent.includes('对比原图'));
            const img = dlg?.querySelector('.compare-img-native');
            const url = img ? img.src : null;
            if (!url) return { url: null };
            // fetch
            let fr = null;
            try { const r = await fetch(url); const b = await r.blob(); fr = { ok: true, size: b.size, type: b.type }; }
            catch (e) { fr = { ok: false, err: String(e) }; }
            // new Image()
            const ir = await new Promise(res => {
                const i = new Image();
                const t = setTimeout(() => res({ ok: false, timeout: true }), 15000);
                i.onload = () => { clearTimeout(t); res({ ok: true, w: i.naturalWidth, h: i.naturalHeight }); };
                i.onerror = () => { clearTimeout(t); res({ ok: false, err: 'onerror' }); };
                i.src = url;
            });
            return { url: url.slice(0, 60), fetch: fr, image: ir };
        }"""
    )
    print(json.dumps(res, ensure_ascii=False, indent=1))
    browser.close()
