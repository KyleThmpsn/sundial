//! Prepared firing curves belong to the imported graph and its validated native carrier.
use super::*;
use sha2::{Digest, Sha256};
mod curves;
mod overrides;

#[derive(Deserialize)]
pub(crate) struct Timing {
    native_key: u32,
    native_resource: u64,
    native_sha256: String,
    edits: Vec<Edit>,
    #[serde(default)]
    appended_curves: Vec<curves::Curve>,
    #[serde(default)]
    native_translations: Option<u32>,
    #[serde(default)]
    stat_overrides: Vec<overrides::Change>,
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
    // Rise of Iron graphs name their kind. Modern weapon graphs carry none.
    let weapon = match graph["source"].as_str() {
        Some("d1_roi") => graph["kind"] == "weapon",
        Some("d2_mot") => graph["kind"].is_null() || graph["kind"] == "weapon",
        Some("marathon") => graph["kind"].is_null() || graph["kind"] == "weapon",
        _ => false,
    };
    if !weapon {
        return Err(invalid("Imported firing curves have an unsupported source"));
    }
    let value: Timing = serde_json::from_value(timing.clone())
        .map_err(|error| invalid(format!("Imported firing curves: {error}")))?;
    Ok(Some(value))
}

impl Timing {
    /// Additional columns have private storage. Existing native columns, named outputs and
    /// unselected tables retain their samples and domains.
    pub(crate) fn appends(
        &self,
        manager: &PackageManager,
        entity: &[u8],
        group: u32,
    ) -> AuthoringResult<Vec<crate::item::RuntimeResourceAppend>> {
        if self.appended_curves.is_empty() && self.stat_overrides.is_empty() {
            return Ok(Vec::new());
        }
        let (owner, resource) = self.carrier(manager, entity, group)?;
        let (_, mut appends) = overrides::plans(&owner, resource, &self.stat_overrides)?;
        if !self.appended_curves.is_empty() {
            appends.push(curves::append(
                &owner,
                resource,
                group,
                self.native_translations
                    .ok_or_else(|| invalid("Imported curves have no table descriptor"))?,
                &self.appended_curves,
            )?);
        }
        Ok(appends)
    }

    fn carrier(
        &self,
        manager: &PackageManager,
        entity: &[u8],
        group: u32,
    ) -> AuthoringResult<(Vec<u8>, usize)> {
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
        Ok((owner, resource))
    }

    /// Reimports replace the graph, while native carrier changes require a new conversion.
    /// Reads participate in the normal runtime cache's native dependency fingerprints.
    pub(crate) fn patches(
        &self,
        manager: &PackageManager,
        entity: &[u8],
        group: u32,
    ) -> AuthoringResult<Vec<RuntimeResourcePatch>> {
        let (owner, resource) = self.carrier(manager, entity, group)?;
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
                // A sample or literal is a finite float. A rewritten row's component read also
                // carries the unused-selector word.
                || !(f32::from_le_bytes(edit.after).is_finite() || edit.after == [0xFF; 4])
            {
                return Err(invalid(
                    "Imported firing curve has an invalid native sample",
                ));
            }
            patches.push(RuntimeResourcePatch {
                binding_hash: WEAPON_STAT_TRANSLATOR_COMPONENT_KEY,
                resource_index: 0,
                offset: edit.offset,
                bytes: edit.after.to_vec(),
                graph_values: Vec::new(),
                graph_removals: Vec::new(),
                graph_trajectories: None,
            });
        }
        patches.extend(overrides::plans(&owner, resource, &self.stat_overrides)?.0);
        Ok(patches)
    }
}
