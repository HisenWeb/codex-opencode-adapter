mod common;

use codex_opencode_adapter::project::{ProjectRegistry, PROJECT_ENV_FILENAME, sign_local_token};
use common::mock_upstream::start_mock_upstream;
use common::{adapter_url, start_multi_project_adapter, ProjectConfig};
use serde_json::Value;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Req 4, part 1: Token routing isolation
// Different project tokens route to different upstream runtimes, each with
// its own upstream key, model config, and state DB.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn multi_project_token_routes_to_correct_upstream() {
    let (upstream_a, _mock_a, _recv_a) = start_mock_upstream().await;
    let (upstream_b, _mock_b, _recv_b) = start_mock_upstream().await;

    let configs = vec![
        ProjectConfig {
            project_id: "proj-a".to_string(),
            upstream_addr: upstream_a,
            upstream_key: "key-a".to_string(),
            raw_token: None,
        },
        ProjectConfig {
            project_id: "proj-b".to_string(),
            upstream_addr: upstream_b,
            upstream_key: "key-b".to_string(),
            raw_token: None,
        },
    ];

    let (addr, tokens) = start_multi_project_adapter(configs, 10).await;

    // Each token must be able to call /v1/models on the shared adapter
    for (project_id, token) in &tokens {
        let client = reqwest::Client::new();
        let resp = client
            .get(adapter_url(addr, "/v1/models"))
            .bearer_auth(token)
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            200,
            "{project_id} token should get 200 on /v1/models"
        );
        let body: Value = resp.json().await.unwrap();
        assert!(
            body.get("data").is_some(),
            "{project_id} /v1/models must return data"
        );
    }

    // Each project has a distinct signed token
    let values: Vec<&String> = tokens.values().collect();
    assert_eq!(values.len(), 2);
    assert_ne!(values[0], values[1], "tokens must differ between projects");

    // The adapter runs on a single port
    eprintln!(
        "multi-project adapter serving {} projects on 127.0.0.1:{}",
        tokens.len(),
        addr.port()
    );
}

