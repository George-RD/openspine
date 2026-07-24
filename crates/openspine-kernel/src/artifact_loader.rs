// openspine:allow-large-module reason: Unified seven-kind artifact loader and identity/source validation must remain one audit boundary.
//! Load and validate every `artifacts/**/*.yaml` fixture at startup (build
//! plan 4a). Only `lifecycle_state: active` artifacts join routing — that
//! constraint already lives in `resolve_route`/`compose_authority`
//! (Step 2/3), so this loader just parses everything into typed registries
//! without pre-filtering; loading itself is what gets audited here.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::model_gateway::PromptTemplate;
use openspine_schemas::agent::AgentManifest;
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::ids::ArtifactId;
use openspine_schemas::model_swap::{GoldenSet, ModelSwapManifest};
use openspine_schemas::pack::CapabilityPack;
use openspine_schemas::persona::PersonaElement;
use openspine_schemas::policy::Policy;
use openspine_schemas::route::Route;
use openspine_schemas::standing_rule::StandingRuleManifest;
use openspine_schemas::workflow::WorkflowManifest;

#[path = "artifact_loader_impl.rs"]
mod artifact_loader_impl;
/// Project a logical artifact identity into a filesystem-safe, deterministic name.
/// The manifest keeps the exact logical id; only this boundary uses the digest.
pub fn overlay_filename(artifact_id: &str, version: u32) -> String {
    let digest = openspine_schemas::digest::digest_of(&serde_json::json!({
        "artifact_id": artifact_id,
        "version": version,
    }));
    format!("{}-v{}.yaml", digest, version)
}
#[derive(Debug, thiserror::Error)]
pub enum ArtifactLoadError {
    #[error("failed to read {path}: {source}")]
    Read {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse {path}: {source}")]
    Parse {
        path: std::path::PathBuf,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("invalid {kind} artifact {id}: {reason}")]
    Invalid {
        kind: String,
        id: String,
        reason: String,
    },
    #[error(
        "artifact collision: {kind} id={id} v{version} appears more than once (check fixtures and the data/artifacts.d overlay)"
    )]
    #[allow(dead_code)]
    Collision {
        kind: String,
        id: String,
        version: u32,
    },
}

/// Every declarative artifact the kernel loaded at startup, keyed by id
/// where the schema has one. `routes` stays a `Vec` (many routes share no
/// natural single-id lookup pattern in the pipeline — resolution always
/// scans the whole active set, per `openspine_authority::resolve_route`).
#[derive(Clone, Debug, Default)]
pub struct ArtifactRegistry {
    pub routes: Vec<Route>,
    pub agents: HashMap<ArtifactId, AgentManifest>,
    pub workflows: HashMap<ArtifactId, WorkflowManifest>,
    pub packs: HashMap<ArtifactId, CapabilityPack>,
    pub policies: HashMap<ArtifactId, Policy>,
    pub templates: HashMap<String, PromptTemplate>,
    pub golden_sets: HashMap<String, GoldenSet>,
    pub model_swaps: HashMap<ArtifactId, ModelSwapManifest>,
    /// Personality seed elements (AD-080): free-text behavioral guidance,
    /// shipped as learnable overlay artifacts and loaded here so they are
    /// discoverable by the registry like any other kind. They are not
    /// authority-bearing, so they carry no `ParsedProposal` and never enter
    /// the proposal/approval pipeline.
    pub personas: HashMap<ArtifactId, PersonaElement>,
    /// Standing rules: inert composition-input bookkeeping only. NEVER
    /// consulted at gate time (D-007 — the task grant is the only live
    /// authority object); the runtime `standing_rules` table in the store is
    /// the sole gate-consultation source. Present only so activation writes
    /// a uniform registry entry and survives restart reload.
    pub standing_rules: HashMap<ArtifactId, openspine_schemas::standing_rule::StandingRuleManifest>,
    /// Raw source bytes + path retained for every loaded artifact so legacy
    /// migration and digest-bound reconfirmation never assume a canonical
    /// `{id}-v{version}.yaml` filename (AD-070). Keyed by (kind, id, version).
    pub sources: HashMap<(String, String, u32), ArtifactSource>,
}

