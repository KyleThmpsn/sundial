//! Prepared source selection is applied only to the native carrier it qualified.
use super::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};

mod material;

#[derive(Deserialize)]
pub(crate) struct Prepared {
    source_key: u32,
    native_key: u32,
    native_graph: u32,
    native_content_owner: u32,
    native_content_resource: u64,
    #[serde(default)]
    native_content_default: bool,
    #[serde(default)]
    native_barrel: Option<Barrel>,
    fingerprints: Vec<Fingerprint>,
    patches: Vec<NativeAssetResourcePatch>,
    #[serde(default)]
    material_patches: Vec<NativeAssetResourcePatch>,
    #[serde(default)]
    root: Option<String>,
    #[serde(default)]
    private_bank: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Barrel {
    owner: u32,
    resource: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fingerprint {
    tag: u32,
    sha256: String,
}

pub(crate) fn load(graph: &serde_json::Value) -> AuthoringResult<Option<Prepared>> {
    let report = &graph["firing_translation"]["projectile"];
    if report["status"] != "retained_native" {
        return Ok(None);
    }
    if graph["source"] != "d1_roi" || graph["kind"] != "weapon" {
        return Err(invalid("Prepared projectile has an unsupported source"));
    }
    let prepared: Prepared = serde_json::from_value(report.clone())
        .map_err(|error| invalid(format!("Prepared projectile: {error}")))?;
    if prepared.private_bank != prepared.root.is_some()
        || prepared.root.as_ref().is_some_and(|root| {
            root.trim().is_empty()
                || graph["projectile"]["root"].as_str() != Some(root.as_str())
                || graph["projectile"]["installable"] != true
                || graph["projectile"]["conversion"]["launch_policy"]
                    != "retained_native_trajectory"
        })
    {
        return Err(invalid(
            "Prepared projectile bank has no matching private asset group",
        ));
    }
    Ok(Some(prepared))
}

/// An authored graph, behavior, or Projectile edit replaces the imported default.
pub(crate) fn overridden(overrides: &crate::item::WeaponCloneOverrides) -> bool {
    overrides.fired_graph.is_some()
        || overrides.projectile.is_some()
        || !overrides.additional_behaviors.is_empty()
        || overrides.behavior_donor.is_some()
        || overrides
            .component_splices
            .iter()
            .any(|(binding, _)| matches!(*binding, BINDING | WEAPON_BARREL_COMPONENT_KEY))
}

impl Prepared {
    fn selection(
        &self,
        manager: &PackageManager,
        entity: &[u8],
        content: &Content,
    ) -> AuthoringResult<bool> {
        let definition = usize::try_from(u64_at(&content.owner, content.resource + 8)?)
            .map_err(|_| invalid("Projectile content definition offset is too large"))?;
        let (count, _, rows, class) =
            crate::tag_payload::array_at(&content.owner, definition + 0x240)?;
        if count > 0 && class != 0x80803ACF {
            return Err(invalid(
                "Projectile content variants have an unexpected class",
            ));
        }
        let mut selected = Vec::new();
        for index in 0..count {
            let row = index
                .checked_mul(0x1C0)
                .and_then(|off| rows.checked_add(off))
                .ok_or_else(|| invalid("Projectile content variant offset is too large"))?;
            if u32_at(&content.owner, row + 16)? == self.source_key {
                selected.push(row);
            }
        }
        let row = match selected.as_slice() {
            [] if self.native_content_default => definition + 0x80,
            [row] if !self.native_content_default => *row,
            _ => return Ok(false),
        };
        let selected_graph = u32_at(&content.owner, row + GRAPH_OFFSET)?;
        let Some(barrel) = &self.native_barrel else {
            return Ok(selected_graph == self.native_graph);
        };
        if !matches!(selected_graph, 0 | u32::MAX) {
            return Ok(false);
        }
        let bindings =
            weapon_component_bindings(entity, WEAPON_BARREL_COMPONENT_KEY).map_err(invalid)?;
        let [binding] = bindings.as_slice() else {
            return Ok(false);
        };
        if binding.owner_tag != barrel.owner || binding.resource_offset != barrel.resource {
            return Ok(false);
        }
        let owner = manager
            .read_tag(TagHash(barrel.owner))
            .map_err(|e| invalid(e.to_string()))?;
        let slots = sundial::package_authoring::entity::barrel::projectile_slots(&owner, *binding)
            .map_err(invalid)?;
        Ok(slots.len() == 2
            && slots
                .iter()
                .all(|(_, graph)| *graph == Some(self.native_graph)))
    }

    pub(crate) fn fired(
        &self,
        manager: &PackageManager,
        entity: &[u8],
        group: u32,
    ) -> AuthoringResult<FiredGraph> {
        let content = content(manager, entity)?;
        if group != self.native_key
            || content.owner_tag != self.native_content_owner
            || content.resource as u64 != self.native_content_resource
            || !self.selection(manager, entity, &content)?
        {
            return Err(invalid(
                "The weapon's firing setup differs from its prepared projectile. Reimport it with this base weapon.",
            ));
        }
        let mut checked = BTreeSet::new();
        for fingerprint in &self.fingerprints {
            if !checked.insert(fingerprint.tag) {
                return Err(invalid("Prepared projectile repeats a native fingerprint"));
            }
            let bytes = manager
                .read_tag(TagHash(fingerprint.tag))
                .map_err(|error| invalid(error.to_string()))?;
            if hex::encode(Sha256::digest(&bytes)) != fingerprint.sha256 {
                return Err(invalid(
                    "Native projectile assets changed after preparation. Reimport the weapon.",
                ));
            }
        }
        let graph = manager
            .read_tag(TagHash(self.native_graph))
            .map_err(|error| invalid(error.to_string()))?;
        let bindings = weapon_component_bindings(&graph, 0x0437756D).map_err(invalid)?;
        let [movement] = bindings.as_slice() else {
            return Err(invalid("Prepared projectile requires one movement owner"));
        };
        if ![
            self.native_graph,
            self.native_content_owner,
            movement.owner_tag,
        ]
        .iter()
        .all(|tag| checked.contains(tag))
            || self
                .native_barrel
                .as_ref()
                .is_some_and(|barrel| !checked.contains(&barrel.owner))
        {
            return Err(invalid(
                "Prepared projectile is missing its carrier fingerprints",
            ));
        }
        let mut offsets = BTreeSet::new();
        for patch in &self.patches {
            if patch.binding_hash != 0x0437756D
                || patch.resource_index != 0
                || patch.expected.len() != 8
                || patch.bytes.len() != 8
                || patch.imported_particle.is_some()
                || !offsets.insert(patch.offset)
                || !f32::from_le_bytes(patch.bytes[4..8].try_into().unwrap()).is_finite()
            {
                return Err(invalid("Prepared projectile has invalid response operands"));
            }
        }
        let mut patches = self.patches.clone();
        if !self.material_patches.is_empty() {
            let owner = manager
                .read_tag(TagHash(movement.owner_tag))
                .map_err(|e| invalid(e.to_string()))?;
            let resource = usize::try_from(movement.resource_offset)
                .map_err(|_| invalid("Projectile movement offset is too large"))?;
            material::validate(manager, &owner, resource, &self.material_patches, &checked)?;
            patches.extend_from_slice(&self.material_patches);
        }
        let fired = FiredGraph {
            bundled: None,
            source_graph: if self.root.is_some() {
                0
            } else {
                self.native_graph
            },
            imported: self.root.clone(),
            patches: if self.root.is_some() {
                Vec::new()
            } else {
                patches
            },
            appends: Vec::new(),
            particles: Vec::new(),
        };
        fired.validate()?;
        Ok(fired)
    }
}
