# -*- coding: utf-8 -*-
"""检查对比图 img 元素的加载细节 + 直接 fetch 代理 URL 对比。"""
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
    page.wait_for_timeout(2000)
    page.evaluate("""() => { const b=[...document.querySelectorAll('.el-button')].find(e=>e.textContent.includes('对比原图')); if(b)b.click(); }""")
    time.sleep(3)
    detail = page.evaluate(
        """async () => {
            const dlg = [...document.querySelectorAll('.el-dialog')].find(d => d.textContent.includes('对比原图'));
            if (!dlg) return null;
            const sides = dlg.querySelectorAll('.compare-side');
            const right = sides[sides.length - 1];
            const img = right?.querySelector('img');
            const info = {
                src: img?.src,
                currentSrc: img?.currentSrc,
                complete: img?.complete,
                naturalW: img?.naturalWidth,
                hasSrcAttr: img?.hasAttribute('src'),
            };
            // 直接用 fetch 测代理（同源，无 CORS）
            const proxyUrl = img ? new URL(img.src).pathname + new URL(img.src).search : null;
            let fetchInfo = null;
            if (proxyUrl) {
                try {
                    const r = await fetch(proxyUrl);
                    const b = await r.blob();
                    fetchInfo = { status: r.status, type: b.type, size: b.size };
                } catch (e) { fetchInfo = { err: String(e) }; }
            }
            return { info, fetchInfo };
        }"""
    )
    print(json.dumps(detail, ensure_ascii=False))
    browser.close()
