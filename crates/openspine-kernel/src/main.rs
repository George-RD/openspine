//! `openspine` — the OpenSpine kernel binary.
//!
//! Implements `openspec/changes/implement-telegram-owner-control-slice/`
//! (4a-4d): config, storage, artifact store/loader, connectors, minimal
//! model gateway, and the axum kernel API, wired into two concurrent
//! long-running tasks — the HTTP API the shell talks to, and the Telegram
//! long-poll loop that turns owner messages into task grants.

mod action_catalog;
mod api;
mod artifact_loader;
mod artifact_store;
mod benchmark;
mod config;
mod connectors;
mod gmail;
mod model_gateway;
mod pipeline;
mod sandbox;
mod store;
mod telegram;

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod kernel_tests;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use crate::api::handler_registry::ActionHandlerRegistry;
use crate::connectors::ConnectorRegistry;
use anyhow::Context as _;
use clap::Parser;

/// Kernel-owned grant-chain verification key. Production requires explicit
/// secret intake; tests use a deterministic fixture key only under cfg(test).
pub(crate) fn grant_hmac_key() -> Option<Vec<u8>> {
    #[cfg(test)]
    {
        Some(b"openspine-test-grant-hmac-key-v1".to_vec())
    }
    #[cfg(not(test))]
    {
        std::env::var("OPENSPINE_GRANT_HMAC_KEY")
            .ok()
            .filter(|key| !key.is_empty())
            .map(|key| key.into_bytes())
    }
}

/// The kernel binary's own CLI — distinct from `openspine-shell`'s (which
/// takes `--kernel`/`--task`, never a config path: the shell never reads
/// `openspine.yaml`, only `KERNEL_ENDPOINT`/`TASK_TOKEN`, see
/// `docs/kernel-http-contract.md`).
#[derive(Debug, Parser)]
#[command(name = "openspine")]
struct Cli {
    /// Path to `openspine.yaml`.
    #[arg(long, default_value = "openspine.yaml")]
    config: PathBuf,

    /// Run benchmarks instead of starting the daemon.
    #[arg(long)]
    benchmark: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    if cli.benchmark {
        benchmark::run_benchmarks()?;
        return Ok(());
    }
    let cfg = config::Config::load(&cli.config)
        .with_context(|| format!("loading {}", cli.config.display()))?;

    let bot_token = config::telegram_bot_token()?;
    let artifact_key = config::artifact_key_bytes()?;

    let artifacts =
        artifact_store::ArtifactStore::open(cfg.data_dir.join("artifacts"), artifact_key)
            .context("opening artifact store")?;
    let store =
        store::Store::open(&cfg.data_dir.join("kernel.db")).context("opening kernel store")?;
    // PRD §18: the audit log is append-only and hash-chained specifically
    // so tampering/corruption is detectable — detect it now, at boot,
    // rather than never. A broken chain means someone edited the SQLite
    // file directly or the process crashed mid-write in a way that left a
    // torn row; either way, refuse to start rather than serve on top of an
    // audit trail that can no longer be trusted.
    if !store
        .verify_audit_chain()
        .context("verifying audit chain")?
    {
        anyhow::bail!(
            "audit_log hash chain is broken in {} — refusing to start on an untrustworthy audit trail",
            cfg.data_dir.join("kernel.db").display()
        );
    }
    let mut registry = artifact_loader::load_registry(&cfg.lyra_dir)
        .with_context(|| format!("loading artifact registry from {}", cfg.lyra_dir.display()))?;
    // 5a: the `data/artifacts.d` overlay holds artifacts activated via
    // `artifact.propose` approvals, so they survive restart. Same per-kind
    // subdir layout as the fixtures; a missing dir is an empty overlay. A
    // `(kind, id, version)` already in the fixtures fails startup rather
    // than silently shadowing it.
    let overlay_dir = cfg.data_dir.join("artifacts.d");
    artifact_loader::load_registry_into(&mut registry, &overlay_dir)
        .with_context(|| format!("loading artifact overlay from {}", overlay_dir.display()))?;

    let sandbox = match cfg.sandbox.driver {
        config::SandboxDriverKind::Process => {
            sandbox::Sandbox::Process(sandbox::ProcessDriver::default())
        }
        config::SandboxDriverKind::Docker => sandbox::Sandbox::Docker(sandbox::DockerDriver {
            image_tag: cfg
                .sandbox
                .docker_image
                .clone()
                .unwrap_or_else(|| "openspine-shell:latest".to_string()),
            network: cfg
                .sandbox
                .docker_network
                .clone()
                .unwrap_or_else(|| "openspine-internal".to_string()),
            run_as_uid: 10001,
        }),
    };

    let provider_config = cfg
        .providers
        .first()
        .ok_or_else(|| anyhow::anyhow!("openspine.yaml must configure at least one provider"))?;
    let provider_key = config::provider_api_key(provider_config)?;
    let provider = model_gateway::ProviderClient::from_config(provider_config, provider_key);

    let telegram = telegram::TelegramConnector::new(bot_token);

    let gmail = match &cfg.gmail {
        Some(gmail_cfg) => {
            let client_secret = config::gmail_client_secret(gmail_cfg)?;
            let refresh_token = config::gmail_refresh_token(gmail_cfg)?;
            Some(gmail::GmailConnector::new(
                gmail_cfg.client_id.clone(),
                client_secret,
                refresh_token,
                gmail_cfg.mailbox_address.clone(),
            ))
        }
        None => None,
    };

    let state = Arc::new(pipeline::AppState {
        store,
        artifacts,
        registry: parking_lot::RwLock::new(registry),
        action_catalog: crate::action_catalog::canonical_catalog(),
        sandbox,
        connectors: ConnectorRegistry::new(telegram, gmail),
        owner_user_id: cfg.owner.telegram_user_id,
        kernel_endpoint: cfg
            .kernel
            .advertise_endpoint
            .clone()
            .unwrap_or_else(|| format!("http://{}", cfg.kernel.bind_addr)),
        unsafe_allow_uncontained_private_data: cfg.unsafe_allow_uncontained_private_data,
        action_handlers: ActionHandlerRegistry::default_registrations(),
        provider,
        started_at: Instant::now(),
        overlay_dir,
    });

    let listener = tokio::net::TcpListener::bind(&cfg.kernel.bind_addr)
        .await
        .with_context(|| format!("binding {}", cfg.kernel.bind_addr))?;
    tracing::info!(addr = %cfg.kernel.bind_addr, owner = cfg.owner.telegram_user_id, "openspine kernel starting");

    let http_server =
        axum::serve(listener, api::router(state.clone())).with_graceful_shutdown(shutdown_signal());
    let telegram_poll = pipeline::run_telegram_poll_loop(&state);

    tokio::select! {
        res = http_server => res.context("http server failed")?,
        res = telegram_poll => res.context("telegram poll loop failed")?,
    }

    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown signal received, draining in-flight requests");
}
