/// RV7-V3: SSH Policy Fetch API
///
/// Triggers an out-of-band SSH-based policy configuration retrieval for a
/// given router.  The Rust handler resolves credentials from the vault and
/// spawns `bmppy/policy_fetcher.py` as a child process, passing credentials
/// via env vars (never CLI args, never in logs).
///
/// POST /api/policy/fetch
///   Body: { "router_addr": "10.1.1.1", "credential_alias": "pe1", "platform": "ios-xr" }
///   Response: { "status": "queued" | "ok", "job_id": "<uuid>" }
///
/// GET  /api/policy/fetch/{job_id}
///   Response: { "status": "running"|"done"|"failed", "output": "..." }
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;
use chrono::Utc;
use rbmp_enrichment::ResolvePurpose;
use crate::state::AppState;

// ─── Job registry ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct FetchJob {
    pub job_id:       String,
    pub router_addr:  String,
    pub platform:     String,
    pub started_at:   String,
    pub status:       String,   // "running" | "done" | "failed"
    pub output:       String,
}

/// Global in-process job registry — suitable for a single-node server.
/// Production would use a DuckDB table instead.
type JobRegistry = Arc<Mutex<HashMap<String, FetchJob>>>;

pub fn new_job_registry() -> JobRegistry {
    Arc::new(Mutex::new(HashMap::new()))
}

// ─── Request / response types ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct PolicyFetchRequest {
    pub peer_addr:         String,
    pub speaker_addr:      Option<String>,
    pub credential_alias:  String,
    pub vendor:            String,
    pub policy:            String,
    pub direction:         Option<String>,
    pub port:              Option<u16>,
}

// ─── POST /api/policy/fetch ───────────────────────────────────────────────────

pub async fn trigger_policy_fetch(
    State(state): State<AppState>,
    Json(body):   Json<PolicyFetchRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if body.peer_addr.is_empty() || body.credential_alias.is_empty()
        || body.vendor.is_empty() || body.policy.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "peer_addr, credential_alias, vendor and policy are required" })),
        ));
    }

    // Resolve credentials from vault — fail fast if not found
    let cred = state.vault
        .resolve(&body.credential_alias, ResolvePurpose::SshFetch)
        .map_err(|e| (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": format!("credential error: {e}") })),
        ))?;

    let job_id   = Uuid::new_v4().to_string();
    let direction = body.direction.clone().unwrap_or_else(|| "in".to_string());
    let port      = body.port.unwrap_or(22);

    // Build job entry
    let job = FetchJob {
        job_id:      job_id.clone(),
        router_addr: body.peer_addr.clone(),
        platform:    body.vendor.clone(),
        started_at:  Utc::now().to_rfc3339(),
        status:      "running".to_string(),
        output:      String::new(),
    };

    // Insert into registry
    {
        let mut reg = state.policy_jobs.lock().unwrap();
        reg.insert(job_id.clone(), job);
    }

    // Spawn policy_fetcher.py as a child process.
    // Credentials are passed via env vars, never CLI args.
    let peer_addr_clone = body.peer_addr.clone();
    let vendor_clone    = body.vendor.clone();
    let policy_clone    = body.policy.clone();
    let job_id_clone    = job_id.clone();
    let registry_clone  = Arc::clone(&state.policy_jobs);
    let username        = cred.username.clone();
    let password        = cred.password.as_str().to_string();

    // Detect python binary: prefer .venv if present
    let python_bin = if std::path::Path::new(".venv/bin/python").exists() {
        ".venv/bin/python".to_string()
    } else {
        "python3".to_string()
    };

    tokio::spawn(async move {
        let result = tokio::process::Command::new(&python_bin)
            .arg("bmppy/policy_fetcher.py")
            .arg("--peer-addr").arg(&peer_addr_clone)
            .arg("--vendor").arg(&vendor_clone)
            .arg("--policy").arg(&policy_clone)
            .arg("--direction").arg(&direction)
            .arg("--port").arg(port.to_string())
            .env("RUSTYBMP_SSH_USERNAME", &username)
            .env("RUSTYBMP_SSH_PASSWORD", &password)
            .output()
            .await;

        let (new_status, output_text) = match result {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                if out.status.success() {
                    ("done".to_string(), stdout)
                } else {
                    ("failed".to_string(), format!("stderr: {stderr}\nstdout: {stdout}"))
                }
            }
            Err(e) => ("failed".to_string(), format!("spawn error: {e}")),
        };

        let mut reg = registry_clone.lock().unwrap();
        if let Some(job) = reg.get_mut(&job_id_clone) {
            job.status = new_status;
            job.output = output_text;
        }
    });

    Ok(Json(json!({ "status": "queued", "job_id": job_id })))
}

// ─── GET /api/policy/fetch/{job_id} ──────────────────────────────────────────

pub async fn get_fetch_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let reg = state.policy_jobs.lock().unwrap();
    match reg.get(&job_id) {
        Some(job) => Ok(Json(serde_json::to_value(job).unwrap_or_default())),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("job '{job_id}' not found") })),
        )),
    }
}
