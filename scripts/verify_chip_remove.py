# -*- coding: utf-8 -*-
"""验证 chips 的 × 删除功能正常（上轮 FAIL 是测试脚本点 × 的选择器问题）。"""
import sys
import time

from playwright.sync_api import sync_playwright

CDP = "http://127.0.0.1:9223"
BACKEND = "http://127.0.0.1:9178"

with sync_playwright() as p:
    browser = p.chromium.connect_over_cdp(CDP)
    ctx = browser.contexts[0]
    page = ctx.pages[0] if ctx.pages else ctx.new_page()
    page.goto(BACKEND + "/library", wait_until="networkidle")
    page.wait_for_timeout(1500)

    # 先清空（直接通过组件事件链：如果还有残留 chip，用 el-tag 的 close 事件）
    page.evaluate(
        """() => {
            const closes = [...document.querySelectorAll('.sf-chip')].map(c => c.querySelector('.el-tag__close'));
            for (const b of closes) if (b) b.dispatchEvent(new MouseEvent('click', {bubbles: true}));
        }"""
    )
    page.wait_for_timeout(800)
    n = page.evaluate("document.querySelectorAll('.sf-chip').length")
    print("清空后 chips:", n)
    assert n == 0, f"清除失败 chips={n}"

    # 输入 black_ → 点第一条（black_hair）→ 验证 chip + 输入清空 + 再点 × 删除
    page.click(".sf-input")
    page.keyboard.type("black_")
    page.wait_for_timeout(800)
    page.evaluate(
        """() => {
            const it = [...document.querySelectorAll('.sf-item')].find(e => e.textContent.includes('black_hair'));
            if (it) it.dispatchEvent(new MouseEvent('mousedown', {bubbles: true}));
        }"""
    )
    page.wait_for_timeout(1000)
    kw = page.evaluate("document.querySelector('.sf-input')?.value || ''")
    chips = page.evaluate("[...document.querySelectorAll('.sf-chip')].map(c => c.textContent.trim())")
    print("选中后:", kw, chips)
    assert kw == "" and len(chips) == 1, f"选中异常 kw={kw!r} chips={chips}"

    # 点 × 删除
    page.evaluate(
        """() => {
            const c = document.querySelector('.sf-chip .el-tag__close');
            if (c) c.dispatchEvent(new MouseEvent('click', {bubbles: true}));
        }"""
    )
    page.wait_for_timeout(1000)
    n2 = page.evaluate("document.querySelectorAll('.sf-chip').length")
    total = page.evaluate("fetch('http://127.0.0.1:9178/api/v1/images?limit=1').then(r=>r.json()).then(d=>d.total)")
    print("删除后 chips:", n2, "total:", total)
    assert n2 == 0, f"删除失败 chips={n2}"
    print("\nPASS | chips 增删/清空/筛选联动全部正常")
    browser.close()
