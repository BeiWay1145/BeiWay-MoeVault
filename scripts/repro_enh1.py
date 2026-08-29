# -*- coding: utf-8 -*-
"""增强1 问题复现：真实拖入流程（合成 drag-drop）在主目录页面上，
全程监听 window 事件与 DOM 变化，定位「完成后界面不刷新」的断点。

步骤：
1. /imports 展开 days[0].dirs[0] 来源组，记录组计数文本 + 卡片数；
   页面里安装探针：window.__events 记录 moevault:import-done/failed 触发。
2. 向该组 source_dir 写入一张唯一新图（文件名 moerepro_HHMMSS.png）。
3. 合成 tauri://drag-drop（该文件路径）→ DragImport 对话框 → 切复制 → 开始导入。
4. 轮询批次 state=done，然后 10 秒内观察：
   - __events 是否记录（WS → 窗口事件是否到达页面）
   - 完成通知 toast 是否出现
   - 组计数文本是否变化（树刷新）
   - 组内卡片 DOM 数是否变化（dirImages 缓存刷新 ← 高度怀疑的缺陷点）
"""
import json
import subprocess
import sys
import time

from playwright.sync_api import sync_playwright

CDP = "http://127.0.0.1:9223"
BACKEND = "http://127.0.0.1:9178"
VENV_PY = r"C:\Users\BeiWay1145\AppData\Local\BeiWay-MoeVault\python\.venv\Scripts\python.exe"

results = []


def check(name, ok, detail=""):
    results.append((name, ok))
    print(("PASS" if ok else "FAIL") + f" | {name}" + (f" | {detail}" if detail else ""))


def api(page, path):
    return page.evaluate(f"fetch('{BACKEND}{path}').then(r=>r.json())")


stamp = time.strftime("%H%M%S")
test_name = f"moerepro_{stamp}.png"
test_path = None

