# -*- coding: utf-8 -*-
"""三个问题验证：
BUG3: 详情页栏收起 → wrap 宽度过渡存在（动画生效）+ viewer 平滑变宽（无瞬时偏移）
改进2: 本地推理 tab 末尾有独立「查重」卡片（pHash）
追加BUG1: SideNav 无 width transition（瞬时切换，无重排卡顿）
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

    # ---- BUG3: 宽度过渡存在 + viewer 平滑（先清除持久化状态确保展开初始）----
    page.goto(BACKEND + "/library/85", wait_until="networkidle")
    page.wait_for_timeout(1500)
    page.evaluate("localStorage.removeItem('moevault-detail-tag-collapsed'); localStorage.removeItem('moevault-detail-info-collapsed')")
    page.reload(wait_until="networkidle")
    page.wait_for_timeout(2000)
    wrap = page.evaluate(
        """() => {
            const w = document.querySelector('.tag-panel-wrap');
            if (!w) return null;
            const cs = getComputedStyle(w);
            return { transition: cs.transition, width: w.getBoundingClientRect().width };
        }"""
    )
    print("左栏 wrap:", json.dumps(wrap))
    check("BUG3: 左栏宽度过渡生效", bool(wrap and "width" in wrap["transition"] and wrap["width"] == 300),
          json.dumps(wrap, ensure_ascii=False))
    # 收起左栏 → 宽度动画类切换（过渡由 CSS 驱动，DOM 宽度在动画中）
    page.evaluate("""() => { const b=[...document.querySelectorAll('.tag-panel-header .el-button')].find(e=>e.textContent.includes('收起')); if(b)b.click(); }""")
    page.wait_for_timeout(80)  # 动画进行中（未结束）
    mid_w = page.evaluate("document.querySelector('.tag-panel-wrap')?.getBoundingClientRect().width || 0")
    page.wait_for_timeout(400)  # 动画结束
    end_w = page.evaluate("document.querySelector('.tag-panel-wrap')?.getBoundingClientRect().width || 0")
    print(f"左栏宽度: 动画中={mid_w} 结束={end_w}")
    check("BUG3: 收起动画过程宽度渐变（非瞬跳）", 0 < mid_w < 300, f"mid={mid_w} end={end_w}")
    check("BUG3: 动画结束宽度归零", end_w == 0, f"end={end_w}")
    # 展开
    page.evaluate("document.querySelector('.panel-expand-bar.left')?.dispatchEvent(new MouseEvent('click', {bubbles:true}))")
    page.wait_for_timeout(400)
    back_w = page.evaluate("document.querySelector('.tag-panel-wrap')?.getBoundingClientRect().width || 0")
    check("BUG3: 展开恢复 300", back_w == 300, f"back={back_w}")

    # ---- 改进2: 本地推理 tab 末尾查重卡片 ----
    page.goto(BACKEND + "/settings", wait_until="networkidle")
    page.wait_for_timeout(1800)
    page.evaluate("""() => { const t=[...document.querySelectorAll('.el-tabs__item')].find(e=>e.textContent.includes('本地推理')); if(t)t.click(); }""")
    page.wait_for_timeout(800)
    has_dedup_card = page.evaluate("!![...document.querySelectorAll('.el-card__header')].find(e => e.textContent.includes('查重'))")
    check("改进2: 本地推理页末尾查重卡片", has_dedup_card)

    # ---- 追加BUG1: SideNav 无 width transition ----
    nav = page.evaluate(
        """() => {
            const n = document.querySelector('.side-nav');
            return n ? getComputedStyle(n).transition : null;
        }"""
    )
    print("side-nav transition:", nav)
    check("追加BUG1: 侧边栏无宽度过渡（防卡顿）", nav == "all 0s ease 0s" or "width" not in (nav or ""), nav or "")

    print("console errors:", errs[:3])
    browser.close()

fails = [r for r in results if not r[1]]
print(f"\n=== {len(results) - len(fails)}/{len(results)} 通过 ===")
sys.exit(1 if fails else 0)