/// The actual on-disk source of a loaded artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactSource {
    pub path: std::path::PathBuf,
    pub bytes: Vec<u8>,
}

/// Rehydrate one already-validated loader source into the typed registry.
pub fn rehydrate_source(
    registry: &mut ArtifactRegistry,
    kind: &str,
    source: &ArtifactSource,
) -> anyhow::Result<()> {
    let yaml = std::str::from_utf8(&source.bytes)?;
    let parsed = parse_proposal(kind, yaml)?;
    if parsed.kind() != kind {
        anyhow::bail!(
            "source kind mismatch: expected {kind}, got {}",
            parsed.kind()
        );
    }
    parsed.insert_into(registry)?;
    Ok(())
}

/// Return the namespace identity pairs represented by a registry snapshot.
pub fn artifact_identity_pairs(registry: &ArtifactRegistry) -> HashSet<(String, String)> {
    let mut pairs = HashSet::new();
    pairs.extend(
        registry
            .routes
            .iter()
            .map(|a| ("route".into(), a.id.clone())),
    );
    pairs.extend(
        registry
            .agents
            .keys()
            .map(|id| ("agent".into(), id.clone())),
    );
    pairs.extend(
        registry
            .workflows
            .keys()
            .map(|id| ("workflow".into(), id.clone())),
    );
    pairs.extend(
        registry
            .model_swaps
            .keys()
            .map(|id| ("model_swap".into(), id.clone())),
    );
    pairs.extend(registry.packs.keys().map(|id| ("pack".into(), id.clone())));
    pairs.extend(
        registry
            .policies
            .keys()
            .map(|id| ("policy".into(), id.clone())),
    );
    pairs.extend(
        registry
            .templates
            .keys()
            .map(|id| ("template".into(), id.clone())),
    );
    pairs.extend(
        registry
            .personas
            .keys()
            .map(|id| ("persona".into(), id.clone())),
    );
    pairs
}

pub fn exclude_identity_pairs(
    registry: &mut ArtifactRegistry,
    excluded: &HashSet<(String, String)>,
) {
    registry
        .routes
        .retain(|a| !excluded.contains(&("route".into(), a.id.clone())));
    registry
        .agents
        .retain(|id, _| !excluded.contains(&("agent".into(), id.clone())));
    registry
        .workflows
        .retain(|id, _| !excluded.contains(&("workflow".into(), id.clone())));
    registry
        .packs
        .retain(|id, _| !excluded.contains(&("pack".into(), id.clone())));
    registry
        .policies
        .retain(|id, _| !excluded.contains(&("policy".into(), id.clone())));
    registry
        .templates
        .retain(|id, _| !excluded.contains(&("template".into(), id.clone())));
    registry
        .model_swaps
        .retain(|id, _| !excluded.contains(&("model_swap".into(), id.clone())));
    registry
        .personas
        .retain(|id, _| !excluded.contains(&("persona".into(), id.clone())));
}

