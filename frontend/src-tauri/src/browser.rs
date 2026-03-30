/// Browser automation tools via chromiumoxide (CDP).
///
/// Exposes a `BrowserTools` struct implementing `agentix::Tool` via the `#[tool]` macro.
/// Chrome is launched lazily on first use and kept alive across calls.
use std::sync::Arc;

use agentix::tool;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chromiumoxide::{Browser, BrowserConfig, Page};
use futures::StreamExt;
use serde_json::json;
use tokio::sync::Mutex;

// ── State ─────────────────────────────────────────────────────────────────────

pub struct BrowserState {
    browser: Option<Browser>,
    pages: Vec<Arc<Page>>,
    active: usize,
}

pub type SharedBrowserState = Arc<Mutex<BrowserState>>;

pub fn new_browser_state() -> SharedBrowserState {
    Arc::new(Mutex::new(BrowserState { browser: None, pages: vec![], active: 0 }))
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
        while handler.next().await.is_some() {}
    });
    let page = browser.new_page("about:blank").await
        .map_err(|e| format!("新建页面失败: {e}"))?;
    state.browser = Some(browser);
    state.pages = vec![Arc::new(page)];
    state.active = 0;
    Ok(())
}

fn active_page(state: &BrowserState) -> Result<Arc<Page>, String> {
    state.pages.get(state.active).cloned().ok_or_else(|| "没有活跃标签页".into())
}

// ── Tools ─────────────────────────────────────────────────────────────────────

pub struct BrowserTools {
    pub state: SharedBrowserState,
}

#[tool]
impl agentix::Tool for BrowserTools {
    /// 导航到指定 URL，等待页面加载完成。
    /// url: 要访问的完整网址，如 https://www.example.com
    async fn browser_navigate(&self, url: String) -> Result<String, String> {
        let mut s = self.state.lock().await;
        ensure_browser(&mut s).await?;
        let page = active_page(&s)?;
        drop(s);
        page.goto(url.as_str()).await.map_err(|e| format!("导航失败: {e}"))?;
        let title = page.get_title().await.unwrap_or_default().unwrap_or_default();
        Ok(format!("已导航到 {url}，页面标题：{title}"))
    }

    /// 返回上一页（浏览器历史后退）。
    async fn browser_navigate_back(&self) -> Result<String, String> {
        let s = self.state.lock().await;
        let page = active_page(&s)?;
        drop(s);
        page.evaluate("history.back()").await.map_err(|e| format!("后退失败: {e}"))?;
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        Ok("已后退".into())
    }

    /// 获取页面可访问性快照，返回所有可见和可交互元素的结构树（tag、selector、role、name、value）。
    /// 这是理解页面结构、定位元素的首选工具，优先于截图。
    async fn browser_snapshot(&self) -> Result<String, String> {
        let s = self.state.lock().await;
        let page = active_page(&s)?;
        drop(s);
        // Extract a concise accessibility tree from the DOM via JS
        let script = r#"
(function snapshot(el, depth) {
    if (!el || depth > 8) return null;
    const tag = (el.tagName || '').toLowerCase();
    const skip = ['script','style','noscript','head','meta','link'];
    if (skip.includes(tag)) return null;

    const role = el.getAttribute && el.getAttribute('role') || '';
    const ariaLabel = el.getAttribute && el.getAttribute('aria-label') || '';
    const placeholder = el.getAttribute && el.getAttribute('placeholder') || '';
    const title = el.getAttribute && el.getAttribute('title') || '';
    const inputType = el.getAttribute && el.getAttribute('type') || '';
    const contentEditable = el.getAttribute && el.getAttribute('contenteditable');
    const ownText = Array.from(el.childNodes)
        .filter(n => n.nodeType === 3)
        .map(n => n.textContent.trim())
        .join(' ')
        .trim()
        .slice(0, 120);

    const name = ariaLabel || placeholder || title || ownText;

    // Visibility check
    const style = window.getComputedStyle(el);
    if (style.display === 'none' || style.visibility === 'hidden' || style.opacity === '0') return null;
    const rect = el.getBoundingClientRect();
    const isVisible = rect.width > 0 || rect.height > 0 || el.children.length > 0;
    if (!isVisible) return null;

    const interactive = ['a','button','input','select','textarea'].includes(tag)
        || role === 'button' || role === 'link' || role === 'textbox'
        || role === 'searchbox' || role === 'combobox' || role === 'checkbox'
        || role === 'menuitem' || role === 'option' || role === 'tab'
        || contentEditable === 'true' || contentEditable === '';

    const children = Array.from(el.children)
        .map(c => snapshot(c, depth + 1))
        .filter(Boolean);

    if (!interactive && children.length === 0 && !name) return null;

    const node = { tag };
    // Build a stable selector: prefer id, then data-* attrs, then tag+class
    if (el.id) node.selector = '#' + CSS.escape(el.id);
    else if (el.getAttribute && el.getAttribute('data-testid')) node.selector = `[data-testid="${el.getAttribute('data-testid')}"]`;
    else {
        const cls = el.className && typeof el.className === 'string'
            ? el.className.trim().split(/\s+/).slice(0, 2).join('.') : '';
        node.selector = cls ? `${tag}.${cls}` : tag;
    }
    if (role) node.role = role;
    if (name) node.name = name;
    if (inputType) node.type = inputType;
    if (el.value !== undefined && el.value !== '') node.value = String(el.value).slice(0, 80);
    if (el.href) node.href = el.href.split('?')[0];
    if (interactive) node.interactive = true;
    if (children.length) node.children = children;
    return node;
})(document.body, 0)
"#;
        let v = page.evaluate(script).await.map_err(|e| format!("snapshot 失败: {e}"))?;
        let tree: Value = v.into_value().map_err(|e| format!("snapshot 解析失败: {e}"))?;
        Ok(serde_json::to_string_pretty(&tree).unwrap_or_default())
    }

