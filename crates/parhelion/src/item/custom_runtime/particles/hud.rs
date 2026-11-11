//! Source display bindings on prepared private status owners.
use super::*;
use sundial::package_authoring::sandbox_perk::program::HudStatus;

#[derive(Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Binding {
    owner: String,
    name: u32,
    status: HudStatus,
}

impl Binding {
    pub(super) fn remap(&self, names: &BTreeMap<String, String>) -> Option<Self> {
        Some(Self {
            owner: names.get(&self.owner)?.clone(),
            name: self.name,
            status: self.status.clone(),
        })
    }
}

impl ImportedParticles {
    pub(in crate::item) fn bind_hud(
        &mut self,
        manager: &PackageManager,
    ) -> AuthoringResult<Vec<HudStatus>> {
        let mut seen = BTreeSet::new();
        let mut writes = Vec::new();
        for binding in &self.hud {
            if !seen.insert(&binding.owner)
                || binding.name == 0x811C9DC5
                || binding.status.icon.is_none()
                || binding.status.image.is_none()
                || binding.status.name.trim().is_empty()
            {
                return Err(invalid("Imported HUD binding is duplicated or incomplete"));
            }
            let index = self
                .nodes
                .iter()
                .position(|node| node.symbol == binding.owner)
                .ok_or_else(|| invalid("Imported HUD owner is absent"))?;
            let node = &self.nodes[index];
            if node.reference.is_some()
                || manager
                    .get_entry(TagHash(node.template))
                    .is_none_or(|entry| entry.reference != 0x80809C36)
            {
                return Err(invalid("Imported HUD owner is not a component"));
            }
            let payload = &node.payload;
            let live = crate::tag_payload::relative_target(payload, 16)?;
            let schema = crate::tag_payload::relative_target(payload, 24)?;
            let definition = usize::try_from(read_u64(payload, live + 8)?)
                .map_err(|_| invalid("Imported status definition exceeds the address space"))?;
            if live < 4
                || schema < 4
                || read_u32(payload, live - 4)? != 0x80804209
                || read_u32(payload, schema - 4)? != 0x80804205
            {
                return Err(invalid("Imported HUD binding does not name a status owner"));
            }
            let offsets = [definition, schema]
                .into_iter()
                .map(|at| {
                    at.checked_add(0x160)
                        .ok_or_else(|| invalid("Imported HUD offset overflow"))
                })
                .collect::<AuthoringResult<BTreeSet<_>>>()?;
            let name = crate::item::hud_status::identity(manager, &binding.status)?;
            for offset in offsets {
                if read_u32(payload, offset)? != binding.name
                    || node
                        .patches
                        .iter()
                        .any(|(at, _)| *at < offset + 4 && at.saturating_add(4) > offset)
                {
                    return Err(invalid(
                        "Imported status identity changed or overlaps a resource patch",
                    ));
                }
                writes.push((index, offset, name));
            }
        }
        for (index, offset, name) in writes {
            write_u32(&mut self.nodes[index].payload, offset, name)?;
        }
        Ok(std::mem::take(&mut self.hud)
            .into_iter()
            .map(|binding| binding.status)
            .collect())
    }
}