/// Retain only persona source versions that have durable row and digest
/// backing. Sources are kept for every persona version so an ineligible
/// higher version cannot hide an eligible lower version.
pub fn exclude_unbacked_persona_versions(
    registry: &mut ArtifactRegistry,
    eligible: &HashSet<(String, u32)>,
) -> anyhow::Result<Vec<(String, u32)>> {
    let persona_sources: Vec<(String, u32)> = registry
        .sources
        .keys()
        .filter(|(kind, _, _)| kind == "persona")
        .map(|(_, id, version)| (id.clone(), *version))
        .collect();
    let mut excluded = Vec::new();
    for (id, version) in persona_sources {
        if eligible.contains(&(id.clone(), version)) {
            continue;
        }
        registry
            .sources
            .remove(&("persona".into(), id.clone(), version));
        if registry
            .personas
            .get(&id)
            .is_some_and(|persona| persona.version == version)
        {
            registry.personas.remove(&id);
        }
        excluded.push((id, version));
    }
    let mut to_rehydrate: Vec<((String, String, u32), ArtifactSource)> = registry
        .sources
        .iter()
        .filter(|((kind, id, version), _)| {
            kind == "persona"
                && eligible.contains(&(id.clone(), *version))
                && registry
                    .personas
                    .get(id)
                    .is_none_or(|persona| persona.version < *version)
        })
        .map(|((kind, id, version), source)| ((kind.clone(), id.clone(), *version), source.clone()))
        .collect();
    to_rehydrate.sort_by(
        |((_, left_id, left_version), _), ((_, right_id, right_version), _)| {
            left_id.cmp(right_id).then(left_version.cmp(right_version))
        },
    );
    for ((_kind, id, version), source) in to_rehydrate {
        let persona: PersonaElement = serde_yaml::from_slice(&source.bytes)?;
        if persona.id != id || persona.version != version {
            anyhow::bail!("persona source identity mismatch for {id} v{version}");
        }
        registry.personas.insert(id, persona);
    }
    Ok(excluded)
}
pub fn artifact_version(registry: &ArtifactRegistry, kind: &str, id: &str) -> Option<u32> {
    match kind {
        "route" => registry
            .routes
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.version),
        "agent" => registry.agents.get(id).map(|a| a.version),
        "workflow" => registry.workflows.get(id).map(|a| a.version),
        "pack" => registry.packs.get(id).map(|a| a.version),
        "policy" => registry.policies.get(id).map(|a| a.version),
        "template" => registry.templates.get(id).map(|a| a.version),
        "model_swap" => registry.model_swaps.get(id).map(|a| a.version),
        "persona" => registry.personas.get(id).map(|p| p.version),
        _ => None,
    }
}

/// Merge an overlay registry into a base registry (5a/5d: approved overlay
/// artifacts survive restart). Every field is merged; an overlay route
/// replaces any prior route of the same id (highest version wins, matching
/// the loader's cutover) so base and overlay versions never coexist and the
/// live registry equals the restart registry (AD-070).
pub fn merge_registry(dst: &mut ArtifactRegistry, src: ArtifactRegistry) {
    for route in src.routes {
        dst.routes.retain(|a| a.id != route.id);
        dst.routes.push(route);
    }
    dst.agents.extend(src.agents);
    dst.workflows.extend(src.workflows);
    dst.packs.extend(src.packs);
    dst.policies.extend(src.policies);
    dst.templates.extend(src.templates);
    dst.model_swaps.extend(src.model_swaps);
    dst.sources.extend(src.sources);
    dst.personas.extend(src.personas);
}

fn load_yaml_dir<T, F>(dir: &Path, mut on_each: F) -> Result<(), ArtifactLoadError>
where
    T: serde::de::DeserializeOwned,
    F: FnMut(&Path, Vec<u8>, T) -> Result<(), ArtifactLoadError>,
{
    if !dir.is_dir() {
        return Ok(());
    }
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|source| ArtifactLoadError::Read {
            path: dir.to_path_buf(),
            source,
        })?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|ext| ext == "yaml" || ext == "yml")
        })
        .collect();
    entries.sort();

    for path in entries {
        let bytes = std::fs::read(&path).map_err(|source| ArtifactLoadError::Read {
            path: path.clone(),
            source,
        })?;
        let value: T =
            serde_yaml::from_slice(&bytes).map_err(|source| ArtifactLoadError::Parse {
                path: path.clone(),
                source,
            })?;
        on_each(&path, bytes, value)?;
    }
    Ok(())
}

/// Parse and validate every artifact under `dir` (e.g. `artifacts/lyra`).
/// Each subdirectory maps to one artifact kind; `deny_unknown_fields` on
/// every schema type *is* the validation (D-028) — a malformed fixture
/// fails to parse rather than silently loading with dropped fields.
pub fn load_registry(dir: &Path) -> Result<ArtifactRegistry, ArtifactLoadError> {
    let mut registry = ArtifactRegistry::default();
    load_registry_into(&mut registry, dir)?;
    Ok(registry)
}

/// Load all non-persona overlay kinds. Persona admission is caller-gated by
/// durable learned rows and exact YAML digests before any persona parse or
/// version precedence can occur.
pub fn load_registry_without_personas(dir: &Path) -> Result<ArtifactRegistry, ArtifactLoadError> {
    load_registry(dir)
}

