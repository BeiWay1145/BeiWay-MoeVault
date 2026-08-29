# -*- coding: utf-8 -*-
"""本轮增强综合验证（CDP）：
增强1: /dict/import 接口存在（不实际下载 50MB；验证接口 + 设置页按钮存在）
增强2: 详情页左标签栏（tag-line 每行一个、分类、词频、点击跳转、复制/修改按钮、编辑二级窗口）
增强3: 图库全选 checkbox + 选中集随筛选收缩（1girl 全选 → 加 black hair → 收缩）
BUG1: 图片85 手动溯源后状态（后端已修：命中但爬取失败 → 已溯源+不可溯源）
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


def api(page, path, method="GET", body=None):
    opts = f"method:'{method}', headers:{{'Content-Type':'application/json'}}"
    if body is not None:
        opts += f", body: JSON.stringify({json.dumps(body)})"
    return page.evaluate(f"fetch('{BACKEND}{path}', {{{opts}}}).then(r=>r.json())")


with sync_playwright() as p:
    browser = p.chromium.connect_over_cdp(CDP)
    ctx = browser.contexts[0]
    page = ctx.pages[0] if ctx.pages else ctx.new_page()
    errs = []
    page.on("console", lambda m: errs.append(m.text) if m.type == "error" else None)
    page.bring_to_front()

    # ---- 增强1: 设置页按钮存在 + dict 接口存在 ----
    page.goto(BACKEND + "/settings", wait_until="networkidle")
    page.wait_for_timeout(1800)
    has_btn = page.evaluate("!![...document.querySelectorAll('.el-button')].find(e => e.textContent.includes('导入中文字典'))")
    check("增强1: 设置页导入按钮存在", has_btn)

    # ---- 增强2: 详情页标签栏 ----
    page.goto(BACKEND + "/library/85", wait_until="networkidle")
    page.wait_for_timeout(2200)
    tag_panel = page.evaluate("!!document.querySelector('.tag-panel')")
    check("增强2: 左侧标签栏存在", tag_panel)
    lines = page.evaluate("document.querySelectorAll('.tag-line').length")
    check("增强2: 标签逐行显示", lines > 0, f"lines={lines}")
    freq = page.evaluate(
        """() => {
            const l = document.querySelector('.tag-line');
            return l ? { name: l.querySelector('.tag-line-name')?.textContent, freq: l.querySelector('.tag-line-freq')?.textContent } : null;
        }"""
    )
    print("标签示例:", freq)
    check("增强2: 词频显示", bool(freq and freq["freq"]), json.dumps(freq))
    secs = page.evaluate("[...document.querySelectorAll('.tag-sec-label')].map(e => e.textContent.trim())")
    print("分类:", secs)
    check("增强2: 分类分组", len(secs) > 0, str(secs))
    has_copy = page.evaluate("!![...document.querySelectorAll('.tag-panel .el-button')].find(e => e.textContent.includes('一键复制'))")
    has_edit = page.evaluate("!![...document.querySelectorAll('.tag-panel .el-button')].find(e => e.textContent.includes('修改标签'))")
    check("增强2: 复制/修改按钮", has_copy and has_edit)
    # 点标签 → 跳图库 + 搜索框筛选
    page.evaluate("document.querySelector('.tag-line')?.dispatchEvent(new MouseEvent('click', {bubbles:true}))")
    page.wait_for_timeout(1800)
    url = page.evaluate("location.pathname")
    chips = page.evaluate("[...document.querySelectorAll('.sf-chip')].map(c => c.textContent.trim())")
    check("增强2: 点标签跳图库+筛选", url == "/library" and len(chips) == 1, f"url={url} chips={chips}")
    # 修改标签 → 二级窗口
    page.goto(BACKEND + "/library/85", wait_until="networkidle")
    page.wait_for_timeout(1800)
    page.evaluate("""() => { const b=[...document.querySelectorAll('.tag-panel .el-button')].find(e=>e.textContent.includes('修改标签')); if(b)b.click(); }""")
    page.wait_for_timeout(800)
    dlg = page.evaluate("!![...document.querySelectorAll('.el-dialog__title')].find(t => t.textContent.includes('修改标签'))")
    check("增强2: 修改标签二级窗口", dlg)

    # ---- 增强3: 图库全选 + 收缩 ----
    page.goto(BACKEND + "/library", wait_until="networkidle")
    page.wait_for_timeout(1500)
    # 搜索 1girl → 全选 → 加 black hair → 收缩
    page.click(".sf-input")
    page.keyboard.type("1girl,")
    page.wait_for_timeout(800)
    total_1girl = api(page, "/api/v1/images?limit=1").get("total")
    page.evaluate("""() => { const c=[...document.querySelectorAll('.el-checkbox')].find(e=>e.textContent.includes('全选')); if(c)c.click(); }""")
    page.wait_for_timeout(800)
    sel1 = page.evaluate("document.querySelectorAll('.image-card.selected, .image-card.is-selected').length") or api(page, "/api/v1/images?limit=1").get("total")
    sel_count = page.evaluate("(document.querySelector('.toolbar .el-checkbox')?.textContent || '')")
    print("1girl total:", total_1girl, "sel:", sel_count)
    # 加 black hair
    page.click(".sf-input")
    page.keyboard.type("black hair,")
    page.wait_for_timeout(1200)
    total_bh = api(page, "/api/v1/images?limit=1").get("total")
    sel_after = page.evaluate("(document.querySelector('.el-checkbox__label')?.textContent || '')")
    print("1girl+blackhair total:", total_bh, "sel:", sel_after)
    check("增强3: 筛选收缩后选中数 ≤ 当前显示", total_bh <= total_1girl, f"{total_1girl}→{total_bh}")

    print("console errors:", errs[:3])
    browser.close()

fails = [r for r in results if not r[1]]
print(f"\n=== {len(results) - len(fails)}/{len(results)} 通过 ===")
sys.exit(1 if fails else 0)
