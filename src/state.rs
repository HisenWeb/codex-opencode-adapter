use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredResponse {
    pub response_id: String,
    pub model_alias: String,
    pub model_upstream: String,
    pub messages: Vec<Value>,
    #[serde(default)]
    pub pending_call_ids: Vec<String>,
    #[serde(default)]
    pub output: Vec<Value>,
    pub created_at: i64,
    #[serde(default)]
    pub previous_response_id: String,
}

#[derive(Clone)]
pub struct StateStore {
    path: String,
    ttl_seconds: i64,
    lock: Arc<Mutex<()>>,
}

impl StateStore {
    pub fn new(path: impl Into<String>, ttl_seconds: i64) -> anyhow::Result<Self> {
        let path = path.into();
        if let Some(parent) = Path::new(&path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        let store = Self {
            path,
            ttl_seconds,
            lock: Arc::new(Mutex::new(())),
        };
        store.init()?;
        Ok(store)
    }

    fn connect(&self) -> anyhow::Result<Connection> {
        Ok(Connection::open(&self.path)?)
    }

    fn init(&self) -> anyhow::Result<()> {
        let _guard = self.lock.lock().expect("state lock poisoned");
        let db = self.connect()?;
        db.execute(
            "CREATE TABLE IF NOT EXISTS responses (response_id TEXT PRIMARY KEY, created_at INTEGER NOT NULL, payload TEXT NOT NULL)",
            [],
        )?;
        Ok(())
    }

    pub fn put(&self, item: &StoredResponse) -> anyhow::Result<()> {
        let payload = serde_json::to_string(item)?;
        let _guard = self.lock.lock().expect("state lock poisoned");
        let db = self.connect()?;
        db.execute(
            "INSERT OR REPLACE INTO responses(response_id, created_at, payload) VALUES(?1, ?2, ?3)",
            params![item.response_id, item.created_at, payload],
        )?;
        Ok(())
    }

    pub fn get(&self, response_id: &str) -> anyhow::Result<Option<StoredResponse>> {
        let cutoff = now_ts() - self.ttl_seconds;
        let _guard = self.lock.lock().expect("state lock poisoned");
        let db = self.connect()?;
        let mut stmt =
            db.prepare("SELECT payload FROM responses WHERE response_id=?1 AND created_at>=?2")?;
        let mut rows = stmt.query(params![response_id, cutoff])?;
        if let Some(row) = rows.next()? {
            let payload: String = row.get(0)?;
            Ok(Some(serde_json::from_str(&payload)?))
        } else {
            tracing::warn!(
                event = "stored_response_not_found",
                response_id,
                cutoff,
                "requested previous_response_id was not found in non-expired state"
            );
            Ok(None)
        }
    }

    pub fn find_by_call_ids(&self, call_ids: &[String]) -> anyhow::Result<Option<StoredResponse>> {
        if call_ids.is_empty() {
            return Ok(None);
        }
        let cutoff = now_ts() - self.ttl_seconds;
        let wanted: HashSet<&str> = call_ids.iter().map(String::as_str).collect();
        let _guard = self.lock.lock().expect("state lock poisoned");
        let db = self.connect()?;
        let mut stmt = db.prepare(
            "SELECT payload FROM responses WHERE created_at>=?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![cutoff], |row| row.get::<_, String>(0))?;

        let mut matched_by_call_id: HashMap<String, String> = HashMap::new();
        let mut unique_response: Option<StoredResponse> = None;
        let mut unique_response_id: Option<String> = None;

        for row in rows {
            let payload = row?;
            let item: StoredResponse = serde_json::from_str(&payload)?;
            let pending: HashSet<&str> = item.pending_call_ids.iter().map(String::as_str).collect();
            let mut intersects_wanted = false;

            for &call_id in &wanted {
                if !pending.contains(call_id) {
                    continue;
                }
                intersects_wanted = true;
                match matched_by_call_id.get(call_id) {
                    Some(existing_response_id) if existing_response_id != &item.response_id => {
                        tracing::warn!(
                            event = "tool_history_call_id_ambiguous",
                            call_id,
                            candidate_count = 2,
                            existing_response_id,
                            candidate_response_id = %item.response_id,
                            requested_call_ids = ?call_ids,
                            "call_id matched multiple stored responses; unique fallback disabled"
                        );
                        return Ok(None);
                    }
                    Some(_) => {}
                    None => {
                        matched_by_call_id.insert(call_id.to_string(), item.response_id.clone());
                    }
                }
            }

            if intersects_wanted && wanted.is_subset(&pending) {
                match &unique_response_id {
                    Some(existing_response_id) if existing_response_id != &item.response_id => {
                        tracing::warn!(
                            event = "tool_history_response_ambiguous",
                            candidate_count = 2,
                            existing_response_id,
                            candidate_response_id = %item.response_id,
                            requested_call_ids = ?call_ids,
                            "requested call ids matched multiple stored responses; unique fallback disabled"
                        );
                        return Ok(None);
                    }
                    Some(_) => {}
                    None => {
                        unique_response_id = Some(item.response_id.clone());
                        unique_response = Some(item);
                    }
                }
            }
        }

        if matched_by_call_id.len() == wanted.len() {
            if let Some(response) = unique_response.as_ref() {
                tracing::debug!(
                    event = "tool_history_unique_fallback_hit",
                    response_id = %response.response_id,
                    requested_call_ids = ?call_ids,
                    pending_call_ids = ?response.pending_call_ids,
                    "restored tool continuation by unique pending call_id fallback"
                );
            }
            Ok(unique_response)
        } else {
            tracing::warn!(
                event = "tool_history_call_id_not_found",
                requested_call_ids = ?call_ids,
                matched_call_id_count = matched_by_call_id.len(),
                requested_call_id_count = wanted.len(),
                "could not restore tool continuation by pending call_id fallback"
            );
            Ok(None)
        }
    }
}

pub fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
