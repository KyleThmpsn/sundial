//! Emits the mapped single-group, policy-zero action format from authored nodes.
use super::*;
use crate::{
    investment_schema::NESTED_ARRAY_TRAILER,
    package_payload::{native_array_at, u32_at},
    sandbox_perk::projectile,
    weapon_entity::{WEAPON_ENTITY_CLASS, validate_weapon_entity},
};
use tiger_pkg::{PackageManager, TagHash};

const EMPTY_KEY: u32 = 0x811C_9DC5;
const LABEL_GLOBALS: u32 = 0x80C7_0CA1;
const LABEL_PATH: &str = "content/common/native/sandbox/label_globals.label_globals.tft";
const CONDITION_ROWS: u32 = 0x8080_40BA;
const EFFECT_ROWS: u32 = 0x8080_40AC;

/// Compiles fresh action nodes. No stock action bytes are inherited.
/// Entity data is validated here and cloned by Parhelion before parameter mutation.
pub struct Compiled {
    pub payload: Vec<u8>,
    /// One tag lane per authored action, in the same order as `Program::actions`.
    pub graph_offsets: Vec<usize>,
}

pub fn compile(manager: &PackageManager, program: &Program) -> Result<Compiled, String> {
    program.validate()?;
    for action in &program.actions {
        let asset = action.asset();
        let tag = TagHash(asset.graph);
        let entry = manager
            .get_entry(tag)
            .ok_or_else(|| format!("Asset {tag} is missing."))?;
        if entry.file_type != 8 || entry.reference != WEAPON_ENTITY_CLASS {
            return Err(format!("Asset {tag} is not an entity graph."));
        }
        let payload = manager
            .read_tag(tag)
            .map_err(|error| format!("Could not read asset {tag}: {error}"))?;
        validate_weapon_entity(&payload)?;
        let kind = projectile::kind(&payload)?;
        if kind.is_none()
            || (matches!(action, Action::Pattern { .. })
                && kind != Some(projectile::Kind::Projectile))
        {
            return Err(format!(
                "{} requires {}.",
                action.label(),
                if matches!(action, Action::Pattern { .. }) {
                    "a projectile"
                } else {
                    "a projectile or emitter"
                }
            ));
        }
    }
    let label_mask = if program.trigger.is_event() {
        let labels: &[u32] = match program.trigger {
            Trigger::PrecisionKill => &[0x962E_A19B],
            Trigger::MeleeKill => &[0xBF39_E12B, 0xE175_76C9, 0x5D3A_7C84],
            Trigger::GrenadeKill => &[0xC20D_D425],
            _ => &[],
        };
        let registry = manager
            .read_tag(TagHash(LABEL_GLOBALS))
            .map_err(|error| format!("Could not read label globals: {error}"))?;
        Some((labels, compile_labels(&registry, labels)?))
    } else {
        None
    };
    let mut out = Payload::new();
    let activation = match program.trigger {
        Trigger::Equipped => out.weapon_condition(14, 0x8080_3DFA, 0),
        Trigger::Drawn => out.weapon_condition(16, 0x8080_3DF5, 0),
        trigger => {
            let (labels, mask) = label_mask.expect("kill trigger has a label mask");
            out.kill_condition(trigger, labels, mask, program.chance_permyriad)
        }
    };
    out.nodes(0x20, CONDITION_ROWS, &[activation]);
    let mut effects = Vec::new();
    // The native initial dispatcher walks from last to first. Preserve authored execution order.
    for action in program.actions.iter().rev() {
        effects.push(out.action(action));
    }
    out.nodes(0x38, EFFECT_ROWS, &effects);

    let removal = match program.trigger {
        Trigger::Equipped => out.weapon_condition(15, 0x8080_3DF7, 1),
        Trigger::Drawn => out.weapon_condition(17, 0x8080_3DDB, 1),
        _ => out.timer(program.duration_ms, 1),
    };
    out.nodes(0x48, CONDITION_ROWS, &[removal]);
    let has_cooldown = program.trigger.is_event() && program.cooldown_ms != 0;
    if has_cooldown {
        let rearm = out.timer(program.cooldown_ms, 2);
        out.nodes(0x58, CONDITION_ROWS, &[rearm]);
    }
    let active_kind = match program.trigger {
        Trigger::Equipped => 14,
        Trigger::Drawn => 16,
        _ => 2,
    };
    let removal_kind = match program.trigger {
        Trigger::Equipped => 15,
        Trigger::Drawn => 17,
        _ => 1,
    };
    out.u64(0x88, 1 << active_kind);
    out.u64(0x90, 1 << removal_kind);
    out.u64(0x98, if has_cooldown { 2 } else { 1 });
    out.bytes[0xCC] = 1 + program
        .actions
        .iter()
        .filter(|action| action.retained())
        .count() as u8;
    out.bytes[0xCD] = u8::from(program.trigger.is_event()) + u8::from(has_cooldown);
    out.u64(0, out.bytes.len() as u64);
    let graph_offsets = effects.iter().rev().map(|at| at + 0x10).collect();
    Ok(Compiled {
        payload: out.bytes,
        graph_offsets,
    })
}

