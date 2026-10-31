use amara_router::{config, engine};
use std::path::PathBuf;

fn usage() -> ! {
    eprintln!("usage: amara-router [--config PATH] [--check]\n  default config: $AMARA_ROUTER_CONFIG or ./config.toml\n  --check: validate the config and secrets, then exit");
    std::process::exit(2)
}

#[tokio::main]
async fn main() {
    let mut path: Option<PathBuf> = std::env::var_os("AMARA_ROUTER_CONFIG").map(PathBuf::from);
    let mut check = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--config" | "-c" => {
                path = Some(args.next().map(PathBuf::from).unwrap_or_else(|| usage()))
            }
            "--check" => check = true,
            _ => usage(),
        }
    }
    let path = path.unwrap_or_else(|| PathBuf::from("config.toml"));
    let fail = |e: String| -> ! {
        eprintln!("[amara-router] {e}");
        std::process::exit(1)
    };
    let cfg = config::Config::load(&path).unwrap_or_else(|e| fail(e));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(m) = std::fs::metadata(&path) {
            if m.permissions().mode() & 0o077 != 0 {
                eprintln!(
                    "[amara-router] warning: {} is readable by others; run: chmod 600 {}",
                    path.display(),
                    path.display()
                );
            }
        }
    }
    let secrets = match &cfg.secrets_file {
        Some(p) => config::load_secrets(&config::expand_home(p)).unwrap_or_else(|e| fail(e)),
        None => config::Secrets::default(),
    };
    if check {
        println!(
            "config OK: fast={} deep={} bind={} lan={:?} cloud={} secrets={secrets:?}",
            cfg.models.fast, cfg.models.deep, cfg.bind, cfg.lan_bind, cfg.cloud.anthropic_enabled
        );
        return;
    }
    let state = engine::State::new(cfg, secrets, config::outbound_disabled());
    let shutdown = async {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("signal");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
        eprintln!("[amara-router] shutting down");
    };
    if let Err(e) = amara_router::run(state, shutdown).await {
        fail(e);
    }
}