/// Load only persona files whose raw YAML digest is explicitly admitted by a
/// durable learned row. Unadmitted files are ignored before schema parsing or
/// version precedence, so malformed/orphan higher versions cannot fail startup
/// or hide an eligible lower version.
pub fn load_admitted_personas(
    registry: &mut ArtifactRegistry,
    dir: &Path,
    admitted: &HashMap<(String, u32), String>,
) -> Result<(), ArtifactLoadError> {
    let personas_dir = dir.join("personas");
    if !personas_dir.is_dir() {
        return Ok(());
    }
    let mut entries: Vec<_> = std::fs::read_dir(&personas_dir)
        .map_err(|source| ArtifactLoadError::Read {
            path: personas_dir.clone(),
            source,
        })?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "yaml" || extension == "yml")
        })
        .collect();
    entries.sort();
    for path in entries {
        let bytes = std::fs::read(&path).map_err(|source| ArtifactLoadError::Read {
            path: path.clone(),
            source,
        })?;
        let Ok(value) = serde_yaml::from_slice::<serde_yaml::Value>(&bytes) else {
            continue;
        };
        let Some(id) = value.get("id").and_then(serde_yaml::Value::as_str) else {
            continue;
        };
        let Some(version) = value
            .get("version")
            .and_then(serde_yaml::Value::as_u64)
            .map(|value| value as u32)
        else {
            continue;
        };
        let key = (id.to_string(), version);
        if admitted.get(&key)
            != Some(&openspine_schemas::digest::digest_of_bytes(&bytes).to_string())
        {
            continue;
        }
        let persona: PersonaElement =
            serde_yaml::from_slice(&bytes).map_err(|source| ArtifactLoadError::Parse {
                path: path.clone(),
                source,
            })?;
        let existing_version = registry.personas.get(id).map(|persona| persona.version);
        if existing_version.is_some_and(|existing| existing >= version) {
            if existing_version.is_some_and(|existing| existing > version) {
                registry.sources.insert(
                    ("persona".into(), id.to_string(), version),
                    ArtifactSource { path, bytes },
                );
            }
            continue;
        }
        registry.personas.insert(id.to_string(), persona);
        registry.sources.insert(
            ("persona".into(), id.to_string(), version),
            ArtifactSource { path, bytes },
        );
    }
    Ok(())
}

/// Load the base (kernel-shipped) fixture tree. Unlike [`load_registry`],
/// this rejects a non-empty `personas/` subdirectory outright rather than
/// parsing it: personas are an overlay-only artifact kind that must ship as
/// learnable, pre-populated overlay artifacts, never as base fixtures
/// (AD-080). A persona YAML placed in the base tree would otherwise load
/// silently, be counted in `base_artifact_ids`, and be admitted without any
/// `ProducedBy` provenance row — defeating the overlay-only provenance
/// boundary. This is deliberately a separate entry point (not a parameter
/// on [`load_registry`]/[`load_registry_into`]) so every existing overlay
/// load and test — which legitimately load personas — is unaffected.
/// Load the base (kernel-shipped) fixture tree. Unlike [`load_registry`],
/// this enforces the overlay-only provenance boundary for personas (AD-080):
/// personas must ship as learnable, pre-populated overlay artifacts, never as
/// base fixtures. A persona YAML placed in the base tree would otherwise load
/// silently, be counted in `base_artifact_ids`, and be admitted without any
/// `ProducedBy` provenance row — defeating the boundary. The check is done
/// *after* loading (not check-then-load) so a file appearing between the
/// probe and the real read cannot slip through. This is a separate entry
/// point (not a parameter on [`load_registry`]/[`load_registry_into`]) so
/// every existing overlay load and test — which legitimately load personas —
/// is unaffected.
pub fn load_base_registry(dir: &Path) -> Result<ArtifactRegistry, ArtifactLoadError> {
    let registry = load_registry(dir)?;
    let persona_dir = dir.join("personas");
    let has_persona_fixture = persona_dir.is_dir()
        && std::fs::read_dir(&persona_dir)
            .map_err(|source| ArtifactLoadError::Read {
                path: persona_dir.clone(),
                source,
            })?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .any(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "yaml" || extension == "yml")
            });
    if has_persona_fixture {
        return Err(ArtifactLoadError::Invalid {
            kind: "persona".to_string(),
            id: "*".to_string(),
            reason: format!(
                "personas are overlay-only (AD-080); base tree {} must not contain persona fixtures",
                dir.display()
            ),
        });
    }
    Ok(registry)
}

