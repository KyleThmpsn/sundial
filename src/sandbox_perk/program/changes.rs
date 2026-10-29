//! Read action-specific modifier edits without converting or loading their stock templates.
use super::{Action, ModifierRow, NativeAssetResourcePatch, Program, ValueSource};
use crate::package_payload::{u16_at, u32_at};
use crate::sandbox_perk::action::native::{Graph, value};

/// Numeric property changes carried by this action, rather than by its shared template.
pub struct Changes {
    pub rows: Vec<ModifierRow>,
    pub driver: Option<Driver>,
    pub target: u8,
    /// Other resource edits remain outside this numeric reading.
    pub additional: bool,
}

pub struct Driver {
    pub source: ValueSource,
    pub normalized: bool,
    pub uses_input: bool,
}

impl Program {
    /// Read complete native modifier rows and explicitly authored additional rows. No stock
    /// behavior is inferred from the source graph's identity or from arbitrary resource bytes.
    #[must_use]
    pub fn action_changes(&self, index: usize) -> Option<Changes> {
        let action = self.actions.get(index)?;
        let (graph, driver, target) = match action {
            Action::Attach { asset, mode, .. } => (asset.graph, None, mode.byte()),
            Action::Native { node } if matches!(node.kind, 1 | 2) => {
                let class = crate::sandbox_perk::nodes::effect(node.kind)?.class;
                let graph = Graph::read(&node.bytes, 0, class).ok()?;
                let driver = if node.kind == 2 {
                    let program = value::Program::read(&graph, 0, 0x20).ok()?;
                    program.validate().ok()?;
                    Some(Driver {
                        source: ValueSource::from_byte(*node.bytes.get(0x50)?),
                        normalized: *node.bytes.get(0x51)? != 0,
                        uses_input: program.fast_path == 1
                            || program
                                .instructions
                                .iter()
                                .any(|instruction| instruction.opcode == 0x3C),
                    })
                } else {
                    None
                };
                (u32_at(&node.bytes, 0x10).ok()?, driver, *node.bytes.get(2)?)
            }
            _ => return None,
        };
        let mut changes = Changes {
            rows: Vec::new(),
            driver,
            target,
            additional: false,
        };
        if let Some(asset) = action.asset() {
            for row in &asset.rows {
                if row.operation <= 1 && row.amount().is_finite() {
                    changes.rows.push(row.clone());
                } else {
                    changes.additional = true;
                }
            }
            changes.additional |= !asset.values.is_empty();
        }
        for edit in self
            .native_asset_patches
            .iter()
            .filter(|edit| edit.action_index == index && edit.source_graph == graph)
        {
            for patch in &edit.patches {
                match modifier(patch) {
                    Some(row) => {
                        changes.rows.push(row);
                        changes.additional |=
                            patch.bytes.iter().zip(&patch.expected).enumerate().any(
                                |(offset, (value, expected))| {
                                    value != expected
                                        && !matches!(offset, 0x28..=0x2C | 0x48..=0x4C)
                                },
                            );
                    }
                    None => changes.additional = true,
                }
            }
            changes.additional |= !edit.appends.is_empty()
                || !edit.remove_owners.is_empty()
                || !edit.components.is_empty()
                || !edit.programs.is_empty();
        }
        (!changes.rows.is_empty()).then_some(changes)
    }
}

fn modifier(patch: &NativeAssetResourcePatch) -> Option<ModifierRow> {
    // A Property Modifiers settings row points back to its 3B05 instance. The resource
    // binding, complete row extents and matching instance prefixes distinguish this form.
    if patch.binding_hash != 0x7330_E39F
        || patch.bytes.len() != 0x58
        || patch.expected.len() != 0x58
        || patch.imported_particle.is_some()
        || u32_at(&patch.bytes, 4).ok()? != 0x8080_3B05
        || u32_at(&patch.expected, 4).ok()? != 0x8080_3B05
        || patch.bytes[..0x28] != patch.expected[..0x28]
    {
        return None;
    }
    let amount_bits = u32_at(&patch.bytes, 0x28).ok()?;
    let operation = patch.bytes[0x2C];
    if !f32::from_bits(amount_bits).is_finite() || operation > 1 {
        return None;
    }
    Some(ModifierRow {
        component: patch.bytes[0x4C],
        ability: u16_at(&patch.bytes, 0x48).ok()? as i16,
        input: u16_at(&patch.bytes, 0x4A).ok()? as i16,
        operation,
        amount_bits,
    })
}