with sync_playwright() as p:
    browser = p.chromium.connect_over_cdp(CDP)
    ctx = browser.contexts[0]
    page = ctx.pages[0] if ctx.pages else ctx.new_page()
    page.on("console", lambda m: print("[console]", m.type, m.text[:160]) if m.type in ("error", "warning") else None)
    page.bring_to_front()
    page.goto(BACKEND + "/imports", wait_until="networkidle")
    page.wait_for_timeout(2000)

    # 1) 树 + 展开第一组 + 探针
    tree = api(page, "/api/v1/imports/tree")
    d0 = tree["days"][0]
    g0 = d0["dirs"][0]
    src_dir = g0.get("source_dir")
    print(f"目标组: date={d0['date']} name={g0['name']} count={g0['count']} src={src_dir}")

    page.evaluate(
        """() => {
            window.__events = [];
            window.addEventListener('moevault:import-done', e => window.__events.push({t:'done', d: e.detail}));
            window.addEventListener('moevault:import-failed', e => window.__events.push({t:'failed', d: e.detail}));
            const dirs = [...document.querySelectorAll('.dir-header')];
            if (dirs.length > 0) dirs[0].click();
        }"""
    )
    page.wait_for_timeout(2000)

    count_before = page.evaluate(
        """() => {
            const h = document.querySelector('.dir-header');
            const c = h ? h.querySelector('.dir-count') : null;
            return c ? c.textContent.trim() : null;
        }"""
    )
    cards_before = page.evaluate("document.querySelectorAll('.dir-images .image-card').length")
    print(f"计数前: {count_before!r}, 卡片前: {cards_before}")

    # 2) 写唯一测试图到该组对应的真实来源目录（source_dir 在库内为相对记录，映射到用户 Downloads）
    import os
    src_abs = os.path.join(r"C:\Users\BeiWay1145\Downloads", g0["name"])
    if not os.path.isdir(src_abs):
        src_abs = r"C:\Users\BeiWay1145\Downloads"
    test_path = os.path.join(src_abs, test_name)
    code = (
        "from PIL import Image; import time"
        f"; im = Image.new('RGB', (48, 48), (10, (int(time.time()) // 3) % 256, 200))"
        f"; im.save(r'{test_path}'); print('ok')"
    )
    subprocess.run([VENV_PY, "-c", code], check=True, capture_output=True)
    print("测试图已写入:", test_path)

    # 3) 合成拖入 → 对话框 → 复制模式 → 开始导入
    page.evaluate(
        """async (p) => {
            await window.__TAURI_INTERNALS__.invoke('plugin:event|emit_to', {
                event: 'tauri://drag-drop',
                target: { kind: 'Webview', label: 'main' },
                payload: { type: 'drop', paths: [p], position: { type: 'Physical', x: 300, y: 300 } },
            });
        }""",
        test_path,
    )
    page.wait_for_timeout(1000)
    dlg = page.evaluate("!![...document.querySelectorAll('.el-dialog__title')].find(t => t.textContent.includes('拖入导入'))")
    check("拖入对话框打开", dlg)
    page.evaluate(
        """() => {
            const labels = [...document.querySelectorAll('.el-dialog .el-radio__label')];
            const copy = labels.find(l => l.textContent.includes('复制进库'));
            if (copy) copy.click();
        }"""
    )
    page.locator(".el-dialog__footer button", has_text="开始导入").click()
    page.wait_for_timeout(1200)

    # 4) 等批次完成
    batch = None
    deadline = time.time() + 20
    while time.time() < deadline:
        items = api(page, "/api/v1/import/batches?limit=1")["items"]
        if items and items[0].get("state") == "done":
            batch = items[0]
            break
        time.sleep(1)
    check("批次 state=done", batch is not None, json.dumps(batch, ensure_ascii=False)[:130] if batch else "timeout")

    # 5) 观察信号（toast 检查要在 5 秒消失窗口内尽早做）
    toast = page.evaluate(
        "!![...document.querySelectorAll('.el-message')].find(m => m.textContent.includes('已完成'))"
    )
    if not toast:
        time.sleep(2)
        toast = page.evaluate(
            "!![...document.querySelectorAll('.el-message')].find(m => m.textContent.includes('已完成'))"
        )
    check("完成通知出现", toast)

    events = page.evaluate("window.__events || []")
    check("窗口事件到达（moevault:import-done）", len(events) > 0, json.dumps(events, ensure_ascii=False)[:200])
    count_after = page.evaluate(
        """() => {
            const h = document.querySelector('.dir-header');
            const c = h ? h.querySelector('.dir-count') : null;
            return c ? c.textContent.trim() : null;
        }"""
    )
    check("组计数文本已更新（树刷新）", count_after != count_before, f"{count_before!r} → {count_after!r}")
    cards_after = page.evaluate("document.querySelectorAll('.dir-images .image-card').length")
    check("组内卡片数已更新（dirImages 刷新）", cards_after > cards_before, f"{cards_before} → {cards_after}")
    # 名称级检查：库里文件按 MD5 重命名存储（显示名 = MD5 文件名），不能用原始文件名匹配；
    # 改为按目标组（Downloads）定位其卡片容器，比较组内卡片数
    group_cards = page.evaluate(
        """() => {
            const headers = [...document.querySelectorAll('.dir-header')];
            const h = headers.find(e => e.textContent.includes('Downloads'));
            if (!h) return null;
            const group = h.closest('.dir-group');
            return group ? group.querySelectorAll('.dir-images .image-card').length : null;
        }"""
    )
    check("目标组内卡片数已增加", group_cards is not None and group_cards > cards_before, f"{cards_before} → {group_cards}")

    browser.close()

fails = [r for r in results if not r[1]]
print(f"\n=== {len(results) - len(fails)}/{len(results)} 通过 ===")

# 清理提示（不自动删——输出需手动清理的文件与标识）
print("\n[清理信息] 测试文件:", test_path)
print("[清理信息] 库内图片按文件名匹配:", test_name)
sys.exit(1 if fails else 0)
