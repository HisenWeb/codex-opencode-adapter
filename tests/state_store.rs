use codex_opencode_adapter::state::{now_ts, StateStore, StoredResponse};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_db_path(name: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir()
        .join(format!(
            "codex_opencode_adapter_state_{name}_{}_{}.sqlite",
            std::process::id(),
            nanos
        ))
        .to_string_lossy()
        .to_string()
}

fn stored_response(response_id: &str, pending_call_ids: Vec<&str>) -> StoredResponse {
    StoredResponse {
        response_id: response_id.to_string(),
        model_alias: "opencode-go/test".to_string(),
        model_upstream: "test".to_string(),
        messages: vec![json!({"role":"assistant","content":""})],
        pending_call_ids: pending_call_ids.into_iter().map(str::to_string).collect(),
        output: Vec::<Value>::new(),
        created_at: now_ts(),
        previous_response_id: String::new(),
    }
}

#[test]
fn find_by_call_ids_returns_unique_match() {
    let path = temp_db_path("unique_match");
    let store = StateStore::new(&path, 21_600).unwrap();
    store
        .put(&stored_response("resp_1", vec!["call_1"]))
        .unwrap();

    let found = store
        .find_by_call_ids(&["call_1".to_string()])
        .unwrap()
        .unwrap();

    assert_eq!(found.response_id, "resp_1");
    let _ = std::fs::remove_file(path);
}

#[test]
fn find_by_call_ids_returns_none_for_ambiguous_call_id() {
    let path = temp_db_path("ambiguous_call_id");
    let store = StateStore::new(&path, 21_600).unwrap();
    store
        .put(&stored_response("resp_1", vec!["call_1"]))
        .unwrap();
    store
        .put(&stored_response("resp_2", vec!["call_1"]))
        .unwrap();

    let found = store.find_by_call_ids(&["call_1".to_string()]).unwrap();

    assert!(found.is_none());
    let _ = std::fs::remove_file(path);
}

#[test]
fn find_by_call_ids_returns_none_when_requested_calls_are_split_across_responses() {
    let path = temp_db_path("split_calls");
    let store = StateStore::new(&path, 21_600).unwrap();
    store
        .put(&stored_response("resp_1", vec!["call_1"]))
        .unwrap();
    store
        .put(&stored_response("resp_2", vec!["call_2"]))
        .unwrap();

    let found = store
        .find_by_call_ids(&["call_1".to_string(), "call_2".to_string()])
        .unwrap();

    assert!(found.is_none());
    let _ = std::fs::remove_file(path);
}
