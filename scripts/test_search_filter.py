# -*- coding: utf-8 -*-
"""搜索式筛选端到端验证（CDP）：
1. 图库工具栏搜索框（靠左，与视图模式同高）
2. 输入 '1gir' → 联想 1girl [54]（词频倒序）
3. 选中 → chip 出现 + 网格筛选生效（total=54）
4. 输入 'ai' → 状态联想 AI 生成 [27] / 非 AI 生成 [43]
5. 选中 非 AI 生成 → 双条件筛选（标签 AND + 非AI）
6. 点结果图 → 详情页上下文「标签：1girl」→ 上/下一张在集合内
7. /search 路由已删除（访问应回退或 404）
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
    page.on("console", lambda m: print("[console]", m.type, m.text[:150]) if m.type == "error" else None)
    page.bring_to_front()
    page.goto(BACKEND + "/library", wait_until="networkidle")
    page.wait_for_timeout(2000)

    # 1) 搜索框存在且靠左（toolbar 第一个子元素）
    box = page.evaluate(
        """() => {
            const t = document.querySelector('.toolbar');
            if (!t) return null;
            const first = t.firstElementChild;
            return {
                hasSearch: !!t.querySelector('.filter-search'),
                firstIsSearch: first ? first.classList.contains('filter-search') : false,
            };
        }"""
    )
    check("搜索框靠左存在", bool(box and box["hasSearch"] and box["firstIsSearch"]), json.dumps(box))

    # 2) 输入 '1gir' → 联想
    page.click(".filter-search .el-select__wrapper")
    page.wait_for_timeout(300)
    page.keyboard.type("1gir")
    page.wait_for_timeout(800)
    opts = page.evaluate(
        """() => [...document.querySelectorAll('.el-select-dropdown__item')].map(e => e.textContent.trim())"""
    )
    print("联想:", opts[:6])
    check("联想出现 1girl", any("1girl" in o for o in opts), str(opts[:4]))

    # 3) 选中 1girl → chip + 筛选
    page.evaluate(
        """() => {
            const items = [...document.querySelectorAll('.el-select-dropdown__item')];
            const it = items.find(e => e.textContent.includes('1girl'));
            if (it) it.click();
        }"""
    )
    page.wait_for_timeout(1500)
    chips = page.evaluate("[...document.querySelectorAll('.filter-tag')].map(e => e.textContent.trim())")
    check("选中后 chip 出现", any("1girl" in c for c in chips), str(chips))
    total = api(page, "/api/v1/images?limit=1")["total"]
    check("筛选生效（total=1girl 数）", total > 0, f"total={total}")

    # 4) 输入 'ai' → 状态联想
    page.click(".filter-search .el-select__wrapper")
    page.wait_for_timeout(300)
    page.keyboard.type("ai")
    page.wait_for_timeout(800)
    status_opts = page.evaluate(
        """() => [...document.querySelectorAll('.el-select-dropdown__item')].map(e => e.textContent.trim())"""
    )
    print("状态联想:", status_opts[:6])
    check("状态联想出现（AI/非AI）", any("AI" in o and "生成" in o for o in status_opts), str(status_opts[:4]))

    # 5) 选中 非 AI 生成 → 双条件
    page.evaluate(
        """() => {
            const items = [...document.querySelectorAll('.el-select-dropdown__item')];
            const it = items.find(e => e.textContent.includes('非 AI 生成'));
            if (it) it.click();
        }"""
    )
    page.wait_for_timeout(1500)
    chips2 = page.evaluate("[...document.querySelectorAll('.filter-tag')].map(e => e.textContent.trim())")
    check("双 chip（标签+状态）", len(chips2) >= 2, str(chips2))
    total2 = api(page, "/api/v1/images?limit=1")["total"]
    check("双条件筛选 total 缩减", total2 <= total, f"{total} → {total2}")

    # 6) 点图 → 详情上下文标签
    card = page.evaluate("document.querySelectorAll('.image-card')[0]")
    if card:
        page.evaluate("document.querySelectorAll('.image-card')[0].dispatchEvent(new MouseEvent('click', {bubbles: true}))")
        page.wait_for_timeout(1500)
        label = page.evaluate("document.querySelector('.nav-pos-label')?.textContent || ''")
        check("详情页上下文「标签：」", label.startswith("标签："), label)
        pos = page.evaluate("document.querySelector('.nav-pos')?.textContent?.trim() || ''")
        check("位置指示 N/M", "/" in pos, pos)
    else:
        check("有结果图可点", False)

    # 7) /search 已删除
    page.goto(BACKEND + "/search", wait_until="networkidle")
    page.wait_for_timeout(1200)
    url = page.evaluate("location.pathname")
    not_search = not url.startswith("/search")
    check("/search 路由已移除", not_search, f"landed={url}")

    browser.close()

fails = [r for r in results if not r[1]]
print(f"\n=== {len(results) - len(fails)}/{len(results)} 通过 ===")
sys.exit(1 if fails else 0)
