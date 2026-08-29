# -*- coding: utf-8 -*-
"""回收站全部行检查：缩略图缺失/回退/尺寸异常。"""
import json

from playwright.sync_api import sync_playwright

CDP = "http://127.0.0.1:9223"
BACKEND = "http://127.0.0.1:9178"

with sync_playwright() as p:
    browser = p.chromium.connect_over_cdp(CDP)
    ctx = browser.contexts[0]
    page = ctx.pages[0] if ctx.pages else ctx.new_page()
    page.goto(BACKEND + "/trash", wait_until="networkidle")
    page.wait_for_timeout(2500)
    rows = page.evaluate(
        """() => [...document.querySelectorAll('.el-table__row')].map((row, idx) => {
            const img = row.querySelector('.el-image img');
            const fallback = row.querySelector('.thumb-fallback');
            const reason = row.querySelector('.el-tag')?.textContent?.trim() ?? '';
            const orig = row.querySelectorAll('td')[1]?.textContent?.trim() ?? '';
            return {
                idx,
                hasImg: !!img,
                imgSrc: img ? img.src.split('/').pop() : null,
                imgW: img ? img.naturalWidth : 0,
                imgH: img ? img.naturalHeight : 0,
                fallback: !!fallback,
                reason,
                orig: orig.slice(0, 60),
            };
        })"""
    )
    for r in rows:
        flag = ""
        if not r["hasImg"] or r["fallback"]:
            flag = " <== 无图!"
        if r["imgW"] == 0 and r["hasImg"]:
            flag += " <== 图片未加载!"
        print(f"#{r['idx']} img={r['hasImg']} {r['imgSrc']} {r['imgW']}x{r['imgH']} reason={r['reason']} {r['orig']}{flag}")
    print("total rows:", len(rows))
    browser.close()
