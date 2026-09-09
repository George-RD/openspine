//! Immutable, non-autoloaded package objects. Publication precedes indexing.
use super::install_types::{crash_at, InstallError as Error, PackageIdentity};
use super::{object_fs as fs, PackageCapture, PackageSnapshot};
use openspine_schemas::digest::digest_of_bytes;
use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use ulid::Ulid;

/// Inventory-v1 identity only, deliberately independent of today's declaration
/// schema. Direct struct parsing rejects duplicate/missing identity fields;
/// values preserve scalar kinds instead of coercing YAML null/bool into text.
/// Other fields remain covered by the recorded manifest and inventory digests.
#[derive(serde::Deserialize)]
struct RetainedManifestIdentity {
    id: serde_yaml::Value,
    version: serde_yaml::Value,
}

pub(crate) struct PackageObjects {
    data_path: PathBuf,
    data: File,
    namespace: File,
    objects: File,
    staging: File,
}

impl PackageObjects {
    pub(crate) fn open(data_path: &Path, active_base: &Path) -> Result<Self, Error> {
        let namespace_path = data_path.join("packages");
        for active in [active_base.to_path_buf(), data_path.join("artifacts.d")] {
            let active = fs::resolve(&active)?;
            if active.starts_with(&namespace_path) || namespace_path.starts_with(&active) {
                return Err(Error::Destination);
            }
        }
        let data = fs::root(data_path)?;
        let namespace = fs::ensure_directory(&data, "packages")?;
        let objects = fs::ensure_directory(&namespace, "objects")?;
        let staging = fs::ensure_directory(&namespace, "staging")?;
        let result = Self {
            data_path: data_path.to_owned(),
            data,
            namespace,
            objects,
            staging,
        };
        result.check_anchors()?;
        Ok(result)
    }

    pub(crate) fn check_anchors(&self) -> Result<(), Error> {
        fs::same(&self.data, &fs::root(&self.data_path)?)?;
        fs::same(
            &self.namespace,
            &fs::open_directory(&self.data, "packages")?,
        )?;
        fs::same(
            &self.objects,
            &fs::open_directory(&self.namespace, "objects")?,
        )?;
        fs::same(
            &self.staging,
            &fs::open_directory(&self.namespace, "staging")?,
        )
    }

    pub(crate) fn publish(&self, id: Ulid, snapshot: &PackageSnapshot) -> Result<(), Error> {
        self.check_anchors()?;
        let stage = fs::create_directory(&self.staging, &id.to_string())?;
        let mut directories = BTreeMap::new();
        for (path, bytes) in snapshot.files() {
            match path.split_once('/') {
                Some((family, file)) => {
                    if !directories.contains_key(family) {
                        directories.insert(family, fs::create_directory(&stage, family)?);
                    }
                    fs::write_new(&directories[family], file, bytes)?;
                }
                None => fs::write_new(&stage, path, bytes)?,
            }
            crash_at("during-staging");
        }
        for directory in directories.values() {
            fs::sync(directory)?;
        }
        fs::sync(&stage)?;
        fs::sync(&self.staging)?;
        crash_at("before-publish");
        self.check_anchors()?;
        fs::same(&stage, &fs::open_directory(&self.staging, &id.to_string())?)?;
        let identity = snapshot.identity();
        let _published = fs::publish(
            &self.staging,
            &id.to_string(),
            &self.objects,
            object_name(&identity)?,
        )?;
        // Even an existing orphan must match the complete captured inventory.
        // Re-sync before committing a retry after a prior directory sync error.
        self.verify(&identity)?;
        let object = fs::open_directory(&self.objects, object_name(&identity)?)?;
        fs::sync_snapshot(&object, snapshot)?;
        self.verify(&identity)?;
        fs::sync(&self.objects)?;
        fs::sync(&self.staging)?;
        crash_at("after-publish");
        self.check_anchors()
    }

    /// Verify recorded bytes and stable identity, not current runtime or
    /// declaration compatibility. Success does not construct a trusted snapshot.
    pub(crate) fn verify(&self, identity: &PackageIdentity) -> Result<(), Error> {
        self.inventory(identity).map(|_| ())
    }

    /// Return metadata from the exact capture whose integrity was verified.
    /// Consumers must not reopen the path or treat this as loader validation.
    pub(crate) fn inventory(
        &self,
        identity: &PackageIdentity,
    ) -> Result<Vec<super::InventoryFile>, Error> {
        self.capture(identity).map(|capture| capture.inventory)
    }

    /// Retain the exact bytes verified against the supplied recorded identity.
    /// The caller establishes receipt trust and keeps its provenance separately.
    /// Capture performs no loader staging or current-schema validation. A later
    /// `validate` consumes these bytes, not the object's possibly changed path.
    /// Selection must still recheck current state; this value grants no approval.
    pub(crate) fn capture(&self, identity: &PackageIdentity) -> Result<PackageCapture, Error> {
        self.check_anchors()?;
        let name = object_name(identity)?;
        let object = fs::open_directory(&self.objects, name).map_err(|_| Error::ObjectCorrupt)?;
        let files = super::source::capture_opened(&object).map_err(|_| Error::ObjectCorrupt)?;
        let capture = PackageCapture::new(files);
        let manifest = capture.files.get("package.yaml").ok_or(Error::ObjectCorrupt)?;
        if identity.inventory_format_version != super::INVENTORY_FORMAT_VERSION
            || capture.content_digest != identity.content_digest
            || digest_of_bytes(manifest) != identity.manifest_digest
        {
            return Err(Error::ObjectCorrupt);
        }
        let declaration: RetainedManifestIdentity =
            serde_yaml::from_slice(manifest).map_err(|_| Error::ObjectCorrupt)?;
        if declaration.id.as_str() != Some(identity.package_id.as_str())
            || declaration.version.as_u64() != Some(u64::from(identity.revision))
        {
            return Err(Error::ObjectCorrupt);
        }
        fs::same(
            &object,
            &fs::open_directory(&self.objects, name).map_err(|_| Error::ObjectCorrupt)?,
        )?;
        self.check_anchors()?;
        Ok(capture)
    }

    pub(crate) fn cleanup(&self, id: Ulid) -> Result<(), Error> {
        self.check_anchors()?;
        fs::remove_stage(&self.staging, &id.to_string())
    }
}

fn object_name(identity: &PackageIdentity) -> Result<&str, Error> {
    let name = identity
        .content_digest
        .as_str()
        .strip_prefix("sha256:")
        .ok_or(Error::ObjectCorrupt)?;
    if name.len() != 64 || !name.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::ObjectCorrupt);
    }
    Ok(name)
}
