use std::cell::RefCell;
use std::rc::Rc;

use tao::event_loop::EventLoopProxy;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{take_pwstr, WebResourceRequestedEventHandler};
use windows_core::{HSTRING, Interface, PWSTR};
use wry::{WebView, WebViewExtWindows};

use crate::blocker;
use crate::events::UserEvent;

fn req_type(ctx: i32) -> &'static str {
    // COREWEBVIEW2_WEB_RESOURCE_CONTEXT
    match ctx {
        1 => "document",   // DOCUMENT
        2 => "stylesheet", // STYLESHEET
        3 => "image",      // IMAGE
        4 => "media",      // MEDIA
        5 => "font",       // FONT
        6 => "script",     // SCRIPT
        7 => "xhr",        // XML_HTTP_REQUEST
        8 => "xhr",        // FETCH
        9 => "other",      // TEXT_TRACK
        10 => "other",     // EVENT_SOURCE
        11 => "websocket", // WEBSOCKET
        12 => "other",     // MANIFEST
        13 => "other",     // SIGNED_EXCHANGE
        14 => "ping",      // PING
        15 => "csp_report", // CSP_VIOLATION_REPORT
        _ => "other",
    }
}

pub fn install(w: &WebView, id: u64, top_url: Rc<RefCell<String>>, proxy: EventLoopProxy<UserEvent>) {
    let core = w.webview();
    let env = w.environment();

    let handler = WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
        unsafe {
            let Some(args) = args else { return Ok(()) };
            let Ok(request) = args.Request() else { return Ok(()) };

            let uri = {
                let mut p = PWSTR::null();
                let _ = request.Uri(&mut p);
                take_pwstr(p)
            };
            if !(uri.starts_with("http://") || uri.starts_with("https://")) {
                return Ok(());
            }

            let mut ctx = 0i32;
            {
                let mut arg = COREWEBVIEW2_WEB_RESOURCE_CONTEXT(0);
                if args.ResourceContext(&mut arg).is_ok() {
                    ctx = arg.0;
                }
            }
            let req_type = req_type(ctx);
            let method = {
                let mut p = PWSTR::null();
                let _ = request.Method(&mut p);
                take_pwstr(p)
            };

            // para el documento principal el "origen" es la propia URL
            // (evita falsos third-party en la navegacion inicial)
            let source = if ctx == 1 {
                uri.clone()
            } else {
                top_url.borrow().clone()
            };

            if blocker::should_block(id, &uri, &source, req_type, &method) {
                if let Ok(resp) = env.CreateWebResourceResponse(
                    None::<&windows::Win32::System::Com::IStream>,
                    403,
                    &HSTRING::from("Forbidden"),
                    &HSTRING::from(""),
                ) {
                    let _ = args.SetResponse(&resp);
                }
                if blocker::count(id) % 10 == 0 {
                    let _ = proxy.send_event(UserEvent::BlocksChanged(id));
                }
            }
        }
        Ok(())
    }));

    unsafe {
        let mut token = 0i64;
        match core.cast::<ICoreWebView2_22>() {
            Ok(c22) => {
                let r = c22.AddWebResourceRequestedFilterWithRequestSourceKinds(
                    &HSTRING::from("*"),
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                    COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
                );
                if let Err(e) = r {
                    log::warn!("adblock: filtro con source kinds fallo: {e}");
                    return;
                }
            }
            Err(_) => {
                if let Err(e) =
                    core.AddWebResourceRequestedFilter(&HSTRING::from("*"), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)
                {
                    log::warn!("adblock: filtro basico fallo: {e}");
                    return;
                }
            }
        }
        match core.add_WebResourceRequested(&handler, &mut token) {
            Ok(()) => log::debug!("adblock: interceptor instalado (pestana {id})"),
            Err(e) => log::warn!("adblock: registro de evento fallo (pestana {id}): {e}"),
        }
    }
}
