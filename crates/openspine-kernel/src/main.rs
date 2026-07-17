mod action_catalog;
mod api;
mod artifact_loader;
mod artifact_store;
mod benchmark;
mod briefcase;
mod briefcase_visibility;
mod config;
mod connectors;
mod escalation;
mod failure_surfacing;
mod gmail;
mod identity;
#[cfg(test)]
mod kernel_tests;
mod model_gateway;
mod model_swap;
mod model_swap_recovery;
mod overlay_compat;
mod overlay_eval_gate;
mod overlay_recovery;
mod overlay_startup;
mod pipeline;
mod sandbox;
mod secret_intake;
mod secret_store;
mod spend;
mod store;
mod telegram;
mod workflow;
pub mod workflow_state_machine;

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod model_swap_recovery_tests;

use crate::api::handler_registry::ActionHandlerRegistry;
use crate::connectors::ConnectorRegistry;
use anyhow::Context as _;
use clap::Parser;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
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
pub(crate) fn commit_post_bind_clock(
    store: &store::Store,
    pre_setup_ms: i64,
    clock: impl Fn() -> i64,
) -> anyhow::Result<()> {
    let commit_now_ms = clock();
    if commit_now_ms < pre_setup_ms.saturating_sub(60_000) {
        anyhow::bail!(
            "wall clock regressed during startup: post-bind now ({commit_now_ms} ms) is behind pre-setup candidate ({pre_setup_ms} ms)"
        );
    }
    let commit_ms = pre_setup_ms.max(commit_now_ms);
    match store.commit_boot_clock(commit_ms).context("committing boot clock high-water")? {
        store::BootClockCheck::Ok { .. } => Ok(()),
        store::BootClockCheck::Regressed { high_water_ms, now_ms } => anyhow::bail!(
            "wall clock regressed during startup: now ({now_ms} ms) is behind the persisted high-water ({high_water_ms} ms)"
        ),
    }
}
#[derive(Debug, Parser)]
#[command(name = "openspine")]
struct Cli {
    #[arg(long, default_value = "openspine.yaml")]
    config: PathBuf,
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

