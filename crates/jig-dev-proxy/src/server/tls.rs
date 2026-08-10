use std::fs::{File, OpenOptions};
use std::io::{BufReader, ErrorKind};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use rustls::{Error as RustlsError, ServerConfig};
use tokio_rustls::TlsAcceptor;

use crate::state::StateStore;

use super::{TLS_RELOAD_FILE_ATTEMPTS, TLS_RELOAD_FILE_RETRY_DELAY};

pub(super) fn tls_acceptor(store: &StateStore, http2: bool) -> Result<TlsAcceptor> {
    tls_acceptor_with_retry(store, http2, || {
        std::thread::sleep(TLS_RELOAD_FILE_RETRY_DELAY);
    })
}

pub(super) fn tls_acceptor_with_retry(
    store: &StateStore,
    http2: bool,
    mut retry: impl FnMut(),
) -> Result<TlsAcceptor> {
    let mut last_error = None;
    for attempt in 0..TLS_RELOAD_FILE_ATTEMPTS {
        match tls_acceptor_once(store, http2) {
            Ok(acceptor) => return Ok(acceptor),
            Err(error)
                if attempt + 1 < TLS_RELOAD_FILE_ATTEMPTS
                    && tls_acceptor_error_is_retryable(&error) =>
            {
                last_error = Some(error);
                retry();
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow!("Failed to load TLS certificate")))
}

fn tls_acceptor_error_is_retryable(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == ErrorKind::NotFound)
            || cause
                .downcast_ref::<RustlsError>()
                .is_some_and(|error| matches!(error, RustlsError::InconsistentKeys(_)))
    })
}

pub(super) fn tls_acceptor_once(store: &StateStore, http2: bool) -> Result<TlsAcceptor> {
    let cert_file = open_tls_file(&store.leaf_path())?;
    let key_file = open_tls_file(&store.leaf_key_path())?;
    let certs = rustls_pemfile::certs(&mut BufReader::new(cert_file))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let key = rustls_pemfile::private_key(&mut BufReader::new(key_file))?
        .context("TLS key file did not contain a private key")?;
    let mut config =
        ServerConfig::builder_with_provider(rustls::crypto::aws_lc_rs::default_provider().into())
            // Local dev browsers still vary; rustls safe defaults intentionally
            // allow TLS 1.2 and 1.3 here.
            .with_safe_default_protocol_versions()?
            .with_no_client_auth()
            .with_single_cert(certs, key)?;
    config.alpn_protocols = if http2 {
        vec![b"h2".to_vec(), b"http/1.1".to_vec()]
    } else {
        vec![b"http/1.1".to_vec()]
    };
    Ok(TlsAcceptor::from(Arc::new(config)))
}

pub(super) fn open_tls_file(path: &Path) -> Result<File> {
    // Called from tls_acceptor through TlsCache::acceptor's spawn_blocking
    // path, so the short NotFound retry sleep stays off Tokio worker threads.
    let mut last_error = None;
    for attempt in 0..TLS_RELOAD_FILE_ATTEMPTS {
        match open_tls_file_once(path) {
            Ok(file) => return Ok(file),
            Err(error)
                if error.kind() == ErrorKind::NotFound
                    && attempt + 1 < TLS_RELOAD_FILE_ATTEMPTS =>
            {
                last_error = Some(error);
                std::thread::sleep(TLS_RELOAD_FILE_RETRY_DELAY);
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("Failed to open TLS certificate file {}", path.display())
                });
            }
        }
    }
    let error = last_error.unwrap_or_else(|| std::io::Error::from(ErrorKind::NotFound));
    Err(anyhow::Error::new(error))
        .with_context(|| format!("Failed to open TLS certificate file {}", path.display()))
}

pub(super) fn open_tls_file_once(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            ErrorKind::InvalidInput,
            "TLS file is not a regular file",
        ));
    }
    Ok(file)
}
