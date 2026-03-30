/// Browser tools via chromiumoxide (CDP).
///
/// Manages a single Chrome instance shared across all tool calls.
/// Chrome is launched lazily on first use and kept alive until `shutdown()`.
use std::sync::Arc;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chromiumoxide::{Browser, BrowserConfig, Page};
use futures::StreamExt;
use serde_json::{Value, json};
use tokio::sync::Mutex;

// ── State ─────────────────────────────────────────────────────────────────────

pub struct BrowserState {
    browser: Option<Browser>,
    pages: Vec<Arc<Page>>,
    active: usize,
}

impl BrowserState {
    pub fn new() -> Self {
        Self { browser: None, pages: vec![], active: 0 }
    }
}

pub type SharedBrowserState = Arc<Mutex<BrowserState>>;

pub fn new_browser_state() -> SharedBrowserState {
    Arc::new(Mutex::new(BrowserState::new()))
}

// ── Launch helpers ────────────────────────────────────────────────────────────

fn find_chrome() -> Option<String> {
    let candidates = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
        "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
        "/usr/bin/google-chrome",
        "/usr/bin/chromium-browser",
        "/usr/bin/chromium",
    ];
    candidates.iter().find(|p| std::path::Path::new(p).exists()).map(|s| s.to_string())
}

async fn ensure_browser(state: &mut BrowserState) -> Result<(), String> {
    if state.browser.is_some() {
        return Ok(());
    }
    let chrome_path = find_chrome().ok_or("找不到 Chrome，请安装 Google Chrome")?;
    let config = BrowserConfig::builder()
        .chrome_executable(chrome_path)
        .with_head()
        .build()
        .map_err(|e| format!("BrowserConfig 构建失败: {e}"))?;
    let (browser, mut handler) = Browser::launch(config).await
        .map_err(|e| format!("Chrome 启动失败: {e}"))?;
    tokio::spawn(async move {
        while let Some(_) = handler.next().await {}
    });
    // open initial tab
    let page = browser.new_page("about:blank").await
        .map_err(|e| format!("新建页面失败: {e}"))?;
    state.browser = Some(browser);
    state.pages = vec![Arc::new(page)];
    state.active = 0;
    Ok(())
}

fn active_page(state: &BrowserState) -> Result<Arc<Page>, String> {
    state.pages.get(state.active).cloned().ok_or("没有活跃标签页".into())
}

// ── Tool implementations ───────────────────────────────────────────────────────

pub async fn browser_navigate(state: &SharedBrowserState, args: &Value) -> Value {
    let url = match args.get("url").and_then(|v| v.as_str()) {
        Some(u) => u.to_string(),
        None => return json!({"error": "缺少 url 参数"}),
    };
    let mut s = state.lock().await;
    if let Err(e) = ensure_browser(&mut s).await { return json!({"error": e}); }
    let page = match active_page(&s) { Ok(p) => p, Err(e) => return json!({"error": e}) };
    match page.goto(url.as_str()).await {
        Ok(_) => json!({"ok": true, "url": url}),
        Err(e) => json!({"error": format!("导航失败: {e}")}),
    }
}

pub async fn browser_screenshot(state: &SharedBrowserState, _args: &Value) -> Value {
    let s = state.lock().await;
    let page = match active_page(&s) { Ok(p) => p, Err(e) => return json!({"error": e}) };
    match page.screenshot(chromiumoxide::page::ScreenshotParams::builder().build()).await {
        Ok(bytes) => {
            let b64 = BASE64.encode(&bytes);
            json!({"type": "image", "data": b64, "mimeType": "image/png"})
        }
        Err(e) => json!({"error": format!("截图失败: {e}")}),
    }
}

pub async fn browser_click(state: &SharedBrowserState, args: &Value) -> Value {
    let selector = match args.get("selector").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return json!({"error": "缺少 selector 参数"}),
    };
    let s = state.lock().await;
    let page = match active_page(&s) { Ok(p) => p, Err(e) => return json!({"error": e}) };
    match page.find_element(selector.as_str()).await {
        Ok(el) => match el.click().await {
            Ok(_) => json!({"ok": true}),
            Err(e) => json!({"error": format!("点击失败: {e}")}),
        },
        Err(e) => json!({"error": format!("找不到元素 {selector}: {e}")}),
    }
}