/// Merge every non-persona artifact under `dir` into an existing registry.
/// Persona loading is exclusively handled by `load_admitted_personas`.
pub fn load_registry_into(
    registry: &mut ArtifactRegistry,
    dir: &Path,
) -> Result<(), ArtifactLoadError> {
    artifact_loader_impl::load_registry_into(registry, dir)?;
    load_yaml_dir(
        &dir.join("golden_sets"),
        |path: &Path, bytes: Vec<u8>, g: GoldenSet| {
            g.validate().map_err(|err| ArtifactLoadError::Invalid {
                kind: "golden_set".to_string(),
                id: g.id.clone(),
                reason: err.to_string(),
            })?;
            let id = g.id.clone();
            if registry.golden_sets.contains_key(&id) {
                return Err(ArtifactLoadError::Collision {
                    kind: "golden_set".to_string(),
                    id,
                    version: 1,
                });
            }
            registry.golden_sets.insert(id.clone(), g);
            registry.sources.insert(
                ("golden_set".into(), id, 1),
                ArtifactSource {
                    path: path.to_path_buf(),
                    bytes,
                },
            );
            Ok(())
        },
    )?;
    load_yaml_dir(
        &dir.join("model_swaps"),
        |path: &Path, bytes: Vec<u8>, m: ModelSwapManifest| {
            if !m.identity_valid() {
                return Err(ArtifactLoadError::Invalid {
                    kind: "model_swap".to_string(),
                    id: m.id.clone(),
                    reason: "id must equal role name".to_string(),
                });
            }
            if let Some(existing) = registry.model_swaps.get(&m.id) {
                if existing.version == m.version {
                    return Err(ArtifactLoadError::Collision {
                        kind: "model_swap".to_string(),
                        id: m.id.clone(),
                        version: m.version,
                    });
                }
                if existing.version > m.version {
                    return Ok(());
                }
            }
            let id = m.id.clone();
            let version = m.version;
            registry.model_swaps.insert(id.clone(), m);
            registry.sources.insert(
                ("model_swap".into(), id, version),
                ArtifactSource {
                    path: path.to_path_buf(),
                    bytes,
                },
            );
            Ok(())
        },
    )?;
    Ok(())
}

/// Read-only access to the content `version` of a versioned declarative
/// artifact, so `collide_keyed` can stay generic over the keyed kinds.
trait Versioned {
    fn version(&self) -> u32;
}

impl Versioned for AgentManifest {
    fn version(&self) -> u32 {
        self.version
    }
}

impl Versioned for WorkflowManifest {
    fn version(&self) -> u32 {
        self.version
    }
}

impl Versioned for CapabilityPack {
    fn version(&self) -> u32 {
        self.version
    }
}

impl Versioned for Policy {
    fn version(&self) -> u32 {
        self.version
    }
}

impl Versioned for PromptTemplate {
    fn version(&self) -> u32 {
        self.version
    }
}

impl Versioned for ModelSwapManifest {
    fn version(&self) -> u32 {
        self.version
    }
}
impl Versioned for PersonaElement {
    fn version(&self) -> u32 {
        self.version
    }
}
impl Versioned for StandingRuleManifest {
    fn version(&self) -> u32 {
        self.version
    }
}
/// A declarative artifact parsed from a proposal's YAML, tagged by kind.
/// `route | agent | workflow | pack | policy | model_swap` are proposable;
/// prompt templates and golden sets remain fixture-only because they define
/// the instruction/evaluation surface.
#[derive(Debug)]
pub enum ParsedProposal {
    Route(Route),
    Agent(AgentManifest),
    Workflow(WorkflowManifest),
    Pack(CapabilityPack),
    Policy(Policy),
    ModelSwap(ModelSwapManifest),
    StandingRule(StandingRuleManifest),
    /// Persona element (AD-094/AD-135): non-authority behavioral guidance,
    /// proposable through the normal lifecycle so owner corrections to the
    /// learned digest default converge as reviewed overlay artifacts.
    Persona(PersonaElement),
}

