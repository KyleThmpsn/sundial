//! Full action programs retain every declared allocation, including non-scalar values.
use super::{Asset, Program};
use crate::sandbox_perk::action::{
    self,
    native::{Graph, schema},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeProgram {
    pub graph: Graph,
    /// Component edits belong to their referenced entity, independently of node ordering.
    pub assets: Vec<Asset>,
}

/// A source-independent readiness failure, located in execution order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeIssue {
    pub group: usize,
    pub action: usize,
    pub field: &'static str,
    pub message: String,
}

/// Shared with the compiler. Weighted spawning may omit its optional attachment.
pub(super) fn entity_reference(class: u32, bytes: &[u8]) -> Result<Option<u32>, String> {
    if !matches!(
        class,
        0x80803E45 | 0x80803E44 | 0x80803E43 | 0x80803E47 | 0x80803E12
    ) {
        return Ok(None);
    }
    let tag = crate::package_payload::u32_at(bytes, 16)?;
    if matches!(tag, 0 | u32::MAX) {
        if class == 0x80803E47 {
            return Ok(None);
        }
        return Err(if class == 0x80803E12 {
            "Choose a projectile."
        } else {
            "Choose an object or effect."
        }
        .into());
    }
    Ok(Some(tag))
}

impl NativeProgram {
    pub fn authoring_issue(&self) -> Result<Option<NativeIssue>, String> {
        let decoded = action::decode(&self.graph.emit()?)?;
        for (group, behavior) in decoded.groups.iter().enumerate() {
            for (action, effect) in behavior.effects.iter().rev().enumerate() {
                if let Err(message) = entity_reference(effect.class, &effect.native) {
                    return Ok(Some(NativeIssue {
                        group,
                        action,
                        field: if effect.class == 0x80803E12 {
                            "Projectile"
                        } else {
                            "Object"
                        },
                        message,
                    }));
                }
            }
        }
        Ok(None)
    }

    pub fn empty() -> Self {
        let mut bytes = vec![0; 0xD0];
        bytes[..8].copy_from_slice(&0xD0u64.to_le_bytes());
        for at in [8, 0x80, 0xBC, 0xC0, 0xC4, 0xC8] {
            bytes[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        }
        Self::read(&bytes).expect("empty native program")
    }
    pub fn read(payload: &[u8]) -> Result<Self, String> {
        let mut native = Self {
            graph: Graph::read(payload, 0, action::ACTION_ROOT_CLASS)?,
            assets: Vec::new(),
        };
        native.sync_assets()?;
        native.validate()?;
        Ok(native)
    }

    /// Entity-bearing effect records expose the same component editor as simple actions.
    /// Existing entries for other resource slots are retained while still referenced.
    pub fn sync_assets(&mut self) -> Result<(), String> {
        let resources = self.resources()?;
        self.assets.retain(|asset| resources.contains(&asset.graph));
        let graph = Graph::read(&self.graph.emit()?, 0, action::ACTION_ROOT_CLASS)?;
        for block in &graph.blocks {
            let offset = if block.class == 0x80803E42 {
                Some(24)
            } else if matches!(
                block.class,
                0x80803E45 | 0x80803E44 | 0x80803E43 | 0x80803E47 | 0x80803E12
            ) {
                Some(16)
            } else {
                None
            };
            if let Some(offset) = offset {
                let graph = crate::package_payload::u32_at(&block.bytes, offset)?;
                if !matches!(graph, 0 | u32::MAX)
                    && !self.assets.iter().any(|asset| asset.graph == graph)
                {
                    self.assets.push(Asset {
                        graph,
                        ..Asset::default()
                    });
                }
            }
        }
        Ok(())
    }

    pub fn resources(&self) -> Result<BTreeSet<u32>, String> {
        let mut tags = BTreeSet::new();
        // Emit/read discards unreachable allocations before collecting resource fields.
        let graph = Graph::read(&self.graph.emit()?, 0, action::ACTION_ROOT_CLASS)?;
        for block in graph.blocks.iter().filter(|block| block.class != 0) {
            let record = schema::record(block.class)?;
            for row in 0..block.count.unwrap_or(1) {
                for &(at, code) in &record.fields {
                    if matches!(code, 4 | 9) {
                        tags.insert(crate::package_payload::u32_at(
                            &block.bytes,
                            row * record.size + at,
                        )?);
                    }
                }
            }
        }
        Ok(tags)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.graph.validate_program()?;
        let resources = self.resources()?;
        let mut used = BTreeSet::new();
        for asset in &self.assets {
            if matches!(asset.graph, 0 | u32::MAX)
                || !resources.contains(&asset.graph)
                || !used.insert(asset.graph)
            {
                return Err("Program component edits need a unique, referenced entity.".into());
            }
            if asset.path.contains('\0') || asset.path.len() > 1024 {
                return Err("The selected asset has an invalid native path.".into());
            }
        }
        Ok(())
    }
}

impl Program {
    pub fn from_native(payload: &[u8], name: impl Into<String>) -> Result<Self, String> {
        Ok(Self {
            name: name.into(),
            native: Some(NativeProgram::read(payload)?),
            ..Self::default()
        })
    }

    pub fn assets(&self) -> impl Iterator<Item = &Asset> {
        self.actions
            .iter()
            .filter_map(|action| action.asset())
            .chain(self.native.iter().flat_map(|native| native.assets.iter()))
    }

    pub fn assets_mut(&mut self) -> impl Iterator<Item = &mut Asset> {
        self.actions
            .iter_mut()
            .filter_map(|action| action.asset_mut())
            .chain(
                self.native
                    .iter_mut()
                    .flat_map(|native| native.assets.iter_mut()),
            )
    }

    pub fn asset(&self, index: usize) -> Option<&Asset> {
        match &self.native {
            Some(native) => native.assets.get(index),
            None => self.actions.get(index).and_then(|action| action.asset()),
        }
    }

    pub fn asset_mut(&mut self, index: usize) -> Option<&mut Asset> {
        match &mut self.native {
            Some(native) => native.assets.get_mut(index),
            None => self
                .actions
                .get_mut(index)
                .and_then(|action| action.asset_mut()),
        }
    }
}

impl super::Action {
    /// The native masterwork orb operation, positioned using its activation event.
    pub fn generate_orb(position: super::Position) -> Self {
        let mut bytes = vec![0; 32];
        bytes[0] = 5;
        bytes[2] = u8::from(position == super::Position::Event);
        bytes[4..8].copy_from_slice(&1u32.to_le_bytes());
        bytes[8..12].copy_from_slice(&1f32.to_le_bytes());
        bytes[24..28].copy_from_slice(&0x80EF_AE02u32.to_le_bytes());
        Self::Native {
            node: super::NativeNode { kind: 5, bytes },
        }
    }
}
