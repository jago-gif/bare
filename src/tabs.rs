use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use wry::{
    MemoryUsageLevel, NewWindowResponse, PageLoadEvent, Rect, WebContext, WebView, WebViewBuilder,
    WebViewBuilderExtWindows, WebViewExtWindows,
    dpi::{PhysicalPosition, PhysicalSize},
};
use tao::event_loop::EventLoopProxy;

use crate::blocker;
use crate::chrome;
use crate::events::{self, ChromeMsg, ChromeTab, PanelData, PanelEntry, UserEvent};
use crate::net_interceptor;
use crate::store;

const CHROME_H: f64 = 44.0;
const PANEL_H: f64 = 260.0;
/// pestanas inactivas se hibernan (about:blank) tras este tiempo sin uso;
/// se restauran recargando su URL al seleccionarlas.
const HIBERNATE_AFTER: Duration = Duration::from_secs(45);

struct Tab {
    webview: WebView,
    title: String,
    url: String,
    top_url: Rc<RefCell<String>>,
    hibernated: bool,
    last_active: Instant,
}

pub struct AppState {
    window: tao::window::Window,
    chrome: WebView,
    proxy: EventLoopProxy<UserEvent>,
    context: WebContext,
    store: store::Store,
    tabs: HashMap<u64, Tab>,
    order: Vec<u64>,
    active: u64,
    next_id: u64,
    panel_open: bool,
}

impl AppState {
    pub fn new(
        window: tao::window::Window,
        chrome: WebView,
        proxy: EventLoopProxy<UserEvent>,
        context: WebContext,
        store: store::Store,
    ) -> Self {
        Self {
            window,
            chrome,
            proxy,
            context,
            store,
            tabs: HashMap::new(),
            order: Vec::new(),
            active: 0,
            next_id: 1,
            panel_open: false,
        }
    }

    fn chrome_h(&self) -> i32 {
        let mut h = (CHROME_H * self.window.scale_factor()).round() as i32;
        if self.panel_open {
            h += (PANEL_H * self.window.scale_factor()).round() as i32;
        }
        h
    }

    fn rect_chrome(&self) -> Rect {
        let s = self.window.inner_size();
        Rect {
            position: PhysicalPosition::new(0, 0).into(),
            size: PhysicalSize::new(s.width as i32, self.chrome_h()).into(),
        }
    }

    fn rect_content(&self) -> Rect {
        let s = self.window.inner_size();
        Rect {
            position: PhysicalPosition::new(0, self.chrome_h()).into(),
            size: PhysicalSize::new(s.width as i32, s.height as i32 - self.chrome_h()).into(),
        }
    }

    pub fn new_tab(&mut self, url: Option<String>) -> wry::Result<()> {
        let id = self.next_id;
        self.next_id += 1;

        let proxy_title = self.proxy.clone();
        let proxy_load = self.proxy.clone();
        let proxy_window = self.proxy.clone();
        let proxy_blocks = self.proxy.clone();
        let bounds = self.rect_content();

        let top_url: Rc<RefCell<String>> = Rc::new(RefCell::new(url.clone().unwrap_or_default()));
        let top_url_load = top_url.clone();

        let base = WebViewBuilder::new_with_web_context(&mut self.context)
            .with_bounds(bounds)
            .with_general_autofill_enabled(false)
            .with_additional_browser_args(events::BROWSER_ARGS)
            .with_document_title_changed_handler(move |title| {
                let _ = proxy_title.send_event(UserEvent::TitleChanged(id, title));
            })
            .with_on_page_load_handler(move |ev, url| {
                *top_url_load.borrow_mut() = url.clone();
                match ev {
                    PageLoadEvent::Started => {
                        let _ = proxy_load.send_event(UserEvent::PageStarted(id, url));
                    }
                    PageLoadEvent::Finished => {
                        let _ = proxy_load.send_event(UserEvent::PageFinished(id, url));
                    }
                }
            })
            .with_new_window_req_handler(move |url, _| {
                let _ = proxy_window.send_event(UserEvent::NewWindow(url));
                NewWindowResponse::Deny
            });

        let builder = if let Some(u) = url.clone() {
            base.with_url(u)
        } else {
            base.with_html(chrome::home_html())
        };

        let webview = builder.build_as_child(&self.window)?;

        net_interceptor::install(&webview, id, top_url.clone(), proxy_blocks);

        if let Some(prev) = self.tabs.get(&self.active) {
            let _ = prev.webview.set_visible(false);
        }
        self.tabs.insert(
            id,
            Tab {
                webview,
                title: String::new(),
                url: url.unwrap_or_default(),
                top_url,
                hibernated: false,
                last_active: Instant::now(),
            },
        );
        self.order.push(id);
        self.active = id;
        self.sync_chrome();
        self.set_background(false);
        Ok(())
    }

