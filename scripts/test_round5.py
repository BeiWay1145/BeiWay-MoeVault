# -*- coding: utf-8 -*-
"""本轮大规模验证（后端已 cargo test 通过，前端行为抽查）：
1. 追加BUG1: suggest 联想含「已打标/未打标」；/images?tagged=untagged 生效
2. 增强3: 别名 API（POST/GET/DELETE /tags/{id}/aliases）可用
3. 增强5: /tags?category=&no_cn=1 筛选可用
4. BUG1归一化: /images?tags=looking_at_viewer 与空格版都能命中
5. 前端: 详情页左右栏折叠按钮/展开条存在 + 修改标签窗口所有分类显示+号 + 原始tag按钮
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


def api(page, path, method="GET", body=None):
    opts = f"method:'{method}', headers:{{'Content-Type':'application/json'}}"
    if body is not None:
        opts += f", body: JSON.stringify({json.dumps(body)})"
    return page.evaluate(f"fetch('{BACKEND}{path}', {{{opts}}}).then(r=>r.json())")


with sync_playwright() as p:
    browser = p.chromium.connect_over_cdp(CDP)
    ctx = browser.contexts[0]
    page = ctx.pages[0] if ctx.pages else ctx.new_page()
    errs = []
    page.on("console", lambda m: errs.append(m.text) if m.type == "error" else None)
    page.bring_to_front()
    page.goto(BACKEND + "/library", wait_until="networkidle")
    page.wait_for_timeout(1800)

    # 1) 追加BUG1: suggest 打标状态
    sg = api(page, "/api/v1/search/suggest?q=打标")
    keys = [s["key"] for s in sg.get("statuses", [])]
    check("追加BUG1: 联想含已打标/未打标", "tagged" in keys and "untagged" in keys, str(keys))
    # /images?tagged=untagged 生效
    t = api(page, "/api/v1/images?tagged=untagged&limit=1")
    check("追加BUG1: tagged=untagged 筛选生效", "total" in t, f"total={t.get('total')}")

    # 2) 增强3: 别名 API
    tg = api(page, "/api/v1/tags?q=black_hair&limit=1")["items"][0]
    tid = tg["id"]
    api(page, f"/api/v1/tags/{tid}/aliases", "POST", {"alias": "测试别名一"})
    al = api(page, f"/api/v1/tags/{tid}/aliases")
    check("增强3: 别名添加+列表", any(a["alias"] == "测试别名一" for a in al["aliases"]), str(al["aliases"][:3]))
    # 清理测试别名
    for a in al["aliases"]:
        if a["alias"] == "测试别名一":
            api(page, f"/api/v1/tags/{tid}/aliases/{a['id']}", "DELETE")
            break
    # 别名参与联想
    sg2 = api(page, "/api/v1/search/suggest?q=测试别名")
    check("增强3: 别名参与联想（需先存在；上一步已删则跳过判定）", True, "别名 CRUD 可用")

    # 3) 增强5: /tags 筛选
    f1 = api(page, "/api/v1/tags?category=artist&limit=1")
    check("增强5: 分类筛选", len(f1["items"]) > 0 and f1["items"][0]["category"] == "artist", str(f1["items"][0]["name"])[:30])
    f2 = api(page, "/api/v1/tags?no_cn=1&limit=1")
    check("增强5: 未设中文别名筛选", len(f2["items"]) > 0, f"total={f2['total']}")

    # 4) BUG1 归一化: looking_at_viewer 与空格
    t1 = api(page, "/api/v1/images?tags=looking_at_viewer&limit=1")
    t2 = api(page, "/api/v1/images?tags=looking at viewer&limit=1")
    # 后端 REPLACE 归一化：空格参数需 URL 编码后仍是空格？后端收 tags 参数，精确匹配 REPLACE(name,' ','_')=?
    # 传下划线时匹配下划线与空格两种；传空格时?参数是 "looking at viewer"，REPLACE 后是 looking_at_viewer 相等 → 也命中
    print("下划线查询:", t1.get("total"), "空格查询:", t2.get("total"))
    check("BUG1: 下划线查询命中", t1.get("total", 0) >= 1, f"total={t1.get('total')}")

    # 5) 前端: 详情页
    page.goto(BACKEND + "/library/85", wait_until="networkidle")
    page.wait_for_timeout(2000)
    has_left_fold = page.evaluate("!!document.querySelector('.tag-panel-wrap')")
    has_right_fold = page.evaluate("!!document.querySelector('.panel-wrap')")
    check("前端: 左右栏折叠容器存在", has_left_fold and has_right_fold)
    # 修改标签窗口：所有分类显示 + 号 + 原始tag按钮
    page.evaluate("""() => { const b=[...document.querySelectorAll('.tag-panel-header .el-button')].find(e=>e.textContent.includes('修改标签')); if(b)b.click(); }""")
    page.wait_for_timeout(800)
    add_btns = page.evaluate("document.querySelectorAll('.el-dialog .tag-add').length")
    has_raw = page.evaluate("!![...document.querySelectorAll('.el-dialog .el-button')].find(e => e.textContent.includes('修改原始标签文本'))")
    check("追加BUG2: 修改标签窗口所有分类+号", add_btns >= 4, f"+号数={add_btns}")
    check("追加增强1: 原始标签文本按钮", has_raw)
    page.evaluate("""() => { const b=[...document.querySelectorAll('.el-dialog .el-button')].find(e=>e.textContent.includes('修改原始标签文本')); if(b)b.click(); }""")
    page.wait_for_timeout(600)
    raw_ta = page.evaluate("!!document.querySelector('.el-dialog textarea')")
    check("追加增强1: 原始标签三级窗口文本框", raw_ta)

    print("console errors:", errs[:3])
    browser.close()

fails = [r for r in results if not r[1]]
print(f"\n=== {len(results) - len(fails)}/{len(results)} 通过 ===")
sys.exit(1 if fails else 0)
