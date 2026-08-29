# -*- coding: utf-8 -*-
"""两个增强的端到端验证（CDP 连接桌面壳）。

增强1：提交导入（每次运行先生成唯一新图）→ 批次 state=done → WS 广播 →
       全局完成通知自动出现 + 图库 total 增长。
增强2：主目录来源组展开 → 点第 2 张（B）→ 详情页位置指示 "2 / N"（组上下文），
       → 下一张位置 "3 / N" 且 URL 变化 → 上一张 URL 回到 B。
"""
import json
import subprocess
import sys
import time

from playwright.sync_api import sync_playwright

CDP = "http://127.0.0.1:9223"
BACKEND = "http://127.0.0.1:9178"
VENV_PY = r"C:\Users\BeiWay1145\AppData\Local\BeiWay-MoeVault\python\.venv\Scripts\python.exe"
NEW_IMAGE = r"D:\Game\AI\cl_tagger\.venv\Lib\site-packages\gradio\media_assets\images\enh_test.png"

results = []


def check(name, ok, detail=""):
    results.append((name, ok))
    print(("PASS" if ok else "FAIL") + f" | {name}" + (f" | {detail}" if detail else ""))


def api(page, path):
    return page.evaluate(f"fetch('{BACKEND}{path}').then(r=>r.json())")


def regenerate_test_image():
    """生成内容唯一的新图（颜色随时间变），确保 MD5 唯一。"""
    code = (
        "from PIL import Image; import time"
        f"; im = Image.new('RGB', (64, 64), (200, (int(time.time()) // 2) % 256, 60))"
        f"; im.save(r'{NEW_IMAGE}'); print('ok')"
    )
    subprocess.run([VENV_PY, "-c", code], check=True, capture_output=True)


with sync_playwright() as p:
    browser = p.chromium.connect_over_cdp(CDP)
    ctx = browser.contexts[0]
    page = ctx.pages[0] if ctx.pages else ctx.new_page()
    print("connected:", page.url)
    page.bring_to_front()
    page.goto(BACKEND + "/imports", wait_until="networkidle")
    page.wait_for_timeout(1500)

    # ---------- 增强1 ----------
    regenerate_test_image()
    lib_before = api(page, "/api/v1/images?limit=1")["total"]

    page.click(".top-bar .left .el-button")
    page.wait_for_timeout(500)
    page.fill(".el-dialog textarea", NEW_IMAGE)
    page.evaluate(
        """() => {
            const labels = [...document.querySelectorAll('.el-dialog .el-radio__label')];
            const copy = labels.find(l => l.textContent.includes('复制进库'));
            if (copy) copy.click();
        }"""
    )
    page.locator(".el-dialog__footer button", has_text="开始导入").click()
    page.wait_for_timeout(1200)

    deadline = time.time() + 20
    batch = None
    while time.time() < deadline:
        items = api(page, "/api/v1/import/batches?limit=1")["items"]
        if items and items[0].get("state") == "done":
            batch = items[0]
            break
        time.sleep(1)
    check("批次 state=done", batch is not None, json.dumps(batch, ensure_ascii=False)[:150] if batch else "timeout")

    toast = False
    deadline = time.time() + 8
    while time.time() < deadline:
        toast = page.evaluate(
            "!![...document.querySelectorAll('.el-message')].find(m => m.textContent.includes('已完成'))"
        )
        if toast:
            break
        time.sleep(0.5)
    check("完成通知自动出现（WS）", toast)

    lib_after = api(page, "/api/v1/images?limit=1")["total"]
    check("图库 total 增长（导入生效）", lib_after == lib_before + 1, f"{lib_before} → {lib_after}")

    # ---------- 增强2：主目录组内上下文导航 ----------
    page.reload(wait_until="networkidle")
    page.wait_for_timeout(2000)
    page.evaluate(
        """() => {
            const dirs = [...document.querySelectorAll('.dir-header')];
            if (dirs.length > 0) dirs[0].click();
        }"""
    )
    page.wait_for_timeout(2000)
    n_cards = page.evaluate("document.querySelectorAll('.dir-images .image-card').length")
    print("cards:", n_cards)
    if n_cards >= 2:
        # 点击第 2 张（B）
        page.evaluate(
            "document.querySelectorAll('.dir-images .image-card')[1]"
            ".dispatchEvent(new MouseEvent('click', {bubbles: true}))"
        )
        page.wait_for_timeout(1500)
        pos = page.evaluate("document.querySelector('.nav-pos')?.textContent?.trim() || ''")
        label = page.evaluate("document.querySelector('.nav-pos-label')?.textContent || ''")
        url_b = page.evaluate("location.pathname")
        check("详情页位置指示 2 / N（组上下文）", pos.startswith("2 /"), f"pos={pos!r} label={label!r}")
        check("上下文标签为组名", len(label) > 0, label)
        # 下一张 → 3 / N，URL 变化
        page.keyboard.press("ArrowRight")
        page.wait_for_timeout(1500)
        pos2 = page.evaluate("document.querySelector('.nav-pos')?.textContent?.trim() || ''")
        url_c = page.evaluate("location.pathname")
        check("下一张位置 3 / N", pos2.startswith("3 /"), f"pos={pos2!r}")
        check("下一张 URL 已切换（C ≠ B）", url_c != url_b, f"{url_b} → {url_c}")
        # 上一张 → 回到 B
        page.keyboard.press("ArrowLeft")
        page.wait_for_timeout(1500)
        url_b2 = page.evaluate("location.pathname")
        check("上一张回到 B", url_b2 == url_b, f"{url_b2} vs {url_b}")
    else:
        check("主目录组内可点击卡片 >= 2", False, f"cards={n_cards}")

    browser.close()

fails = [r for r in results if not r[1]]
print(f"\n=== {len(results) - len(fails)}/{len(results)} 通过 ===")
sys.exit(1 if fails else 0)
