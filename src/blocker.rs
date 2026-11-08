use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use adblock::engine::Engine;
use adblock::lists::{FilterSet, ParseOptions};
use adblock::request::Request;
use adblock::url_parser::{ResolvesDomain, set_domain_resolver};
use tao::event_loop::EventLoopProxy;

use crate::events::UserEvent;

const EASYLIST: &str = "https://easylist.to/easylist/easylist.txt";
const EASYPRIVACY: &str = "https://easylist.to/easylist/easyprivacy.txt";
const MAX_AGE_SECS: u64 = 7 * 24 * 60 * 60;

#[derive(Default)]
struct State {
    engine: Option<Engine>,
    counts: HashMap<u64, u64>,
}

/// Resolutor de dominios (eTLD+1) ligero, sustituye la PSL completa de `addr`.
/// Sufijos multiparte comunes; para el resto basta con los 2 ultimos labels.
struct SimpleResolver;

const MULTI_SUFFIXES: &[&str] = &[
    // LatAm (mercado principal del navegador)
    "com.ar", "org.ar", "gob.ar", "edu.ar", "net.ar",
    "com.br", "org.br", "gov.br", "edu.br", "net.br",
    "com.mx", "org.mx", "gob.mx", "edu.mx", "net.mx",
    "com.co", "org.co", "gov.co", "edu.co", "net.co",
    "gob.uy", "com.uy", "org.uy", "edu.uy", "net.uy",
    "com.pe", "org.pe", "gob.pe", "edu.pe",
    "com.ec", "com.bo", "com.py", "com.ve",
    "com.do", "com.gt", "com.cr", "com.cu", "com.pa", "com.sv", "com.hn", "com.ni",
    // anglosajones y comunes
    "co.uk", "org.uk", "gov.uk", "ac.uk", "net.uk",
    "com.au", "net.au", "org.au", "edu.au", "gov.au",
    "com.cn", "net.cn", "org.cn", "gov.cn",
    "com.tw", "com.hk", "co.in", "co.jp", "co.kr", "co.nz", "org.nz", "net.nz",
    "co.za", "org.za", "net.za", "co.il", "org.il",
    "com.sg", "com.my", "com.ph", "com.pk", "com.sa", "com.ua",
    "com.tr", "com.vn", "co.th", "org.tr",
];

impl ResolvesDomain for SimpleResolver {
    fn get_host_domain(&self, host: &str) -> (usize, usize) {
        let n = host.len();
        if n == 0 {
            return (0, n);
        }
        // adblock ya decodifico punycode: host ascii
        let mut label_count = 1;
        let mut numeric = true;
        for b in host.bytes() {
            if b == b'.' {
                label_count += 1;
                numeric = true;
                continue;
            }
            if !b.is_ascii_digit() {
                numeric = false;
            }
        }
        // IPv4: al menos 2 labels y todos digitos/puntos
        if numeric {
            return (0, n);
        }
        if label_count <= 2 {
            return (0, n);
        }
        // buscar el principio del eTLD+1 recorriendo puntos desde el final
        let mut dot3 = None;
        let mut dot2 = None;
        for (i, b) in host.bytes().enumerate().rev() {
            if b == b'.' {
                if dot2.is_none() {
                    dot2 = Some(i);
                } else {
                    dot3 = Some(i);
                    break;
                }
            }
        }
        let d2 = dot2.map(|i| i + 1).unwrap_or(0);
        let last2 = &host[d2..];
        let take3 = MULTI_SUFFIXES.iter().any(|s| last2.eq_ignore_ascii_case(s));
        if take3 && let Some(d3) = dot3 {
            let d = d3 + 1;
            return (d, n);
        }
        (d2, n)
    }
}

fn ensure_resolver() {
    let _ = set_domain_resolver(Box::new(SimpleResolver));
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State {
        engine: None,
        counts: HashMap::new(),
    });
}

fn set_engine(engine: Engine) {
    STATE.with(|s| s.borrow_mut().engine = Some(engine));
}

pub fn dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("Bare").join("adblock"))
}

fn fresh(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    let Ok(age) = modified.elapsed() else {
        return false;
    };
    age.as_secs() < MAX_AGE_SECS
}

fn fetch_text(url: &str) -> Option<String> {
    let mut resp = ureq::get(url).call().ok()?;
    resp.body_mut().read_to_string().ok()
}

pub fn startup(proxy: EventLoopProxy<UserEvent>) {
    ensure_resolver();
    let Some(dir) = dir() else {
        log::warn!("adblock: sin directorio de datos; bloqueo desactivado");
        return;
    };
    if std::fs::create_dir_all(&dir).is_err() {
        log::warn!("adblock: no se pudo crear {}", dir.display());
        return;
    }

    let bin = dir.join("engine.bin");
    if bin.exists() {
        if load_from_disk(&dir) {
            if !fresh(&bin) {
                log::info!("adblock: listas desactualizadas; actualizando en segundo plano");
                spawn_builder(dir, proxy);
            }
            return;
        }
        log::warn!("adblock: engine.bin invalido; regenerando");
    } else {
        log::info!("adblock: primer arranque; descargando listas en segundo plano");
        spawn_builder(dir, proxy);
    }
}

