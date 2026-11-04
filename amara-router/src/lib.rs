//! Amara Router: one local endpoint (OpenAI- and Ollama-compatible) that sends each request
//! to a resident fast model or an on-demand deep model. See README.md.

pub mod config;
pub mod engine;
pub mod route;
pub mod server;
pub mod think;
pub mod translate;

use std::sync::Arc;

/// Binds the listeners (loopback, plus LAN when configured) and serves until shutdown.
pub async fn run(
    state: Arc<engine::State>,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<(), String> {
    let cfg = &state.cfg;
    let local = tokio::net::TcpListener::bind(&cfg.bind)
        .await
        .map_err(|e| format!("cannot bind {} ({e}); is another router or service using the port? Check with: lsof -nP -iTCP:{} -sTCP:LISTEN", cfg.bind, cfg.bind.rsplit(':').next().unwrap_or("")))?;
    let lan = match config::lan_api_key(cfg, &state.secrets)? {
        Some(key) => {
            let addr = cfg.lan_bind.as_deref().unwrap_or_default();
            let l = tokio::net::TcpListener::bind(addr)
                .await
                .map_err(|e| format!("cannot bind LAN {addr}: {e}"))?;
            Some((l, key))
        }
        None => None,
    };
    eprintln!(
        "[amara-router] listening on http://{} (fast={}, deep={})",
        cfg.bind, cfg.models.fast, cfg.models.deep
    );
    if cfg.cloud.anthropic_enabled {
        eprintln!("[amara-router] WARNING: cloud tier (Anthropic) is ENABLED: requests that ask for Claude leave this Mac{}", if state.outbound_disabled { " (but NEXUS_OUTBOUND_DISABLED is set, so none will)" } else { "" });
    }
    let (tx, _) = tokio::sync::broadcast::channel::<()>(1);
    let mut tasks = Vec::new();
    let mut rx = tx.subscribe();
    let app = server::local_router(Arc::clone(&state));
    tasks.push(tokio::spawn(async move {
        axum::serve(local, app)
            .with_graceful_shutdown(async move {
                let _ = rx.recv().await;
            })
            .await
    }));
    if let Some((l, key)) = lan {
        eprintln!(
            "[amara-router] LAN listener on http://{} (Bearer auth required)",
            l.local_addr().map(|a| a.to_string()).unwrap_or_default()
        );
        let app = server::lan_router(Arc::clone(&state), key);
        let mut rx = tx.subscribe();
        tasks.push(tokio::spawn(async move {
            axum::serve(l, app)
                .with_graceful_shutdown(async move {
                    let _ = rx.recv().await;
                })
                .await
        }));
    }
    if cfg.loading.warm_fast_on_start {
        let st = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = st.warm_fast().await {
                eprintln!("[amara-router] could not warm the fast model yet: {e}");
            }
        });
    }
    shutdown.await;
    let _ = tx.send(());
    for t in tasks {
        let _ = t.await;
    }
    Ok(())
}
