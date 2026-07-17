use super::{
    load_yaml_dir, AgentManifest, ArtifactLoadError, ArtifactRegistry, ArtifactSource,
    CapabilityPack, Identified, Policy, PromptTemplate, Route, Versioned, WorkflowManifest,
};
use std::collections::HashSet;
use std::path::Path;

pub fn load_registry_into(
    registry: &mut ArtifactRegistry,
    dir: &Path,
) -> Result<(), ArtifactLoadError> {
    let mut seen: HashSet<(String, String, u32)> = HashSet::new();
    load_yaml_dir(
        &dir.join("routes"),
        |path: &Path, bytes: Vec<u8>, r: Route| {
            if !seen.insert(("route".into(), r.id.clone(), r.version)) {
                return Err(ArtifactLoadError::Collision {
                    kind: "route".into(),
                    id: r.id.clone(),
                    version: r.version,
                });
            }
            if registry
                .routes
                .iter()
                .any(|e| e.id == r.id && e.version == r.version)
            {
                return Err(ArtifactLoadError::Collision {
                    kind: "route".into(),
                    id: r.id,
                    version: r.version,
                });
            }
            registry.sources.insert(
                ("route".into(), r.id.clone(), r.version),
                ArtifactSource {
                    path: path.to_path_buf(),
                    bytes,
                },
            );
            let current = registry
                .routes
                .iter()
                .filter(|e| e.id == r.id)
                .map(|e| e.version)
                .max();
            if current.is_none_or(|v| r.version > v) {
                registry.routes.retain(|e| e.id != r.id);
                registry.routes.push(r);
            }
            Ok(())
        },
    )?;
    load_yaml_dir(
        &dir.join("agents"),
        |path: &Path, bytes: Vec<u8>, a: AgentManifest| {
            load_keyed(
                &mut seen,
                &mut registry.agents,
                &mut registry.sources,
                "agent",
                path,
                bytes,
                a,
            )
        },
    )?;
    load_yaml_dir(
        &dir.join("workflows"),
        |path: &Path, bytes: Vec<u8>, w: WorkflowManifest| {
            load_keyed(
                &mut seen,
                &mut registry.workflows,
                &mut registry.sources,
                "workflow",
                path,
                bytes,
                w,
            )
        },
    )?;
    load_yaml_dir(
        &dir.join("packs"),
        |path: &Path, bytes: Vec<u8>, p: CapabilityPack| {
            load_keyed(
                &mut seen,
                &mut registry.packs,
                &mut registry.sources,
                "pack",
                path,
                bytes,
                p,
            )
        },
    )?;
    load_yaml_dir(
        &dir.join("policies"),
        |path: &Path, bytes: Vec<u8>, p: Policy| {
            load_keyed(
                &mut seen,
                &mut registry.policies,
                &mut registry.sources,
                "policy",
                path,
                bytes,
                p,
            )
        },
    )?;
    load_yaml_dir(
        &dir.join("templates"),
        |path: &Path, bytes: Vec<u8>, t: PromptTemplate| {
            load_keyed(
                &mut seen,
                &mut registry.templates,
                &mut registry.sources,
                "template",
                path,
                bytes,
                t,
            )
        },
    )?;
    Ok(())
}

/// Insert a keyed artifact, keeping the highest version and recording its
/// source. An exact `(id, version)` duplicate is a hard collision error; a
/// lower version is ignored (the higher on-disk version wins, matching the
/// route loader's highest-only cutover — AD-070). `map` and `sources` are
/// passed as disjoint borrows so the call sites supply the concrete map.
impl Identified for AgentManifest {
    fn id(&self) -> &str {
        &self.id
    }
}
impl Identified for WorkflowManifest {
    fn id(&self) -> &str {
        &self.id
    }
}
impl Identified for CapabilityPack {
    fn id(&self) -> &str {
        &self.id
    }
}
impl Identified for Policy {
    fn id(&self) -> &str {
        &self.id
    }
}
impl Identified for PromptTemplate {
    fn id(&self) -> &str {
        &self.id
    }
}
fn load_keyed<T: Versioned + Identified>(
    seen: &mut HashSet<(String, String, u32)>,
    map: &mut std::collections::HashMap<String, T>,
    sources: &mut std::collections::HashMap<(String, String, u32), ArtifactSource>,
    kind: &str,
    path: &Path,
    bytes: Vec<u8>,
    artifact: T,
) -> Result<(), ArtifactLoadError> {
    let id = artifact.id().to_owned();
    let version = artifact.version();
    if !seen.insert((kind.into(), id.clone(), version)) {
        return Err(ArtifactLoadError::Collision {
            kind: kind.into(),
            id,
            version,
        });
    }
    if map.get(&id).is_some_and(|e| e.version() == version) {
        return Err(ArtifactLoadError::Collision {
            kind: kind.into(),
            id,
            version,
        });
    }
    sources.insert(
        (kind.into(), id.clone(), version),
        ArtifactSource {
            path: path.to_path_buf(),
            bytes,
        },
    );
    if map.get(&id).is_none_or(|e| version > e.version()) {
        map.insert(id.clone(), artifact);
    }
    Ok(())
}