pub async fn browser_tap(state: &SharedBrowserState, args: &Value) -> Value {
    // tap = click for CDP purposes
    browser_click(state, args).await
}

pub async fn browser_type(state: &SharedBrowserState, args: &Value) -> Value {
    let selector = match args.get("selector").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return json!({"error": "缺少 selector 参数"}),
    };
    let text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return json!({"error": "缺少 text 参数"}),
    };
    let s = state.lock().await;
    let page = match active_page(&s) { Ok(p) => p, Err(e) => return json!({"error": e}) };
    match page.find_element(selector.as_str()).await {
        Ok(el) => match el.type_str(text.as_str()).await {
            Ok(_) => json!({"ok": true}),
            Err(e) => json!({"error": format!("输入失败: {e}")}),
        },
        Err(e) => json!({"error": format!("找不到元素 {selector}: {e}")}),
    }
}

pub async fn browser_run_script(state: &SharedBrowserState, args: &Value) -> Value {
    let script = match args.get("script").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return json!({"error": "缺少 script 参数"}),
    };
    let s = state.lock().await;
    let page = match active_page(&s) { Ok(p) => p, Err(e) => return json!({"error": e}) };
    match page.evaluate(script.as_str()).await {
        Ok(result) => {
            let val: Value = result.into_value().unwrap_or(Value::Null);
            json!({"result": val})
        }
        Err(e) => json!({"error": format!("脚本执行失败: {e}")}),
    }
}

pub async fn browser_scroll(state: &SharedBrowserState, args: &Value) -> Value {
    let x = args.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let y = args.get("y").and_then(|v| v.as_f64()).unwrap_or(300.0);
    let script = format!("window.scrollBy({x}, {y})");
    let s = state.lock().await;
    let page = match active_page(&s) { Ok(p) => p, Err(e) => return json!({"error": e}) };
    match page.evaluate(script.as_str()).await {
        Ok(_) => json!({"ok": true}),
        Err(e) => json!({"error": format!("滚动失败: {e}")}),
    }
}

pub async fn browser_get_text(state: &SharedBrowserState, args: &Value) -> Value {
    let selector = args.get("selector").and_then(|v| v.as_str()).unwrap_or("body");
    let script = format!(
        "document.querySelector({:?})?.innerText ?? ''",
        selector
    );
    let s = state.lock().await;
    let page = match active_page(&s) { Ok(p) => p, Err(e) => return json!({"error": e}) };
    match page.evaluate(script.as_str()).await {
        Ok(result) => {
            let text = result.into_value::<String>().unwrap_or_default();
            json!({"text": text})
        }
        Err(e) => json!({"error": format!("获取文本失败: {e}")}),
    }
}

pub async fn browser_new_tab(state: &SharedBrowserState, args: &Value) -> Value {
    let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("about:blank");
    let mut s = state.lock().await;
    if let Err(e) = ensure_browser(&mut s).await { return json!({"error": e}); }
    let browser = s.browser.as_ref().unwrap();
    match browser.new_page(url).await {
        Ok(page) => {
            s.pages.push(Arc::new(page));
            s.active = s.pages.len() - 1;
            json!({"ok": true, "tab_index": s.active})
        }
        Err(e) => json!({"error": format!("新建标签页失败: {e}")}),
    }
}

pub async fn browser_close_tab(state: &SharedBrowserState, args: &Value) -> Value {
    let mut s = state.lock().await;
    if s.pages.is_empty() { return json!({"ok": true}); }
    let idx = args.get("tab_index").and_then(|v| v.as_u64()).unwrap_or(s.active as u64) as usize;
    if idx >= s.pages.len() { return json!({"error": "标签页索引越界"}); }
    let page = s.pages.remove(idx);
    let _ = Arc::try_unwrap(page).map(|p| async move { let _ = p.close().await; });
    if s.pages.is_empty() {
        s.active = 0;
    } else {
        s.active = s.active.min(s.pages.len() - 1);
    }
    json!({"ok": true})
}

