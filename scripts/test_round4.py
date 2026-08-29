# -*- coding: utf-8 -*-
"""最终验证：
1. 对比原图：网络图经后端代理加载（应显示图片而非失败占位）
2. 增强1：左右栏整栏收起/展开（图片区变大）
"""
import json
import sys
import time

from playwright.sync_api import sync_playwright

CDP = "http://127.0.0.1:9223"
BACKEND = "http://127.0.0.1:9178"

results = []


def check(name, ok, detail=""):
    results.append((name, ok))
    print(("PASS" if ok else "FAIL") + f" | {name}" + (f" | {detail}" if detail else ""))


with sync_playwright() as p:
    browser = p.chromium.connect_over_cdp(CDP)
    ctx = browser.contexts[0]
    page = ctx.pages[0] if ctx.pages else ctx.new_page()
    errs = []
    page.on("console", lambda m: errs.append(m.text) if m.type == "error" else None)
    page.bring_to_front()
    page.goto(BACKEND + "/library/85", wait_until="networkidle")
    page.wait_for_timeout(2200)

    # ---- BUG1: 对比原图（代理加载）----
    page.evaluate("""() => { const b=[...document.querySelectorAll('.el-button')].find(e=>e.textContent.includes('对比原图')); if(b)b.click(); }""")
    page.wait_for_timeout(4000)
    cmp = page.evaluate(
        """() => {
            const dlg = [...document.querySelectorAll('.el-dialog')].find(d => d.textContent.includes('对比原图'));
            if (!dlg) return { found: false };
            const sides = dlg.querySelectorAll('.compare-side');
            const right = sides[sides.length - 1];
            const img = right?.querySelector('img');
            const errSlot = right?.querySelector('.placeholder-name');
            return {
                found: true,
                imgSrc: img ? img.src.slice(0, 60) : null,
                imgLoaded: img ? (img.complete && img.naturalWidth > 0) : false,
                errText: errSlot ? errSlot.textContent.trim() : '',
                netMeta: right?.querySelector('.compare-meta')?.textContent?.trim() || '',
            };
        }"""
    )
    print("对比:", json.dumps(cmp, ensure_ascii=False))
    check("BUG1: 网络图经代理成功加载", cmp["found"] and cmp["imgLoaded"], f"src={cmp['imgSrc']} err={cmp['errText']}")
    check("BUG1: 网络图信息显示", "MB" in cmp["netMeta"] or "KB" in cmp["netMeta"], cmp["netMeta"][:60])
    page.evaluate("""() => { const dlg=[...document.querySelectorAll('.el-dialog')].find(d=>d.textContent.includes('对比原图')); if(dlg){ const c=dlg.querySelector('.el-dialog__headerbtn'); if(c)c.click(); } }""")
    page.wait_for_timeout(500)

    # ---- 增强1: 左右栏整栏收起 ----
    vw_before = page.evaluate("document.querySelector('.viewer')?.getBoundingClientRect().width || 0")
    # 收起左栏
    page.evaluate("""() => { const b=[...document.querySelectorAll('.tag-panel-header .el-button')].find(e=>e.textContent.includes('收起')); if(b)b.click(); }""")
    page.wait_for_timeout(600)
    tag_hidden = page.evaluate("!document.querySelector('.tag-panel')")
    vw_mid = page.evaluate("document.querySelector('.viewer')?.getBoundingClientRect().width || 0")
    check("增强1: 左栏收起", tag_hidden, f"w {vw_before}→{vw_mid}")
    check("增强1: 图片区变大", vw_mid > vw_before, f"{vw_before}→{vw_mid}")
    # 展开左栏
    page.evaluate("document.querySelector('.panel-expand-bar.left')?.dispatchEvent(new MouseEvent('click', {bubbles:true}))")
    page.wait_for_timeout(600)
    tag_back = page.evaluate("!!document.querySelector('.tag-panel')")
    check("增强1: 左栏展开恢复", tag_back)

    # 收起右栏
    page.evaluate("""() => { const b=[...document.querySelectorAll('.panel-collapse-bar .el-button')].find(e=>e.textContent.includes('收起')); if(b)b.click(); }""")
    page.wait_for_timeout(600)
    panel_hidden = page.evaluate("!document.querySelector('.panel')")
    vw_after = page.evaluate("document.querySelector('.viewer')?.getBoundingClientRect().width || 0")
    check("增强1: 右栏收起", panel_hidden, f"w {vw_mid}→{vw_after}")
    check("增强1: 图片区变大(右栏收起)", vw_after > vw_mid, f"{vw_mid}→{vw_after}")
    # 展开右栏
    page.evaluate("document.querySelector('.panel-expand-bar:not(.left)')?.dispatchEvent(new MouseEvent('click', {bubbles:true}))")
    page.wait_for_timeout(600)
    panel_back = page.evaluate("!!document.querySelector('.panel')")
    check("增强1: 右栏展开恢复", panel_back)

    print("console errors:", errs[:3])
    browser.close()

fails = [r for r in results if not r[1]]
print(f"\n=== {len(results) - len(fails)}/{len(results)} 通过 ===")
sys.exit(1 if fails else 0)