impl ParsedProposal {
    pub fn kind(&self) -> &'static str {
        match self {
            ParsedProposal::Route(_) => "route",
            ParsedProposal::Agent(_) => "agent",
            ParsedProposal::Workflow(_) => "workflow",
            ParsedProposal::Pack(_) => "pack",
            ParsedProposal::Policy(_) => "policy",
            ParsedProposal::ModelSwap(_) => "model_swap",
            ParsedProposal::StandingRule(_) => "standing_rule",
            ParsedProposal::Persona(_) => "persona",
        }
    }

    pub fn artifact_id(&self) -> &str {
        match self {
            ParsedProposal::Route(r) => &r.id,
            ParsedProposal::Agent(a) => &a.id,
            ParsedProposal::Workflow(w) => &w.id,
            ParsedProposal::Pack(p) => &p.id,
            ParsedProposal::Policy(p) => &p.id,
            ParsedProposal::ModelSwap(m) => &m.id,
            ParsedProposal::StandingRule(r) => &r.id,
            ParsedProposal::Persona(p) => &p.id,
        }
    }

    pub fn version(&self) -> u32 {
        match self {
            ParsedProposal::Route(r) => r.version,
            ParsedProposal::Agent(a) => a.version,
            ParsedProposal::Workflow(w) => w.version,
            ParsedProposal::Pack(p) => p.version,
            ParsedProposal::Policy(p) => p.version,
            ParsedProposal::ModelSwap(m) => m.version,
            ParsedProposal::StandingRule(r) => r.version,
            ParsedProposal::Persona(p) => p.version,
        }
    }

    pub fn lifecycle_state(&self) -> Lifecycle {
        match self {
            ParsedProposal::Route(r) => r.lifecycle_state,
            ParsedProposal::Agent(a) => a.lifecycle_state,
            ParsedProposal::Workflow(w) => w.lifecycle_state,
            ParsedProposal::Pack(p) => p.lifecycle_state,
            ParsedProposal::Policy(p) => p.lifecycle_state,
            ParsedProposal::ModelSwap(m) => m.lifecycle_state,
            ParsedProposal::StandingRule(r) => r.lifecycle_state,
            ParsedProposal::Persona(p) => p.lifecycle_state,
        }
    }
    /// Overlay subdirectory name matching the loader's per-kind layout
    /// (5d writes `<overlay>/<subdir>/<id>-v<version>.yaml`). Derived from
    /// the kind table — the single source of truth for per-kind layout.
    pub fn overlay_subdir(&self) -> &'static str {
        find_kind_spec(self.kind())
            .expect("every ParsedProposal variant has a kind-table entry")
            .overlay_subdir
    }

    /// Flip the artifact's `lifecycle_state` to `active` (5d activation).
    pub fn activate(&mut self) {
        let active = Lifecycle::Active;
        match self {
            ParsedProposal::Route(r) => r.lifecycle_state = active,
            ParsedProposal::Agent(a) => a.lifecycle_state = active,
            ParsedProposal::Workflow(w) => w.lifecycle_state = active,
            ParsedProposal::Pack(p) => p.lifecycle_state = active,
            ParsedProposal::Policy(p) => p.lifecycle_state = active,
            ParsedProposal::ModelSwap(m) => m.lifecycle_state = active,
            ParsedProposal::StandingRule(r) => r.lifecycle_state = active,
            ParsedProposal::Persona(p) => p.lifecycle_state = active,
        }
    }

    /// Serialize back to YAML (the overlay file's content).
    pub fn to_yaml(&self) -> serde_yaml::Result<String> {
        match self {
            ParsedProposal::Route(r) => serde_yaml::to_string(r),
            ParsedProposal::Agent(a) => serde_yaml::to_string(a),
            ParsedProposal::Workflow(w) => serde_yaml::to_string(w),
            ParsedProposal::Pack(p) => serde_yaml::to_string(p),
            ParsedProposal::ModelSwap(m) => serde_yaml::to_string(m),
            ParsedProposal::Policy(p) => serde_yaml::to_string(p),
            ParsedProposal::StandingRule(r) => serde_yaml::to_string(r),
            ParsedProposal::Persona(p) => serde_yaml::to_string(p),
        }
    }

    /// Insert into a live registry (5d). For every kind this atomically
    /// replaces the prior version of the same identity so the live registry
    /// mirrors restart loading (only the highest active version of an id is
    /// ever effective — AD-070 version-state cutover).
    ///
    /// Returns `Ok(Some(replaced_version))` when an older version was
    /// superseded, `Ok(None)` for a fresh insert, and `Err` for an exact
    /// duplicate (same id+version already active) or a lower version than
    /// the currently-active one (rejected — monotonic versions only).
    pub fn insert_into(self, registry: &mut ArtifactRegistry) -> anyhow::Result<Option<u32>> {
        match self {
            ParsedProposal::Route(r) => {
                if let Some(existing) = registry.routes.iter().find(|e| e.id == r.id) {
                    if existing.version == r.version {
                        anyhow::bail!("exact duplicate route {} v{}", r.id, r.version);
                    }
                    if existing.version > r.version {
                        anyhow::bail!(
                            "lower version route {} v{} rejected; active is v{}",
                            r.id,
                            r.version,
                            existing.version
                        );
                    }
                    let old = existing.version;
                    registry.routes.retain(|e| e.id != r.id);
                    registry.routes.push(r);
                    return Ok(Some(old));
                }
                registry.routes.push(r);
                Ok(None)
            }
            ParsedProposal::StandingRule(r) => {
                replace_keyed(&mut registry.standing_rules, r.id.clone(), r)
            }
            ParsedProposal::ModelSwap(m) => {
                replace_keyed(&mut registry.model_swaps, m.id.clone(), m)
            }
            ParsedProposal::Agent(a) => replace_keyed(&mut registry.agents, a.id.clone(), a),
            ParsedProposal::Workflow(w) => replace_keyed(&mut registry.workflows, w.id.clone(), w),
            ParsedProposal::Pack(p) => replace_keyed(&mut registry.packs, p.id.clone(), p),
            ParsedProposal::Policy(p) => replace_keyed(&mut registry.policies, p.id.clone(), p),
            ParsedProposal::Persona(p) => replace_keyed(&mut registry.personas, p.id.clone(), p),
        }
    }
}

