//! Immutable, non-autoloaded package objects. Publication precedes indexing.
use super::install_types::{crash_at, InstallError as Error, PackageIdentity};
use super::{object_fs as fs, PackageSnapshot};
use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use ulid::Ulid;

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

    pub(crate) fn verify(&self, identity: &PackageIdentity) -> Result<(), Error> {
        self.check_anchors()?;
        let name = object_name(identity)?;
        let object = fs::open_directory(&self.objects, name).map_err(|_| Error::ObjectCorrupt)?;
        let files = super::source::capture_opened(&object).map_err(|_| Error::ObjectCorrupt)?;
        let captured = super::from_files(files).map_err(|_| Error::ObjectCorrupt)?;
        if captured.identity() != *identity {
            return Err(Error::ObjectCorrupt);
        }
        fs::same(
            &object,
            &fs::open_directory(&self.objects, name).map_err(|_| Error::ObjectCorrupt)?,
        )?;
        self.check_anchors()
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
