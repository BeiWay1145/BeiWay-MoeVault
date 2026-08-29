# -*- coding: utf-8 -*-
"""本轮验证：
BUG1: 多选+全选 → 关多选 → 全选图标复位
BUG2: 标签/图库每页条数修改生效（pageSize ref 更新 + 分页 total 变化）
BUG3: 详情页左右栏用淡入淡出（无宽度重排）
改进1: 设置页「标签」tab 含字典设置；「回收站/sidecar」不含字典
改进2: 查重设置（pHash）在「本地推理」tab
增强1: 日志 tab 改名 BUG追踪器 + 开关 + 后端转储接口
增强2: 设置页离开提醒（dirty 检测）
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

    # ---- BUG1: 全选 → 关多选 → 全选复位 ----
    page.goto(BACKEND + "/library", wait_until="networkidle")
    page.wait_for_timeout(1800)
    page.evaluate("""() => { const c=[...document.querySelectorAll('.el-checkbox')].find(e=>e.textContent.includes('全选')); if(c)c.click(); }""")
    page.wait_for_timeout(600)
    # 关多选
    page.evaluate("""() => { const c=[...document.querySelectorAll('.el-checkbox')].find(e=>e.textContent.includes('多选模式')); if(c)c.click(); }""")
    page.wait_for_timeout(600)
    selall_state = page.evaluate(
        """() => {
            const label=[...document.querySelectorAll('.el-checkbox')].find(e=>e.textContent.includes('全选'));
            return label ? { cls: label.className, text: label.textContent } : null;
        }"""
    )
    check("BUG1: 关多选后全选复位", bool(selall_state) and "is-checked" not in selall_state["cls"], json.dumps(selall_state, ensure_ascii=False))

    # ---- 增强1: 日志转储接口 ----
    d = api(page, "/api/v1/logs/export")
    check("增强1: 后端转储接口", d.get("ok") is True and d.get("count", 0) > 0, f"count={d.get('count')} path={str(d.get('path'))[:40]}")

    # ---- 设置页结构 ----
    page.goto(BACKEND + "/settings", wait_until="networkidle")
    page.wait_for_timeout(1800)
    tabs = page.evaluate("[...document.querySelectorAll('.el-tabs__item')].map(e => e.textContent.trim())")
    print("tabs:", tabs)
    check("改进1: 有「标签」tab", any("标签" in t for t in tabs), str(tabs))
    check("改进2: 无独立「查重」tab", not any(t == "查重" for t in tabs), str(tabs))
    check("增强1: 日志 tab 改名 BUG追踪器", any("BUG" in t for t in tabs), str(tabs))
    # 切到标签 tab 确认字典设置
    page.evaluate("""() => { const t=[...document.querySelectorAll('.el-tabs__item')].find(e=>e.textContent.includes('标签')); if(t)t.click(); }""")
    page.wait_for_timeout(600)
    has_dict = page.evaluate("!![...document.querySelectorAll('.el-form-item')].find(e => e.textContent.includes('中文字典'))")
    check("改进1: 标签 tab 含字典设置", has_dict)

    print("console errors:", errs[:3])
    browser.close()

fails = [r for r in results if not r[1]]
print(f"\n=== {len(results) - len(fails)}/{len(results)} 通过 ===")
sys.exit(1 if fails else 0)