fn compile_labels(registry: &[u8], labels: &[u32]) -> Result<[u8; 40], String> {
    let (count, _, rows, class) = native_array_at(registry, 8)?;
    if class != 0x8080_0070 || count > 320 {
        return Err("Label globals have an unsupported layout.".into());
    }
    let (group_count, _, groups, group_class) = native_array_at(registry, 0x18)?;
    if group_class != 0x8080_94BE {
        return Err("Label groups have an unsupported layout.".into());
    }
    let mut mask = [0; 40];
    for &label in labels {
        if let Some(index) =
            (0..count).find(|index| u32_at(registry, rows + index * 4).ok() == Some(label))
        {
            mask[index / 8] |= 1 << (index % 8);
        } else if let Some(index) =
            (0..group_count).find(|index| u32_at(registry, groups + index * 44).ok() == Some(label))
        {
            let source = registry
                .get(groups + index * 44 + 4..groups + index * 44 + 44)
                .ok_or("Label group is truncated.")?;
            for (target, source) in mask.iter_mut().zip(source) {
                *target |= source;
            }
        } else {
            return Err(format!("Label 0x{label:08X} is not registered."));
        }
    }
    Ok(mask)
}

struct Payload {
    bytes: Vec<u8>,
    label_path: usize,
}

impl Payload {
    fn new() -> Self {
        let mut out = Self {
            bytes: vec![0; 0xD0],
            label_path: 0,
        };
        out.u32(8, EMPTY_KEY);
        out.u32(0x80, EMPTY_KEY);
        for offset in [0xBC, 0xC0, 0xC4, 0xC8] {
            out.u32(offset, u32::MAX);
        }
        out.label_path = out.string(LABEL_PATH);
        out
    }
    fn u32(&mut self, at: usize, value: u32) {
        self.bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn u64(&mut self, at: usize, value: u64) {
        self.bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    fn pointer(&mut self, at: usize, target: usize) {
        self.bytes[at..at + 8].copy_from_slice(&((target as i64) - (at as i64)).to_le_bytes());
    }
    fn align(&mut self, alignment: usize) {
        self.bytes
            .resize(self.bytes.len().next_multiple_of(alignment), 0);
    }
    fn string(&mut self, value: &str) -> usize {
        let at = self.bytes.len();
        self.bytes.extend_from_slice(value.as_bytes());
        self.bytes.push(0);
        at
    }
    fn node(&mut self, class: u32, size: usize) -> usize {
        let at = (self.bytes.len() + 4).next_multiple_of(8);
        self.bytes.resize(at + size, 0);
        self.u32(at - 4, class);
        at
    }
    fn rows(&mut self, descriptor: usize, class: u32, count: usize, stride: usize) -> usize {
        self.align(16);
        let header = self.bytes.len();
        let rows = header + 16;
        self.bytes.resize(rows + count * stride, 0);
        self.bytes.extend_from_slice(&NESTED_ARRAY_TRAILER);
        self.u64(header, count as u64);
        self.u32(header + 8, class);
        self.u64(descriptor, count as u64);
        self.pointer(descriptor + 8, header);
        rows
    }
    fn nodes(&mut self, descriptor: usize, class: u32, nodes: &[usize]) {
        let rows = self.rows(descriptor, class, nodes.len(), 8);
        for (index, target) in nodes.iter().enumerate() {
            self.pointer(rows + index * 8, *target);
        }
    }
    fn condition(&mut self, class: u32, kind: u8, size: usize, ordinal: u8) -> usize {
        let at = self.node(class, size);
        self.u32(at, 1.0_f32.to_bits());
        self.bytes[at + 4] = 0xFF;
        self.bytes[at + 5] = kind;
        self.bytes[at + 7] = ordinal;
        at
    }
    fn label_reference(&mut self, at: usize) {
        self.pointer(at, self.label_path);
        self.u64(at + 8, u64::from(LABEL_GLOBALS));
    }
    fn weapon_condition(&mut self, kind: u8, class: u32, ordinal: u8) -> usize {
        let at = self.condition(class, kind, 112, ordinal);
        self.bytes[at + 8] = 1;
        self.label_reference(at + 0x50);
        self.bytes[at + 0x60] = 0xFF;
        at
    }
    fn timer(&mut self, millis: u32, ordinal: u8) -> usize {
        let at = self.condition(0x8080_3DCD, 1, 12, ordinal);
        self.u32(at + 8, (millis as f32 / 1000.0).to_bits());
        at
    }
    fn kill_condition(
        &mut self,
        trigger: Trigger,
        labels: &[u32],
        mask: [u8; 40],
        chance: u16,
    ) -> usize {
        let at = self.condition(0x8080_3DE7, 2, 344, 0);
        self.u32(at, (f32::from(chance) / 10_000.0).to_bits());
        self.label_reference(at + 0x48);
        self.bytes[at + 0x58] = 0xFF;
        self.label_reference(at + 0x110);
        self.bytes[at + 0x138] = 1;
        self.bytes[at + 0x139] = 1;
        self.bytes[at + 0x141] = u8::from(matches!(
            trigger,
            Trigger::WeaponKill | Trigger::PrecisionKill
        ));
        self.u32(at + 0x144, EMPTY_KEY);
        self.u32(at + 0x148, EMPTY_KEY);
        self.u32(at + 0x154, (-1.0_f32).to_bits());
        if !labels.is_empty() {
            let rows = self.rows(at + 0xD0, 0x8080_94B3, labels.len(), 24);
            for (index, label) in labels.iter().enumerate() {
                self.u32(rows + index * 24, *label);
                self.label_reference(rows + index * 24 + 8);
            }
        }
        // Ability predicate at +128 uses its pointer at +130, not the source label list.
        let predicate = self.node(0x8080_93F6, 84);
        self.bytes[predicate..predicate + 40].copy_from_slice(&mask);
        self.bytes[predicate + 80] = u8::from(labels.is_empty());
        self.bytes[predicate + 81] = 1;
        self.pointer(at + 0x130, predicate);
        at
    }
    fn action(&mut self, action: &Action) -> usize {
        let (class, kind, size) = match action {
            Action::Spawn { .. } => (0x8080_3E43, 3, 24),
            Action::Attach { .. } => (0x8080_3E45, 1, 64),
            Action::Pattern { .. } => (0x8080_3E12, 26, 24),
        };
        let at = self.node(class, size);
        self.bytes[at] = kind;
        self.bytes[at + 1] = u8::from(action.retained());
        match action {
            Action::Spawn { position, .. } => {
                self.bytes[at + 4] = u8::from(*position == Position::Event);
            }
            Action::Attach { .. } => {
                self.bytes[at + 2] = 1;
                for offset in [0x18, 0x1C, 0x30] {
                    self.u32(at + offset, EMPTY_KEY);
                }
            }
            Action::Pattern { .. } => {}
        }
        let asset = action.asset();
        if !asset.path.is_empty() {
            let path = self.string(&asset.path);
            self.pointer(at + 8, path);
        }
        self.u64(at + 0x10, u64::from(asset.graph));
        at
    }
}
