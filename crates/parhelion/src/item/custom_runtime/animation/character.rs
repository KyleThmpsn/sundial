//! Bind prepared human clips when rebuilding a saved weapon from its imported graph.
use super::*;
use parhelion_import::tiger::{payload::Payload, rig::attachment_rows};
use sundial::package_authoring::entity::weapon_component_bindings;

#[derive(Clone, Copy)]
pub(in crate::item) struct Binding {
    pub(super) parent: u32,
    pub(super) private: u32,
}

pub(in crate::item) fn load(graph: &Inputs) -> AuthoringResult<Option<Binding>> {
    let character = &graph.value()["animation"]["character"];
    if character.is_null() || character["binding"]["builtin"] != 3 {
        return Ok(None);
    }
    if character["schema"] != 2 {
        return Err(invalid("Reimport this weapon's character animations"));
    }
    let binding = &character["binding"];
    let private = tag(binding, "private")?;
    if private != tag(character, "private")? {
        return Err(invalid(
            "Imported character profile disagrees with its clips",
        ));
    }
    Ok(Some(Binding {
        parent: tag(binding, "parent")?,
        private,
    }))
}

impl Binding {
    pub(in crate::item) fn apply(
        &self,
        manager: &PackageManager,
        entity: &[u8],
        content: u32,
        patches: &mut Vec<WeaponRuntimeResourcePatch>,
    ) -> AuthoringResult<()> {
        let prepare = || -> anyhow::Result<Vec<WeaponRuntimeResourcePatch>> {
            use anyhow::{Context, ensure};
            let bindings =
                weapon_component_bindings(entity, 0xD3A5500E).map_err(anyhow::Error::msg)?;
            let [binding] = bindings.as_slice() else {
                anyhow::bail!("Imported character needs one weapon attachment");
            };
            let owner = Payload(read_tag(
                manager,
                TagHash(binding.owner_tag),
                "character attachment",
            )?);
            let definition = owner.pointer(24)?;
            ensure!(
                owner.u32(definition - 4)? == 0x80804221,
                "Unsupported character attachment"
            );
            let rows = attachment_rows(&owner, definition, content, false)?;
            ensure!(!rows.is_empty(), "No selected character attachment");
            rows.into_iter()
                .map(|row| {
                    ensure!(
                        owner.u32(row + 0x6C)? == self.parent,
                        "Imported character profile does not fit this weapon's animation donor"
                    );
                    let offset = (row + 0x6C)
                        .checked_sub(usize::try_from(binding.resource_offset)?)
                        .context("Character attachment offset underflow")?;
                    Ok(WeaponRuntimeResourcePatch {
                        binding_hash: binding.binding_hash,
                        resource_index: u16::try_from(binding.resource_index)?,
                        offset: u32::try_from(offset)?,
                        bytes: self.private.to_le_bytes().to_vec(),
                        graph_values: Vec::new(),
                        graph_removals: Vec::new(),
                        graph_trajectories: None,
                    })
                })
                .collect()
        };
        for patch in
            prepare().map_err(|error| invalid(format!("Imported character binding: {error:#}")))?
        {
            let mut duplicate = false;
            for existing in patches.iter().filter(|existing| {
                existing.binding_hash == patch.binding_hash
                    && existing.resource_index == patch.resource_index
            }) {
                let a = u64::from(existing.offset)
                    ..u64::from(existing.offset) + existing.bytes.len() as u64;
                let b = u64::from(patch.offset)..u64::from(patch.offset) + patch.bytes.len() as u64;
                if a.start < b.end && b.start < a.end {
                    if existing != &patch {
                        return Err(invalid(
                            "Imported character profile conflicts with an authored attachment patch",
                        ));
                    }
                    duplicate = true;
                }
            }
            if !duplicate {
                patches.push(patch);
            }
        }
        Ok(())
    }
}
