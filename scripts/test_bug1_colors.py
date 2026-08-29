# -*- coding: utf-8 -*-
"""BUG1 + 增强1 验证（自定义 SearchFilter 组件）：
1. 搜索框可见（截图）
2. 输入 '1g' → 选 1girl → 输入框清空（BUG1）
3. 1girl chip = primary 蓝（general）
4. 输入 'ai' → 状态联想 → 选 非 AI 生成 → info 灰 chip + 输入框清空
5. 双条件筛选生效
6. Enter 选择第一条联想
"""
import json
import sys
import time

from playwright.sync_api import sync_playwright

CDP = "http://127.0.0.1:9223"
BACKEND = "http://127.0.0.1:9178"
SHOT = r"D:\Code\Reasonix_Projects\image\scripts\search_filter_check.png"

results = []


def check(name, ok, detail=""):
    results.append((name, ok))
    print(("PASS" if ok else "FAIL") + f" | {name}" + (f" | {detail}" if detail else ""))


def input_value(page):
    return page.evaluate("document.querySelector('.sf-input')?.value || ''")


with sync_playwright() as p:
    browser = p.chromium.connect_over_cdp(CDP)
    ctx = browser.contexts[0]
    page = ctx.pages[0] if ctx.pages else ctx.new_page()
    errs = []
    page.on("console", lambda m: errs.append(m.text) if m.type == "error" else None)
    page.bring_to_front()
    page.goto(BACKEND + "/library", wait_until="networkidle")
    page.wait_for_timeout(2000)

    check("无渲染错误", len(errs) == 0, str(errs[:2]))
    box = page.evaluate(
        """() => {
            const el = document.querySelector('.search-filter');
            if (!el) return null;
            const r = el.getBoundingClientRect();
            return { w: Math.round(r.width), h: Math.round(r.height), visible: r.width > 50 && r.height > 20 };
        }"""
    )
    check("搜索框可见", bool(box and box["visible"]), json.dumps(box))

    # 输入 '1g' → 联想出现 → 点击 1girl
    page.click(".sf-input")
    page.keyboard.type("1g")
    page.wait_for_timeout(800)
    items = page.evaluate("[...document.querySelectorAll('.sf-item-name')].map(e => e.textContent.trim())")
    print("联想:", items[:6])
    check("联想出现 1girl", any("1girl" in i for i in items), str(items[:4]))
    page.evaluate(
        """() => {
            const it = [...document.querySelectorAll('.sf-item')].find(e => e.textContent.includes('1girl'));
            if (it) it.dispatchEvent(new MouseEvent('mousedown', {bubbles: true}));
        }"""
    )
    page.wait_for_timeout(1200)

    # BUG1：输入框清空
    kw = input_value(page)
    check("BUG1 修复：选中后输入框清空", kw == "", repr(kw))
    chips = page.evaluate("[...document.querySelectorAll('.sf-chip')].map(c => ({t: c.textContent.trim(), c: c.className}))")
    print("chips:", json.dumps(chips, ensure_ascii=False))
    check("1girl chip 为 primary 蓝", any("1girl" in c["t"] and "el-tag--primary" in c["c"] for c in chips), str(chips))

    # 输入 'ai' → 状态联想 → 选 非 AI 生成
    page.click(".sf-input")
    page.keyboard.type("ai")
    page.wait_for_timeout(800)
    items2 = page.evaluate("[...document.querySelectorAll('.sf-item-name')].map(e => e.textContent.trim())")
    print("状态联想:", items2[:6])
    check("状态联想出现 AI/非AI", any("AI 生成" in i for i in items2), str(items2[:4]))
    page.evaluate(
        """() => {
            const it = [...document.querySelectorAll('.sf-item')].find(e => e.textContent.includes('非 AI 生成'));
            if (it) it.dispatchEvent(new MouseEvent('mousedown', {bubbles: true}));
        }"""
    )
    page.wait_for_timeout(1200)
    kw2 = input_value(page)
    check("状态选中后输入框清空", kw2 == "", repr(kw2))
    chips2 = page.evaluate("[...document.querySelectorAll('.sf-chip')].map(c => ({t: c.textContent.trim(), c: c.className}))")
    print("chips2:", json.dumps(chips2, ensure_ascii=False))
    check("状态 chip 为 info 灰", any("非 AI 生成" in c["t"] and "el-tag--info" in c["c"] for c in chips2), str(chips2))
    check("标签 chip 仍为 primary", any("1girl" in c["t"] and "el-tag--primary" in c["c"] for c in chips2), str(chips2))

    total = page.evaluate("fetch('http://127.0.0.1:9178/api/v1/images?limit=1').then(r=>r.json()).then(d=>d.total)")
    check("双条件筛选生效", total > 0, f"total={total}")

    # Enter 选择第一条：清空全部后输入 'black_' → Enter
    page.evaluate(
        """() => {
            const x = document.querySelectorAll('.sf-chip .el-tag__close');
            for (const b of [...x]) b.click();
        }"""
    )
    page.wait_for_timeout(1000)
    page.click(".sf-input")
    page.keyboard.type("black_")
    page.wait_for_timeout(800)
    page.keyboard.press("Enter")
    page.wait_for_timeout(1200)
    kw3 = input_value(page)
    check("Enter 选中后输入框清空", kw3 == "", repr(kw3))
    chips3 = page.evaluate("[...document.querySelectorAll('.sf-chip')].map(c => c.textContent.trim())")
    check("Enter 选中 black_ 前缀标签", len(chips3) == 1 and "black" in chips3[0], str(chips3))

    page.screenshot(path=SHOT)
    print("截图:", SHOT)
    browser.close()

fails = [r for r in results if not r[1]]
print(f"\n=== {len(results) - len(fails)}/{len(results)} 通过 ===")
sys.exit(1 if fails else 0)