/// Read-only access to the content `version` of a versioned declarative
pub(super) trait Identified {
    fn id(&self) -> &str;
}

fn replace_keyed<T: Versioned>(
    map: &mut HashMap<String, T>,
    id: String,
    artifact: T,
) -> anyhow::Result<Option<u32>> {
    if let Some(existing) = map.get(&id) {
        if existing.version() == artifact.version() {
            anyhow::bail!("exact duplicate {} v{}", id, artifact.version());
        }
        if existing.version() > artifact.version() {
            anyhow::bail!(
                "lower version {} v{} rejected; active is v{}",
                id,
                artifact.version(),
                existing.version()
            );
        }
        let old = existing.version();
        map.insert(id, artifact);
        return Ok(Some(old));
    }
    map.insert(id, artifact);
    Ok(None)
}

/// Single source of truth for the seven proposable artifact kinds (PRD §13/5c,
/// D-048, AD-152). Each entry pairs a kind's name and overlay subdirectory
/// with parsing and duplicate-check behavior. Prompt templates and golden
/// sets are deliberately fixture-only.
pub struct ArtifactKindSpec {
    pub name: &'static str,
    pub overlay_subdir: &'static str,
    pub parse: fn(&str) -> Result<ParsedProposal, serde_yaml::Error>,
    pub duplicate_exists: fn(&ArtifactRegistry, &str, u32) -> bool,
}