    /// 截取当前页面截图，返回 base64 编码的 PNG 图片。
    async fn browser_screenshot(&self) -> Result<Value, String> {
        let s = self.state.lock().await;
        let page = active_page(&s)?;
        drop(s);
        let bytes = page.screenshot(chromiumoxide::page::ScreenshotParams::builder().build())
            .await
            .map_err(|e| format!("截图失败: {e}"))?;
        let b64 = BASE64.encode(&bytes);
        Ok(json!({"type": "image", "data": b64, "mimeType": "image/png"}))
    }

    /// 点击页面元素。支持 CSS selector、aria-label、placeholder、可见文字多种定位方式。
    /// 先尝试 CSS selector，失败则按 aria-label / 可见文字回退，点击后等待 300ms。
    /// selector: CSS 选择器，或元素的 aria-label / 可见文本
    async fn browser_click(&self, selector: String) -> Result<String, String> {
        let s = self.state.lock().await;
        let page = active_page(&s)?;
        drop(s);

        // JS: try selector first, then aria-label, then visible text match
        let script = format!(r#"
(function() {{
    const sel = {sel:?};

    // 1. Direct CSS selector
    let el = null;
    try {{ el = document.querySelector(sel); }} catch(_) {{}}

    // 2. aria-label / placeholder / title match
    if (!el) {{
        el = document.querySelector(`[aria-label="${{sel}}"]`)
          || document.querySelector(`[placeholder="${{sel}}"]`)
          || document.querySelector(`[title="${{sel}}"]`);
    }}

    // 3. Visible text match (button, a, span, div with role)
    if (!el) {{
        const all = document.querySelectorAll('button,a,[role="button"],[role="link"],[role="menuitem"],[role="tab"],input,textarea');
        for (const node of all) {{
            const t = (node.getAttribute('aria-label') || node.textContent || '').trim();
            if (t === sel || t.includes(sel)) {{ el = node; break; }}
        }}
    }}

    if (!el) return {{ ok: false, error: '找不到元素: ' + sel }};

    // Scroll into view
    el.scrollIntoView({{ block: 'center', behavior: 'instant' }});

    // Dispatch realistic mouse events
    const rect = el.getBoundingClientRect();
    const cx = rect.left + rect.width / 2;
    const cy = rect.top + rect.height / 2;
    const opts = {{ bubbles: true, cancelable: true, clientX: cx, clientY: cy }};
    el.dispatchEvent(new MouseEvent('mouseover', opts));
    el.dispatchEvent(new MouseEvent('mousedown', opts));
    el.dispatchEvent(new MouseEvent('mouseup', opts));
    el.dispatchEvent(new MouseEvent('click', opts));
    if (el.focus) el.focus();

    return {{ ok: true, tag: el.tagName, selector: sel }};
}})()
"#, sel = selector);

        let v = page.evaluate(script.as_str()).await
            .map_err(|e| format!("click 执行失败: {e}"))?;
        let result: Value = v.into_value().map_err(|e| format!("click 结果解析失败: {e}"))?;
        if result.get("ok") == Some(&Value::Bool(true)) {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            Ok(format!("已点击: {selector}"))
        } else {
            Err(result["error"].as_str().unwrap_or("点击失败").to_string())
        }
    }

    /// 在输入框中输入文字。先聚焦元素，清空现有内容，然后逐字符输入触发 input/change 事件。
    /// selector: CSS 选择器或 aria-label / placeholder 文字
    /// text: 要输入的文字
    async fn browser_type(&self, selector: String, text: String) -> Result<String, String> {
        let s = self.state.lock().await;
        let page = active_page(&s)?;
        drop(s);

        let script = format!(r#"
(function() {{
    const sel = {sel:?};
    let el = null;
    try {{ el = document.querySelector(sel); }} catch(_) {{}}
    if (!el) el = document.querySelector(`[aria-label="${{sel}}"]`)
                || document.querySelector(`[placeholder="${{sel}}"]`);

    // Also search by placeholder text directly
    if (!el) {{
        const inputs = document.querySelectorAll('input,textarea,[contenteditable]');
        for (const node of inputs) {{
            if ((node.getAttribute('placeholder') || '') === sel) {{ el = node; break; }}
        }}
    }}

    if (!el) return {{ ok: false, error: '找不到输入元素: ' + sel }};

    el.scrollIntoView({{ block: 'center', behavior: 'instant' }});
    el.focus();

    // Clear existing value
    if (el.isContentEditable) {{
        el.textContent = '';
    }} else {{
        const nativeInputValueSetter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')?.set
            || Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, 'value')?.set;
        if (nativeInputValueSetter) nativeInputValueSetter.call(el, '');
        else el.value = '';
    }}
    el.dispatchEvent(new Event('input', {{ bubbles: true }}));

    return {{ ok: true }};
}})()
"#, sel = selector);

        let v = page.evaluate(script.as_str()).await
            .map_err(|e| format!("type 准备失败: {e}"))?;
        let result: Value = v.into_value().map_err(|e| format!("type 结果解析失败: {e}"))?;
        if result.get("ok") != Some(&Value::Bool(true)) {
            return Err(result["error"].as_str().unwrap_or("找不到输入元素").to_string());
        }

        // Type each character via CDP Input.dispatchKeyEvent for realistic input
        use chromiumoxide::cdp::browser_protocol::input::{DispatchKeyEventParams, DispatchKeyEventType};
        for ch in text.chars() {
            let s = ch.to_string();
            let down = DispatchKeyEventParams::builder()
                .r#type(DispatchKeyEventType::KeyDown)
                .text(s.clone())
                .build().unwrap();
            let _ = page.execute(down).await;
            let char_ev = DispatchKeyEventParams::builder()
                .r#type(DispatchKeyEventType::Char)
                .text(s.clone())
                .build().unwrap();
            let _ = page.execute(char_ev).await;
            let up = DispatchKeyEventParams::builder()
                .r#type(DispatchKeyEventType::KeyUp)
                .text(s)
                .build().unwrap();
            let _ = page.execute(up).await;
        }

        // Trigger React/Vue synthetic events via JS after typing
        let flush = format!(r#"
(function() {{
    const sel = {sel:?};
    let el = null;
    try {{ el = document.querySelector(sel); }} catch(_) {{}}
    if (!el) el = document.querySelector(`[aria-label="${{sel}}"]`)
                || document.querySelector(`[placeholder="${{sel}}"]`);
    if (el) {{
        el.dispatchEvent(new Event('input', {{ bubbles: true }}));
        el.dispatchEvent(new Event('change', {{ bubbles: true }}));
    }}
}})()
"#, sel = selector);
        let _ = page.evaluate(flush.as_str()).await;

        Ok(format!("已在 {selector} 输入: {text}"))
    }

    /// 执行 JavaScript 表达式，返回结果。
    /// script: 要执行的 JavaScript 代码
    async fn browser_run_script(&self, script: String) -> Result<Value, String> {
        let s = self.state.lock().await;
        let page = active_page(&s)?;
        drop(s);
        let v = page.evaluate(script.as_str()).await
            .map_err(|e| format!("脚本执行失败: {e}"))?;
        v.into_value::<Value>().map_err(|e| format!("结果解析失败: {e}"))
    }

    /// 滚动页面。x/y 为像素偏移量，正值向右/下，负值向左/上。
    /// x: 水平滚动量（像素）
    /// y: 垂直滚动量（像素）
    async fn browser_scroll(&self, x: f64, y: f64) -> Result<String, String> {
        let s = self.state.lock().await;
        let page = active_page(&s)?;
        drop(s);
        let script = format!("window.scrollBy({x}, {y})");
        page.evaluate(script.as_str()).await.map_err(|e| format!("滚动失败: {e}"))?;
        Ok(format!("已滚动 x={x} y={y}"))
    }

    /// 获取页面元素的文本内容。selector 为空时返回整个页面正文。
    /// selector: CSS 选择器，为空则返回整个 body 文本
    async fn browser_get_text(&self, selector: String) -> Result<String, String> {
        let s = self.state.lock().await;
        let page = active_page(&s)?;
        drop(s);
        let script = if selector.is_empty() {
            "document.body.innerText".to_string()
        } else {
            format!("document.querySelector({:?})?.innerText || ''", selector)
        };
        let v = page.evaluate(script.as_str()).await
            .map_err(|e| format!("获取文本失败: {e}"))?;
        v.into_value::<String>().map_err(|e| format!("文本解析失败: {e}"))
    }

    /// 悬停在页面元素上，触发 mouseover/mouseenter 事件。
    /// selector: CSS 选择器
    async fn browser_hover(&self, selector: String) -> Result<String, String> {
        let s = self.state.lock().await;
        let page = active_page(&s)?;
        drop(s);
        let script = format!(r#"
(function() {{
    const el = document.querySelector({sel:?});
    if (!el) return {{ ok: false }};
    el.scrollIntoView({{ block: 'center' }});
    const rect = el.getBoundingClientRect();
    const opts = {{ bubbles: true, clientX: rect.left + rect.width/2, clientY: rect.top + rect.height/2 }};
    el.dispatchEvent(new MouseEvent('mouseover', opts));
    el.dispatchEvent(new MouseEvent('mouseenter', opts));
    el.dispatchEvent(new MouseEvent('mousemove', opts));
    return {{ ok: true }};
}})()
"#, sel = selector);
        page.evaluate(script.as_str()).await.map_err(|e| format!("hover 失败: {e}"))?;
        Ok(format!("已悬停: {selector}"))
    }

    /// 按下键盘按键。常用键名：Enter、Tab、Escape、Backspace、ArrowDown、ArrowUp 等。
    /// key: 键名，符合 DOM KeyboardEvent.key 规范
    async fn browser_press_key(&self, key: String) -> Result<String, String> {
        let s = self.state.lock().await;
        let page = active_page(&s)?;
        drop(s);
        use chromiumoxide::cdp::browser_protocol::input::{DispatchKeyEventParams, DispatchKeyEventType};
        let down = DispatchKeyEventParams::builder()
            .r#type(DispatchKeyEventType::KeyDown)
            .key(key.clone())
            .build().unwrap();
        let _ = page.execute(down).await;
        let up = DispatchKeyEventParams::builder()
            .r#type(DispatchKeyEventType::KeyUp)
            .key(key.clone())
            .build().unwrap();
        page.execute(up).await.map_err(|e| format!("按键失败: {e}"))?;
        Ok(format!("已按键: {key}"))
    }

    /// 选择下拉框（select 元素）的选项。
    /// selector: select 元素的 CSS 选择器
    /// value: 选项的 value 值
    async fn browser_select_option(&self, selector: String, value: String) -> Result<String, String> {
        let s = self.state.lock().await;
        let page = active_page(&s)?;
        drop(s);
        let script = format!(r#"
(function() {{
    const el = document.querySelector({sel:?});
    if (!el) return false;
    el.value = {val:?};
    el.dispatchEvent(new Event('change', {{ bubbles: true }}));
    el.dispatchEvent(new Event('input', {{ bubbles: true }}));
    return true;
}})()
"#, sel = selector, val = value);
        let v = page.evaluate(script.as_str()).await.map_err(|e| format!("select 失败: {e}"))?;
        let ok = v.into_value::<bool>().unwrap_or(false);
        if ok { Ok(format!("已选择 {selector} = {value}")) } else { Err(format!("找不到元素: {selector}")) }
    }

    /// 等待页面中出现指定文字，或等待指定毫秒数。超时返回失败。
    /// text: 等待出现的文字（可选）
    /// ms: 等待毫秒数，同时也是超时时间（默认 5000）
    async fn browser_wait_for(&self, text: Option<String>, ms: Option<u64>) -> Result<String, String> {
        let timeout_ms = ms.unwrap_or(5000);
        if let Some(t) = text {
            let s = self.state.lock().await;
            let page = active_page(&s)?;
            drop(s);
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
            let script = format!("document.body.innerText.includes({:?})", t);
            loop {
                if std::time::Instant::now() > deadline {
                    return Err(format!("等待超时：未找到文字 '{t}'"));
                }
                let found = page.evaluate(script.as_str()).await
                    .ok()
                    .and_then(|v| v.into_value::<bool>().ok())
                    == Some(true);
                if found { return Ok(format!("已找到: {t}")); }
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            }
        } else {
            tokio::time::sleep(std::time::Duration::from_millis(timeout_ms)).await;
            Ok(format!("已等待 {timeout_ms}ms"))
        }
    }

    /// 调整浏览器视口大小。
    /// width: 宽度（像素，默认 1280）
    /// height: 高度（像素，默认 800）
    async fn browser_resize(&self, width: Option<u32>, height: Option<u32>) -> Result<String, String> {
        let w = width.unwrap_or(1280);
        let h = height.unwrap_or(800);
        let s = self.state.lock().await;
        let page = active_page(&s)?;
        drop(s);
        use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
        let params = SetDeviceMetricsOverrideParams::builder()
            .width(w).height(h).device_scale_factor(1.0).mobile(false)
            .build().unwrap();
        page.execute(params).await.map_err(|e| format!("resize 失败: {e}"))?;
        Ok(format!("视口已设为 {w}x{h}"))
    }

    /// 新建标签页，可选导航到指定 URL。
    /// url: 新标签页的初始 URL（可选，默认 about:blank）
    async fn browser_new_tab(&self, url: Option<String>) -> Result<String, String> {
        let mut s = self.state.lock().await;
        ensure_browser(&mut s).await?;
        let browser = s.browser.as_ref().ok_or("浏览器未启动")?;
        let target_url = url.as_deref().unwrap_or("about:blank");
        let page = browser.new_page(target_url).await
            .map_err(|e| format!("新建标签页失败: {e}"))?;
        let idx = s.pages.len();
        s.pages.push(Arc::new(page));
        s.active = idx;
        Ok(format!("已新建标签页 {idx}，URL: {target_url}"))
    }

    /// 关闭指定标签页。
    /// tab_index: 标签页索引（从 0 开始），不指定则关闭当前标签页
    async fn browser_close_tab(&self, tab_index: Option<usize>) -> Result<String, String> {
        let mut s = self.state.lock().await;
        let idx = tab_index.unwrap_or(s.active);
        if idx >= s.pages.len() {
            return Err(format!("标签页 {idx} 不存在"));
        }
        s.pages.remove(idx);
        if s.pages.is_empty() {
            s.active = 0;
        } else {
            s.active = s.active.min(s.pages.len() - 1);
        }
        Ok(format!("已关闭标签页 {idx}"))
    }

    /// 切换到指定标签页。
    /// tab_index: 目标标签页索引（从 0 开始）
    async fn browser_switch_tab(&self, tab_index: usize) -> Result<String, String> {
        let mut s = self.state.lock().await;
        if tab_index >= s.pages.len() {
            return Err(format!("标签页 {tab_index} 不存在，共 {} 个标签页", s.pages.len()));
        }
        s.active = tab_index;
        Ok(format!("已切换到标签页 {tab_index}"))
    }

    /// 列出所有标签页及当前活跃标签页。
    async fn browser_list_tabs(&self) -> Value {
        let s = self.state.lock().await;
        json!({ "total": s.pages.len(), "active": s.active })
    }
}
