use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::api::types::Baby;

const SESSION_REVISION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionData {
    pub revision: u32,
    pub auth_token: String,
    pub refresh_token: String,
    pub auth_time: String,
    pub last_seen_message_time: String,
    pub babies: Vec<Baby>,
}

impl Default for SessionData {
    fn default() -> Self {
        Self {
            revision: SESSION_REVISION,
            auth_token: String::new(),
            refresh_token: String::new(),
            auth_time: String::new(),
            last_seen_message_time: String::new(),
            babies: Vec::new(),
        }
    }
}

pub struct SessionStore {
    pub filename: String,
    pub session: SessionData,
}

impl SessionStore {
    pub fn new(filename: &str) -> Self {
        Self {
            filename: filename.to_string(),
            session: SessionData::default(),
        }
    }

    pub fn load(&mut self) {
        let path = Path::new(&self.filename);
        if !path.exists() {
            return;
        }

        let raw = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => return,
        };

        let parsed: SessionData = match serde_json::from_str(&raw) {
            Ok(d) => d,
            Err(_) => return,
        };

        if parsed.revision != SESSION_REVISION {
            return;
        }

        self.session = parsed;
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        let path = Path::new(&self.filename);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(&self.session)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn auth_token(&self) -> &str {
        &self.session.auth_token
    }

    pub fn set_auth_token(&mut self, token: &str) {
        self.session.auth_token = token.to_string();
    }

    pub fn refresh_token(&self) -> &str {
        &self.session.refresh_token
    }

    pub fn set_refresh_token(&mut self, token: &str) {
        self.session.refresh_token = token.to_string();
    }

    pub fn babies(&self) -> &[Baby] {
        &self.session.babies
    }

    pub fn set_babies(&mut self, babies: Vec<Baby>) {
        self.session.babies = babies;
    }

    pub fn auth_time(&self) -> Option<DateTime<Utc>> {
        if self.session.auth_time.is_empty() {
            return None;
        }
        self.session.auth_time.parse::<DateTime<Utc>>().ok()
    }

    pub fn set_auth_time(&mut self, time: DateTime<Utc>) {
        self.session.auth_time = time.to_rfc3339();
    }

    #[allow(dead_code)]
    pub fn last_seen_message_time(&self) -> Option<DateTime<Utc>> {
        if self.session.last_seen_message_time.is_empty() {
            return None;
        }
        self.session
            .last_seen_message_time
            .parse::<DateTime<Utc>>()
            .ok()
    }

    #[allow(dead_code)]
    pub fn set_last_seen_message_time(&mut self, time: DateTime<Utc>) {
        self.session.last_seen_message_time = time.to_rfc3339();
    }

    pub fn is_token_expired(&self, lifetime_ms: u64) -> bool {
        if self.session.auth_token.is_empty() || self.session.auth_time.is_empty() {
            return true;
        }
        let auth_time = match self.auth_time() {
            Some(t) => t,
            None => return true,
        };
        let elapsed = Utc::now()
            .signed_duration_since(auth_time)
            .num_milliseconds();
        elapsed < 0 || elapsed as u64 > lifetime_ms
    }
}

pub fn init_session_store(filename: &str) -> SessionStore {
    let mut store = SessionStore::new(filename);
    store.load();
    store
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_file(name: &str) -> String {
        let dir = std::env::temp_dir().join("nanit-test");
        fs::create_dir_all(&dir).unwrap();
        dir.join(name).to_string_lossy().to_string()
    }

    #[test]
    fn creates_with_default_empty_session() {
        let store = SessionStore::new("/tmp/nanit-test/nonexistent.json");
        assert_eq!(store.auth_token(), "");
        assert_eq!(store.refresh_token(), "");
        assert!(store.babies().is_empty());
        assert!(store.auth_time().is_none());
        assert!(store.last_seen_message_time().is_none());
    }

    #[test]
    fn saves_and_loads_session_data() {
        let path = test_file("save-load.json");
        let _ = fs::remove_file(&path);

        let mut store = SessionStore::new(&path);
        store.set_auth_token("test-auth-token");
        store.set_refresh_token("test-refresh-token");
        store.set_auth_time("2024-01-01T00:00:00Z".parse().unwrap());
        store.set_babies(vec![Baby {
            uid: "baby-1".into(),
            name: "Baby".into(),
            camera_uid: "cam-1".into(),
        }]);
        store.save().unwrap();

        assert!(Path::new(&path).exists());

        let raw = fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["authToken"], "test-auth-token");

        let mut store2 = SessionStore::new(&path);
        store2.load();
        assert_eq!(store2.auth_token(), "test-auth-token");
        assert_eq!(store2.refresh_token(), "test-refresh-token");
        assert_eq!(store2.babies().len(), 1);
        assert_eq!(store2.babies()[0].uid, "baby-1");

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn ignores_session_file_with_wrong_revision() {
        let path = test_file("wrong-revision.json");
        let _ = fs::remove_file(&path);

        let mut store = SessionStore::new(&path);
        store.set_auth_token("old-token");
        store.save().unwrap();

        // Tamper with revision
        let raw = fs::read_to_string(&path).unwrap();
        let mut parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        parsed["revision"] = serde_json::json!(999);
        fs::write(&path, serde_json::to_string(&parsed).unwrap()).unwrap();

        let mut store2 = SessionStore::new(&path);
        store2.load();
        assert_eq!(store2.auth_token(), ""); // Should not have loaded

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn handles_missing_session_file() {
        let mut store = SessionStore::new("/tmp/nanit-test/nonexistent-2.json");
        store.load(); // Should not panic
        assert_eq!(store.auth_token(), "");
    }

    #[test]
    fn reports_token_expired_when_no_auth_time() {
        let store = SessionStore::new("/tmp/nanit-test/no-auth.json");
        assert!(store.is_token_expired(60_000));
    }

    #[test]
    fn reports_token_not_expired_within_lifetime() {
        let mut store = SessionStore::new("/tmp/nanit-test/not-expired.json");
        store.set_auth_token("token");
        store.set_auth_time(Utc::now());
        assert!(!store.is_token_expired(60_000));
    }

    #[test]
    fn reports_token_expired_past_lifetime() {
        let mut store = SessionStore::new("/tmp/nanit-test/expired.json");
        store.set_auth_token("token");
        store.set_auth_time(Utc::now() - chrono::Duration::milliseconds(120_000));
        assert!(store.is_token_expired(60_000));
    }

    #[test]
    fn init_session_store_loads_existing() {
        let path = test_file("init-load.json");
        let _ = fs::remove_file(&path);

        let mut store = SessionStore::new(&path);
        store.set_auth_token("loaded-token");
        store.save().unwrap();

        let loaded = init_session_store(&path);
        assert_eq!(loaded.auth_token(), "loaded-token");
        assert_eq!(loaded.session.revision, SESSION_REVISION);

        let _ = fs::remove_file(&path);
    }
}
