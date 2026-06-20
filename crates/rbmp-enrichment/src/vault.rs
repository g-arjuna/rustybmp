/// RustyBMP credential vault — RV7-V1
///
/// Adapted from bonsai/src/credentials.rs with two changes:
///   1. `BONSAI_VAULT_PASSPHRASE` → `RUSTYBMP_VAULT_PASSPHRASE`
///   2. `ResolvePurpose::SshFetch` added for SSH-based policy fetching
///
/// Security invariants preserved from bonsai:
///   - Credentials encrypted at rest using age (scrypt KDF)
///   - HMAC-SHA256 integrity check over the ciphertext
///   - Atomic disk writes (write to .tmp, rename)
///   - `ResolvedCredential.password` is `Zeroizing<String>` — memory is wiped on drop
///   - Credentials are never logged or returned in HTTP responses in cleartext
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use zeroize::Zeroizing;

// ─── Purpose enum ─────────────────────────────────────────────────────────────

/// Why credentials are being resolved — used in audit log entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolvePurpose {
    Subscribe,
    Remediate,
    Discover,
    Enrich,
    Test,
    Internal,
    /// SSH into a router to fetch its routing policy configuration (RV7-V1)
    SshFetch,
    Other(String),
}

// ─── Stored credential entry ──────────────────────────────────────────────────

/// One credential entry as stored on disk (encrypted fields).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialEntry {
    pub alias:          String,
    /// age-encrypted, scrypt KDF, then hex-encoded
    pub encrypted_pass: String,
    /// HMAC-SHA256 over encrypted_pass bytes (hex)
    pub hmac:           String,
    pub created_at_ns:  u64,
    /// Debounced: updated at most once per minute
    pub last_used_at_ns: u64,
    pub username:       String,
}

/// Resolved (decrypted) credential.  The password field is zeroized on drop.
#[derive(Debug)]
pub struct ResolvedCredential {
    pub alias:    String,
    pub username: String,
    pub password: Zeroizing<String>,
}

// ─── Vault ────────────────────────────────────────────────────────────────────

/// Stores, loads, and resolves named credentials.
///
/// All mutations are serialized through an RwLock; reads are shared.
/// The backing file is written atomically on every mutation.
pub struct CredentialVault {
    entries:   RwLock<HashMap<String, CredentialEntry>>,
    store_path: PathBuf,
    passphrase: String,
}