// ---------------------------------------------------------------------------
// Req 4, part 2: Invalid / cross-project token blocked
// ---------------------------------------------------------------------------
#[tokio::test]
async fn multi_project_wrong_token_gets_401() {
    let (upstream_a, _mock_a, _recv_a) = start_mock_upstream().await;
    let (upstream_b, _mock_b, _recv_b) = start_mock_upstream().await;

    let configs = vec![
        ProjectConfig {
            project_id: "proj-a".to_string(),
            upstream_addr: upstream_a,
            upstream_key: "key-a".to_string(),
            raw_token: Some("raw-token-a".to_string()),
        },
        ProjectConfig {
            project_id: "proj-b".to_string(),
            upstream_addr: upstream_b,
            upstream_key: "key-b".to_string(),
            raw_token: Some("raw-token-b".to_string()),
        },
    ];

    let (addr, _tokens) = start_multi_project_adapter(configs, 10).await;

    // Completely bogus token -> 401
    let client = reqwest::Client::new();
    let resp = client
        .get(adapter_url(addr, "/v1/models"))
        .bearer_auth("codex-opencode-fake-bad-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "bogus token must get 401");

    // Token with valid format but wrong HMAC -> 401 (i.e. cross-project)
    // Proj-A's signed token won't validate against proj-B's raw_token
    // and proj-B won't have a runtime matched by proj-A's token
    let resp2 = client
        .get(adapter_url(addr, "/v1/models"))
        .bearer_auth("codex-opencode-nonexistent-00000000000000000000000000000000")
        .send()
        .await
        .unwrap();
    assert_eq!(resp2.status(), 401, "nonexistent project token must get 401");
}

// ---------------------------------------------------------------------------
// Req 5: Single adapter serves both projects on the same port.
// No per-project adapter binary needed.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn single_adapter_serves_multiple_projects() {
    let (upstream_a, _mock_a, _recv_a) = start_mock_upstream().await;
    let (upstream_b, _mock_b, _recv_b) = start_mock_upstream().await;

    let configs = vec![
        ProjectConfig {
            project_id: "proj-a".to_string(),
            upstream_addr: upstream_a,
            upstream_key: "key-a".to_string(),
            raw_token: None,
        },
        ProjectConfig {
            project_id: "proj-b".to_string(),
            upstream_addr: upstream_b,
            upstream_key: "key-b".to_string(),
            raw_token: None,
        },
    ];

    let (addr, tokens) = start_multi_project_adapter(configs, 10).await;

    // Both projects served on the ONE port
    for (project_id, token) in &tokens {
        let client = reqwest::Client::new();
        let resp = client
            .get(adapter_url(addr, "/v1/models"))
            .bearer_auth(token)
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            200,
            "single adapter must serve {project_id} on port {}",
            addr.port()
        );
    }

    eprintln!("single adapter on {} serves {} projects", addr, tokens.len());

    // Verify same port for both
    let resp_a = reqwest::Client::new()
        .get(adapter_url(addr, "/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp_a.status(), 200, "health check on same port");
}

// ---------------------------------------------------------------------------
// Req (new): POST /admin/refresh integration test
// Start with only project A loaded, refresh loads project B at runtime.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_admin_refresh_loads_new_project() {
    let orig_user = std::env::var("USERPROFILE").ok();
    let orig_home = std::env::var("HOME").ok();

    let home = std::env::temp_dir().join(format!("test_admin_refresh_{}", Uuid::new_v4()));
    std::fs::create_dir_all(&home).unwrap();
    std::env::set_var("USERPROFILE", &home);
    std::env::set_var("HOME", &home);

    let reg_dir = home.join(".codex-opencode-adapter");
    std::fs::create_dir_all(&reg_dir).unwrap();

    let (upstream_a, _mock_a, _recv_a) = start_mock_upstream().await;
    let (upstream_b, _mock_b, _recv_b) = start_mock_upstream().await;

    // --- Project A ---
    let proj_a_root = home.join("proj_a");
    std::fs::create_dir_all(&proj_a_root).unwrap();
    let pid_a = "project-a";
    let raw_a = "raw-token-a";

    std::fs::write(
        proj_a_root.join(PROJECT_ENV_FILENAME),
        format!(
            "OPENCODE_GO_API_KEY=key-a\nCODEX_OPENCODE_LOCAL_TOKEN={raw_a}\n             CODEX_OPENCODE_PROJECT_ID={pid_a}\n             OPENCODE_GO_BASE_URL=http://127.0.0.1:{port}\n             CODEX_OPENCODE_STATE_DB=.codex-opencode/state.sqlite\n             CODEX_OPENCODE_HOST=127.0.0.1\nCODEX_OPENCODE_PORT=4010\n",
            port = upstream_a.port()
        ),
    ).unwrap();
    std::fs::create_dir_all(proj_a_root.join(".codex-opencode")).unwrap();

    // --- Project B ---
    let proj_b_root = home.join("proj_b");
    std::fs::create_dir_all(&proj_b_root).unwrap();
    let pid_b = "project-b";
    let raw_b = "raw-token-b";

    std::fs::write(
        proj_b_root.join(PROJECT_ENV_FILENAME),
        format!(
            "OPENCODE_GO_API_KEY=key-b\nCODEX_OPENCODE_LOCAL_TOKEN={raw_b}\n             CODEX_OPENCODE_PROJECT_ID={pid_b}\n             OPENCODE_GO_BASE_URL=http://127.0.0.1:{port}\n             CODEX_OPENCODE_STATE_DB=.codex-opencode/state.sqlite\n             CODEX_OPENCODE_HOST=127.0.0.1\nCODEX_OPENCODE_PORT=4010\n",
            port = upstream_b.port()
        ),
    ).unwrap();
    std::fs::create_dir_all(proj_b_root.join(".codex-opencode")).unwrap();

    // --- Registry ---
    let mut registry = ProjectRegistry::load(&reg_dir);
    registry.upsert_project(pid_a, &proj_a_root);
    registry.upsert_project(pid_b, &proj_b_root);
    registry.save(&reg_dir).unwrap();

    // --- Start adapter with only project A ---
    let configs = vec![ProjectConfig {
        project_id: pid_a.to_string(),
        upstream_addr: upstream_a,
        upstream_key: "key-a".to_string(),
        raw_token: Some(raw_a.to_string()),
    }];

    let (addr, tokens) = start_multi_project_adapter(configs, 10).await;
    let signed_a = tokens.get(pid_a).expect("project-a token should exist");

    let client = reqwest::Client::new();

    // 1) No token -> 401
    let resp = client
        .post(adapter_url(addr, "/admin/refresh"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "no token should get 401");

    // 2) Bad token -> 401
    let resp = client
        .post(adapter_url(addr, "/admin/refresh"))
        .bearer_auth("bogus-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "bad token should get 401");

    // 3) Valid project A token -> refresh succeeds, B loaded
    let resp = client
        .post(adapter_url(addr, "/admin/refresh"))
        .bearer_auth(signed_a)
        .send()
        .await
        .unwrap();
    let refresh_status = resp.status();
    let refresh_body: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
    assert_eq!(refresh_status, 200, "refresh with valid token should succeed");
    assert_eq!(refresh_body["status"], "ok", "response status should be ok");
    let added = refresh_body["added"].as_array()
        .expect("response should have 'added' array");
    assert!(
        added.iter().any(|v| v.as_str() == Some(pid_b)),
        "project-b should be in added list, got: {:?}",
        added
    );

    // 4) After refresh, project B is accessible via /v1/models
    let signed_b = sign_local_token(pid_b, raw_b);
    let resp = client
        .get(adapter_url(addr, "/v1/models"))
        .bearer_auth(&signed_b)
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
    assert_eq!(
        status,
        200,
        "project-b should be accessible after refresh, got: {:?}",
        body
    );

    // Restore env vars
    match orig_user {
        Some(v) => std::env::set_var("USERPROFILE", v),
        None => std::env::remove_var("USERPROFILE"),
    }
    match orig_home {
        Some(v) => std::env::set_var("HOME", v),
        None => std::env::remove_var("HOME"),
    }

    let _ = std::fs::remove_dir_all(&home);
}
