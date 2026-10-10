//! The exact file inputs consumed by runtime import, shared by every runtime loader.
use super::*;
use parhelion_import::GraphReference;
use sha2::{Digest, Sha256};
use std::{
    cell::{Cell, RefCell},
    path::Component,
};

pub(in crate::item) type Fingerprints = BTreeMap<PathBuf, [u8; 32]>;

pub(in crate::item) struct Inputs {
    root: PathBuf,
    value: serde_json::Value,
    reads: RefCell<Fingerprints>,
    cacheable: Cell<bool>,
}

/// A recorded input must remain a normal file inside the graph, including every parent.
pub(in crate::item) fn input_path(root: &Path, name: &Path) -> Option<PathBuf> {
    let mut path = root.to_path_buf();
    if name.as_os_str().is_empty() {
        return None;
    }
    for component in name.components() {
        let Component::Normal(part) = component else {
            return None;
        };
        path.push(part);
        if fs::symlink_metadata(&path).ok()?.file_type().is_symlink() {
            return None;
        }
    }
    path.is_file().then_some(path)
}

impl Inputs {
    pub(in crate::item) fn open(graph: &GraphReference) -> AuthoringResult<Self> {
        Self::open_directory(&graph.directory)
    }

    pub(in crate::item) fn open_directory(directory: &Path) -> AuthoringResult<Self> {
        let root = fs::canonicalize(directory)
            .map_err(|error| invalid(format!("Imported runtime graph: {error}")))?;
        let mut inputs = Self {
            root,
            value: serde_json::Value::Null,
            reads: RefCell::new(BTreeMap::new()),
            cacheable: Cell::new(true),
        };
        inputs.value = serde_json::from_slice(&inputs.read("asset-graph.json")?)
            .map_err(|error| invalid(format!("Imported runtime graph: {error}")))?;
        Ok(inputs)
    }

    pub(in crate::item) fn value(&self) -> &serde_json::Value {
        &self.value
    }

    pub(in crate::item) fn read(&self, name: impl AsRef<Path>) -> AuthoringResult<Vec<u8>> {
        let name = name.as_ref();
        if name.as_os_str().is_empty()
            || !name
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
        {
            return Err(invalid(format!(
                "Imported runtime input {} leaves its graph",
                name.display()
            )));
        }
        // Preserve compilation of prepared graphs using links. Only persistent reuse is
        // disabled, as it was for the full-directory fingerprint.
        if input_path(&self.root, name).is_none() {
            self.cacheable.set(false);
        }
        let path = self.root.join(name);
        let bytes = fs::read(&path).map_err(|error| {
            invalid(format!(
                "Imported runtime input {}: {error}",
                name.display()
            ))
        })?;
        let digest = Sha256::digest(&bytes).into();
        if self
            .reads
            .borrow_mut()
            .insert(name.to_path_buf(), digest)
            .is_some_and(|previous| previous != digest)
        {
            return Err(invalid(format!(
                "Imported runtime input {} changed while loading",
                name.display()
            )));
        }
        Ok(bytes)
    }

    pub(in crate::item) fn fingerprints(self) -> Option<Fingerprints> {
        self.cacheable.get().then(|| self.reads.into_inner())
    }
}
