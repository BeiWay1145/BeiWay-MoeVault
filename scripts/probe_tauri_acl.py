# -*- coding: utf-8 -*-
"""ACL 探测：emit vs emit_to vs listen，找出被拒的原因。"""
from playwright.sync_api import sync_playwright

CDP = "http://127.0.0.1:9223"

with sync_playwright() as p:
    browser = p.chromium.connect_over_cdp(CDP)
    ctx = browser.contexts[0]
    # 找 9178 的页面
    page = None
    for pg in ctx.pages:
        if "9178" in pg.url:
            page = pg
            break
    if page is None:
        print("pages:", [pg.url for pg in ctx.pages])
        raise SystemExit(1)
    print("page:", page.url)
    page.wait_for_load_state("networkidle", timeout=15000)

    probes = {
        "listen": """async () => {
            try {
                const un = await window.__TAURI_INTERNALS__.invoke('plugin:event|listen', {
                    event: 'probe-test-ev',
                    target: { kind: 'Any' },
                    handler: window.__TAURI_INTERNALS__.transformCallback(() => {}, true),
                });
                return 'listen OK, id=' + un;
            } catch (e) { return 'listen FAIL: ' + e; }
        }""",
        "emit": """async () => {
            try {
                await window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
                    event: 'probe-test-ev',
                    payload: { hello: 1 },
                });
                return 'emit OK';
            } catch (e) { return 'emit FAIL: ' + e; }
        }""",
        "emit_to_any": """async () => {
            try {
                await window.__TAURI_INTERNALS__.invoke('plugin:event|emit_to', {
                    event: 'probe-test-ev',
                    target: { kind: 'Any' },
                    payload: { hello: 1 },
                });
                return 'emit_to(Any) OK';
            } catch (e) { return 'emit_to(Any) FAIL: ' + e; }
        }""",
        "emit_to_webview_main": """async () => {
            try {
                await window.__TAURI_INTERNALS__.invoke('plugin:event|emit_to', {
                    event: 'probe-test-ev',
                    target: { kind: 'Webview', label: 'main' },
                    payload: { hello: 1 },
                });
                return 'emit_to(Webview:main) OK';
            } catch (e) { return 'emit_to(Webview:main) FAIL: ' + e; }
        }""",
        "emit_to_webviewwindow_main": """async () => {
            try {
                await window.__TAURI_INTERNALS__.invoke('plugin:event|emit_to', {
                    event: 'probe-test-ev',
                    target: { kind: 'WebviewWindow', label: 'main' },
                    payload: { hello: 1 },
                });
                return 'emit_to(WebviewWindow:main) OK';
            } catch (e) { return 'emit_to(WebviewWindow:main) FAIL: ' + e; }
        }""",
    }
    for name, js in probes.items():
        r = page.evaluate(js)
        print(f"{name}: {r}")

    # 页面环境信息
    info = page.evaluate(
        """() => ({
            internals: !!window.__TAURI_INTERNALS__,
            metadata: window.__TAURI_INTERNALS__ ? JSON.stringify(window.__TAURI_INTERNALS__.metadata || {}) : null,
        })"""
    )
    print("env:", info)
    browser.close()
