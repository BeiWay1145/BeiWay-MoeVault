# -*- coding: utf-8 -*-
"""本轮 5 项修复的综合验证（CDP）：
BUG1: 输入→选中→点外部→再聚焦：不应显示残留候选
BUG2: 标签页分页 total 应为真实总数（>50 可翻页）
BUG3: 回收站显示（reason 中文化 + 无测试杂物）
增强1: 逗号分隔多标签（1girl, → chip，继续输 1boy 出候选）
增强2: 标签页无「中文字典」按钮；中文别名设置后搜索联想可用
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


def api(page, path):
    return page.evaluate(f"fetch('{BACKEND}{path}').then(r=>r.json())")


with sync_playwright() as p:
    browser = p.chromium.connect_over_cdp(CDP)
    ctx = browser.contexts[0]
    page = ctx.pages[0] if ctx.pages else ctx.new_page()
    errs = []
    page.on("console", lambda m: errs.append(m.text) if m.type == "error" else None)
    page.bring_to_front()

    # ---- BUG2: 标签页分页 total ----
    t = api(page, "/api/v1/tags?limit=50&offset=0")
    check("BUG2: 标签 total 为真实总数", t["total"] > 50, f"total={t['total']} items={len(t['items'])}")

    # ---- BUG3: 回收站 ----
    page.goto(BACKEND + "/trash", wait_until="networkidle")
    page.wait_for_timeout(1800)
    tr = api(page, "/api/v1/trash?limit=200")
    check("BUG3: 回收站仅剩用户项目", tr["total"] == 5 and all(it["reason"] == "manual" for it in tr["items"]),
          f"total={tr['total']} reasons={[it['reason'] for it in tr['items']]}")
    # reason 中文化（页面渲染）
    labels = page.evaluate("[...document.querySelectorAll('.el-table__row .el-tag')].map(e => e.textContent.trim())")
    check("BUG3: reason 中文化", all(l == "手动删除" for l in labels), str(labels))

    # ---- BUG1 + 增强1: 图库搜索框 ----
    page.goto(BACKEND + "/library", wait_until="networkidle")
    page.wait_for_timeout(1800)
    page.click(".sf-input")
    page.keyboard.type("1g")
    page.wait_for_timeout(800)
    page.evaluate(
        """() => { const it=[...document.querySelectorAll('.sf-item')].find(e=>e.textContent.includes('1girl')); if(it) it.dispatchEvent(new MouseEvent('mousedown',{bubbles:true})); }"""
    )
    page.wait_for_timeout(1000)
    kw = page.evaluate("document.querySelector('.sf-input')?.value || ''")
    check("选中后输入清空", kw == "", repr(kw))
    # 点外部 → 再聚焦
    page.evaluate("document.querySelector('.wall-container').dispatchEvent(new MouseEvent('mousedown',{bubbles:true}))")
    page.wait_for_timeout(400)
    page.click(".sf-input")
    page.wait_for_timeout(500)
    open_state = page.evaluate("!!document.querySelector('.sf-dropdown')")
    check("BUG1: 重新聚焦无残留候选", not open_state, f"dropdown={open_state}")

    # 增强1: 输入 '1girl,' → 自动变 chip → 继续输 1boy
    page.keyboard.type("1girl,")
    page.wait_for_timeout(600)
    chips = page.evaluate("[...document.querySelectorAll('.sf-chip')].map(c => c.textContent.trim())")
    kw2 = page.evaluate("document.querySelector('.sf-input')?.value || ''")
    check("增强1: 逗号提交 chip", any("1girl" in c for c in chips), f"chips={chips} kw={kw2!r}")
    page.keyboard.type("1boy")
    page.wait_for_timeout(800)
    items = page.evaluate("[...document.querySelectorAll('.sf-item-name')].map(e => e.textContent.trim())")
    check("增强1: 继续输入出 1boy 候选", any("1boy" in i for i in items), str(items[:4]))

    # ---- 增强2: 标签页无字典按钮 + 中文别名设置 ----
    page.goto(BACKEND + "/tags", wait_until="networkidle")
    page.wait_for_timeout(1800)
    has_dict_btn = page.evaluate("!![...document.querySelectorAll('.toolbar *')].find(e => e.textContent.includes('中文字典'))")
    check("增强2: 标签页无中文字典按钮", not has_dict_btn)
    # 给 1girl 设置中文别名 → 验证 suggest 可按中文联想
    tg = api(page, "/api/v1/tags?q=1girl&limit=1")["items"][0]
    api(page, "/api/v1/tags/%d/name-cn" % tg["id"]) if False else None
    page.evaluate(f"fetch('{BACKEND}/api/v1/tags/{tg['id']}/name-cn', {{method:'PUT', headers:{{'Content-Type':'application/json'}}, body: JSON.stringify({{name_cn:'一位女孩'}})}})")
    page.wait_for_timeout(600)
    sg = api(page, "/api/v1/search/suggest?q=一位女")
    check("增强2: 中文别名参与搜索联想", any(t["name"] == "1girl" for t in sg["tags"]), json.dumps(sg["tags"][:3], ensure_ascii=False))

    page.screenshot(path=r"D:\Code\Reasonix_Projects\image\scripts\fixes_check.png")
    print("console errors:", errs[:3])
    browser.close()

fails = [r for r in results if not r[1]]
print(f"\n=== {len(results) - len(fails)}/{len(results)} 通过 ===")
sys.exit(1 if fails else 0)
