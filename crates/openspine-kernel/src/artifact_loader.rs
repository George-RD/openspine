//! Load and validate every `artifacts/**/*.yaml` fixture at startup (build
//! plan 4a). Only `lifecycle_state: active` artifacts join routing — that
//! constraint already lives in `resolve_route`/`compose_authority`
//! (Step 2/3), so this loader just parses everything into typed registries
//! without pre-filtering; loading itself is what gets audited here.

use std::collections::HashMap;
use std::path::Path;

use crate::model_gateway::PromptTemplate;
use openspine_schemas::agent::AgentManifest;
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::ids::ArtifactId;
use openspine_schemas::model_swap::{GoldenSet, ModelSwapManifest};
use openspine_schemas::pack::CapabilityPack;
use openspine_schemas::policy::Policy;
use openspine_schemas::route::Route;
use openspine_schemas::workflow::WorkflowManifest;

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
    #[error("artifact collision: {kind} id={id} v{version} appears more than once (check fixtures and the data/artifacts.d overlay)")]
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
#[derive(Debug, Default)]
pub struct ArtifactRegistry {
    pub routes: Vec<Route>,
    pub agents: HashMap<ArtifactId, AgentManifest>,
    pub workflows: HashMap<ArtifactId, WorkflowManifest>,
    pub packs: HashMap<ArtifactId, CapabilityPack>,
    pub policies: HashMap<ArtifactId, Policy>,
    pub templates: HashMap<String, PromptTemplate>,
    pub golden_sets: HashMap<String, GoldenSet>,
    pub model_swaps: HashMap<ArtifactId, ModelSwapManifest>,
}

fn load_yaml_dir<T, F>(dir: &Path, mut on_each: F) -> Result<(), ArtifactLoadError>
where
    T: serde::de::DeserializeOwned,
    F: FnMut(T) -> Result<(), ArtifactLoadError>,
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
        let text = std::fs::read_to_string(&path).map_err(|source| ArtifactLoadError::Read {
            path: path.clone(),
            source,
        })?;
        let value: T = serde_yaml::from_str(&text).map_err(|source| ArtifactLoadError::Parse {
            path: path.clone(),
            source,
        })?;
        on_each(value)?;
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

/// Merge every artifact under `dir` into an existing registry (5a: the
/// startup loader first fills the registry from the fixture dir, then
/// calls this again for the `data/artifacts.d` overlay so approved
/// proposals survive restart). A `(kind, id, version)` already present
/// in the registry is a hard error rather than silent precedence —
/// activation-time checks make such a collision unreachable except by a
/// manual file edit, and fail-fast beats a silently-shadowed artifact.
pub fn load_registry_into(
    registry: &mut ArtifactRegistry,
    dir: &Path,
) -> Result<(), ArtifactLoadError> {
    load_yaml_dir(&dir.join("routes"), |r: Route| {
        collide_route(&registry.routes, &r)?;
        registry.routes.push(r);
        Ok(())
    })?;
    load_yaml_dir(&dir.join("agents"), |a: AgentManifest| {
        collide_keyed(registry.agents.get(&a.id), "agent", &a.id, a.version)?;
        registry.agents.insert(a.id.clone(), a);
        Ok(())
    })?;
    load_yaml_dir(&dir.join("workflows"), |w: WorkflowManifest| {
        collide_keyed(registry.workflows.get(&w.id), "workflow", &w.id, w.version)?;
        registry.workflows.insert(w.id.clone(), w);
        Ok(())
    })?;
    load_yaml_dir(&dir.join("packs"), |p: CapabilityPack| {
        collide_keyed(registry.packs.get(&p.id), "pack", &p.id, p.version)?;
        registry.packs.insert(p.id.clone(), p);
        Ok(())
    })?;
    load_yaml_dir(&dir.join("policies"), |p: Policy| {
        collide_keyed(registry.policies.get(&p.id), "policy", &p.id, p.version)?;
        registry.policies.insert(p.id.clone(), p);
        Ok(())
    })?;
    load_yaml_dir(&dir.join("templates"), |t: PromptTemplate| {
        collide_keyed(registry.templates.get(&t.id), "template", &t.id, t.version)?;
        registry.templates.insert(t.id.clone(), t);
        Ok(())
    })?;
    load_yaml_dir(&dir.join("golden_sets"), |g: GoldenSet| {
        g.validate().map_err(|err| ArtifactLoadError::Invalid {
            kind: "golden_set".to_string(),
            id: g.id.clone(),
            reason: err.to_string(),
        })?;
        let id = g.id.clone();
        if registry.golden_sets.insert(id.clone(), g).is_some() {
            return Err(ArtifactLoadError::Collision {
                kind: "golden_set".to_string(),
                id,
                version: 1,
            });
        }
        Ok(())
    })?;
    load_yaml_dir(&dir.join("model_swaps"), |m: ModelSwapManifest| {
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
        registry.model_swaps.insert(m.id.clone(), m);
        Ok(())
    })?;
    Ok(())
}

