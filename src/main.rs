#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod blocker;
mod chrome;
mod events;
mod net_interceptor;
mod store;
mod tabs;

use tao::{
    dpi::LogicalSize,
    event::{Event, StartCause, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    window::WindowBuilder,
};
use wry::{WebContext, WebViewBuilderExtWindows};

use events::UserEvent;

fn window_icon() -> tao::window::Icon {
    const RGBA: &[u8] = include_bytes!("../assets/icon.window.rgba");
    tao::window::Icon::from_rgba(RGBA.to_vec(), 32, 32).unwrap()
}

fn init_logging(data_dir: &std::path::Path) {
    let logs_dir = data_dir.join("logs");
    let _ = std::fs::create_dir_all(&logs_dir);
    let level = if cfg!(debug_assertions) {
        simplelog::LevelFilter::Debug
    } else {
        simplelog::LevelFilter::Info
    };
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs_dir.join("bare.log"))
    {
        Ok(file) => {
            let logger: Box<dyn simplelog::SharedLogger> =
                simplelog::WriteLogger::new(level, simplelog::Config::default(), file);
            let _ = simplelog::CombinedLogger::init(vec![logger]);
        }
        Err(_) => {
            let logger = simplelog::TermLogger::new(
                level,
                simplelog::Config::default(),
                simplelog::TerminalMode::Mixed,
                simplelog::ColorChoice::Auto,
            );
            let _ = simplelog::CombinedLogger::init(vec![logger]);
        }
    }
}

fn normalize_url(a: &str) -> Option<String> {
    if a.starts_with("http://") || a.starts_with("https://") {
        return Some(a.to_string());
    }
    let host = a.split('/').next().unwrap_or("");
    let looks_like_host = host == "localhost"
        || host.contains('.')
        || host.parse::<std::net::IpAddr>().is_ok();
    if looks_like_host {
        Some(format!("https://{a}"))
    } else {
        None
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut loop_builder = EventLoopBuilder::<UserEvent>::with_user_event();
    let event_loop = loop_builder.build();
    let proxy = event_loop.create_proxy();

    let mut data_dir = dirs::data_local_dir().unwrap_or_else(std::env::temp_dir);
    data_dir.push("Bare");
    let webview_dir = data_dir.join("webview");
    std::fs::create_dir_all(&webview_dir)?;

    init_logging(&data_dir);
    log::info!("Bare iniciando");

    let proxy_adblock = proxy.clone();
    blocker::startup(proxy_adblock);

    let db_path = data_dir.join("bare.db");
    let store = store::Store::open(&db_path)?;
    log::info!("historial/marcadores en {}", db_path.display());

    let window = WindowBuilder::new()
        .with_title("Bare")
        .with_inner_size(LogicalSize::new(1000.0, 700.0))
        .with_min_inner_size(LogicalSize::new(520.0, 420.0))
        .with_window_icon(Some(window_icon()))
        .build(&event_loop)?;

    let mut context = WebContext::new(Some(webview_dir));

    let proxy_ipc = proxy.clone();
    let chrome_view = wry::WebViewBuilder::new_with_web_context(&mut context)
        .with_html(chrome::CHROME_HTML)
        .with_additional_browser_args(events::BROWSER_ARGS)
        .with_ipc_handler(move |req: wry::http::Request<String>| {
            let _ = proxy_ipc.send_event(UserEvent::ChromeIpc(req.into_body()));
        })
        .build_as_child(&window)?;

    let mut state = tabs::AppState::new(window, chrome_view, proxy, context, store);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let urls: Vec<String> = args.iter().filter_map(|a| normalize_url(a)).collect();
    if urls.is_empty() {
        state.new_tab(None)?;
    } else {
        for url in urls {
            if let Err(e) = state.new_tab(Some(url)) {
                log::warn!("no se pudo abrir pestaña de CLI: {e}");
            }
        }
    }

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,
            Event::WindowEvent {
                event: WindowEvent::Resized(_),
                ..
            } => state.layout(),
            Event::WindowEvent {
                event: WindowEvent::Focused(focused),
                ..
            } => state.set_background(!focused),
            Event::UserEvent(ev) => state.handle(ev),
            Event::NewEvents(StartCause::Init | StartCause::ResumeTimeReached { .. }) => {
                state.hibernate_idle();
            }
            _ => {}
        }
        // punto de awakening del hibernator de pestañas
        if let Some(t) = state.next_deadline() {
            *control_flow = ControlFlow::WaitUntil(t);
        }
    });
}
