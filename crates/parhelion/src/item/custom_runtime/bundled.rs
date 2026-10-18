//! Reconstruct bundled native graphs from pinned stock templates and checked byte edits.
use super::assets::{self, Node};
use super::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    native_build: String,
    root: String,
    nodes: Vec<Template>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Template {
    symbol: String,
    template: u32,
    native_reference: u32,
    source_sha256: String,
    payload_sha256: String,
    size: usize,
    edits: Vec<Edit>,
    reference: Option<String>,
    patches: Vec<Link>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Edit {
    offset: usize,
    expected: Vec<u8>,
    bytes: Vec<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Link {
    offset: usize,
    symbol: String,
}

pub(in crate::item) struct Projectile {
    root: String,
    nodes: Vec<Node>,
}

impl Projectile {
    pub(in crate::item) fn load(
        manager: &PackageManager,
        preset: crate::weapon::projectile::Preset,
    ) -> AuthoringResult<Self> {
        let manifest: Manifest = serde_json::from_str(crate::weapon::projectile::manifest(preset))
            .map_err(|error| invalid(format!("Bundled projectile manifest: {error}")))?;
        if manifest.schema != 1
            || manifest.native_build != "86657.20.08.23.1800.d2_rc___release"
            || manifest.nodes.is_empty()
        {
            return Err(invalid("Unsupported bundled projectile source build"));
        }
        let symbols = manifest
            .nodes
            .iter()
            .map(|node| node.symbol.as_str())
            .collect::<BTreeSet<_>>();
        if symbols.len() != manifest.nodes.len() || !symbols.contains(manifest.root.as_str()) {
            return Err(invalid("Bundled projectile has duplicate or missing nodes"));
        }
        let nodes = manifest
            .nodes
            .iter()
            .map(|template| template.reconstruct(manager, &symbols))
            .collect::<AuthoringResult<Vec<_>>>()?;
        let root = nodes
            .iter()
            .find(|node| node.symbol == manifest.root)
            .ok_or_else(|| invalid("Bundled projectile root is missing"))?;
        if sundial::package_authoring::sandbox_perk::entity::kind(&root.payload).map_err(invalid)?
            != Some(sundial::package_authoring::sandbox_perk::entity::Kind::Projectile)
        {
            return Err(invalid("Bundled projectile root is not a projectile"));
        }
        assets::replication::validate(&nodes, root)?;
        assets::replication::validate_templates(&nodes, root, manager)?;
        Ok(Self {
            root: manifest.root,
            nodes,
        })
    }

    pub(in crate::item) fn prepare(
        &mut self,
        manager: &PackageManager,
        host: &[u8],
        group: u32,
        boost: f32,
    ) -> AuthoringResult<()> {
        assets::projectile::prepare(&mut self.nodes, &self.root, manager, host, group, boost)
    }

    pub(in crate::item) fn author(
        &self,
        manager: &PackageManager,
        packages: &mut crate::asset_packages::AssetPackages,
    ) -> AuthoringResult<(TagHash, Vec<TagHash>)> {
        let entities = self
            .nodes
            .iter()
            .filter(|node| {
                manager
                    .get_entry(TagHash(node.template))
                    .is_some_and(|entry| entry.reference == 0x8080_9C0F)
            })
            .map(|node| node.symbol.clone())
            .collect();
        let (symbols, eager) = assets::author(
            &self.nodes,
            &[&self.root],
            &entities,
            manager,
            packages,
            |_| Ok(BTreeMap::new()),
            |_, _, _| Ok(()),
        )?;
        Ok((symbols[&self.root], eager))
    }
}

impl Template {
    fn reconstruct(
        &self,
        manager: &PackageManager,
        symbols: &BTreeSet<&str>,
    ) -> AuthoringResult<Node> {
        let tag = TagHash(self.template);
        let entry = manager
            .get_entry(tag)
            .ok_or_else(|| invalid(format!("Bundled projectile template {tag} is unavailable")))?;
        let mut payload = read_tag(manager, tag, "bundled projectile template")?;
        if entry.reference != self.native_reference
            || payload.len() != self.size
            || hex::encode(Sha256::digest(&payload)) != self.source_sha256
        {
            return Err(invalid(format!(
                "Bundled projectile template {tag} differs from the supported native build"
            )));
        }
        let mut end = 0;
        for edit in &self.edits {
            let next = edit
                .offset
                .checked_add(edit.expected.len())
                .ok_or_else(|| invalid("Bundled projectile edit overflow"))?;
            if edit.offset < end
                || edit.expected.len() != edit.bytes.len()
                || payload.get(edit.offset..next) != Some(edit.expected.as_slice())
            {
                return Err(invalid(format!(
                    "Bundled projectile template {tag} has an invalid edit"
                )));
            }
            payload[edit.offset..next].copy_from_slice(&edit.bytes);
            end = next;
        }
        if hex::encode(Sha256::digest(&payload)) != self.payload_sha256 {
            return Err(invalid(format!(
                "Bundled projectile template {tag} has an invalid result digest"
            )));
        }
        let mut offsets = BTreeSet::new();
        for link in &self.patches {
            if !symbols.contains(link.symbol.as_str())
                || !offsets.insert(link.offset)
                || read_u32(&payload, link.offset)? != u32::MAX
            {
                return Err(invalid("Bundled projectile has an invalid symbolic link"));
            }
        }
        if self
            .reference
            .as_ref()
            .is_some_and(|target| !symbols.contains(target.as_str()))
        {
            return Err(invalid("Bundled projectile has an invalid resource pair"));
        }
        Ok(Node {
            symbol: self.symbol.clone(),
            template: self.template,
            payload,
            reference: self.reference.clone(),
            storage: crate::NewTagStorageMode::InheritTemplate,
            patches: self
                .patches
                .iter()
                .map(|link| (link.offset, link.symbol.clone()))
                .collect(),
            metadata: (),
        })
    }
}
