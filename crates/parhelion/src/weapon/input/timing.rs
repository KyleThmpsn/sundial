//! Prepared firing curves belong to the imported graph and its validated native carrier.
use super::*;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
pub(crate) struct Timing {
    native_key: u32,
    native_resource: u64,
    native_sha256: String,
    edits: Vec<Edit>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Edit {
    offset: u32,
    before: [u8; 4],
    after: [u8; 4],
}

pub(crate) fn imported(graph: &serde_json::Value) -> AuthoringResult<Option<Timing>> {
    let timing = &graph["firing_translation"]["timing"];
    if timing["status"] != "translated" {
        return Ok(None);
    }
    if graph["source"] != "d1_roi" || graph["kind"] != "weapon" {
        return Err(invalid("Imported firing curves have an unsupported source"));
    }
    let value: Timing = serde_json::from_value(timing.clone())
        .map_err(|error| invalid(format!("Imported firing curves: {error}")))?;
    Ok(Some(value))
}

impl Timing {
    /// Reimports replace the graph, while native carrier changes require a new conversion.
    /// Reads participate in the normal runtime cache's native dependency fingerprints.
    pub(crate) fn patches(
        &self,
        manager: &PackageManager,
        entity: &[u8],
        group: u32,
    ) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
        let bindings = weapon_component_bindings(entity, WEAPON_STAT_TRANSLATOR_COMPONENT_KEY)
            .map_err(invalid)?;
        let [binding] = bindings.as_slice() else {
            return Err(invalid(
                "Imported firing curves require one native stat translator",
            ));
        };
        let owner = manager
            .read_tag(TagHash(binding.owner_tag))
            .map_err(|error| invalid(error.to_string()))?;
        if group != self.native_key
            || binding.resource_offset != self.native_resource
            || hex::encode(Sha256::digest(&owner)) != self.native_sha256
        {
            return Err(invalid(
                "The weapon's firing setup differs from its converted source timing. Reimport it with this base weapon.",
            ));
        }
        let resource = usize::try_from(binding.resource_offset)
            .map_err(|_| invalid("Imported firing resource offset overflow"))?;
        let mut offsets = std::collections::BTreeSet::new();
        let mut patches = Vec::new();
        for edit in &self.edits {
            let at = resource
                .checked_add(edit.offset as usize)
                .ok_or_else(|| invalid("Imported firing offset overflow"))?;
            let end = at
                .checked_add(4)
                .ok_or_else(|| invalid("Imported firing extent overflow"))?;
            if edit.offset % 4 != 0
                || !offsets.insert(edit.offset)
                || owner.get(at..end) != Some(edit.before.as_slice())
                || !f32::from_le_bytes(edit.after).is_finite()
            {
                return Err(invalid(
                    "Imported firing curve has an invalid native sample",
                ));
            }
            patches.push(WeaponRuntimeResourcePatch {
                binding_hash: WEAPON_STAT_TRANSLATOR_COMPONENT_KEY,
                resource_index: 0,
                offset: edit.offset,
                bytes: edit.after.to_vec(),
                graph_values: Vec::new(),
                graph_removals: Vec::new(),
                graph_trajectories: None,
            });
        }
        Ok(patches)
    }
}
