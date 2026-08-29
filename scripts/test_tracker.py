# -*- coding: utf-8 -*-
"""BUG追踪器三态 + 全量追踪验证：
1. 设置页 BUG追踪器 tab：开关 + 仅本次会话勾选
2. 开启后产生 API 请求日志（category=track）
3. 转储接口可用
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
    page.goto(BACKEND + "/settings", wait_until="networkidle")
    page.wait_for_timeout(1800)

    # 切到 BUG追踪器 tab
    page.evaluate("""() => { const t=[...document.querySelectorAll('.el-tabs__item')].find(e=>e.textContent.includes('BUG')); if(t)t.click(); }""")
    page.wait_for_timeout(800)
    has_switch = page.evaluate("!!document.querySelector('.log-panel .el-switch')")
    has_session = page.evaluate("!![...document.querySelectorAll('.log-panel .el-checkbox')].find(e=>e.textContent.includes('仅本次会话'))")
    check("追踪器三态 UI（开关+会话勾选）", has_switch and has_session)

    # 开启追踪（持久）
    page.evaluate("""() => { const s=document.querySelector('.log-panel .el-switch'); if(s)s.click(); }""")
    page.wait_for_timeout(500)
    # 触发一些 API 请求（切到图库）
    page.goto(BACKEND + "/library", wait_until="networkidle")
    page.wait_for_timeout(2500)
    # 查追踪日志
    logs = page.evaluate("fetch('http://127.0.0.1:9178/api/v1/logs?limit=30').then(r=>r.json())")
    track_entries = [l for l in logs["items"] if l["category"] == "track"]
    print("track 日志数:", len(track_entries))
    check("追踪器产生 API 请求日志", len(track_entries) > 0, f"count={len(track_entries)}")
    if track_entries:
        sample = track_entries[0]["message"][:120]
        print("样例:", sample)
        check("日志含请求记录(t/api)", "api" in track_entries[0]["message"] or '"t":"api"' in track_entries[0]["message"] or "t\":\"api" in track_entries[0]["message"], sample)

    # 转储
    d = page.evaluate("fetch('http://127.0.0.1:9178/api/v1/logs/export').then(r=>r.json())")
    check("转储接口可用", d.get("ok") is True, f"count={d.get('count')}")

    print("console errors:", errs[:3])
    browser.close()

fails = [r for r in results if not r[1]]
print(f"\n=== {len(results) - len(fails)}/{len(results)} 通过 ===")
sys.exit(1 if fails else 0)