    pub fn close_tab(&mut self, id: u64) {
        if let Some(pos) = self.order.iter().position(|&i| i == id) {
            self.order.remove(pos);
            if let Some(tab) = self.tabs.remove(&id) {
                let _ = tab.webview.set_visible(false);
            }
            blocker::forget_count(id);
            self.active = *self.order.last().unwrap_or(&0);
            if let Some(tab) = self.tabs.get(&self.active) {
                let _ = tab.webview.set_visible(true);
                let _ = tab.webview.focus();
            }
            if self.order.is_empty() {
                    let _ = self.new_tab(None);
            }
            self.sync_chrome();
        }
    }

    pub fn select_tab(&mut self, id: u64) {
        if self.active != id && self.tabs.contains_key(&id) {
            let now = Instant::now();
            if let Some(prev) = self.tabs.get_mut(&self.active) {
                prev.last_active = now;
                let _ = prev.webview.set_visible(false);
                let _ = prev.webview.set_memory_usage_level(MemoryUsageLevel::Low);
            }
            self.active = id;
            if let Some(tab) = self.tabs.get_mut(&id) {
                if tab.hibernated {
                    tab.hibernated = false;
                    // restauracion: reload de la URL guardada
                    let url = tab.url.clone();
                    let url = if url.starts_with("http") { url } else { "about:blank".to_string() };
                    let _ = tab.webview.load_url(&url);
                }
                tab.last_active = now;
                let _ = tab.webview.set_visible(true);
                let _ = tab.webview.set_memory_usage_level(MemoryUsageLevel::Normal);
                let _ = tab.webview.focus();
            }
            self.sync_chrome();
        }
    }

    pub fn set_background(&mut self, unfocused: bool) {
        log::info!("set_background(unfocused={unfocused})");
        let background = if unfocused {
            MemoryUsageLevel::Low
        } else {
            MemoryUsageLevel::Normal
        };
        for (id, tab) in &self.tabs {
            let level = if *id == self.active {
                background
            } else {
                MemoryUsageLevel::Low
            };
            let _ = tab.webview.set_memory_usage_level(level);
        }
        // sin foco no hay UI activa esperando: hibernar todo lo inmediatamente
        // inactivo. Con foco, se respeta el temporizador de HIBERNATE_AFTER.
        if unfocused {
            self.hibernate_now();
        }
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        let mut min: Option<Instant> = None;
        for (id, tab) in &self.tabs {
            if *id == self.active || tab.hibernated || tab.url.is_empty() {
                continue;
            }
            let d = tab.last_active + HIBERNATE_AFTER;
            min = Some(match min {
                Some(m) => m.min(d),
                None => d,
            });
        }
        min
    }

    pub fn hibernate_idle(&mut self) {
        let now = Instant::now();
        let active = self.active;
        let mut due: Vec<u64> = Vec::new();
        for (id, t) in &self.tabs {
            if *id != active
                && !t.hibernated
                && !t.url.is_empty()
                && now.duration_since(t.last_active) > HIBERNATE_AFTER
            {
                due.push(*id);
            }
        }
        self.hibernate_internal(due);
    }

