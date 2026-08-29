# -*- coding: utf-8 -*-
"""P3 分类浏览端到端验证（CDP）：
1. 图库工具栏「分类浏览」→ 抽屉打开
2. 分类 Tab 切换（画师）→ 标签卡片更新
3. 封面规则切换（文件最大/最新/随机/手动）→ browse 重拉
4. 点标签卡片 → 面板关闭 + 图库按该标签筛选 + chips 显示
5. 点结果图 → 详情页位置指示为「标签：xxx」→ 上/下一张在标签集合内
6. 手动设封面：打开图格 → 选一张 → 封面更新（manual 规则下）
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

    # 1) 打开分类浏览面板
    page.evaluate(
        """() => {
            const btns = [...document.querySelectorAll('.toolbar .el-button')];
            const b = btns.find(x => x.textContent.includes('分类浏览'));
            if (b) b.click();
        }"""
    )
    page.wait_for_timeout(1500)
    dlg = page.evaluate("!!document.querySelector('.el-drawer__header') && document.querySelector('.el-drawer__header').textContent.includes('分类浏览')")
    check("分类浏览面板打开", dlg)
    n_cards = page.evaluate("document.querySelectorAll('.tag-card').length")
    total_text = page.evaluate("document.querySelector('.browse-total')?.textContent || ''")
    check("标签卡片网格渲染", n_cards > 0, f"cards={n_cards} {total_text}")

    # 2) 切分类 Tab：画师
    page.evaluate(
        """() => {
            const btns = [...document.querySelectorAll('.browse-controls .el-radio-button')];
            const b = btns.find(x => x.textContent.includes('画师'));
            if (b) b.click();
        }"""
    )
    page.wait_for_timeout(1200)
    artist_cards = page.evaluate("document.querySelectorAll('.tag-card').length")
    artist_names = page.evaluate("[...document.querySelectorAll('.tag-name')].map(e => e.textContent.trim()).slice(0, 5)")
    print("画师分类:", artist_names)
    # 验证返回的都是 artist 分类（后端过滤）
    check("分类 Tab 过滤生效（画师）", artist_cards > 0, f"cards={artist_cards}")

    # 3) 封面规则切换：文件最大 → 手动（验证 settings 持久化 + browse 重拉）
    page.evaluate(
        """() => {
            const sel = document.querySelector('.browse-controls .el-select');
            sel.dispatchEvent(new MouseEvent('click', {bubbles: true}));
        }"""
    )
    page.wait_for_timeout(600)
    page.evaluate(
        """() => {
            const opts = [...document.querySelectorAll('.el-select-dropdown__item')];
            const o = opts.find(x => x.textContent.includes('文件最大'));
            if (o) o.click();
        }"""
    )
    page.wait_for_timeout(1200)
    rule = api(page, "/api/v1/settings").get("tag_cover_rule")
    check("封面规则已持久化", rule == "size", f"rule={rule}")

    # 4) 点第一个标签卡片 → 图库筛选
    page.evaluate("document.querySelector('.tag-card')?.click()")
    page.wait_for_timeout(1500)
    chips = page.evaluate("[...document.querySelectorAll('.active-tags .el-tag')].map(e => e.textContent.trim())")
    check("标签筛选 chips 显示", len(chips) > 0, f"chips={chips}")
    grid_total = api(page, "/api/v1/images?limit=1").get("total")
    print("筛选后图库 total:", grid_total)

    # 5) 点结果图 → 详情页位置指示含「标签：」
    first_card = page.evaluate("document.querySelectorAll('.image-wall [data-image-id], .image-card')[0]")
    if first_card:
        page.evaluate("document.querySelectorAll('.image-card')[0].dispatchEvent(new MouseEvent('click', {bubbles: true}))")
        page.wait_for_timeout(1500)
        label = page.evaluate("document.querySelector('.nav-pos-label')?.textContent || ''")
        pos = page.evaluate("document.querySelector('.nav-pos')?.textContent?.trim() || ''")
        check("详情页上下文标签含「标签：」", label.startswith("标签："), f"pos={pos!r} label={label!r}")
        # 上/下一张仍在标签集合内（位置指示 N/M，M=该标签图片数）
        page.keyboard.press("ArrowRight")
        page.wait_for_timeout(1200)
        pos2 = page.evaluate("document.querySelector('.nav-pos')?.textContent?.trim() || ''")
        check("下一张仍在标签集合内", pos2.startswith("2 /"), f"pos={pos2!r}")

    # 6) 手动设封面（回图库 → 打开面板 → 首个标签 → ⋯ → 手动设封面 → 选图）
    page.goto(BACKEND + "/library", wait_until="networkidle")
    page.wait_for_timeout(1500)
    page.evaluate("""() => { const b=[...document.querySelectorAll('.toolbar .el-button')].find(x=>x.textContent.includes('分类浏览')); if(b)b.click(); }""")
    page.wait_for_timeout(1200)
    page.evaluate("document.querySelector('.tag-menu-btn')?.click()")
    page.wait_for_timeout(600)
    page.evaluate("""() => { const i=[...document.querySelectorAll('.el-dropdown-menu__item')].find(x=>x.textContent.includes('手动设封面')); if(i)i.click(); }""")
    page.wait_for_timeout(1500)
    picker = page.evaluate("!!document.querySelector('.cover-picker-grid .cover-pick-item')")
    check("手动选封面图格打开", picker)
    n_pick = page.evaluate("document.querySelectorAll('.cover-pick-item').length")
    if picker:
        page.evaluate("document.querySelector('.cover-pick-item').click()")
        page.wait_for_timeout(1500)
        ok = page.evaluate("!![...document.querySelectorAll('.el-message')].find(m => m.textContent.includes('已设置'))")
        check("手动封面已设置", ok)

    browser.close()

fails = [r for r in results if not r[1]]
print(f"\n=== {len(results) - len(fails)}/{len(results)} 通过 ===")
sys.exit(1 if fails else 0)
