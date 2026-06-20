/// TLS listener for BMP TCP connections (RV4-1 T2).
///
/// Wraps `tokio::net::TcpListener` with a `tokio_rustls::TlsAcceptor`.
/// When `cfg.tls.enabled = false`, falls back to plain TCP (zero cost).
///
/// Usage in receiver.rs:
///   let acceptor = tls::build_acceptor(&cfg.tls)?;
///   // Then per-connection:
///   let stream = tls::accept(acceptor.as_ref(), tcp_stream).await?;
use std::fs;
use std::sync::Arc;

use anyhow::{Context, Result};
use rustls::ServerConfig;
use rustls_pemfile::{certs, pkcs8_private_keys};
use tokio::net::TcpStream;
use tokio_rustls::{server::TlsStream, TlsAcceptor};

use crate::config::TlsConfig;

/// Build a `TlsAcceptor` from the configured cert/key paths.
/// Returns `None` when TLS is disabled.
pub fn build_acceptor(cfg: &TlsConfig) -> Result<Option<TlsAcceptor>> {
    if !cfg.enabled {
        return Ok(None);
    }

    let cert_pem = fs::read(&cfg.cert_pem)
        .with_context(|| format!("reading TLS cert: {}", cfg.cert_pem))?;
    let key_pem  = fs::read(&cfg.key_pem)
        .with_context(|| format!("reading TLS key: {}", cfg.key_pem))?;

    let certs = certs(&mut cert_pem.as_slice())
        .collect::<Result<Vec<_>, _>>()
        .context("parsing TLS certificate chain")?;

    let mut keys = pkcs8_private_keys(&mut key_pem.as_slice())
        .collect::<Result<Vec<_>, _>>()
        .context("parsing TLS private key")?;

    let key = keys.pop().context("no PKCS#8 private key found in key_pem")?;

    let tls_cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, rustls::pki_types::PrivateKeyDer::Pkcs8(key))
        .context("building TLS ServerConfig")?;

    Ok(Some(TlsAcceptor::from(Arc::new(tls_cfg))))
}

/// Accept a TLS handshake on an already-accepted TCP stream.
/// Caller must have obtained `acceptor` from `build_acceptor`.
pub async fn accept(
    acceptor: &TlsAcceptor,
    stream: TcpStream,
) -> Result<TlsStream<TcpStream>> {
    acceptor.accept(stream).await.context("TLS handshake failed")
}