    pub fn hibernate_now(&mut self) {
        let active = self.active;
        let mut due: Vec<u64> = Vec::new();
        for (id, t) in &self.tabs {
            if *id != active && !t.hibernated && !t.url.is_empty() {
                due.push(*id);
            }
        }
        self.hibernate_internal(due);
    }

    fn hibernate_internal(&mut self, ids: Vec<u64>) {
        if ids.is_empty() {
            return;
        }
        for id in ids {
            if let Some(tab) = self.tabs.get_mut(&id) {
                tab.hibernated = true;
                log::info!("hibernando pestaña {id}: {}", tab.url);
                let _ = tab.webview.load_url("about:blank");
            }
        }
        self.sync_chrome();
    }

    pub fn handle(&mut self, ev: UserEvent) {
        match ev {
            UserEvent::ChromeIpc(raw) => self.handle_ipc(&raw),
            UserEvent::ListsReady => {
                blocker::on_lists_ready();
                log::debug!("adblock listo; sincronizando pestañas");
                self.sync_chrome();
            }
            UserEvent::TitleChanged(id, title) => {
                if let Some(tab) = self.tabs.get_mut(&id) {
                    tab.title = title.clone();
                }
                if id == self.active {
                    if title.is_empty() {
                        self.window.set_title("Bare");
                    } else {
                        self.window.set_title(&format!("{title} — Bare"));
                    }
                }
                self.sync_chrome();            }
            UserEvent::PageStarted(id, url) => {
                blocker::reset_count(id);
                self.update_url(id, url);
                if id == self.active {
                    let _ = self.chrome.evaluate_script("setProgress(true);");
                }
            }
            UserEvent::PageFinished(id, url) => {
                if let Some(tab) = self.tabs.get(&id) {
                    if url.starts_with("http") {
                        self.store.add_history(&url, &tab.title);
                        if self.panel_open {
                            self.send_panel_update();
                        }
                    }
                }
                if id == self.active {
                    let _ = self.chrome.evaluate_script("setProgress(false);");
                }
                self.inject_cosmetics(id, &url);
                self.update_url(id, url);
            }
            UserEvent::BlocksChanged(id) => {
                if id == self.active {
                    self.sync_chrome();
                }
            }
            UserEvent::NewWindow(url) => {
                let _ = self.new_tab(Some(url));
            }
            UserEvent::Shortcut(s) => match s {
                events::Shortcut::NewTab => {
                    let _ = self.new_tab(None);
                }
                events::Shortcut::CloseTab => self.close_tab(self.active),
                events::Shortcut::FocusBar => {
                    let _ = self.chrome.evaluate_script("focusOmnibox();");
                }
                events::Shortcut::Reload => {
                    if let Some(tab) = self.tabs.get(&self.active) {
                        let _ = tab.webview.reload();
                    }
                }
            },
        }
    }

    fn inject_cosmetics(&mut self, id: u64, url: &str) {
        if !url.starts_with("http") {
            return;
        }
        let Some(tab) = self.tabs.get(&id) else {
            return;
        };
        if !blocker::ready() {
            return;
        }
        let (css, script) = blocker::cosmetics(url);
        let mut js = String::new();
        if !css.is_empty() {
            let css_json = serde_json::to_string(&css).unwrap_or_default();
            js.push_str(&format!(
                "(function(){{var e=document.createElement('style');e.textContent={css_json};document.documentElement.appendChild(e);}})()"
            ));
        }
        if !script.is_empty() {
            if !js.is_empty() {
                js.push(';');
            }
            js.push_str("(function(){try{");
            js.push_str(&script);
            js.push_str("}catch(e){}})()");
        }
        if !js.is_empty() {
            let _ = tab.webview.evaluate_script(&js);
        }
    }

    fn update_url(&mut self, id: u64, url: String) {
        if url.starts_with("http") {
            if let Some(tab) = self.tabs.get_mut(&id) {
                tab.url = url;
            }
            self.sync_chrome();
        }
    }