pub async fn browser_switch_tab(state: &SharedBrowserState, args: &Value) -> Value {
    let idx = match args.get("tab_index").and_then(|v| v.as_u64()) {
        Some(i) => i as usize,
        None => return json!({"error": "缺少 tab_index 参数"}),
    };
    let mut s = state.lock().await;
    if idx >= s.pages.len() { return json!({"error": "标签页索引越界"}); }
    s.active = idx;
    json!({"ok": true, "tab_index": idx})
}

pub async fn browser_list_tabs(state: &SharedBrowserState, _args: &Value) -> Value {
    let s = state.lock().await;
    json!({"tabs": s.pages.len(), "active": s.active})
}

/// Accessibility snapshot — the most important tool for understanding page structure.
/// Returns a JSON tree of all interactive and visible elements with their roles, names, and selectors.
pub async fn browser_snapshot(state: &SharedBrowserState, _args: &Value) -> Value {
    let s = state.lock().await;
    let page = match active_page(&s) { Ok(p) => p, Err(e) => return json!({"error": e}) };
    // Use JavaScript to extract accessibility-relevant info from the DOM
    let script = r#"
(function() {
    function nodeInfo(el, depth) {
        if (depth > 10) return null;
        const tag = el.tagName ? el.tagName.toLowerCase() : '';
        const role = el.getAttribute?.('role') || '';
        const name = el.getAttribute?.('aria-label') || el.getAttribute?.('placeholder') ||
                     el.getAttribute?.('title') || el.textContent?.trim().slice(0, 80) || '';
        const id = el.id ? '#' + el.id : '';
        const cls = el.className && typeof el.className === 'string'
            ? '.' + el.className.trim().split(/\s+/).slice(0,2).join('.') : '';
        const selector = id || (tag + cls) || tag;
        const type_ = el.getAttribute?.('type') || '';
        const href = el.href || '';
        const value = el.value !== undefined ? el.value : '';

        const rect = el.getBoundingClientRect?.();
        const visible = rect && rect.width > 0 && rect.height > 0 &&
                        window.getComputedStyle(el).visibility !== 'hidden' &&
                        window.getComputedStyle(el).display !== 'none';
        if (!visible && el.children?.length === 0) return null;

        const interactive = ['a','button','input','select','textarea'].includes(tag) ||
                            role === 'button' || role === 'link' || role === 'textbox' ||
                            el.getAttribute?.('contenteditable') === 'true' ||
                            el.getAttribute?.('tabindex') !== null;

        const children = [];
        for (const child of (el.children || [])) {
            const c = nodeInfo(child, depth + 1);
            if (c) children.push(c);
        }

        if (!interactive && children.length === 0 && !name) return null;

        const node = { tag, selector };
        if (role) node.role = role;
        if (name) node.name = name;
        if (type_) node.type = type_;
        if (value) node.value = value;
        if (href) node.href = href;
        if (interactive) node.interactive = true;
        if (children.length) node.children = children;
        return node;
    }
    return JSON.stringify(nodeInfo(document.body, 0), null, 2);
})()
"#;
    match page.evaluate(script).await {
        Ok(v) => {
            let text = v.into_value::<String>().unwrap_or_default();
            json!({"snapshot": text})
        }
        Err(e) => json!({"error": format!("snapshot 失败: {e}")}),
    }
}

pub async fn browser_navigate_back(state: &SharedBrowserState, _args: &Value) -> Value {
    let s = state.lock().await;
    let page = match active_page(&s) { Ok(p) => p, Err(e) => return json!({"error": e}) };
    match page.evaluate("history.back()").await {
        Ok(_) => json!({"ok": true}),
        Err(e) => json!({"error": format!("返回失败: {e}")}),
    }
}

pub async fn browser_hover(state: &SharedBrowserState, args: &Value) -> Value {
    let selector = match args.get("selector").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return json!({"error": "缺少 selector 参数"}),
    };
    let s = state.lock().await;
    let page = match active_page(&s) { Ok(p) => p, Err(e) => return json!({"error": e}) };
    let script = format!("document.querySelector({:?})?.dispatchEvent(new MouseEvent('mouseover', {{bubbles:true}}))", selector);
    match page.evaluate(script.as_str()).await {
        Ok(_) => json!({"ok": true}),
        Err(e) => json!({"error": format!("hover 失败: {e}")}),
    }
}

