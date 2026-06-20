/// RV7-V2: Credential Vault CRUD API
///
/// Endpoints (all require JWT auth via the normal middleware):
///   GET    /api/credentials            — list aliases + metadata (no passwords)
///   POST   /api/credentials            — add / update a credential
///   DELETE /api/credentials/{alias}    — remove a credential
///
/// Passwords are NEVER returned in any response.
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct AddCredentialRequest {
    pub alias:    String,
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct CredentialListResponse {
    pub count:   usize,
    pub aliases: Vec<Value>,
}

/// GET /api/credentials
pub async fn list_credentials(
    State(state): State<AppState>,
) -> Json<CredentialListResponse> {
    let aliases = state.vault.list();
    let count   = aliases.len();
    Json(CredentialListResponse { count, aliases })
}

/// POST /api/credentials
pub async fn add_credential(
    State(state): State<AppState>,
    Json(body):   Json<AddCredentialRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if body.alias.is_empty() || body.username.is_empty() || body.password.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "alias, username and password are required" })),
        ));
    }
    match state.vault.add(&body.alias, &body.username, &body.password) {
        Ok(()) => Ok(Json(json!({ "status": "ok", "alias": body.alias }))),
        Err(e) => {
            tracing::warn!(alias = %body.alias, %e, "vault add failed");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            ))
        }
    }
}

/// DELETE /api/credentials/{alias}
pub async fn delete_credential(
    State(state): State<AppState>,
    Path(alias):  Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.vault.remove(&alias) {
        Ok(()) => Ok(Json(json!({ "status": "ok", "removed": alias }))),
        Err(e) => Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": e.to_string() })),
        )),
    }
}