    fn handle_ipc(&mut self, raw: &str) {
        let Some(msg) = events::parse_ipc(raw) else {
            return;
        };
        match msg {
            ChromeMsg::Navigate { input } => {
                let url = events::to_url(&input);
                if !url.is_empty() {
                    if let Some(tab) = self.tabs.get_mut(&self.active) {
                        tab.top_url.borrow_mut().replace_range(.., &url);
                        let _ = tab.webview.load_url(&url);
                    }
                }
            }
            ChromeMsg::Back | ChromeMsg::Forward | ChromeMsg::Reload => {
                if let Some(tab) = self.tabs.get(&self.active) {
                    match msg {
                        ChromeMsg::Back => {
                            let _ = tab.webview.go_back();
                        }
                        ChromeMsg::Forward => {
                            let _ = tab.webview.go_forward();
                        }
                        _ => {
                            let _ = tab.webview.reload();
                        }
                    }
                }
            }
            ChromeMsg::NewTab => {
                let _ = self.new_tab(None);
            }
            ChromeMsg::CloseActiveTab => {
                let id = self.active;
                self.close_tab(id);
            }
            ChromeMsg::CloseTab { id } => self.close_tab(id),
            ChromeMsg::SelectTab { id } => self.select_tab(id),
            ChromeMsg::ToggleBookmark => {
                if let Some(tab) = self.tabs.get(&self.active) {
                    let url = tab.url.clone();
                    if url.starts_with("http") {
                        self.store.toggle_bookmark(&url, &tab.title);
                        self.sync_chrome();
                    }
                }
            }
            ChromeMsg::OpenPanel => {
                self.panel_open = true;
                self.send_panel_update();
                let _ = self.chrome.evaluate_script("setPanel(true);");
                self.layout();
            }
            ChromeMsg::ClosePanel => {
                self.panel_open = false;
                let _ = self.chrome.evaluate_script("setPanel(false);");
                self.layout();
            }
            ChromeMsg::RemoveBookmark { url } => {
                self.store.remove_bookmark(&url);
                if self.panel_open {
                    self.send_panel_update();
                }
                self.sync_chrome();
            }
            ChromeMsg::ClearHistory => {
                self.store.clear_history();
                if self.panel_open {
                    self.send_panel_update();
                }
            }
        }
    }

    fn panel_entry(e: store::Entry) -> PanelEntry {
        PanelEntry {
            url: e.url,
            title: e.title,
            ts: e.ts,
            time: store::format_ts(e.ts),
        }
    }

    fn send_panel_update(&self) {
        let data = PanelData {
            bookmarks: self.store.bookmarks().into_iter().map(Self::panel_entry).collect(),
            history: self.store.history(30).into_iter().map(Self::panel_entry).collect(),
        };
        let json = serde_json::to_string(&data).unwrap_or_else(|_| r#"{}"#.to_string());
        let _ = self.chrome.evaluate_script(&format!("updatePanel({json});"));
    }

    fn sync_chrome(&self) {
        let mut data = Vec::with_capacity(self.order.len());
        for &id in &self.order {
            let tab = &self.tabs[&id];
            let (can_back, can_forward) = if id == self.active {
                (
                    tab.webview.can_go_back().unwrap_or(false),
                    tab.webview.can_go_forward().unwrap_or(false),
                )
            } else {
                (false, false)
            };
            data.push(ChromeTab {
                id,
                title: tab.title.clone(),
                url: tab.url.clone(),
                active: id == self.active,
                can_back,
                can_forward,
                blocked: blocker::count(id),
                bookmarked: self.store.is_bookmarked(&tab.url),
            });
        }
        let json = serde_json::to_string(&data).unwrap_or_else(|_| "[]".to_string());
        let js = format!("updateChrome({json});");
        let _ = self.chrome.evaluate_script(&js);
    }

    pub fn layout(&self) {
        let _ = self.chrome.set_bounds(self.rect_chrome());
        let rect = self.rect_content();
        for tab in self.tabs.values() {
            let _ = tab.webview.set_bounds(rect);
        }
    }
}
