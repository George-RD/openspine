//! Load and validate every `artifacts/**/*.yaml` fixture at startup (build
//! plan 4a). Only `lifecycle_state: active` artifacts join routing — that
//! constraint already lives in `resolve_route`/`compose_authority`
//! (Step 2/3), so this loader just parses everything into typed registries
//! without pre-filtering; loading itself is what gets audited here.

use std::collections::HashMap;
use std::path::Path;

use openspine_schemas::agent::AgentManifest;
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::ids::ArtifactId;
use openspine_schemas::pack::CapabilityPack;
use openspine_schemas::policy::Policy;
use openspine_schemas::route::Route;
use openspine_schemas::workflow::WorkflowManifest;

use crate::model_gateway::PromptTemplate;

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

/// A declarative artifact parsed from a proposal's YAML, tagged by kind.
/// Only `route | agent | workflow | pack | policy` are proposable — prompt
/// templates are excluded (a template changes the model's instruction
/// surface; letting chat propose one is an injection-escalation channel).
/// Shared by `artifact.propose` (validate + extract id/version) and
/// `artifact.activate` (re-parse, flip to active, persist).
pub enum ParsedProposal {
    Route(Route),
    Agent(AgentManifest),
    Workflow(WorkflowManifest),
    Pack(CapabilityPack),
    Policy(Policy),
}

impl ParsedProposal {
    pub fn kind(&self) -> &'static str {
        match self {
            ParsedProposal::Route(_) => "route",
            ParsedProposal::Agent(_) => "agent",
            ParsedProposal::Workflow(_) => "workflow",
            ParsedProposal::Pack(_) => "pack",
            ParsedProposal::Policy(_) => "policy",
        }
    }

    pub fn artifact_id(&self) -> &str {
        match self {
            ParsedProposal::Route(r) => &r.id,
            ParsedProposal::Agent(a) => &a.id,
            ParsedProposal::Workflow(w) => &w.id,
            ParsedProposal::Pack(p) => &p.id,
            ParsedProposal::Policy(p) => &p.id,
        }
    }

    pub fn version(&self) -> u32 {
        match self {
            ParsedProposal::Route(r) => r.version,
            ParsedProposal::Agent(a) => a.version,
            ParsedProposal::Workflow(w) => w.version,
            ParsedProposal::Pack(p) => p.version,
            ParsedProposal::Policy(p) => p.version,
        }
    }

    pub fn lifecycle_state(&self) -> Lifecycle {
        match self {
            ParsedProposal::Route(r) => r.lifecycle_state,
            ParsedProposal::Agent(a) => a.lifecycle_state,
            ParsedProposal::Workflow(w) => w.lifecycle_state,
            ParsedProposal::Pack(p) => p.lifecycle_state,
            ParsedProposal::Policy(p) => p.lifecycle_state,
        }
    }

    /// Overlay subdirectory name matching the loader's per-kind layout
    /// (5d writes `<overlay>/<subdir>/<id>-v<version>.yaml`).
    pub fn overlay_subdir(&self) -> &'static str {
        match self {
            ParsedProposal::Route(_) => "routes",
            ParsedProposal::Agent(_) => "agents",
            ParsedProposal::Workflow(_) => "workflows",
            ParsedProposal::Pack(_) => "packs",
            ParsedProposal::Policy(_) => "policies",
        }
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
        }
    }

    /// Serialize back to YAML (the overlay file's content).
    pub fn to_yaml(&self) -> serde_yaml::Result<String> {
        match self {
            ParsedProposal::Route(r) => serde_yaml::to_string(r),
            ParsedProposal::Agent(a) => serde_yaml::to_string(a),
            ParsedProposal::Workflow(w) => serde_yaml::to_string(w),
            ParsedProposal::Pack(p) => serde_yaml::to_string(p),
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
        }
    }
}

/// Parse proposal YAML for `kind` into a [`ParsedProposal`]. `kind` must
/// already be one of the five proposable kinds; an unknown kind yields a
/// serde error rather than a silent accept.
pub fn parse_proposal(kind: &str, yaml: &str) -> Result<ParsedProposal, serde_yaml::Error> {
    use serde::de::Error as _;
    Ok(match kind {
        "route" => ParsedProposal::Route(serde_yaml::from_str(yaml)?),
        "agent" => ParsedProposal::Agent(serde_yaml::from_str(yaml)?),
        "workflow" => ParsedProposal::Workflow(serde_yaml::from_str(yaml)?),
        "pack" => ParsedProposal::Pack(serde_yaml::from_str(yaml)?),
        "policy" => ParsedProposal::Policy(serde_yaml::from_str(yaml)?),
        other => {
            return Err(serde_yaml::Error::custom(format!(
                "unknown artifact kind {other}"
            )));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_lyra_dir() -> std::path::PathBuf {
        // crates/openspine-kernel -> repo root -> artifacts/lyra
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra")
    }

    #[test]
    fn loads_every_real_fixture_without_error() {
        let registry = load_registry(&repo_lyra_dir()).expect("real fixtures must all parse");
        assert!(!registry.routes.is_empty());
        assert!(registry.agents.contains_key("main_assistant_agent"));
        assert!(registry
            .workflows
            .contains_key("owner_control_conversation"));
        assert!(registry.packs.contains_key("owner_control_basic_pack"));
        assert!(registry.policies.contains_key("global"));
        assert!(registry.templates.contains_key("owner_control_template"));

        // Step 5 (implement-selected-thread-email-preview-slice) fixtures.
        assert!(registry.agents.contains_key("email_reply_drafter"));
        assert!(registry
            .workflows
            .contains_key("selected_thread_email_reply_draft"));
        assert!(registry
            .packs
            .contains_key("selected_thread_email_draft_pack"));
        assert!(registry
            .routes
            .iter()
            .any(|r| r.id == "owner_email_selected_thread"));
        assert!(registry
            .templates
            .contains_key("email_reply_draft_template"));
    }

    #[test]
    fn missing_directory_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let registry = load_registry(dir.path()).expect("no subdirectories at all is fine");
        assert!(registry.routes.is_empty());
        assert!(registry.agents.is_empty());
    }

    #[test]
    fn malformed_fixture_fails_to_load() {
        let dir = tempfile::tempdir().unwrap();
        let agents_dir = dir.path().join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();
        std::fs::write(agents_dir.join("bad.yaml"), "id: x\nunknown_field: true\n").unwrap();
        let err = load_registry(dir.path()).unwrap_err();
        assert!(matches!(err, ArtifactLoadError::Parse { .. }));
    }

    #[test]
    fn non_yaml_files_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let routes_dir = dir.path().join("routes");
        std::fs::create_dir_all(&routes_dir).unwrap();
        std::fs::write(routes_dir.join("README.md"), "not yaml").unwrap();
        let registry = load_registry(dir.path()).expect("non-yaml files must be skipped");
        assert!(registry.routes.is_empty());
    }
}