fn collide_route(existing: &[Route], candidate: &Route) -> Result<(), ArtifactLoadError> {
    if existing
        .iter()
        .any(|e| e.id == candidate.id && e.version == candidate.version)
    {
        return Err(ArtifactLoadError::Collision {
            kind: "route".into(),
            id: candidate.id.clone(),
            version: candidate.version,
        });
    }
    Ok(())
}

fn collide_keyed<T: Versioned>(
    existing: Option<&T>,
    kind: &str,
    id: &str,
    version: u32,
) -> Result<(), ArtifactLoadError> {
    if existing.is_some_and(|e| e.version() == version) {
        return Err(ArtifactLoadError::Collision {
            kind: kind.into(),
            id: id.into(),
            version,
        });
    }
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
        }
    }

    /// Insert into a live registry (5d): routes push (resolution scans the
    /// whole set), keyed kinds insert by id.
    pub fn insert_into(self, registry: &mut ArtifactRegistry) {
        match self {
            ParsedProposal::Route(r) => registry.routes.push(r),
            ParsedProposal::Agent(a) => {
                registry.agents.insert(a.id.clone(), a);
            }
            ParsedProposal::Workflow(w) => {
                registry.workflows.insert(w.id.clone(), w);
            }
            ParsedProposal::Pack(p) => {
                registry.packs.insert(p.id.clone(), p);
            }
            ParsedProposal::Policy(p) => {
                registry.policies.insert(p.id.clone(), p);
            }
            ParsedProposal::ModelSwap(m) => {
                registry.model_swaps.insert(m.id.clone(), m);
            }
        }
    }
}

/// Single source of truth for the six proposable artifact kinds (PRD §13/5c,
/// D-048, AD-152). Each entry pairs a kind's name and overlay subdirectory
/// with parsing and duplicate-check behavior. Prompt templates and golden
/// sets are deliberately fixture-only.
pub struct ArtifactKindSpec {
    pub name: &'static str,
    pub overlay_subdir: &'static str,
    pub parse: fn(&str) -> Result<ParsedProposal, serde_yaml::Error>,
    pub duplicate_exists: fn(&ArtifactRegistry, &str, u32) -> bool,
}

/// The six proposable kinds, in a fixed order. This table is the only
/// declaration of what `artifact.propose` accepts; the kind guard, parser,
/// and duplicate-check all derive from it.
pub static ARTIFACT_KIND_SPECS: &[ArtifactKindSpec; 6] = &[
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
];

/// Look up a kind spec by name in the single source of truth.
pub fn find_kind_spec(name: &str) -> Option<&'static ArtifactKindSpec> {
    ARTIFACT_KIND_SPECS.iter().find(|spec| spec.name == name)
}

/// Whether `kind` is one of the five proposable kinds (templates excluded).
pub fn is_proposable_kind(kind: &str) -> bool {
    find_kind_spec(kind).is_some()
}
/// Parse proposal YAML for `kind` into a [`ParsedProposal`]. `kind` must
/// already be one of the five proposable kinds; an unknown kind yields a
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