fn spawn_builder(dir: PathBuf, proxy: EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || {
        log::info!("adblock: hilo de construccion iniciado");
        let success = build_engine(&dir);
        if success {
            log::info!("adblock: engine.bin escrito");
            let _ = proxy.send_event(UserEvent::ListsReady);
        } else {
            log::warn!("adblock: no se pudo construir el motor; se reintentara el proximo arranque");
        }
    });
}

fn build_engine(dir: &Path) -> bool {
    const LISTS: [(&str, &str); 2] = [(EASYLIST, "easylist.txt"), (EASYPRIVACY, "easyprivacy.txt")];

    let mut texts: Vec<String> = Vec::with_capacity(2);
    for (url, name) in LISTS {
        let path = dir.join(name);
        if fresh(&path) {
            match std::fs::read_to_string(&path) {
                Ok(t) => {
                    log::debug!("adblock: caché fresca {}", name);
                    texts.push(t);
                    continue;
                }
                Err(e) => log::warn!("adblock: lectura de {} fallo: {e}", path.display()),
            }
        }
        match fetch_text(url) {
            Some(t) => {
                if let Err(e) = std::fs::write(&path, &t) {
                    log::warn!("adblock: no se pudo cachear {} en {}: {e}", name, path.display());
                }
                log::debug!("adblock: descargado {} ({} bytes)", name, t.len());
                texts.push(t);
            }
            None => match std::fs::read_to_string(&path) {
                Ok(t) => {
                    log::warn!("adblock: sin red; usando caché vieja de {}", name);
                    texts.push(t);
                }
                Err(_) => log::warn!("adblock: sin datos para {}; esta lista quedara fuera", name),
            },
        }
    }

    if texts.is_empty() {
        log::warn!("adblock: sin listas disponibles");
        return false;
    }

    let total: usize = texts.iter().map(String::len).sum();
    let mut set = FilterSet::new(false);
    for text in texts {
        set.add_filter_list(text, ParseOptions::default());
    }
    let engine = Engine::new_with_filter_set(set);
    log::info!("adblock: motor construido desde {total} bytes de reglas");
    let bin = engine.serialize();
    drop(engine);

    let tmp = dir.join("engine.bin.tmp");
    if let Err(e) = std::fs::write(&tmp, &bin) {
        log::warn!("adblock: escritura temporal fallo: {e}");
        return false;
    }
    match std::fs::rename(&tmp, dir.join("engine.bin")) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("adblock: renombrado fallo: {e}");
            let _ = std::fs::remove_file(&tmp);
            false
        }
    }
}

fn load_from_disk(dir: &Path) -> bool {
    let bin_path = dir.join("engine.bin");
    let bin = match std::fs::read(&bin_path) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("adblock: no se pudo leer {}: {e}", bin_path.display());
            return false;
        }
    };
    let mut engine = Engine::default();
    match engine.deserialize(&bin) {
        Ok(()) => {
            set_engine(engine);
            log::info!("adblock: motor cargado ({} bytes)", bin.len());
            true
        }
        Err(e) => {
            log::warn!("adblock: engine.bin ilegible ({e}); se descarta");
            let _ = std::fs::remove_file(&bin_path);
            false
        }
    }
}

pub fn on_lists_ready() {
    if let Some(dir) = dir() {
        if load_from_disk(&dir) {
            log::info!("adblock: bloqueo activo");
        }
    }
}

pub fn ready() -> bool {
    STATE.with(|s| s.borrow().engine.is_some())
}

pub fn should_block(tab_id: u64, url: &str, source: &str, req_type: &str, method: &str) -> bool {
    if url.len() <= 11 || !url.starts_with("http") {
        return false;
    }
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let Some(engine) = s.engine.as_ref() else {
            return false;
        };
        let Ok(req) = Request::new(url, source, req_type, method) else {
            return false;
        };
        let res = engine.check_network_request(&req);
        if res.should_block() {
            *s.counts.entry(tab_id).or_default() += 1;
            true
        } else {
            false
        }
    })
}

pub fn cosmetics(url: &str) -> (String, String) {
    if url.is_empty() {
        return (String::new(), String::new());
    }
    STATE.with(|s| {
        let s = s.borrow();
        let Some(engine) = s.engine.as_ref() else {
            return (String::new(), String::new());
        };
        let res = engine.url_cosmetic_resources(url);
        if res.hide_selectors.is_empty() {
            return (String::new(), res.injected_script);
        }
        let mut selectors: Vec<String> = res.hide_selectors.iter().cloned().collect();
        selectors.sort();
        let css = format!("{} {{ display: none !important; }}", selectors.join(", "));
        (css, res.injected_script)
    })
}

