// openspine:allow-large-module reason: startup wiring for all kernel subsystems (worker result consumer, nerve dispatcher, timer driver, telegram poll, retry worker, HTTP server)
mod action_catalog;
mod anthropic_fingerprint;
mod api;
mod artifact_loader;
mod artifact_store;
mod benchmark;
mod briefcase;
mod briefcase_visibility;
pub(crate) mod cli;
mod codex_fingerprint;
mod config;
mod connector_reality;
mod connectors;
mod counterparty_erasure;
mod counterparty_keys;
pub(crate) mod disclosure;
mod env_file;
mod escalation;
mod failure_surfacing;
mod gmail;
mod identity;
#[cfg(test)]
mod kernel_tests;
mod model_gateway;
mod model_swap;
mod model_swap_recovery;
mod nerve_delivery;
pub(crate) mod oauth;
mod overlay_compat;
mod overlay_eval_gate;
mod overlay_export_restore;
mod overlay_persona_admission;
mod overlay_recovery;
mod overlay_startup;
mod pipeline;
mod reflection_miner_runtime;
use crate::reflection_miner_runtime::run_reflection_miner_driver;
mod sandbox;
mod secret_intake;
mod secret_store;
mod seed_workflows;
mod skill;
mod spend;
mod standing_rules_gate;
mod store;
mod telegram;
mod workflow;
pub mod workflow_state_machine;

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod export_restore_e2e_tests;
#[cfg(test)]
mod model_swap_recovery_tests;
#[cfg(test)]
mod nerve_delivery_tests;
#[cfg(test)]
mod overlay_startup_tests;

use crate::api::effect_executors::EffectExecutorRegistry;
use crate::api::handler_registry::ActionHandlerRegistry;
use crate::connector_reality::WebhookVerifier;
use crate::connectors::ConnectorRegistry;
use anyhow::Context as _;
use clap::{Parser, Subcommand};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
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
    match store
        .commit_boot_clock(commit_ms)
        .context("committing boot clock high-water")?
    {
        store::BootClockCheck::Ok { .. } => Ok(()),
        store::BootClockCheck::Regressed {
            high_water_ms,
            now_ms,
        } => anyhow::bail!(
            "wall clock regressed during startup: now ({now_ms} ms) is behind the persisted high-water ({high_water_ms} ms)"
        ),
    }
}

