//! Load and validate every `artifacts/**/*.yaml` fixture at startup (build
//! plan 4a). Only `lifecycle_state: active` artifacts join routing — that
//! constraint already lives in `resolve_route`/`compose_authority`
//! (Step 2/3), so this loader just parses everything into typed registries
//! without pre-filtering; loading itself is what gets audited here.

use std::collections::HashMap;
use std::path::Path;

use openspine_schemas::agent::AgentManifest;
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
    F: FnMut(T),
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
        on_each(value);
    }
    Ok(())
}

/// Parse and validate every artifact under `lyra_dir` (e.g.
/// `artifacts/lyra`). Each subdirectory maps to one artifact kind;
/// `deny_unknown_fields` on every schema type *is* the validation (D-028)
/// — a malformed fixture fails to parse rather than silently loading with
/// dropped fields.
pub fn load_registry(lyra_dir: &Path) -> Result<ArtifactRegistry, ArtifactLoadError> {
    let mut registry = ArtifactRegistry::default();

    load_yaml_dir(&lyra_dir.join("routes"), |r: Route| registry.routes.push(r))?;
    load_yaml_dir(&lyra_dir.join("agents"), |a: AgentManifest| {
        registry.agents.insert(a.id.clone(), a);
    })?;
    load_yaml_dir(&lyra_dir.join("workflows"), |w: WorkflowManifest| {
        registry.workflows.insert(w.id.clone(), w);
    })?;
    load_yaml_dir(&lyra_dir.join("packs"), |p: CapabilityPack| {
        registry.packs.insert(p.id.clone(), p);
    })?;
    load_yaml_dir(&lyra_dir.join("policies"), |p: Policy| {
        registry.policies.insert(p.id.clone(), p);
    })?;
    load_yaml_dir(&lyra_dir.join("templates"), |t: PromptTemplate| {
        registry.templates.insert(t.id.clone(), t);
    })?;

    Ok(registry)
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