pub async fn browser_press_key(state: &SharedBrowserState, args: &Value) -> Value {
    let key = match args.get("key").and_then(|v| v.as_str()) {
        Some(k) => k.to_string(),
        None => return json!({"error": "缺少 key 参数"}),
    };
    let s = state.lock().await;
    let page = match active_page(&s) { Ok(p) => p, Err(e) => return json!({"error": e}) };
    use chromiumoxide::cdp::browser_protocol::input::{DispatchKeyEventParams, DispatchKeyEventType};
    let params = DispatchKeyEventParams::builder()
        .r#type(DispatchKeyEventType::KeyDown)
        .key(key.clone())
        .build()
        .unwrap();
    let _ = page.execute(params).await;
    let params_up = DispatchKeyEventParams::builder()
        .r#type(DispatchKeyEventType::KeyUp)
        .key(key)
        .build()
        .unwrap();
    match page.execute(params_up).await {
        Ok(_) => json!({"ok": true}),
        Err(e) => json!({"error": format!("按键失败: {e}")}),
    }
}

pub async fn browser_select_option(state: &SharedBrowserState, args: &Value) -> Value {
    let selector = match args.get("selector").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return json!({"error": "缺少 selector 参数"}),
    };
    let value = match args.get("value").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => return json!({"error": "缺少 value 参数"}),
    };
    let s = state.lock().await;
    let page = match active_page(&s) { Ok(p) => p, Err(e) => return json!({"error": e}) };
    let script = format!(
        "var el=document.querySelector({:?}); if(el){{ el.value={:?}; el.dispatchEvent(new Event('change',{{bubbles:true}})); }}",
        selector, value
    );
    match page.evaluate(script.as_str()).await {
        Ok(_) => json!({"ok": true}),
        Err(e) => json!({"error": format!("选择失败: {e}")}),
    }
}