pub fn reset_count(tab_id: u64) {
    STATE.with(|s| {
        s.borrow_mut().counts.insert(tab_id, 0);
    });
}

pub fn count(tab_id: u64) -> u64 {
    STATE.with(|s| *s.borrow().counts.get(&tab_id).unwrap_or(&0))
}

pub fn forget_count(tab_id: u64) {
    STATE.with(|s| {
        s.borrow_mut().counts.remove(&tab_id);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use adblock::lists::ParseOptions;

    fn engine_with(rules: &str) -> Engine {
        ensure_resolver();
        let mut fs = FilterSet::new(false);
        fs.add_filter_list(rules.to_string(), ParseOptions::default());
        Engine::new_with_filter_set(fs)
    }

    #[test]
    fn bloquea_dominio_de_ads() {
        let engine = engine_with("||doubleclick.net^\n||googlesyndication.com^\n");
        let req = Request::new(
            "https://ad.doubleclick.net/ddm/adj/x",
            "https://example.com/",
            "script",
            "GET",
        )
        .unwrap();
        assert!(engine.check_network_request(&req).should_block());
    }

    #[test]
    fn permite_sitio_normal() {
        let engine = engine_with("||doubleclick.net^\n");
        let req = Request::new(
            "https://example.com/script.js",
            "https://example.com/",
            "script",
            "GET",
        )
        .unwrap();
        assert!(!engine.check_network_request(&req).should_block());
    }

    #[test]
    fn excepcion_prevalece() {
        let engine = engine_with("||ads.example.com^\n@@||ads.example.com^\n");
        let req = Request::new(
            "https://ads.example.com/img.png",
            "https://example.com/",
            "image",
            "GET",
        )
        .unwrap();
        assert!(!engine.check_network_request(&req).should_block());
    }

    #[test]
    fn hilo_local_decide_y_cuenta() {
        let saved = STATE.with(|s| std::mem::take(&mut *s.borrow_mut()));
        set_engine(engine_with("||tracker.example^\n"));
        assert!(should_block(1, "https://tracker.example/pixel.gif", "https://site.com/", "image", "GET"));
        assert!(!should_block(1, "https://site.com/style.css", "https://site.com/", "stylesheet", "GET"));
        assert_eq!(count(1), 1);
        reset_count(1);
        assert_eq!(count(1), 0);
        forget_count(1);
        STATE.with(|s| *s.borrow_mut() = saved);
    }

    #[test]
    fn cosmetica_devuelve_selectores() {
        let engine = engine_with("example.com##.advert\nexample.com##.legal-banner");
        let res = engine.url_cosmetic_resources("https://example.com/pagina");
        assert!(res.hide_selectors.contains(".advert"));
        assert!(res.hide_selectors.contains(".legal-banner"));
        let engine2 = engine_with("example.com##.advert");
        let res2 = engine2.url_cosmetic_resources("https://otro.com/");
        assert!(res2.hide_selectors.is_empty());
    }

    /// Test de integración: valida el engine.bin real (EasyList + EasyPrivacy)
    /// contra URLs de anuncios/tracking conocidas. Se omite si no hay caché.
    #[test]
    fn engine_real_bloquea_trackers() {
        let Some(dir) = dir() else {
            return;
        };
        let bin_path = dir.join("engine.bin");
        if !bin_path.exists() {
            eprintln!("SKIP: engine.bin no existe");
            return;
        }
        let bin = std::fs::read(&bin_path).expect("lectura de engine.bin");
        let mut engine = Engine::default();
        engine.deserialize(&bin).expect("engine.bin valido");
        ensure_resolver();

        let ads = [
            "https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js",
            "https://ad.doubleclick.net/ddm/adj/pub.site/banner",
            "https://www.google-analytics.com/analytics.js",
            "https://connect.facebook.net/en_US/fbevents.js",
            "https://pixel.taboola.com/tb?x=1",
            "https://widgets.outbrain.com/outbrain.js",
            "https://c.amazon-adsystem.com/aax2/apstag.js",
        ];
        let mut blocked = 0;
        for url in ads.iter() {
            let req = Request::new(url, "https://example.com/", "script", "GET").unwrap();
            if engine.check_network_request(&req).should_block() {
                blocked += 1;
            } else {
                eprintln!("NO BLOQUEA: {url}");
            }
        }
        assert!(
            blocked >= ads.len() * 3 / 4,
            "solo {blocked}/{} trackers bloqueados con EasyList real",
            ads.len()
        );

        let legits = [
            "https://example.com/noticias/titulo.json",
            "https://cdn.jsdelivr.net/npm/react@18/umd/react.production.min.js",
        ];
        for url in legits {
            let req = Request::new(url, "https://example.com/", "script", "GET").unwrap();
            assert!(
                !engine.check_network_request(&req).should_block(),
                "falso positivo contra {url}"
            );
        }
    }
}
