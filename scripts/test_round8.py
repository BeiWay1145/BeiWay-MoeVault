# -*- coding: utf-8 -*-
"""验证：侧边栏动画恢复（width transition 存在）+ 标签页表格 fixed 布局。"""
import json

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
    page.goto(BACKEND + "/tags", wait_until="networkidle")
    page.wait_for_timeout(1800)

    nav = page.evaluate("""() => { const n=document.querySelector('.side-nav'); return n ? getComputedStyle(n).transition : null; }""")
    print("side-nav transition:", nav)
    check("侧边栏动画已恢复（width transition）", bool(nav and "width" in nav), nav or "")

    table = page.evaluate("""() => { const t=document.querySelector('.el-table'); return t ? getComputedStyle(t).tableLayout : null; }""")
    print("el-table layout:", table)
    check("标签页表格 fixed 布局", table == "fixed", table or "")

    # 悬停展开（模拟 hover 侧边栏）
    page.hover(".side-nav")
    page.wait_for_timeout(400)
    w = page.evaluate("document.querySelector('.side-nav')?.getBoundingClientRect().width || 0")
    print("悬停后侧边栏宽度:", w)
    check("悬停展开正常工作", w >= 190, f"w={w}")

    browser.close()

fails = [r for r in results if not r[1]]
print(f"\n=== {len(results) - len(fails)}/{len(results)} 通过 ===")