pub async fn browser_wait_for(state: &SharedBrowserState, args: &Value) -> Value {
    let ms = args.get("ms").and_then(|v| v.as_u64()).unwrap_or(1000);
    let text = args.get("text").and_then(|v| v.as_str()).map(|s| s.to_string());

    if let Some(t) = text {
        let s = state.lock().await;
        let page = match active_page(&s) { Ok(p) => p, Err(e) => return json!({"error": e}) };
        drop(s);
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(ms.max(10000));
        let script = format!("document.body.innerText.includes({:?})", t);
        loop {
            if std::time::Instant::now() > deadline { return json!({"ok": false, "reason": "timeout"}); }
            let found = page.evaluate(script.as_str()).await
                .ok()
                .and_then(|v| v.into_value::<bool>().ok())
                == Some(true);
            if found { return json!({"ok": true}); }
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
    } else {
        tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
        json!({"ok": true})
    }
}

pub async fn browser_resize(state: &SharedBrowserState, args: &Value) -> Value {
    let width = args.get("width").and_then(|v| v.as_u64()).unwrap_or(1280) as u32;
    let height = args.get("height").and_then(|v| v.as_u64()).unwrap_or(800) as u32;
    let s = state.lock().await;
    let page = match active_page(&s) { Ok(p) => p, Err(e) => return json!({"error": e}) };
    use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
    let params = SetDeviceMetricsOverrideParams::builder()
        .width(width)
        .height(height)
        .device_scale_factor(1.0)
        .mobile(false)
        .build()
        .unwrap();
    match page.execute(params).await {
        Ok(_) => json!({"ok": true, "width": width, "height": height}),
        Err(e) => json!({"error": format!("resize 失败: {e}")}),
    }
}

// ── Dispatch ──────────────────────────────────────────────────────────────────

pub async fn dispatch(state: &SharedBrowserState, tool: &str, args: &Value) -> Value {
    match tool {
        "browser_navigate"    => browser_navigate(state, args).await,
        "browser_screenshot"  => browser_screenshot(state, args).await,
        "browser_click"       => browser_click(state, args).await,
        "browser_tap"         => browser_tap(state, args).await,
        "browser_type"        => browser_type(state, args).await,
        "browser_run_script"  => browser_run_script(state, args).await,
        "browser_scroll"      => browser_scroll(state, args).await,
        "browser_get_text"    => browser_get_text(state, args).await,
        "browser_new_tab"        => browser_new_tab(state, args).await,
        "browser_close_tab"      => browser_close_tab(state, args).await,
        "browser_switch_tab"     => browser_switch_tab(state, args).await,
        "browser_list_tabs"      => browser_list_tabs(state, args).await,
        "browser_snapshot"       => browser_snapshot(state, args).await,
        "browser_navigate_back"  => browser_navigate_back(state, args).await,
        "browser_hover"          => browser_hover(state, args).await,
        "browser_press_key"      => browser_press_key(state, args).await,
        "browser_select_option"  => browser_select_option(state, args).await,
        "browser_wait_for"       => browser_wait_for(state, args).await,
        "browser_resize"         => browser_resize(state, args).await,
        _                        => json!({"error": format!("未知工具: {tool}")}),
    }
}

/// Tool definitions for MCP initialize response
pub fn tool_definitions() -> Vec<Value> {
    vec![
        json!({"name": "browser_navigate", "description": "导航到指定 URL", "inputSchema": {"type": "object", "properties": {"url": {"type": "string"}}, "required": ["url"]}}),
        json!({"name": "browser_screenshot", "description": "截取当前页面截图，返回 base64 PNG", "inputSchema": {"type": "object", "properties": {}}}),
        json!({"name": "browser_click", "description": "点击页面元素", "inputSchema": {"type": "object", "properties": {"selector": {"type": "string"}}, "required": ["selector"]}}),
        json!({"name": "browser_tap", "description": "触摸点击页面元素（mobile）", "inputSchema": {"type": "object", "properties": {"selector": {"type": "string"}}, "required": ["selector"]}}),
        json!({"name": "browser_type", "description": "在元素中输入文字", "inputSchema": {"type": "object", "properties": {"selector": {"type": "string"}, "text": {"type": "string"}}, "required": ["selector", "text"]}}),
        json!({"name": "browser_run_script", "description": "在页面中执行 JavaScript", "inputSchema": {"type": "object", "properties": {"script": {"type": "string"}}, "required": ["script"]}}),
        json!({"name": "browser_scroll", "description": "滚动页面", "inputSchema": {"type": "object", "properties": {"x": {"type": "number"}, "y": {"type": "number"}}}}),
        json!({"name": "browser_get_text", "description": "获取元素文本内容", "inputSchema": {"type": "object", "properties": {"selector": {"type": "string"}}}}),
        json!({"name": "browser_new_tab", "description": "新建标签页", "inputSchema": {"type": "object", "properties": {"url": {"type": "string"}}}}),
        json!({"name": "browser_close_tab", "description": "关闭标签页", "inputSchema": {"type": "object", "properties": {"tab_index": {"type": "integer"}}}}),
        json!({"name": "browser_switch_tab", "description": "切换到指定标签页", "inputSchema": {"type": "object", "properties": {"tab_index": {"type": "integer"}}, "required": ["tab_index"]}}),
        json!({"name": "browser_list_tabs", "description": "列出所有标签页", "inputSchema": {"type": "object", "properties": {}}}),
        json!({"name": "browser_snapshot", "description": "获取页面可访问性快照，返回页面所有可交互元素的结构树（selector、role、name），比截图更适合定位元素", "inputSchema": {"type": "object", "properties": {}}}),
        json!({"name": "browser_navigate_back", "description": "返回上一页", "inputSchema": {"type": "object", "properties": {}}}),
        json!({"name": "browser_hover", "description": "悬停在页面元素上", "inputSchema": {"type": "object", "properties": {"selector": {"type": "string"}}, "required": ["selector"]}}),
        json!({"name": "browser_press_key", "description": "按下键盘按键，如 Enter、Tab、Escape、ArrowDown 等", "inputSchema": {"type": "object", "properties": {"key": {"type": "string"}}, "required": ["key"]}}),
        json!({"name": "browser_select_option", "description": "选择下拉框选项", "inputSchema": {"type": "object", "properties": {"selector": {"type": "string"}, "value": {"type": "string"}}, "required": ["selector", "value"]}}),
        json!({"name": "browser_wait_for", "description": "等待指定文本出现在页面中，或等待指定毫秒数", "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}, "ms": {"type": "integer"}}}}),
        json!({"name": "browser_resize", "description": "调整浏览器窗口大小", "inputSchema": {"type": "object", "properties": {"width": {"type": "integer"}, "height": {"type": "integer"}}}}),
    ]
}
