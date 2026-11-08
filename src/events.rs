use serde::{Deserialize, Serialize};
use url::form_urlencoded;

pub const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
--disable-component-update --disable-domain-reliability --disable-background-networking \
--renderer-process-limit=4 \
--disable-gpu";

#[derive(Clone, Debug)]
pub enum UserEvent {
    ChromeIpc(String),
    TitleChanged(u64, String),
    PageStarted(u64, String),
    PageFinished(u64, String),
    NewWindow(String),
    ListsReady,
    BlocksChanged(u64),
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case", tag = "cmd")]
pub enum ChromeMsg {
    Navigate { input: String },
    Back,
    Forward,
    Reload,
    NewTab,
    CloseActiveTab,
    CloseTab { id: u64 },
    SelectTab { id: u64 },
    ToggleBookmark,
    OpenPanel,
    ClosePanel,
    RemoveBookmark { url: String },
    ClearHistory,
}

pub fn parse_ipc(raw: &str) -> Option<ChromeMsg> {
    serde_json::from_str(raw).ok()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChromeTab {
    pub id: u64,
    pub title: String,
    pub url: String,
    pub active: bool,
    pub can_back: bool,
    pub can_forward: bool,
    pub blocked: u64,
    pub bookmarked: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelEntry {
    pub url: String,
    pub title: String,
    pub ts: i64,
    pub time: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelData {
    pub bookmarks: Vec<PanelEntry>,
    pub history: Vec<PanelEntry>,
}

pub fn to_url(input: &str) -> String {
    let i = input.trim();
    if i.is_empty() {
        return String::new();
    }
    let lower = i.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        return i.to_string();
    }
    if lower == "localhost" || lower.starts_with("localhost/") {
        return format!("https://{i}");
    }
    let is_host = (i.contains('.') || (i.contains(':') && (i.len() < 2 || !i.as_bytes()[1].is_ascii_alphabetic()))) && !i.contains(' ');
    if is_host {
        format!("https://{i}")
    } else {
        let q = form_urlencoded::byte_serialize(i.as_bytes()).collect::<String>();
        format!("https://www.bing.com/search?q={q}")
    }
}
