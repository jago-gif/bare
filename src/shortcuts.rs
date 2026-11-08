use std::sync::Mutex;

use tao::event_loop::EventLoopProxy;
use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId,
    KBDLLHOOKSTRUCT, MSG, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, HC_ACTION,
};

use crate::events::{Shortcut, UserEvent};

static PROXY: Mutex<Option<EventLoopProxy<UserEvent>>> = Mutex::new(None);

pub fn install(proxy: EventLoopProxy<UserEvent>) {
    *PROXY.lock().unwrap() = Some(proxy);
    std::thread::spawn(|| unsafe {
        let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), Some(HMODULE::default().into()), 0)
        {
            Ok(h) => h,
            Err(e) => {
                log::warn!("atajos: hook de teclado no disponible ({e}); quedan solo los atajos con foco en barra");
                return;
            }
        };
        log::debug!("atajos: hook global instalado");
        let mut msg = MSG::default();
        loop {
            let r = GetMessageW(&mut msg, Some(HWND::default()), 0, 0);
            if r.0 <= 0 {
                break;
            }
        }
        let _ = UnhookWindowsHookEx(hook);
    });
}

unsafe extern "system" fn hook_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if code == HC_ACTION as i32 && wp.0 == 256 {
            let k = &*(lp.0 as *const KBDLLHOOKSTRUCT);
            let ctrl = GetAsyncKeyState(VK_CONTROL.0 as i32) < 0;
            if ctrl {
                let fg = GetForegroundWindow();
                let mut pid = 0u32;
                GetWindowThreadProcessId(fg, Some(&mut pid));
                if pid == std::process::id() {
                    let s = match k.vkCode {
                        0x54 => Some(Shortcut::NewTab),     // T
                        0x57 => Some(Shortcut::CloseTab),   // W
                        0x4C => Some(Shortcut::FocusBar),   // L
                        0x52 => Some(Shortcut::Reload),     // R
                        _ => None,
                    };
                    if let Some(s) = s {
                        if let Some(p) = PROXY.lock().unwrap().as_ref() {
                            let _ = p.send_event(UserEvent::Shortcut(s));
                        }
                        return LRESULT(1); // lo traga: evita que tambien lo procese la barra
                    }
                }
            }
        }
        CallNextHookEx(None, code, wp, lp)
    }
}
