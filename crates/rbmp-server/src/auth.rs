/// JWT authentication middleware and /auth endpoint (RV4-1).
///
/// When [auth] enabled = true in config:
///   POST /auth   { "api_key": "<key>" } → { "token": "<JWT>" }
///   All /api/* endpoints require: Authorization: Bearer <token>
///
/// When disabled (default): all requests pass through unchanged.
use axum::{
    body::Body,
    extract::{Request, State},
    http::{Response, StatusCode},
    middleware::Next,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use jsonwebtoken::{
    decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::config::AuthConfig;
use crate::state::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: i64,
    pub iat: i64,
}

/// Axum middleware: validate Bearer JWT on /api/* when auth is enabled.
pub async fn require_auth(
    State(auth_cfg): State<Arc<AuthConfig>>,
    request:         Request,
    next:            Next,
) -> Result<Response<Body>, StatusCode> {
    if !auth_cfg.enabled {
        return Ok(next.run(request).await);
    }

    let token = request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    decode::<Claims>(
        token,
        &DecodingKey::from_secret(auth_cfg.jwt_secret.as_bytes()),
        &Validation::new(Algorithm::HS256),
    )
    .map_err(|_| StatusCode::UNAUTHORIZED)?;

    Ok(next.run(request).await)
}

/// POST /auth — exchange an API key for a JWT bearer token.
#[derive(Deserialize)]
pub struct AuthRequest {
    pub api_key: String,
}

#[derive(Serialize)]
pub struct AuthResponse {
    pub token:      String,
    pub expires_in: u64,
}

pub async fn auth_handler(
    State(state): State<AppState>,
    Json(body):   Json<AuthRequest>,
) -> impl IntoResponse {
    let auth_cfg = &state.auth_cfg;
    if !auth_cfg.api_keys.contains(&body.api_key) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "invalid api_key" })),
        );
    }

    let now    = Utc::now().timestamp();
    let claims = Claims {
        sub: "api".into(),
        iat: now,
        exp: now + auth_cfg.token_ttl_secs as i64,
    };

    match encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(auth_cfg.jwt_secret.as_bytes()),
    ) {
        Ok(token) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "token":      token,
                "expires_in": auth_cfg.token_ttl_secs,
            })),
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "token encoding failed" })),
        ),
    }
}

/// Per-speaker token bucket rate limiter (RV4-1 T3).
///
/// Configured by auth.rate_limit_msgs_per_sec in config.
/// Each BMP speaker gets its own bucket. Call `allow()` per message;
/// returns false if the bucket is empty (message should be dropped).
pub struct TokenBucket {
    tokens:      f64,
    capacity:    f64,
    refill_rate: f64, // tokens per millisecond
    last_refill: std::time::Instant,
}

impl TokenBucket {
    pub fn new(msgs_per_sec: u32) -> Self {
        let rate = msgs_per_sec as f64 / 1000.0; // per-ms
        Self {
            tokens:      msgs_per_sec as f64,
            capacity:    msgs_per_sec as f64,
            refill_rate: rate,
            last_refill: std::time::Instant::now(),
        }
    }

    fn refill(&mut self) {
        let now     = std::time::Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_millis() as f64;
        self.tokens  = (self.tokens + elapsed * self.refill_rate).min(self.capacity);
        self.last_refill = now;
    }

    /// Returns true if the message is allowed, false if rate-limited (drop).
    pub fn allow(&mut self) -> bool {
        if self.capacity == 0.0 {
            return true; // 0 = unlimited
        }
        self.refill();
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_bucket_unlimited() {
        let mut b = TokenBucket::new(0);
        for _ in 0..10_000 {
            assert!(b.allow());
        }
    }

    #[test]
    fn token_bucket_exhaustion() {
        let mut b = TokenBucket::new(10);
        let mut allowed = 0usize;
        for _ in 0..20 {
            if b.allow() { allowed += 1; }
        }
        assert!(allowed <= 10, "should exhaust after capacity");
    }

    #[test]
    fn token_bucket_capacity_1_allows_exactly_one_burst() {
        let mut b = TokenBucket::new(1);
        assert!(b.allow(),  "first call within capacity must succeed");
        assert!(!b.allow(), "second call with empty bucket must be denied");
    }

    #[test]
    fn token_bucket_capacity_100_allows_100_burst() {
        let mut b = TokenBucket::new(100);
        let mut allowed = 0usize;
        for _ in 0..200 {
            if b.allow() { allowed += 1; }
        }
        assert_eq!(allowed, 100, "exactly 100 must be allowed before exhaustion");
    }

    #[test]
    fn token_bucket_refills_over_time() {
        let mut b = TokenBucket::new(1000);
        // Drain the bucket
        for _ in 0..1000 { b.allow(); }
        assert!(!b.allow(), "must be exhausted");
        // Sleep 10ms → should refill ~10 tokens
        std::thread::sleep(std::time::Duration::from_millis(15));
        let mut refilled = 0usize;
        for _ in 0..20 {
            if b.allow() { refilled += 1; }
        }
        assert!(refilled >= 10, "after 15ms at 1000/s must have refilled at least 10 tokens; got {refilled}");
    }

    #[test]
    fn claims_fields_round_trip() {
        let now = 1_700_000_000i64;
        let c = Claims { sub: "api".into(), exp: now + 86400, iat: now };
        assert_eq!(c.sub, "api");
        assert_eq!(c.exp - c.iat, 86400);
    }

    #[test]
    fn jwt_encode_decode_roundtrip() {
        use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
        let secret = b"test-secret-32-bytes-minimum!!!";
        let now    = Utc::now().timestamp();
        let claims = Claims { sub: "roundtrip".into(), iat: now, exp: now + 3600 };

        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret),
        ).expect("encode must succeed");

        let decoded = decode::<Claims>(
            &token,
            &DecodingKey::from_secret(secret),
            &Validation::new(Algorithm::HS256),
        ).expect("decode must succeed");

        assert_eq!(decoded.claims.sub, "roundtrip");
    }

    #[test]
    fn jwt_wrong_secret_rejected() {
        use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
        let now    = Utc::now().timestamp();
        let claims = Claims { sub: "x".into(), iat: now, exp: now + 3600 };
        let token  = encode(&Header::default(), &claims,
                            &EncodingKey::from_secret(b"secret-a-32-bytes-padding!!!!!")).unwrap();
        let result = decode::<Claims>(&token,
                                     &DecodingKey::from_secret(b"secret-b-32-bytes-padding!!!!!"),
                                     &Validation::new(Algorithm::HS256));
        assert!(result.is_err(), "token signed with different secret must be rejected");
    }
}
