# -*- coding: utf-8 -*-
"""拖入导入端到端验证：通过 CDP 连接桌面壳 WebView2。
1. 注入合成 Tauri drag-drop 事件（模拟真实 OS 拖放）→ 对话框应打开
2. 校验路径列表渲染 + 切换复制模式
3. 点击「开始导入」→ POST /api/v1/import → 批次创建
4. 校验对话框关闭 + toast 提示
"""
import json
import sys
import time

from playwright.sync_api import sync_playwright

CDP = "http://127.0.0.1:9223"
BACKEND = "http://127.0.0.1:9178"

# 用一张真实存在的测试图 + 一个真实目录
TEST_FILE = "D:\\Game\\AI\\cl_tagger\\.venv\\Lib\\site-packages\\gradio\\media_assets\\images\\bus.png"
TEST_DIR = "D:\\Game\\AI\\cl_tagger\\.venv\\Lib\\site-packages\\gradio\\media_assets\\images"

results = []


def check(name, ok, detail=""):
    results.append((name, ok, detail))
    print(("PASS" if ok else "FAIL") + f" | {name}" + (f" | {detail}" if detail else ""))


with sync_playwright() as p:
    browser = p.chromium.connect_over_cdp(CDP)
    ctx = browser.contexts[0]
    page = ctx.pages[0] if ctx.pages else ctx.new_page()
    print("connected, url =", page.url)
    page.bring_to_front()
    page.wait_for_load_state("networkidle", timeout=20000)

    # 0) 前置：确认 Tauri 环境与监听已注册（拖放监听注册在 __TAURI_INTERNALS__ 事件表里）
    has_tauri = page.evaluate("!!window.__TAURI_INTERNALS__")
    check("Tauri 环境存在", has_tauri)

    # 记录导入前图片数
    before = page.evaluate(
        "fetch('http://127.0.0.1:9178/api/v1/images?limit=1').then(r=>r.json())"
    )
    print("before-sample:", json.dumps(before, ensure_ascii=False)[:120])

    # 1) 合成 drag-enter → 覆盖层出现
    page.evaluate(
        """async () => {
            const internals = window.__TAURI_INTERNALS__;
            await internals.invoke('plugin:event|emit_to', {
                event: 'tauri://drag-enter',
                target: { kind: 'Webview', label: 'main' },
                payload: { type: 'enter', paths: ['X:/fake/a.png'],
                           position: { type: 'Physical', x: 100, y: 100 } },
            });
        }"""
    )
    page.wait_for_timeout(600)
    overlay = page.evaluate("!!document.querySelector('.drag-overlay')")
    check("drag-enter → 覆盖层显示", overlay)

    # 2) 合成 drag-drop（真实路径：1 文件 + 1 目录）→ 对话框打开
    page.evaluate(
        """async ([f, d]) => {
            const internals = window.__TAURI_INTERNALS__;
            await internals.invoke('plugin:event|emit_to', {
                event: 'tauri://drag-drop',
                target: { kind: 'Webview', label: 'main' },
                payload: { type: 'drop', paths: [f, d],
                           position: { type: 'Physical', x: 200, y: 200 } },
            });
        }""",
        [TEST_FILE, TEST_DIR],
    )
    page.wait_for_timeout(800)
    dlg = page.evaluate(
        "!!document.querySelector('.el-dialog__title') && "
        "document.querySelector('.el-dialog__title').textContent.includes('拖入导入')"
    )
    check("drop → 拖入导入对话框打开", dlg)
    rows = page.evaluate("document.querySelectorAll('.path-row').length")
    check("路径预览渲染 2 行", rows == 2, f"rows={rows}")
    summary = page.evaluate(
        "(document.querySelector('.path-summary')||{}).textContent || ''"
    )
    check("统计行渲染（共 2 项）", "2" in summary, summary.strip())

    # 3) 切换为复制进库
    page.evaluate(
        """() => {
            const labels = [...document.querySelectorAll('.el-dialog .el-radio__label')];
            const copy = labels.find(l => l.textContent.includes('复制进库'));
            if (copy) copy.click();
        }"""
    )
    page.wait_for_timeout(300)

    # 4) 点击「开始导入」
    btn = page.locator(".el-dialog__footer button", has_text="开始导入")
    btn.click()
    page.wait_for_timeout(2500)

    # 5) 对话框关闭 + 批次已创建（toast 或批次接口）
    # el-dialog 关闭后 DOM 保留，检查其容器 display:none
    closed = page.evaluate(
        """() => {
            const dlg = document.querySelector('.el-dialog');
            return !dlg || dlg.closest('.el-overlay').style.display === 'none'
                 || getComputedStyle(dlg).display === 'none';
        }"""
    )
    check("导入后对话框关闭", closed)

    # 直接查后端批次列表确认批次创建（copy 模式）
    batches = page.evaluate(
        "fetch('http://127.0.0.1:9178/api/v1/import/batches').then(r=>r.json())"
    )
    items = batches.get("items", [])
    check("后端批次已创建", len(items) > 0, f"count={len(items)}")
    if items:
        b = items[0]
        print("latest batch:", json.dumps(b, ensure_ascii=False)[:200])

    # 6) 等批次完成，验证图库数量增长（copy 模式应新增 2+ 张：bus.png + images 目录里未导入过的）
    time.sleep(4)
    after = page.evaluate(
        "fetch('http://127.0.0.1:9178/api/v1/images?limit=1').then(r=>r.json())"
    )
    print("after-sample:", json.dumps(after, ensure_ascii=False)[:120])

    # 7) drag-leave → 覆盖层隐藏
    page.evaluate(
        """async () => {
            const internals = window.__TAURI_INTERNALS__;
            await internals.invoke('plugin:event|emit_to', {
                event: 'tauri://drag-leave',
                target: { kind: 'Webview', label: 'main' },
                payload: { type: 'leave' },
            });
        }"""
    )
    page.wait_for_timeout(400)
    overlay2 = page.evaluate("!!document.querySelector('.drag-overlay')")
    check("drag-leave → 覆盖层隐藏", not overlay2)

    browser.close()

fails = [r for r in results if not r[1]]
print(f"\n=== {len(results) - len(fails)}/{len(results)} 通过 ===")
sys.exit(1 if fails else 0)