/// The eight proposable kinds, in a fixed order. This table is the only
/// declaration of what `artifact.propose` accepts; the kind guard, parser,
/// and duplicate-check all derive from it.
pub static ARTIFACT_KIND_SPECS: &[ArtifactKindSpec; 8] = &[
    ArtifactKindSpec {
        name: "route",
        overlay_subdir: "routes",
        parse: |yaml| Ok(ParsedProposal::Route(serde_yaml::from_str(yaml)?)),
        duplicate_exists: |registry, id, version| {
            registry
                .routes
                .iter()
                .any(|r| r.id == id && r.version == version)
        },
    },
    ArtifactKindSpec {
        name: "agent",
        overlay_subdir: "agents",
        parse: |yaml| Ok(ParsedProposal::Agent(serde_yaml::from_str(yaml)?)),
        duplicate_exists: |registry, id, version| {
            registry
                .agents
                .get(id)
                .is_some_and(|a| a.version == version)
        },
    },
    ArtifactKindSpec {
        name: "workflow",
        overlay_subdir: "workflows",
        parse: |yaml| Ok(ParsedProposal::Workflow(serde_yaml::from_str(yaml)?)),
        duplicate_exists: |registry, id, version| {
            registry
                .workflows
                .get(id)
                .is_some_and(|w| w.version == version)
        },
    },
    ArtifactKindSpec {
        name: "pack",
        overlay_subdir: "packs",
        parse: |yaml| Ok(ParsedProposal::Pack(serde_yaml::from_str(yaml)?)),
        duplicate_exists: |registry, id, version| {
            registry.packs.get(id).is_some_and(|p| p.version == version)
        },
    },
    ArtifactKindSpec {
        name: "policy",
        overlay_subdir: "policies",
        parse: |yaml| Ok(ParsedProposal::Policy(serde_yaml::from_str(yaml)?)),
        duplicate_exists: |registry, id, version| {
            registry
                .policies
                .get(id)
                .is_some_and(|p| p.version == version)
        },
    },
    ArtifactKindSpec {
        name: "model_swap",
        overlay_subdir: "model_swaps",
        parse: |yaml| Ok(ParsedProposal::ModelSwap(serde_yaml::from_str(yaml)?)),
        duplicate_exists: |registry, id, version| {
            registry
                .model_swaps
                .get(id)
                .is_some_and(|m| m.version == version)
        },
    },
    ArtifactKindSpec {
        name: "standing_rule",
        overlay_subdir: "standing_rules",
        parse: |yaml| Ok(ParsedProposal::StandingRule(serde_yaml::from_str(yaml)?)),
        duplicate_exists: |registry, id, version| {
            registry
                .standing_rules
                .get(id)
                .is_some_and(|r| r.version == version)
        },
    },
    ArtifactKindSpec {
        name: "persona",
        overlay_subdir: "personas",
        parse: |yaml| Ok(ParsedProposal::Persona(serde_yaml::from_str(yaml)?)),
        duplicate_exists: |registry, id, version| {
            registry
                .personas
                .get(id)
                .is_some_and(|persona| persona.version == version)
        },
    },
];

/// Look up a kind spec by name in the single source of truth.
pub fn find_kind_spec(name: &str) -> Option<&'static ArtifactKindSpec> {
    ARTIFACT_KIND_SPECS.iter().find(|spec| spec.name == name)
}
/// Overlay directory for every loaded kind, including non-proposable prompt
/// templates encountered during legacy migration.
pub fn overlay_subdir_for_kind(kind: &str) -> Option<&'static str> {
    if kind == "template" {
        Some("templates")
    } else {
        find_kind_spec(kind).map(|spec| spec.overlay_subdir)
    }
}

/// Whether `kind` is one of the five proposable kinds (templates excluded).
pub fn is_proposable_kind(kind: &str) -> bool {
    find_kind_spec(kind).is_some()
}
/// Parse proposal YAML for `kind` into a [`ParsedProposal`]. `kind` must
/// already be one of the seven proposable kinds; an unknown kind yields a
/// serde error rather than a silent accept.
pub fn parse_proposal(kind: &str, yaml: &str) -> Result<ParsedProposal, serde_yaml::Error> {
    use serde::de::Error as _;
    match find_kind_spec(kind) {
        Some(spec) => (spec.parse)(yaml),
        None => Err(serde_yaml::Error::custom(format!(
            "unknown artifact kind {kind}"
        ))),
    }
}

#[cfg(test)]
mod tests;