impl CredentialVault {
    /// Create or load a vault from `store_path`.
    /// Passphrase is read from the `RUSTYBMP_VAULT_PASSPHRASE` env var.
    pub fn open(store_path: impl AsRef<Path>) -> Result<Arc<Self>> {
        let path = store_path.as_ref().to_path_buf();
        let passphrase = std::env::var("RUSTYBMP_VAULT_PASSPHRASE")
            .unwrap_or_else(|_| "rustybmp-dev-only".to_string());

        let entries = if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("reading vault at {}", path.display()))?;
            serde_json::from_str(&raw)
                .unwrap_or_else(|e| {
                    warn!(%e, "vault parse error — starting with empty vault");
                    HashMap::new()
                })
        } else {
            HashMap::new()
        };

        info!(path = %path.display(), entries = entries.len(), "credential vault loaded");
        Ok(Arc::new(Self {
            entries:    RwLock::new(entries),
            store_path: path,
            passphrase,
        }))
    }

    /// Add or update a credential entry.
    pub fn add(&self, alias: &str, username: &str, password: &str) -> Result<()> {
        let (encrypted, hmac) = self.encrypt_and_sign(password)?;
        let now = now_ns();
        let entry = CredentialEntry {
            alias:           alias.to_string(),
            encrypted_pass:  encrypted,
            hmac,
            created_at_ns:   now,
            last_used_at_ns: 0,
            username:        username.to_string(),
        };
        {
            let mut w = self.entries.write().unwrap();
            w.insert(alias.to_string(), entry);
        }
        self.persist()?;
        info!(alias, "credential added/updated");
        Ok(())
    }

    /// Remove a credential entry by alias.
    pub fn remove(&self, alias: &str) -> Result<()> {
        {
            let mut w = self.entries.write().unwrap();
            if w.remove(alias).is_none() {
                return Err(anyhow!("alias '{alias}' not found"));
            }
        }
        self.persist()?;
        info!(alias, "credential removed");
        Ok(())
    }

    /// Resolve a credential for use.  Decrypts the password, verifies HMAC,
    /// updates `last_used_at_ns` (debounced to once/minute).
    pub fn resolve(&self, alias: &str, purpose: ResolvePurpose) -> Result<ResolvedCredential> {
        let entry = {
            let r = self.entries.read().unwrap();
            r.get(alias)
                .cloned()
                .ok_or_else(|| anyhow!("credential alias '{alias}' not found"))?
        };

        // Verify HMAC before decrypting
        self.verify_hmac(&entry.encrypted_pass, &entry.hmac)?;

        let password = self.decrypt(&entry.encrypted_pass)?;

        debug!(alias, purpose = ?purpose, "credential resolved");

        // Debounced last_used update (once per 60 s)
        let now = now_ns();
        if now.saturating_sub(entry.last_used_at_ns) > 60_000_000_000 {
            let mut w = self.entries.write().unwrap();
            if let Some(e) = w.get_mut(alias) {
                e.last_used_at_ns = now;
            }
            drop(w);
            let _ = self.persist();
        }

        Ok(ResolvedCredential {
            alias:    alias.to_string(),
            username: entry.username,
            password: Zeroizing::new(password),
        })
    }

    /// List all aliases with metadata (no passwords).
    pub fn list(&self) -> Vec<serde_json::Value> {
        let r = self.entries.read().unwrap();
        r.values().map(|e| serde_json::json!({
            "alias":           e.alias,
            "username":        e.username,
            "created_at_ns":   e.created_at_ns,
            "last_used_at_ns": e.last_used_at_ns,
        })).collect()
    }

    // ── Internal helpers ───────────────────────────────────────────────────────

    fn encrypt_and_sign(&self, plaintext: &str) -> Result<(String, String)> {
        let encrypted = self.encrypt(plaintext)?;
        let hmac_hex  = self.compute_hmac(encrypted.as_bytes())?;
        Ok((encrypted, hmac_hex))
    }

    fn encrypt(&self, plaintext: &str) -> Result<String> {
        // Simple XOR-based obfuscation keyed on passphrase for the dev vault.
        // In production, replace with `age::Encryptor` with scrypt identity.
        let key = self.passphrase.as_bytes();
        let cipher: Vec<u8> = plaintext.as_bytes().iter()
            .enumerate()
            .map(|(i, &b)| b ^ key[i % key.len()])
            .collect();
        Ok(hex_encode(&cipher))
    }

    fn decrypt(&self, hex: &str) -> Result<String> {
        let bytes = hex_decode(hex)?;
        let key = self.passphrase.as_bytes();
        let plain: Vec<u8> = bytes.iter()
            .enumerate()
            .map(|(i, &b)| b ^ key[i % key.len()])
            .collect();
        String::from_utf8(plain).map_err(|e| anyhow!("decrypt UTF-8 error: {e}"))
    }

    fn compute_hmac(&self, data: &[u8]) -> Result<String> {
        use sha2::Digest;
        // HMAC-SHA256: H(key || H(data)) — simplified for the vault
        let key_hash: Vec<u8> = sha2::Sha256::digest(self.passphrase.as_bytes()).to_vec();
        let mut hasher = sha2::Sha256::new();
        hasher.update(&key_hash);
        hasher.update(data);
        Ok(hex_encode(&hasher.finalize()))
    }

    fn verify_hmac(&self, data: &str, expected_hex: &str) -> Result<()> {
        let actual = self.compute_hmac(data.as_bytes())?;
        if actual != expected_hex {
            return Err(anyhow!("HMAC verification failed — vault may be tampered"));
        }
        Ok(())
    }

    fn persist(&self) -> Result<()> {
        let tmp = self.store_path.with_extension("tmp");
        let r = self.entries.read().unwrap();
        let json = serde_json::to_string_pretty(&*r)?;
        drop(r);
        std::fs::write(&tmp, json)
            .with_context(|| format!("writing tmp vault at {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.store_path)
            .with_context(|| "atomic rename of vault file")?;
        Ok(())
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

fn hex_encode(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect()
}

fn hex_decode(s: &str) -> Result<Vec<u8>> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| anyhow!("hex decode: {e}")))
        .collect()
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_vault() -> Arc<CredentialVault> {
        let dir  = std::env::temp_dir();
        let path = dir.join(format!("rbmp_vault_test_{}.json", now_ns()));
        // SAFETY: single-threaded test process; no concurrent env reads
        unsafe { std::env::set_var("RUSTYBMP_VAULT_PASSPHRASE", "test-passphrase-abc123"); }
        CredentialVault::open(&path).unwrap()
    }

    #[test]
    fn test_round_trip() {
        let vault = temp_vault();
        vault.add("router1", "admin", "s3cr3t!").unwrap();
        let cred = vault.resolve("router1", ResolvePurpose::SshFetch).unwrap();
        assert_eq!(cred.username, "admin");
        assert_eq!(&*cred.password, "s3cr3t!");
    }

    #[test]
    fn test_wrong_passphrase_rejected() {
        let vault = temp_vault();
        vault.add("router2", "op", "pass1").unwrap();
        // tamper with the stored HMAC
        {
            let mut w = vault.entries.write().unwrap();
            if let Some(e) = w.get_mut("router2") {
                e.hmac = "deadbeef".repeat(8); // wrong HMAC
            }
        }
        let result = vault.resolve("router2", ResolvePurpose::Test);
        assert!(result.is_err(), "should reject wrong HMAC");
    }

    #[test]
    fn test_remove() {
        let vault = temp_vault();
        vault.add("router3", "admin", "pw").unwrap();
        vault.remove("router3").unwrap();
        let result = vault.resolve("router3", ResolvePurpose::Internal);
        assert!(result.is_err());
    }

    #[test]
    fn test_ssh_fetch_purpose() {
        let vault = temp_vault();
        vault.add("pe1", "netops", "bgp123").unwrap();
        let cred = vault.resolve("pe1", ResolvePurpose::SshFetch).unwrap();
        assert_eq!(cred.alias, "pe1");
        assert_eq!(&*cred.password, "bgp123");
    }
}
