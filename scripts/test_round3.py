# -*- coding: utf-8 -*-
"""本轮两个问题验证：
BUG1(对比原图): 图片85 打开对比 → 明确错误提示（yande.re 不可达），不再静默空白
增强1(折叠键): 详情页标签栏/基本信息栏可折叠收起/展开
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

    # ---- 增强1: 折叠键 ----
    # 标签栏折叠
    has_tag_fold = page.evaluate("!![...document.querySelectorAll('.tag-panel-header .el-button')].find(e => e.textContent.includes('收起'))")
    check("增强1: 标签栏折叠键存在", has_tag_fold)
    page.evaluate("""() => { const b=[...document.querySelectorAll('.tag-panel-header .el-button')].find(e=>e.textContent.includes('收起')); if(b)b.click(); }""")
    page.wait_for_timeout(500)
    tag_body_hidden = page.evaluate("!document.querySelector('.tag-panel-body')")
    check("增强1: 标签栏收起后内容隐藏", tag_body_hidden)
    page.evaluate("""() => { const b=[...document.querySelectorAll('.tag-panel-header .el-button')].find(e=>e.textContent.includes('展开')); if(b)b.click(); }""")
    page.wait_for_timeout(500)
    tag_body_back = page.evaluate("!!document.querySelector('.tag-panel-body')")
    check("增强1: 标签栏展开恢复", tag_body_back)

    # 基本信息折叠
    has_info_fold = page.evaluate("!![...document.querySelectorAll('.panel-title')].find(e => e.textContent.includes('基本信息'))")
    check("增强1: 基本信息折叠标题存在", has_info_fold)
    page.evaluate("""() => { const t=[...document.querySelectorAll('.panel-title')].find(e=>e.textContent.includes('基本信息')); if(t)t.click(); }""")
    page.wait_for_timeout(500)
    info_hidden = page.evaluate("!document.querySelector('.panel .el-descriptions')")
    check("增强1: 基本信息收起后隐藏", info_hidden)
    page.evaluate("""() => { const t=[...document.querySelectorAll('.panel-title')].find(e=>e.textContent.includes('基本信息')); if(t)t.click(); }""")
    page.wait_for_timeout(500)
    info_back = page.evaluate("!!document.querySelector('.panel .el-descriptions')")
    check("增强1: 基本信息展开恢复", info_back)

    # ---- BUG1(对比原图): yande.re 网络图信息获取 ----
    page.evaluate("""() => { const b=[...document.querySelectorAll('.el-button')].find(e=>e.textContent.includes('对比原图')); if(b)b.click(); }""")
    page.wait_for_timeout(3000)
    cmp = page.evaluate(
        """() => {
            const dlg = [...document.querySelectorAll('.el-dialog')].find(d => d.textContent.includes('对比原图'));
            if (!dlg) return { found: false };
            const right = dlg.querySelector('.compare-side:last-child');
            return {
                found: true,
                netSize: right?.querySelector('.compare-meta')?.textContent?.trim() || '',
                hasNetImg: !!right?.querySelector('img'),
            };
        }"""
    )
    print("对比数据:", json.dumps(cmp, ensure_ascii=False))
    check("BUG1: 对比原图获取到网络图信息（yande.re）", cmp["found"] and ("1.9 MB" in cmp["netSize"] or "MB" in cmp["netSize"]), cmp["netSize"][:80])

    print("console errors:", errs[:3])
    browser.close()

fails = [r for r in results if not r[1]]
print(f"\n=== {len(results) - len(fails)}/{len(results)} 通过 ===")
sys.exit(1 if fails else 0)
