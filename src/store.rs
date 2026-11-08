use std::path::Path;

use rusqlite::{Connection, Row};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub url: String,
    pub title: String,
    pub ts: i64,
}

pub struct Store {
    conn: Connection,
}

const DEDUPE_WINDOW_SECS: i64 = 60;

impl Store {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn open_memory() -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> rusqlite::Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS history (
                id    INTEGER PRIMARY KEY,
                url   TEXT NOT NULL,
                title TEXT NOT NULL,
                ts    INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS history_ts ON history (ts DESC);
            CREATE TABLE IF NOT EXISTS bookmarks (
                id    INTEGER PRIMARY KEY,
                url   TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL,
                ts    INTEGER NOT NULL
            );",
        )?;
        Ok(Self { conn })
    }

    /// Guarda una visita. Ignora repetidos identicos dentro de la ventana
    /// (recargas / title updates del mismo documento).
    pub fn add_history(&self, url: &str, title: &str) {
        if url.len() <= 8 || !url.starts_with("http") {
            return;
        }
        let now = now_unix();
        let _ = self.conn.execute(
            "INSERT INTO history (url, title, ts)
             SELECT ?1, ?2, ?3
             WHERE NOT EXISTS (
                SELECT 1 FROM history
                WHERE url = ?1 AND ts > ?3 - ?4
             )",
            rusqlite::params![url, title, now, DEDUPE_WINDOW_SECS],
        );
    }

    pub fn history(&self, limit: i64) -> Vec<Entry> {
        self.query(
            "SELECT url, title, ts FROM history ORDER BY ts DESC LIMIT ?1",
            [limit],
        )
    }

    pub fn clear_history(&self) {
        let _ = self.conn.execute("DELETE FROM history", []);
    }

    /// Marca/desmarca. Devuelve true si quedo marcado.
    pub fn toggle_bookmark(&self, url: &str, title: &str) -> bool {
        if url.len() <= 8 || !url.starts_with("http") {
            return false;
        }
        if self.is_bookmarked(url) {
            self.remove_bookmark(url);
            return false;
        }
        let _ = self.conn.execute(
            "INSERT OR IGNORE INTO bookmarks (url, title, ts) VALUES (?1, ?2, ?3)",
            rusqlite::params![url, title, now_unix()],
        );
        true
    }

    pub fn remove_bookmark(&self, url: &str) -> bool {
        self.conn
            .execute("DELETE FROM bookmarks WHERE url = ?1", [url])
            .map(|n| n > 0)
            .unwrap_or(false)
    }

    pub fn is_bookmarked(&self, url: &str) -> bool {
        self.conn
            .query_row(
                "SELECT 1 FROM bookmarks WHERE url = ?1",
                [url],
                |_: &Row| Ok(()),
            )
            .is_ok()
    }

    pub fn bookmarks(&self) -> Vec<Entry> {
        self.query(
            "SELECT url, title, ts FROM bookmarks ORDER BY ts DESC LIMIT 200",
            [],
        )
    }

    fn query(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> Vec<Entry> {
        let Ok(mut stmt) = self.conn.prepare(sql) else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map(params, |r| {
            Ok(Entry {
                url: r.get(0)?,
                title: r.get(1)?,
                ts: r.get(2)?,
            })
        }) else {
            return Vec::new();
        };
        rows.filter_map(|r| r.ok()).collect()
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn format_ts(ts: i64) -> String {
    let now = now_unix();
    let delta = (now - ts).max(0);
    if delta < 60 {
        "hace un momento".into()
    } else if delta < 3600 {
        format!("hace {} min", delta / 60)
    } else if delta < 86400 {
        format!("hace {} h", delta / 3600)
    } else {
        format!("hace {} d", delta / 86400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn historial_roundtrip() {
        let s = Store::open_memory().unwrap();
        s.add_history("https://example.com/a", "A");
        s.add_history("https://example.org/b", "B");
        let h = s.history(10);
        // mismo segundo => el orden entre ambas no esta garantizado
        assert_eq!(h.len(), 2);
        let urls: Vec<&str> = h.iter().map(|e| e.url.as_str()).collect();
        assert!(urls.contains(&"https://example.com/a"));
        assert!(urls.contains(&"https://example.org/b"));
        let titles: Vec<&str> = h.iter().map(|e| e.title.as_str()).collect();
        assert!(titles.contains(&"A"));
        assert!(titles.contains(&"B"));
        s.clear_history();
        assert!(s.history(10).is_empty());
    }

    #[test]
    fn historial_dedupe() {
        let s = Store::open_memory().unwrap();
        s.add_history("https://example.com/x", "X");
        s.add_history("https://example.com/x", "X");
        assert_eq!(s.history(10).len(), 1);
        s.add_history("https://example.com/y", "Y");
        assert_eq!(s.history(10).len(), 2);
    }

    #[test]
    fn historial_ignora_no_http() {
        let s = Store::open_memory().unwrap();
        s.add_history("data:text/html,hola", "no");
        s.add_history("chrome://version", "no");
        assert!(s.history(10).is_empty());
    }

    #[test]
    fn marcadores_toggle() {
        let s = Store::open_memory().unwrap();
        let url = "https://example.com/articulo";
        assert!(!s.is_bookmarked(url));
        assert!(s.toggle_bookmark(url, "Articulo"));
        assert!(s.is_bookmarked(url));
        let b = s.bookmarks();
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].title, "Articulo");
        // segundo toggle desmarca
        assert!(!s.toggle_bookmark(url, "Articulo"));
        assert!(!s.is_bookmarked(url));
        assert!(s.bookmarks().is_empty());
    }

    #[test]
    fn marcadores_unicos() {
        let s = Store::open_memory().unwrap();
        assert!(s.toggle_bookmark("https://a.com/", "A"));
        assert!(!s.toggle_bookmark("https://a.com/", "A")); // ya marcado -> remove
        assert!(s.toggle_bookmark("https://a.com/", "A"));
        s.remove_bookmark("https://a.com/");
        assert!(s.bookmarks().is_empty());
    }
}
