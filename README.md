# Bare — navegador ligero con bloqueo de anuncios

Navegador de escritorio para Windows construido en Rust sobre WebView2, con bloqueo de
anuncios integrado (mismo motor que Brave: [adblock-rust]) y foco en consumo mínimo de RAM.

**Binario: ~4,7 MB · RAM: ~156 MB frío · ~290 MB con 8 sitios de noticias pesadas.**

![logo](assets/icono.ico)

## Características

- **Bloqueo de anuncios**: EasyList + EasyPrivacy con `adblock-rust` (motor binario
  serializado, `engine.bin`, ~6 MB, regenerado con caché de 7 días).
  Bloqueo de red (403 en interceptor `WebResourceRequested`) + cosmética por inyección
  (CSS `hide` + scripts regionales), contador de bloqueos por pestaña en la barra.
- **Hibernación de pestañas**: las inactivas pasan a `about:blank` tras 45 s, o
  inmediatamente al perder foco la ventana; se restauran recargando su URL al
  seleccionarlas. Además baja el nivel `MemoryUsageLevel` de todas las inactivas.
- **Perfil de baja memoria**: `--renderer-process-limit=4`, `--disable-gpu`,
  `MemoryUsageLevel::Low` para pestañas en segundo plano.
- **CLI**: `bare.exe <url> [<url>...]` abre cada URL como pestaña (útil para
  integración como navegador por defecto).
- **Chrome UI propia**: pestañas, omnibox (Bing como buscador fallback), marcadores,
  historial (SQLite con WAL y deduplicado de 60 s), botón "bloqueados".
- **Atajos**: `Ctrl+T` nueva pestaña, `Ctrl+W` cerrar activa, `Ctrl+L` omnibox,
  `Ctrl+R` recargar. Funcionan con foco en la barra (manejador JS) o en el contenido
  (hook global de teclado). Espera de 45 s para hibernación configurable vía constante
  en código.

## Requisitos

- Windows 10 1803+ / Windows 11 (WebView2 Runtime, ya incluido en Win11)
- Rust stable (edición 2024)

## Compilar y ejecutar

```powershell
cd D:\Proyectos\desarrollos personales\navegador
cargo build --release
.\target\release\bare.exe
# con URLs:
.\target\release\bare.exe https://example.com https://clarin.com
```

## Pruebas

```powershell
cargo test                                  # 11 tests unit + integración del motor real
.\scripts\medir-ram.ps1                     # RAM fría
.\scripts\medir-ram.ps1 -SiteList "https://clarin.com,https://infobae.com" -SettleSec 60
.\scripts\test-fase8.ps1                    # suite de estabilidad (11 checks)
```

Variables de entorno útiles: `BARE_DEBUG=1` activa logs de nivel Debug en release
(`C:\Users\<usuario>\AppData\Local\Bare\logs\bare.log`).

## Arquitectura (`src/`)

| Módulo | Rol |
|---|---|
| `main.rs` | bootstrap, event loop tao, deadline del hibernador (`WaitUntil`) |
| `tabs.rs` | AppState: pestañas, layout, hibernación, IPC, cosmética |
| `chrome.rs` | HTML/CSS/JS de la barra (pestañas, omnibox, panel historial/marcadores) |
| `blocker.rs` | motor adblock-rust: descarga/caché/serialización de listas eTLD/sufijos propios |
| `net_interceptor.rs` | handler COM `WebResourceRequested` → 403 si el motor bloquea |
| `store.rs` | SQLite (historial + marcadores) |
| `events.rs` | `UserEvent` del event loop, tipos IPC chrome↔Rust |
| `shortcuts.rs` | hook global `WH_KEYBOARD_LL` de atajos (solo con nuestra ventana en foreground) |

## Límites conocidos

- Sin soporte de descargas/PDF interno (descarta `msPdfOOUI`).
- Hibernar descarta el estado visual de la pestaña (scroll, historial de sesión).
- `--disable-gpu` baja mucho la RAM, con algo menos de fluidez al hacer scroll.
- Listas regionales (EasyList Spanish, etc.) aún no agregadas; los sufijos LatAm
  están cubiertos por `SimpleResolver`.

[adblock-rust]: https://github.com/BraveSnoop/adblock-rust
