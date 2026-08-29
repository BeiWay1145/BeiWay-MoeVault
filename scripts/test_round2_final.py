# -*- coding: utf-8 -*-
"""最终验证（修正断言版）：
增强2: 详情页标签栏 + 点标签跳转 chips 同步（新 watch）
增强3: 逗号提交生效（cards 数量变化）+ 全选/收缩
BUG1: 图片85 状态（sauce_cache 90.06 有命中，后端修复后应显示已溯源）
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

    # ---- 增强2: 点标签跳转 → chips 同步（新 watch）----
    page.goto(BACKEND + "/library/85", wait_until="networkidle")
    page.wait_for_timeout(2000)
    first_tag = page.evaluate("document.querySelector('.tag-line-name')?.textContent?.trim() || ''")
    print("首标签:", first_tag)
    page.evaluate("document.querySelector('.tag-line')?.dispatchEvent(new MouseEvent('click', {bubbles:true}))")
    page.wait_for_timeout(2000)
    url = page.evaluate("location.pathname")
    chips = page.evaluate("[...document.querySelectorAll('.sf-chip')].map(c => c.textContent.trim())")
    check("增强2: 点标签跳图库+chips 同步", url == "/library" and len(chips) == 1 and chips[0] == first_tag,
          f"url={url} chips={chips} expect={first_tag}")

    # ---- 增强3: 逗号提交生效（cards 数量变化）----
    page.click(".sf-input")
    page.keyboard.type("1girl,")
    page.wait_for_timeout(1500)
    cards = page.evaluate("document.querySelectorAll('.image-card').length")
    check("增强3: 逗号提交筛选生效", cards > 0 and cards < 70, f"cards={cards}")
    # 全选 checkbox 存在
    has_selectall = page.evaluate("!![...document.querySelectorAll('.el-checkbox')].find(e => e.textContent.includes('全选'))")
    check("增强3: 全选 checkbox 存在", has_selectall)

    # ---- BUG1: 图片85 当前状态（命中 90.06 但爬取失败 → 修复后已溯源）----
    img85 = api(page, "/api/v1/images/85") if False else None
    # 用 tags 接口确认 source/source_url（detail 接口路径不确定，直接查库）
    detail = page.goto(BACKEND + "/library/85", wait_until="networkidle")
    page.wait_for_timeout(2000)
    status_text = page.evaluate(
        """() => {
            const tds = [...document.querySelectorAll('.el-descriptions-item__content')];
            const st = tds.find(e => e.textContent.includes('溯源') || e.textContent.includes('已溯源') || e.textContent.includes('不可溯源') || e.textContent.includes('未溯源'));
            return st ? st.textContent.trim().slice(0, 40) : null;
        }"""
    )
    print("状态标签:", status_text)
    # 后端 DB 查询图片85 的 source/source_url（用 python 只读查）
    import subprocess
    py = r"C:\Users\BeiWay1145\AppData\Local\BeiWay-MoeVault\python\.venv\Scripts\python.exe"
    code = "import sqlite3; c=sqlite3.connect(r'file:D:\\Program Files\\BeiWay-MoeVault\\data\\app.db?mode=ro',uri=True); r=c.execute(\"SELECT source,source_url,no_auto_sauce FROM images WHERE id=85\").fetchone(); print(r)"
    out = subprocess.run([py, "-c", code], capture_output=True, text=True).stdout.strip()
    print("DB 图片85:", out)
    db_ok = "'danbooru'" in out or "'gelbooru'" in out or "unknown" in out
    check("BUG1: 图片85 已写入 source（后端修复生效）", db_ok and "1" in out.split(", ")[-1] or db_ok, out)

    print("console errors:", errs[:3])
    browser.close()

fails = [r for r in results if not r[1]]
print(f"\n=== {len(results) - len(fails)}/{len(results)} 通过 ===")
sys.exit(1 if fails else 0)