    let artifact_key = config::artifact_key_bytes()?;
    let artifacts =
        artifact_store::ArtifactStore::open(cfg.data_dir.join("artifacts"), artifact_key)
            .context("opening artifact store")?;
    let secrets = Arc::new(
        secret_store::SecretStore::open(cfg.data_dir.join("credentials"), artifact_key)
            .context("opening secret store")?,
    );
    let bot_token = if let Some(value) = secrets
        .get_string("telegram.bot_token")
        .context("reading Telegram bot token from vault")?
    {
        value
    } else {
        let value = config::telegram_bot_token()?;
        secrets
            .seed_if_absent("telegram.bot_token", value.as_bytes())
            .context("seeding Telegram bot token")?;
        value
    };
    let store =
        store::Store::open(&cfg.data_dir.join("kernel.db")).context("opening kernel store")?;
    let now_ms = jiff::Timestamp::now().as_millisecond();
    match store
        .validate_boot_clock(now_ms)
        .context("checking boot clock high-water")?
    {
        store::BootClockCheck::Ok { .. } => {}
        store::BootClockCheck::Regressed {
            high_water_ms,
            now_ms,
        } => {
            anyhow::bail!(
                "wall clock regressed at boot: now ({now_ms} ms) is behind the persisted high-water ({high_water_ms} ms) beyond the 60s tolerance — refusing to start on a regressed clock; restore the clock or the backup before retrying"
            );
        }
    }
    // Bootstrap the owner principal at startup (idempotent, transactional, fail-closed)
    let owner_principal = store
        .bootstrap_owner_principal(cfg.owner.telegram_user_id, &cfg.owner.display_name)
        .context("bootstrapping owner principal failed")?;
    if !store
        .verify_audit_chain()
        .context("verifying audit chain")?
    {
        anyhow::bail!(
            "audit_log hash chain is broken in {} — refusing to start on an untrustworthy audit trail",
            cfg.data_dir.join("kernel.db").display()
        );
    }
    let overlay_dir = cfg.data_dir.join("artifacts.d");
    model_swap_recovery::reconcile_model_swap_overlay(&store, &artifacts, &overlay_dir)?;
    let overlay_startup = overlay_startup::load(&cfg.lyra_dir, &cfg.data_dir, &store, &artifacts)?;
    let registry = overlay_startup.registry;
    let base_artifact_ids = overlay_startup.base_artifact_ids;
    let base_compatibility_epoch = overlay_startup.base_compatibility_epoch;
    let overlay_dir = overlay_startup.overlay_dir;
    let pending_reconfirm_buttons = overlay_startup.pending_reconfirm_buttons;
    let pending_reconfirm_notices = overlay_startup.pending_reconfirm_notices;

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
    let provider_config_digests: HashMap<String, openspine_schemas::digest::Digest> = cfg
        .providers
        .iter()
        .map(|provider| {
            (
                provider.id.clone(),
                config::provider_config_digest(provider),
            )
        })
        .collect();
    let mut provider_pool = HashMap::new();
    for provider_config in &cfg.providers {
        let provider_key = config::provider_api_key(provider_config)?;
        let provider = model_gateway::ProviderClient::from_config(provider_config, provider_key);
        if provider_pool
            .insert(provider_config.id.clone(), provider)
            .is_some()
        {
            anyhow::bail!("duplicate provider id {}", provider_config.id);
        }
    }
    let default_provider_id = cfg
        .providers
        .first()
        .map(|provider| provider.id.clone())
        .ok_or_else(|| anyhow::anyhow!("openspine.yaml must configure at least one provider"))?;
    let mut active_model_providers = HashMap::from([
        (
            openspine_schemas::model_swap::ModelRole::Base,
            default_provider_id.clone(),
        ),
        (
            openspine_schemas::model_swap::ModelRole::Matcher,
            default_provider_id.clone(),
        ),
        (
            openspine_schemas::model_swap::ModelRole::Miner,
            default_provider_id,
        ),
    ]);
    for (id, version) in store.active_model_swap_ids()? {
        let Some(swap) = registry.model_swaps.get(&id) else {
            anyhow::bail!(
                "active model swap {id} v{version} has no matching active overlay; refusing startup"
            );
        };
        if swap.version != version
            || swap.lifecycle_state != openspine_schemas::artifact::Lifecycle::Active
        {
            anyhow::bail!("active model swap {id} v{version} is not active in the loaded overlay");
        }
    }
    for swap in registry.model_swaps.values() {
        if swap.lifecycle_state == openspine_schemas::artifact::Lifecycle::Active {
            let (provenance_state, provenance_digest) = store
                .find_proposed_artifact_state("model_swap", &swap.id, swap.version)
                .with_context(|| {
                    format!("checking ceremony provenance for active swap {}", swap.id)
                })?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "active model swap {} has no persisted ceremony provenance",
                        swap.id
                    )
                })?;
            if provenance_state != openspine_schemas::artifact::Lifecycle::Active {
                anyhow::bail!(
                    "active model swap {} lacks an Active proposed-artifact provenance row",
                    swap.id
                );
            }
            let verdicts =
                store.eval_verdicts_for_artifact("model_swap", &swap.id, swap.version)?;
            let has_replay = verdicts.iter().any(|v| {
                v.evaluator
                    .as_deref()
                    .is_some_and(|e| e.starts_with("overlay-eval-gate/replay@"))
                    && v.artifact_digest == provenance_digest
                    && v.verdict == "pass"
            });
            let has_judge = verdicts.iter().any(|v| {
                v.evaluator
                    .as_deref()
                    .is_some_and(|e| e.starts_with("overlay-eval-gate/risk-judge@"))
                    && v.artifact_digest == provenance_digest
                    && v.verdict == "pass"
            });
            if !has_replay || !has_judge {
                anyhow::bail!(
                    "active model swap {} has incomplete digest-bound AD-142 provenance",
                    swap.id
                );
            }
            let reviewed_bytes = artifacts.get(&openspine_schemas::artifact::ArtifactRef {
                digest: openspine_schemas::digest::Digest::parse(&provenance_digest)?,
                schema_version: 1,
            })?;
            let reviewed = match artifact_loader::parse_proposal(
                "model_swap",
                std::str::from_utf8(&reviewed_bytes)?,
            )? {
                artifact_loader::ParsedProposal::ModelSwap(manifest) => manifest,
                _ => anyhow::bail!("provenance row for {} is not a model_swap", swap.id),
            };
            let mut loaded_normalized = swap.clone();
            loaded_normalized.lifecycle_state = openspine_schemas::artifact::Lifecycle::Proposed;
            let mut reviewed_normalized = reviewed;
            reviewed_normalized.lifecycle_state = openspine_schemas::artifact::Lifecycle::Proposed;
            if loaded_normalized != reviewed_normalized {
                anyhow::bail!(
                    "active model swap {} differs from its reviewed ceremony manifest",
                    swap.id
                );
            }
            let golden_set = registry
                .golden_sets
                .get(&swap.golden_set_id)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "active model swap {} references missing golden set {}",
                        swap.id,
                        swap.golden_set_id
                    )
                })?;
            if !golden_set.roles.contains(&swap.role) {
                anyhow::bail!(
                    "active model swap {} golden set is not authorized for role {:?}",
                    swap.id,
                    swap.role
                );
            }
            let provider_digest = provider_config_digests
                .get(&swap.target_provider_id)
                .ok_or_else(|| anyhow::anyhow!("missing provider digest"))?;
            model_swap::verify_activation_binding(swap, golden_set, provider_digest)
                .with_context(|| format!("validating active model swap {}", swap.id))?;
            if !provider_pool.contains_key(&swap.target_provider_id) {
                anyhow::bail!(
                    "active model swap {} v{} for role {:?} references missing provider {}; restore it or activate another approved swap",
                    swap.id,
                    swap.version,
                    swap.role,
                    swap.target_provider_id
                );
            }
            active_model_providers.insert(swap.role, swap.target_provider_id.clone());
        }
    }

    let telegram = telegram::TelegramConnector::new_with_store(
        bot_token,
        secrets.clone(),
        "telegram.bot_token".to_string(),
    );
    let gmail = match &cfg.gmail {
        Some(gmail_cfg) => {
            let client_secret_slot = "gmail.client_secret";
            let refresh_token_slot = "gmail.refresh_token";
            if !secrets.contains(client_secret_slot)? {
                if let Ok(value) = config::gmail_client_secret(gmail_cfg) {
                    secrets.seed_if_absent(client_secret_slot, value.as_bytes())?;
                }
            }
            if !secrets.contains(refresh_token_slot)? {
                if let Ok(value) = config::gmail_refresh_token(gmail_cfg) {
                    secrets.seed_if_absent(refresh_token_slot, value.as_bytes())?;
                }
            }
            Some(gmail::GmailConnector::new_with_store(
                gmail_cfg.client_id.clone(),
                secrets.clone(),
                client_secret_slot.to_string(),
                refresh_token_slot.to_string(),
                gmail_cfg.mailbox_address.clone(),
            ))
        }
        None => None,
    };
    let state = Arc::new(pipeline::AppState {
        store,
        artifacts,
        registry: parking_lot::RwLock::new(registry),
        secrets: secrets.clone(),
        action_catalog: crate::action_catalog::canonical_catalog(),
        sandbox,
        connectors: ConnectorRegistry::new(telegram, gmail)?,
        owner_user_id: cfg.owner.telegram_user_id,
        provider_config_digests,
        owner_principal_id: owner_principal.id,
        base_artifact_ids,
        base_compatibility_epoch,
        owner_identity_id: owner_principal.identity_id,
        kernel_endpoint: cfg
            .kernel
            .advertise_endpoint
            .clone()
            .unwrap_or_else(|| format!("http://{}", cfg.kernel.bind_addr)),
        unsafe_allow_uncontained_private_data: cfg.unsafe_allow_uncontained_private_data,
        action_handlers: ActionHandlerRegistry::default_registrations(),
        provider_pool,
        gateway_tier_map: crate::model_gateway::GatewayTierMap::new(),
        active_model_providers: parking_lot::RwLock::new(active_model_providers),
        started_at: Instant::now(),
        spend_cap: cfg.spend_cap,
        overlay_dir,
        conversation_locks: parking_lot::Mutex::new(std::collections::HashMap::new()),
    });
    for (request_id, summary) in &pending_reconfirm_buttons {
        if let Err(err) = state
            .connectors
            .telegram()
            .send_reply_with_approval_button(state.owner_user_id, summary, *request_id)
            .await
        {
            tracing::warn!(error = %err, %request_id, "failed to send reconfirm button");
        }
    }
    for notice in &pending_reconfirm_notices {
        if let Err(err) = state
            .connectors
            .telegram()
            .send_reply(state.owner_user_id, notice)
            .await
        {
            tracing::warn!(error = %err, "failed to send overlay re-proposal notice");
        }
    }

    // AD-143 F1: Recover pending breach alerts that crashed in_flight.
    crate::spend::recover_pending_breach_alerts(&state).await;

    let listener = tokio::net::TcpListener::bind(&cfg.kernel.bind_addr)
        .await
        .with_context(|| format!("binding {}", cfg.kernel.bind_addr))?;
    commit_post_bind_clock(&state.store, now_ms, || {
        jiff::Timestamp::now().as_millisecond()
    })?;
    tracing::info!(addr = %cfg.kernel.bind_addr, owner = cfg.owner.telegram_user_id, "openspine kernel starting");

    let http_server =
        axum::serve(listener, api::router(state.clone())).with_graceful_shutdown(shutdown_signal());
    let telegram_poll = pipeline::run_telegram_poll_loop(&state);
    let retry_worker = failure_surfacing::retry_worker::run_retry_loop(&state);
    // AD-104/AD-012: the kernel-owned dark-window timer driver. Consumers
    // only schedule (`WorkflowCtx::schedule_timer`) and subscribe (poll or
    // ledger replay of `workflow.timer_fired`); this loop is what actually
    // fires due timers, sleeping until the earliest known deadline.
    let timer_driver = workflow::run_timer_driver(&state.store, std::time::Duration::from_secs(5));
    let task_timer_consumer = pipeline::run_task_deadline_consumer(&state);
    tokio::select! {
        res = http_server => res.context("http server failed")?,
        res = telegram_poll => res.context("telegram poll loop failed")?,
        res = retry_worker => res.context("dead-letter retry loop failed")?,
        res = timer_driver => res.context("runtime clock/timer driver failed")?,
        () = task_timer_consumer => unreachable!("task timer consumer loops forever"),
    }
    Ok(())
}
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown signal received, draining in-flight requests");
}