/// Fail closed on startup integrity checks before replaying the authenticated
/// terminal-erasure ledger into the opened database generation.
pub(crate) fn validate_startup_and_reconcile_overlay_terminal_erasures(
    store: &store::Store,
    artifacts: &artifact_store::ArtifactStore,
    overlay_operations: &overlay_export_restore::OverlayOperations,
    data_root: &Path,
    now_ms: i64,
) -> anyhow::Result<()> {
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
    if !store
        .verify_audit_chain()
        .context("verifying audit chain")?
    {
        anyhow::bail!(
            "audit_log hash chain is broken in {} — refusing to start on an untrustworthy audit trail",
            data_root.join("kernel.db").display()
        );
    }
    counterparty_erasure::reconcile_overlay_terminal_erasures(store, artifacts, overlay_operations)
        .context("reconciling overlay terminal-erasure ledger at startup")
}
#[derive(Debug, Parser)]
#[command(name = "openspine")]
pub(crate) struct Cli {
    #[arg(long, default_value = "openspine.yaml")]
    config: PathBuf,
    #[arg(long)]
    benchmark: bool,
    /// Reinstall the retained pre-restore generation for the pending signed restore.
    #[arg(long)]
    rollback_pending_restore: bool,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Commands {
    /// Interactive onboarding setup wizard
    Setup {
        /// Print the readiness report and exit non-zero when anything blocks.
        #[arg(long)]
        check: bool,
    },
    /// Open a direct local terminal conversation with Lyra
    Chat {
        /// Send one message, print the reply, and exit (for smoke tests/scripts)
        #[arg(long)]
        once: Option<String>,
    },
    /// Provider login via OAuth or API key
    Provider {
        #[command(subcommand)]
        command: ProviderCommands,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum ProviderCommands {
    /// Login to a model provider
    Login {
        /// Provider ID (e.g. openai-codex, anthropic)
        provider: Option<String>,
        /// Re-run the browser authorization even when a stored credential
        /// exists (for example to bind a different provider-side account)
        #[arg(long)]
        force: bool,
    },
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cli = Cli::parse();
    let config_path = cli.config.clone();
    // Key material is read from the process environment, so the owner-only file
    // beside the configuration has to be exported before anything reads it.
    if let Err(error) = env_file::load_adjacent(&config_path) {
        eprintln!("openspine failed to start: {error}");
        return std::process::ExitCode::FAILURE;
    }
    match run(cli).await {
        Ok(code) => code,
        Err(error) => {
            cli::remedy::report_failure(&error, &config_path);
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> anyhow::Result<std::process::ExitCode> {
    let terminal_chat_request = match &cli.command {
        Some(Commands::Chat { once }) => Some(once.clone()),
        _ => None,
    };
    let terminal_chat_mode = terminal_chat_request.is_some();
    // Fixed boot timestamp captured at process start. Startup stranded-worker
    // recovery selects only dispatches created BEFORE this instant, so a
    // commission accepted just after boot can never be falsely surfaced as
    // stranded (WkFinalSec P2 cutoff race).
    let boot_started_at = jiff::Timestamp::now();
    if cli.benchmark {
        benchmark::run_benchmarks()?;
        return Ok(std::process::ExitCode::SUCCESS);
    }
    if let Some(Commands::Setup { check }) = &cli.command {
        let ready = cli::wizard::run_setup(&cli.config, *check).await?;
        return Ok(if ready {
            std::process::ExitCode::SUCCESS
        } else {
            std::process::ExitCode::FAILURE
        });
    }
    if let Some(Commands::Provider {
        command: ProviderCommands::Login { provider, force },
    }) = &cli.command
    {
        cli::login::run_provider_login(&cli.config, provider.as_deref(), *force).await?;
        return Ok(std::process::ExitCode::SUCCESS);
    }
    let cfg = config::Config::load(&cli.config)
        .with_context(|| format!("loading {}", cli.config.display()))?;

    let artifact_key = config::artifact_key_bytes()?;
    // Acquire the exclusive data-root lifetime lock and process any pending
    // export/restore before any store opens. Stores and overlay load use only
    // the controller's canonical data-root identity.
    let overlay_operations = Arc::new(
        overlay_export_restore::acquire(&cfg.data_dir, &artifact_key)
            .context("acquiring overlay operations lifetime lock")?,
    );
    let data_root = overlay_operations.canonical_data_root().to_path_buf();
    let pending_overlay_finalization = overlay_operations
        .process_pre_open(cli.rollback_pending_restore, jiff::Timestamp::now())
        .context("processing pre-open overlay export/restore")?;
    let artifacts = artifact_store::ArtifactStore::open(data_root.join("artifacts"), artifact_key)
        .context("opening artifact store")?;
    // AD-140: re-key any pre-AD-140 single-global-key blobs into the new
    // per-counterparty format under SYSTEM_SCOPE before serving anything.
    artifacts.migrate_legacy_blobs(artifact_key)?;
    let secrets = Arc::new(
        secret_store::SecretStore::open(data_root.join("credentials"), artifact_key)
            .context("opening secret store")?,
    );
    let bot_token = if terminal_chat_mode {
        None
    } else if let Some(value) = secrets
        .get_string("telegram.bot_token")
        .context("reading Telegram bot token from vault")?
    {
        Some(value)
    } else {
        let value = config::telegram_bot_token()?;
        secrets
            .seed_if_absent("telegram.bot_token", value.as_bytes())
            .context("seeding Telegram bot token")?;
        Some(value)
    };
    let store = store::Store::open(&data_root.join("kernel.db")).context("opening kernel store")?;
    // Close the DB-commit-before-key-tombstone crash window: the durable
    // SQLite marker is authoritative, so every recorded erasure is replayed
    // into the key ring before any payload can be served.
    for counterparty_id in store
        .erased_counterparty_ids()
        .context("loading erased counterparty markers")?
    {
        artifacts
            .erase_counterparty_key(counterparty_id)
            .with_context(|| {
                format!("reconciling erased counterparty key {counterparty_id} at startup")
            })?;
    }
    let now_ms = jiff::Timestamp::now().as_millisecond();
    // Validate both startup integrity boundaries before the terminal-erasure
    // replay can append DB rows or audit events. Reconciliation still precedes
    // overlay loading and serving.
    validate_startup_and_reconcile_overlay_terminal_erasures(
        &store,
        &artifacts,
        overlay_operations.as_ref(),
        &data_root,
        now_ms,
    )?;
    // Bootstrap only after audit verification so a broken chain leaves the
    // database and audit trail untouched.
    let owner_principal = crate::identity::bootstrap_owner_principal(
        &store,
        cfg.owner.telegram_user_id,
        &cfg.owner.display_name,
    )
    .context("bootstrapping owner principal failed")?;
    let overlay_dir = data_root.join("artifacts.d");
    model_swap_recovery::reconcile_model_swap_overlay(&store, &artifacts, &overlay_dir)?;
    // Pre-populate the Donna×Leo personality seed as learnable overlay
    // artifacts (AD-080). Idempotent: safe to run every boot; only seeds the
    // elements not already present in learned_artifacts. Must run before
    // overlay_startup::load so the seeded YAML is discovered into the registry.
    store::personality_seed::seed_if_missing(&store, &artifacts, &overlay_dir)
        .context("seeding personality seed overlay artifacts")?;
    let overlay_startup = overlay_startup::load(&cfg.lyra_dir, &data_root, &store, &artifacts)?;
    let registry = overlay_startup.registry;
    let base_artifact_ids = overlay_startup.base_artifact_ids;
    let base_compatibility_epoch = overlay_startup.base_compatibility_epoch;
    let overlay_dir = overlay_startup.overlay_dir;
    let pending_reconfirm_buttons = overlay_startup.pending_reconfirm_buttons;
    let pending_reconfirm_notices = overlay_startup.pending_reconfirm_notices;
    // AD-130/AD-132: seed kernel-owned nerve advisee limits from the loaded
    // active agent-manifest registry before any nerve can register.
    store
        .seed_advisee_limits_from_manifests(registry.agents.values())
        .context("seeding nerve advisee limits from agent manifests")?;

    let sandbox = match cfg.sandbox.driver {
        config::SandboxDriverKind::Process => {
            sandbox::Sandbox::Process(sandbox::ProcessDriver::with_data_root(&data_root))
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
    let (provider_pool, default_provider_id) = build_provider_pool(&cfg.providers)?;
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

    let telegram = match bot_token {
        Some(token) => telegram::TelegramConnector::new_with_store(
            token,
            secrets.clone(),
            "telegram.bot_token".to_string(),
        ),
        None => telegram::TelegramConnector::new("0:terminal-disabled".to_string()),
    };
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
    let (terminal_reply_tx, terminal_reply_rx) = if terminal_chat_mode {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };
    let state = Arc::new(pipeline::AppState {
        store,
        artifacts,
        registry: parking_lot::RwLock::new(registry),
        secrets: secrets.clone(),
        action_catalog: crate::action_catalog::canonical_catalog(),
        sandbox,
        connectors: ConnectorRegistry::new(telegram, gmail)?,
        terminal_reply_tx,
        webhook_verifier: WebhookVerifier::new(
            config::webhook_hmac_secret()?,
            Duration::from_secs(300),
        ),
        owner: owner_principal,
        provider_config_digests,
        base_artifact_ids,
        base_compatibility_epoch,
        kernel_endpoint: cfg
            .kernel
            .advertise_endpoint
            .clone()
            .unwrap_or_else(|| format!("http://{}", cfg.kernel.bind_addr)),
        unsafe_allow_uncontained_private_data: cfg.unsafe_allow_uncontained_private_data,
        action_handlers: ActionHandlerRegistry::default_registrations(),
        effect_executors: EffectExecutorRegistry::default_registrations(),
        provider_pool,
        gateway_tier_map: crate::model_gateway::GatewayTierMap::from_model_tiers(&cfg.model_tiers),
        active_model_providers: parking_lot::RwLock::new(active_model_providers),
        started_at: Instant::now(),
        spend_cap: cfg.spend_cap,
        connector_call_timeout: Duration::from_secs(30),
        overlay_dir,
        conversation_locks: parking_lot::Mutex::new(std::collections::HashMap::new()),
        overlay_operations: overlay_operations.clone(),
        #[cfg(test)]
        pre_reserve_erasure_hook: parking_lot::Mutex::new(None),
    });
    let listener = bind_clock_and_finalize_overlay(
        &cfg.kernel.bind_addr,
        &state.store,
        now_ms,
        || jiff::Timestamp::now().as_millisecond(),
        overlay_operations.as_ref(),
        pending_overlay_finalization.as_ref(),
    )
    .await?;

    if let Some(once) = terminal_chat_request {
        let receiver = terminal_reply_rx
            .ok_or_else(|| anyhow::anyhow!("terminal reply channel was not initialized"))?;
        // Assessed with the vault this process already holds open, so the
        // report reflects real credential state without a second data-root lock.
        let mut readiness = cli::readiness::assess(
            &cli.config,
            &cli::readiness::process_env,
            Some(secrets.as_ref()),
        );
        let onboarding_complete = cli::onboarding::is_complete(&cfg.data_dir);
        // Every static check passes on a host with a generated API key and no
        // model server, so recording completion on those alone would silence
        // the guidance while every turn fails. The probe costs one small model
        // call and runs only while onboarding is unrecorded, which is the only
        // moment its answer changes what happens next.
        if once.is_none() && !onboarding_complete && readiness.is_ready() {
            readiness
                .checks
                .push(cli::setup::verify_default_provider(&cfg, Some(secrets.as_ref())).await);
        }
        let login_provider = run_terminal_chat(
            state,
            listener,
            receiver,
            once,
            readiness,
            onboarding_complete,
            &cfg.data_dir,
        )
        .await?;
        if let Some(provider) = login_provider {
            // The chat runtime is gone: `run_terminal_chat` aborted and
            // awaited its HTTP server and dropped the `AppState` it consumed.
            // These two locals are the last owners of the vault and the
            // data-root lifetime lock; the login flow reacquires both through
            // the ordinary `openspine provider login` path (see
            // `login_handoff_can_reacquire_the_lock_after_chat_owners_drop`).
            drop(secrets);
            drop(overlay_operations);
            eprintln!();
            eprintln!(
                "Leaving chat for provider login ({provider}). Run `openspine` again when it finishes."
            );
            cli::login::run_provider_login(&cli.config, Some(&provider), false).await?;
        }
        return Ok(std::process::ExitCode::SUCCESS);
    }

    for (request_id, summary) in &pending_reconfirm_buttons {
        let guard = crate::spend::guard_connector(&state, true).await;
        if let Err(err) = guard {
            tracing::warn!(error = %err, %request_id, "spend guard blocked reconfirm button");
            continue;
        }
        let result = crate::api::connector_breaker::call_with_connector_preflight(
            &state,
            "telegram",
            None,
            state.connectors.telegram().send_reply_with_approval_button(
                &state.telegram_owner_surface(),
                summary,
                *request_id,
            ),
        )
        .await;
        if let Err(err) = result {
            tracing::warn!(error = %err, %request_id, "failed to send reconfirm button");
        }
    }
    for notice in &pending_reconfirm_notices {
        let guard = crate::spend::guard_connector(&state, true).await;
        if let Err(err) = guard {
            tracing::warn!(error = %err, "spend guard blocked overlay notice");
            continue;
        }
        let result = crate::api::connector_breaker::call_with_connector_preflight(
            &state,
            "telegram",
            None,
            state
                .connectors
                .telegram()
                .send_reply(&state.telegram_owner_surface(), notice),
        )
        .await;
        if let Err(err) = result {
            tracing::warn!(error = %err, "failed to send overlay re-proposal notice");
        }
    }

    // AD-143 F1: Recover pending breach alerts that crashed in_flight.
    crate::spend::recover_pending_breach_alerts(&state).await;

    tracing::info!(addr = %cfg.kernel.bind_addr, owner = cfg.owner.telegram_user_id, "openspine kernel starting");

    let http_server =
        axum::serve(listener, api::router(state.clone())).with_graceful_shutdown(shutdown_signal());
    let telegram_poll = pipeline::run_telegram_poll_loop(&state);
    let retry_worker = failure_surfacing::retry_worker::run_retry_loop(&state);
    // AD-104: fires due workflow/dark-window timers, sleeping until the
    // earliest known deadline.
    let timer_driver = workflow::run_timer_driver(&state.store, std::time::Duration::from_secs(5));
    // AD-104/AD-012: the kernel-owned dark-window timer driver. Consumers
    // only schedule (`WorkflowCtx::schedule_timer`) and subscribe (poll or
    // ledger replay of `workflow.timer_fired`); this loop is what actually
    let task_timer_consumer = pipeline::run_task_deadline_consumer(&state);

    // AD-130/AD-132/AD-034: kernel-owned nerve dispatcher. Periodically
    // replays registered nerves (first real handler: the AD-034 screener)
    // over the audit ledger through their persisted filters.
    let nerve_dispatcher = store::nerve_dispatch::run_nerve_dispatcher(
        &state.store,
        std::time::Duration::from_secs(5),
    );

    let nerve_delivery = nerve_delivery::run(state.clone());
    // AD-100/AD-035: recovery runs after the HTTP service has had a chance to
    // bind and poll. Per D-083, a dispatched/handed_off row WITHOUT a
    // completion receipt is NEVER rerun — it is surfaced for owner attention
    // via failure_surfacing and left terminal.
    let recovery_state = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let result: anyhow::Result<()> = async {
            for (grant_id, _token_ref) in
                store::worker_dispatch::pending_worker_dispatches(&recovery_state.store, boot_started_at)
                    .context("loading stranded worker rows")?
            {
                let Some(owner_surface) = store::worker_dispatch::worker_parent_grant(
                    &recovery_state.store,
                    grant_id,
                )
                .ok()
                .flatten()
                .and_then(|parent_id| {
                    recovery_state
                        .store
                        .find_task_grant_by_id(parent_id)
                        .ok()
                        .flatten()
                        .map(|(_, _, owner_surface)| owner_surface)
                })
                .filter(|owner_surface| {
                    owner_surface.kind()
                        == openspine_schemas::owner_surface::OwnerSurfaceKind::TelegramPrivate
                })
                else {
                    tracing::error!(%grant_id, "cannot resolve owner surface for stranded worker; skipping notification");
                    continue;
                };
                let text = format!("Worker {grant_id} was dispatched but never reported a result. The worker's shell may have exited without reporting. No further action is taken automatically.");
                let text_ref = match recovery_state.artifacts.put(text.as_bytes()) {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::error!(%grant_id, error = %e, "storing stranded worker notification text; retrying on next restart");
                        continue;
                    }
                };
                if let Err(e) = store::worker_dispatch::surface_stranded_worker(
                    &recovery_state.store,
                    &owner_surface,
                    text_ref.digest.as_str(),
                    grant_id,
                    "stranded worker dispatch (no result recorded)",
                ) {
                    tracing::error!(%grant_id, error = %e, "enqueuing/marking stranded worker notification; retrying on next restart");
                }
            }
            Ok(())
        }
        .await;
        if let Err(err) = result {
            tracing::error!(error = %err, "worker recovery task failed");
        }
    });
    // Periodic watchdog: detect workers whose shell exited without reporting
    // a result and surface them for owner attention.
    let watchdog_state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            let Ok(stranded) = store::worker_dispatch::stranded_worker_timeouts(
                &watchdog_state.store,
                std::time::Duration::from_secs(7200),
            ) else {
                continue;
            };
            for (grant_id, parent_grant_id) in stranded {
                let Some(owner_surface) = watchdog_state
                    .store
                    .find_task_grant_by_id(parent_grant_id)
                    .ok()
                    .flatten()
                    .map(|(_, _, owner_surface)| owner_surface)
                    .filter(|owner_surface| {
                        owner_surface.kind()
                            == openspine_schemas::owner_surface::OwnerSurfaceKind::TelegramPrivate
                    })
                else {
                    tracing::error!(%grant_id, %parent_grant_id, "cannot resolve owner surface for watchdog notification; retrying");
                    continue;
                };
                let text = format!(
                    "Worker {grant_id} (parent {parent_grant_id}) has not reported a result within 2 hours. The worker's shell may have exited without reporting."
                );
                let text_ref = match watchdog_state.artifacts.put(text.as_bytes()) {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::error!(%grant_id, error = %e, "storing watchdog notification text; retrying next sweep");
                        continue;
                    }
                };
                if let Err(e) = store::worker_dispatch::surface_stranded_worker(
                    &watchdog_state.store,
                    &owner_surface,
                    text_ref.digest.as_str(),
                    grant_id,
                    "stranded worker timeout (no result within 2h)",
                ) {
                    tracing::error!(%grant_id, error = %e, "enqueuing/marking watchdog notification; retrying next sweep");
                }
            }
        }
    });
    let worker_result_consumer = pipeline::run_worker_result_consumer(&state);
    let worker_failed_consumer = pipeline::run_worker_failed_consumer(&state);
    let standing_rule_dark_window_consumer =
        pipeline::run_standing_rule_dark_window_consumer(&state);
    // AD-050/135: scheduled reflection miner. Interval-driven, fail-closed;
    // finds-or-mints a long-lived scheduled grant pair each pass and invokes
    // `run_reflection_miner` under the grant-scoped admission path.
    let reflection_state = state.clone();
    tokio::spawn(async move {
        run_reflection_miner_driver(
            &reflection_state,
            std::time::Duration::from_secs(cfg.reflection_miner_interval_seconds),
        )
        .await;
    });
    tokio::select! {
        res = http_server => res.context("http server failed")?,
        res = telegram_poll => res.context("telegram poll loop failed")?,
        res = retry_worker => res.context("dead-letter retry loop failed")?,
        res = timer_driver => res.context("runtime clock/timer driver failed")?,
        () = task_timer_consumer => unreachable!("task timer consumer loops forever"),
        () = nerve_dispatcher => unreachable!("run_nerve_dispatcher loops forever"),
        () = nerve_delivery => unreachable!("nerve_delivery loops forever"),
        res = worker_result_consumer => res.context("worker result consumer failed")?,
        res = worker_failed_consumer => res.context("worker failed consumer failed")?,
        () = standing_rule_dark_window_consumer => {
            unreachable!("standing-rule dark-window consumer loops forever")
        }
    }
    Ok(std::process::ExitCode::SUCCESS)
}

/// Append digest-safe requested + completed/rolled-back audit for a finalized
/// overlay operation. Uses the workflow-step registry so retries after a crash
/// between audit and durable cleanup remain idempotent.
pub(crate) fn append_overlay_finalization_audits(
    store: &store::Store,
    meta: &overlay_export_restore::CompletionMetadata,
) -> anyhow::Result<()> {
    use overlay_export_restore::{FinalizationOutcome, OverlayOperationKind};

    let (requested_kind, terminal_kind) = match (meta.kind, meta.outcome) {
        (OverlayOperationKind::Export, FinalizationOutcome::Completed) => {
            ("overlay.export_requested", "overlay.export_completed")
        }
        (OverlayOperationKind::Restore, FinalizationOutcome::Completed) => {
            ("overlay.restore_requested", "overlay.restore_completed")
        }
        (OverlayOperationKind::Restore, FinalizationOutcome::RolledBack) => {
            ("overlay.restore_requested", "overlay.restore_rolled_back")
        }
        (OverlayOperationKind::Export, FinalizationOutcome::RolledBack) => {
            anyhow::bail!("export finalization cannot be rolled back")
        }
    };

    // Aggregate by request id so the restored chain can re-bind authorization
    // evidence without plaintext paths or key bytes.
    let run_id = format!("overlay-op:{}", meta.request_id);
    let payload = serde_json::json!({
        "request_id": meta.request_id,
        "action_id": meta.action_id,
        "owner_principal_id": meta.owner_principal_id,
        "grant_id": meta.grant_id,
        "path_digest": meta.path_digest,
        "requested_at": meta.requested_at,
        "completed_at": meta.completed_at,
    });
    let payload_json = serde_json::to_string(&payload).context("encoding overlay audit payload")?;

    store
        .append_workflow_step_if_absent(
            &run_id,
            requested_kind,
            &payload_json,
            &format!("{}:requested", meta.request_id),
        )
        .context("appending overlay requested audit")?;
    store
        .append_workflow_step_if_absent(
            &run_id,
            terminal_kind,
            &payload_json,
            &format!("{}:terminal:{terminal_kind}", meta.request_id),
        )
        .context("appending overlay terminal audit")?;
    Ok(())
}

/// Post-bind overlay finalization: audit then durable cleanup. Called only after
/// listener bind and post-bind clock commit succeed.
pub(crate) fn finalize_overlay_after_bind(
    store: &store::Store,
    overlay_operations: &overlay_export_restore::OverlayOperations,
    pending: Option<&overlay_export_restore::PendingFinalization>,
    now: jiff::Timestamp,
) -> anyhow::Result<()> {
    let Some(pending) = pending else {
        return Ok(());
    };
    let meta = overlay_operations
        .begin_finalization(pending, now)
        .context("beginning overlay operation finalization")?;
    append_overlay_finalization_audits(store, &meta)
        .context("appending overlay finalization audit events")?;
    overlay_operations
        .complete_finalization(&meta)
        .context("completing overlay operation finalization")?;
    Ok(())
}

/// Production provider-pool construction used after overlay install and before bind.
/// Resolves each provider API key and rejects empty or duplicate provider ids.
/// Failure retains any pending overlay finalization (callers have not finalized yet).
pub(crate) fn build_provider_pool(
    providers: &[config::ProviderConfig],
) -> anyhow::Result<(HashMap<String, model_gateway::ProviderClient>, String)> {
    let mut provider_pool = HashMap::new();
    for provider_config in providers {
        let provider_key = config::provider_api_key(provider_config)?;
        let provider = model_gateway::ProviderClient::from_config(provider_config, provider_key);
        if provider_pool
            .insert(provider_config.id.clone(), provider)
            .is_some()
        {
            anyhow::bail!("duplicate provider id {}", provider_config.id);
        }
    }
    let default_provider_id = select_default_provider_id(providers)?;
    Ok((provider_pool, default_provider_id))
}

/// Production default provider selector, used after provider validation.
pub(crate) fn select_default_provider_id(
    providers: &[config::ProviderConfig],
) -> anyhow::Result<String> {
    providers
        .first()
        .map(|provider| provider.id.clone())
        .ok_or_else(|| anyhow::anyhow!("openspine.yaml must configure at least one provider"))
}

/// Production listener bind used after provider validation and before post-bind clock.
pub(crate) async fn bind_kernel_listener(
    bind_addr: &str,
) -> anyhow::Result<tokio::net::TcpListener> {
    tokio::net::TcpListener::bind(bind_addr)
        .await
        .with_context(|| format!("binding {bind_addr}"))
}

/// Production late-startup continuation: bind → post-bind clock → overlay finalization.
/// Injectable clock supports regression tests without mocking the bind path.
pub(crate) async fn bind_clock_and_finalize_overlay(
    bind_addr: &str,
    store: &store::Store,
    pre_setup_ms: i64,
    clock: impl Fn() -> i64,
    overlay_operations: &overlay_export_restore::OverlayOperations,
    pending: Option<&overlay_export_restore::PendingFinalization>,
) -> anyhow::Result<tokio::net::TcpListener> {
    let listener = bind_kernel_listener(bind_addr).await?;
    commit_post_bind_clock(store, pre_setup_ms, clock)?;
    finalize_overlay_after_bind(store, overlay_operations, pending, jiff::Timestamp::now())?;
    Ok(listener)
}
async fn run_terminal_turn(
    state: &Arc<pipeline::AppState>,
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<String>,
    message: String,
) -> anyhow::Result<String> {
    if message.trim().is_empty() {
        anyhow::bail!("terminal message must not be empty");
    }
    let grant = pipeline::handle_terminal_message(state, message).await?;
    if grant.is_none() {
        anyhow::bail!("terminal message was denied before a task grant was created");
    }
    tokio::time::timeout(Duration::from_secs(150), receiver.recv())
        .await
        .context("timed out waiting for the terminal reply action")?
        .ok_or_else(|| anyhow::anyhow!("terminal reply channel closed before a reply arrived"))
}

async fn run_terminal_chat(
    state: Arc<pipeline::AppState>,
    listener: tokio::net::TcpListener,
    mut receiver: tokio::sync::mpsc::UnboundedReceiver<String>,
    once: Option<String>,
    readiness: cli::readiness::Readiness,
    onboarding_complete: bool,
    data_dir: &Path,
) -> anyhow::Result<Option<String>> {
    let server_state = state.clone();
    // Graceful shutdown, not abort: axum spawns a detached task per
    // connection, each holding a Router clone of `state`. Aborting joins only
    // the accept loop, so a trailing shell request (the task-complete report
    // that follows a reply) could keep the data-root lock and vault alive
    // into the `/login` handoff. The signal drains connections before the
    // serve future resolves.
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let http_server = tokio::spawn(async move {
        axum::serve(listener, api::router(server_state))
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
            .context("terminal chat HTTP server failed")
    });

    let result = if let Some(message) = once {
        // One-shot output stays exactly the reply: scripts and smoke tests parse
        // it, so no notice may reach stdout on this path — and no local command,
        // `/login` included, is ever interpreted.
        let reply = run_terminal_turn(&state, &mut receiver, message).await?;
        println!("{reply}");
        Ok(None)
    } else {
        use std::io::Write as _;

        let first_start = cli::onboarding::first_start(&readiness, onboarding_complete);
        if let Some(notice) = &first_start.notice {
            eprintln!();
            eprint!("{notice}");
        }
        if first_start.record_completion {
            if let Err(error) = cli::onboarding::record_complete(data_dir, None) {
                tracing::warn!(%error, "could not record onboarding completion");
            }
        }

        eprintln!("OpenSpine direct terminal chat. Type /help for commands, /exit to leave.");
        let stdin = std::io::stdin();
        let mut login_request: Option<String> = None;
        loop {
            print!("you> ");
            std::io::stdout()
                .flush()
                .context("flushing terminal prompt")?;
            let mut line = String::new();
            if stdin
                .read_line(&mut line)
                .context("reading terminal input")?
                == 0
            {
                break;
            }
            let line = line.trim_end_matches(['\r', '\n']).to_string();
            // Local commands are answered here: they mint no event and consume
            // no task grant.
            match cli::onboarding::parse_local_command(&line) {
                Some(cli::onboarding::LocalCommand::Exit) => break,
                Some(cli::onboarding::LocalCommand::Help) => {
                    print!("{}", cli::onboarding::help_text());
                    continue;
                }
                Some(cli::onboarding::LocalCommand::Status) => {
                    print!("{}", readiness.render());
                    continue;
                }
                Some(cli::onboarding::LocalCommand::Login { provider }) => {
                    login_request = Some(
                        provider.unwrap_or_else(|| cli::setup::OAUTH_PROVIDER_IDS[0].to_string()),
                    );
                    break;
                }
                None => {}
            }
            match run_terminal_turn(&state, &mut receiver, line).await {
                Ok(reply) => println!("lyra> {reply}"),
                Err(error) => eprintln!("error: {error:#}"),
            }
        }
        Ok(login_request)
    };

    if matches!(&result, Ok(Some(_))) {
        // Login handoff: drain until the serve future resolves, which
        // guarantees no connection task still holds the data-root lock or
        // vault. The wait is bounded by construction, because every kernel
        // handler carries its own timeout (model gateway 60s, connectors
        // 30s); a bounded drain with an abort fallback would strand an
        // ordinary in-flight request and make login fail on the held lock.
        let _ = shutdown_tx.send(());
        eprintln!("Waiting for in-flight kernel requests to finish before login...");
        let _ = http_server.await;
    } else {
        // Every other exit keeps the old immediate teardown: the process is
        // about to end and takes the connection tasks with it.
        http_server.abort();
        let _ = http_server.await;
    }
    result
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown signal received, draining in-flight requests");
}
