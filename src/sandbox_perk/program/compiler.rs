//! Emits the mapped single-group, policy-zero action format from authored nodes.
use super::*;
use crate::{
    investment_schema::NESTED_ARRAY_TRAILER,
    package_payload::{native_array_at, u32_at},
    sandbox_perk::projectile,
    weapon_entity::{WEAPON_ENTITY_CLASS, validate_weapon_entity},
};
use tiger_pkg::{PackageManager, TagHash};

const LABEL_GLOBALS: u32 = 0x80C7_0CA1;
const LABEL_PATH: &str = "content/common/native/sandbox/label_globals.label_globals.tft";
const CONDITION_ROWS: u32 = 0x8080_40BA;
const EFFECT_ROWS: u32 = 0x8080_40AC;

mod metadata;
mod native;

#[cfg(test)]
mod coverage;

/// Compiles fresh action nodes. No stock action bytes are inherited.
/// Entity data is validated here and cloned by Parhelion before parameter mutation.
pub struct Compiled {
    pub payload: Vec<u8>,
    /// One entry per authored action, in the same order as `Program::actions`: the offset of
    /// the tag lane for actions that reference an asset, `None` for the rest.
    pub graph_offsets: Vec<Option<usize>>,
    /// Editable asset index and every relocated tag lane that references it.
    pub asset_offsets: Vec<(usize, Vec<usize>)>,
}

/// The activation trigger as the compiler emits it, reused by effects that nest it.
struct Activation<'a> {
    trigger: Trigger,
    labels: &'a [u32],
    mask: [u8; 40],
    chance: u16,
}

pub fn compile(manager: &PackageManager, program: &Program) -> Result<Compiled, String> {
    program.validate()?;
    if let Some(native) = &program.native {
        return native::compile(manager, native);
    }
    for action in &program.actions {
        validate_asset(manager, action)?;
    }
    let mut program = program.clone();
    prepare_native(manager, &mut program)?;
    let label_mask = kill_label_mask(manager, program.trigger)?;
    assemble(&program, label_mask)
}

fn assemble(program: &Program, label_mask: LabelMask) -> Result<Compiled, String> {
    program.validate()?;
    let mut out = Payload::new();
    let activation = match program.trigger {
        Trigger::Always => out.unconditional(0),
        Trigger::Equipped => out.weapon_condition(14, 0x8080_3DFA, 0),
        Trigger::Drawn => out.weapon_condition(16, 0x8080_3DF5, 0),
        Trigger::Native => {
            let node = program
                .native_trigger
                .as_ref()
                .ok_or("Choose a native condition for the trigger.")?;
            out.native_condition(node, 0)?
        }
        trigger => {
            let (labels, mask) = label_mask.expect("kill trigger has a label mask");
            out.kill_condition(trigger, labels, mask, program.chance_permyriad, 0)
        }
    };
    out.nodes(0x20, CONDITION_ROWS, &[activation]);
    let nested = label_mask.map(|(labels, mask)| Activation {
        trigger: program.trigger,
        labels,
        mask,
        chance: program.chance_permyriad,
    });
    let mut effects = Vec::new();
    // The native initial dispatcher walks from last to first. Preserve authored execution order.
    for action in program.actions.iter().rev() {
        effects.push(out.action(action, nested.as_ref())?);
    }
    out.nodes(0x38, EFFECT_ROWS, &effects);
    out.removal_and_rearm(program)?;
    out.u64(0, out.bytes.len() as u64);
    metadata::rebuild(&mut out.bytes)?;
    // Complex native graphs can carry state below their root node. Reserve an upper
    // bound for each node instead of reusing the scalar-only reservation calculation.
    let complex = program.native_trigger.iter().chain(&program.native_removal)
        .any(|node|crate::sandbox_perk::action::layout::condition_layout(node.kind).is_none())
        || program.actions.iter().any(|action|matches!(action,Action::Native{node} if crate::sandbox_perk::action::layout::effect_layout(node.kind).is_none()));
    if complex {
        let decoded = crate::sandbox_perk::action::decode(&out.bytes)?;
        let conditions = decoded.conditions();
        let subgroup_slots = conditions
            .iter()
            .map(|node| node.subgroups.len())
            .sum::<usize>();
        let slots = conditions.len() + subgroup_slots + decoded.effects().count();
        let states = u8::try_from(slots + 1)
            .map_err(|_| "The native program exceeds its state reservation limit.")?;
        out.bytes[0xCC] = states;
        out.bytes[0xCD] = states - 1;
    }
    let mut registry = crate::package_runtime::references::schema::Registry::new()?;
    crate::package_runtime::references::walk(
        &out.bytes,
        crate::sandbox_perk::action::ACTION_ROOT_CLASS,
        |class| {
            registry.record(class, |_| {
                Err("An authored action must use a recovered native schema.".into())
            })
        },
    )
    .map_err(|error| format!("Compiled action structure is invalid: {error}"))?;
    let graph_offsets: Vec<_> = effects
        .iter()
        .rev()
        .zip(&program.actions)
        .map(|(at, action)| action.asset().map(|_| at + 0x10))
        .collect();
    Ok(Compiled {
        payload: out.bytes,
        asset_offsets: graph_offsets
            .iter()
            .enumerate()
            .filter_map(|(index, offset)| offset.map(|at| (index, vec![at])))
            .collect(),
        graph_offsets,
    })
}

/// Every action needs a live, structurally valid entity graph. A pattern override needs a
/// projectile. Spawn uses the native generic object-creation route, including physical
/// props, interactables and pickups. An attach accepts any
/// entity graph, since stock attach effects reference buff and aura entities as well.
fn validate_asset(manager: &PackageManager, action: &Action) -> Result<(), String> {
    let Some(asset) = action.asset() else {
        return Ok(());
    };
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
    let kind = projectile::spawn_kind(&payload)?;
    let acceptable = match action {
        Action::Attach { .. }
        | Action::ExtendTimers { .. }
        | Action::Property { .. }
        | Action::AddRounds { .. }
        | Action::AddFraction { .. }
        | Action::Native { .. } => true,
        Action::Spawn { .. } => kind.is_some(),
        Action::Pattern { .. } => kind == Some(projectile::Kind::Projectile),
    };
    if acceptable {
        Ok(())
    } else {
        Err(format!(
            "{} requires {}.",
            action.label(),
            if matches!(action, Action::Pattern { .. }) {
                "a projectile"
            } else {
                "a projectile, emitter, pickup or world object"
            }
        ))
    }
}

fn prepare_native(manager: &PackageManager, program: &mut Program) -> Result<(), String> {
    use crate::sandbox_perk::action::native::{Graph, labels, schema};
    let mut registry = None;
    let mut entries = Vec::new();
    if let Some(node) = &mut program.native_trigger {
        entries.push((true, node));
    }
    if let Some(node) = &mut program.native_removal {
        entries.push((true, node));
    }
    for action in &mut program.actions {
        if let Action::Native { node } = action {
            entries.push((false, node));
        }
    }
    for (condition, node) in entries {
        let class = if condition {
            crate::sandbox_perk::nodes::condition(node.kind)
        } else {
            crate::sandbox_perk::nodes::effect(node.kind)
        }
        .ok_or("Unknown native node.")?
        .class;
        let mut graph = Graph::read(&node.bytes, 0, class)?;
        let has_labels = graph
            .blocks
            .iter()
            .filter(|block| block.class != 0)
            .any(|block| {
                schema::inline(block.class).is_ok_and(|views| {
                    views
                        .iter()
                        .any(|(_, class, _)| *class == labels::SOURCE_CLASS)
                })
            });
        if has_labels {
            if registry.is_none() {
                registry = Some(
                    manager
                        .read_tag(TagHash(LABEL_GLOBALS))
                        .map_err(|error| format!("Could not read label globals: {error}"))?,
                );
            }
            labels::compile(&mut graph, registry.as_deref().expect("loaded registry"))?;
        }
        validate_native_resources(manager, &graph)?;
        node.bytes = graph.emit()?;
    }
    Ok(())
}

fn validate_native_resources(
    manager: &PackageManager,
    graph: &crate::sandbox_perk::action::native::Graph,
) -> Result<(), String> {
    use crate::sandbox_perk::action::native::schema;
    for block in graph.blocks.iter().filter(|b| b.class != 0) {
        validate_native_entity(manager, block)?;
        let record = schema::record(block.class)?;
        for row in 0..block.count.unwrap_or(1) {
            for &(field, code) in &record.fields {
                if !matches!(code, 4 | 9) {
                    continue;
                }
                let tag = u32_at(&block.bytes, row * record.size + field)?;
                if matches!(tag, 0 | u32::MAX) {
                    continue;
                }
                let entry = manager
                    .get_entry(TagHash(tag))
                    .ok_or_else(|| format!("Native resource 0x{tag:08X} is missing."))?;
                if entry.reference == WEAPON_ENTITY_CLASS {
                    projectile::residency::inspect(manager, tag)?;
                }
            }
        }
    }
    Ok(())
}

fn validate_native_entity(
    manager: &PackageManager,
    block: &crate::sandbox_perk::action::native::Block,
) -> Result<(), String> {
    if !matches!(
        block.class,
        0x80803E45 | 0x80803E44 | 0x80803E43 | 0x80803E47 | 0x80803E12
    ) {
        return Ok(());
    }
    let tag = TagHash(u32_at(&block.bytes, 16)?);
    // Weighted spawning creates its category result without this optional attachment.
    if block.class == 0x80803E47 && matches!(tag.0, 0 | u32::MAX) {
        return Ok(());
    }
    let entry = manager.get_entry(tag).ok_or_else(|| {
        format!(
            "Choose an entity graph for native effect {}.",
            block.bytes[0]
        )
    })?;
    if entry.file_type != 8 || entry.reference != WEAPON_ENTITY_CLASS {
        return Err(format!(
            "Native effect {} requires an entity graph.",
            block.bytes[0]
        ));
    }
    let payload = manager
        .read_tag(tag)
        .map_err(|error| format!("Could not read entity graph {tag}: {error}"))?;
    validate_weapon_entity(&payload)?;
    if block.class == 0x80803E12
        && projectile::kind(&payload)? != Some(projectile::Kind::Projectile)
    {
        return Err("Override Weapon Pattern requires a projectile.".into());
    }
    Ok(())
}

type LabelMask = Option<(&'static [u32], [u8; 40])>;

fn kill_label_mask(manager: &PackageManager, trigger: Trigger) -> Result<LabelMask, String> {
    if !trigger.is_event() {
        return Ok(None);
    }
    let labels: &'static [u32] = match trigger {
        Trigger::PrecisionKill => &[0x962E_A19B],
        Trigger::MeleeKill => &[0xBF39_E12B, 0xE175_76C9, 0x5D3A_7C84],
        Trigger::GrenadeKill => &[0xC20D_D425],
        _ => &[],
    };
    let registry = manager
        .read_tag(TagHash(LABEL_GLOBALS))
        .map_err(|error| format!("Could not read label globals: {error}"))?;
    Ok(Some((labels, compile_labels(&registry, labels)?)))
}

pub(crate) fn compile_labels(registry: &[u8], labels: &[u32]) -> Result<[u8; 40], String> {
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

pub(crate) struct Payload {
    pub(crate) bytes: Vec<u8>,
    label_path: usize,
}

impl Payload {
    pub(crate) fn new() -> Self {
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
    fn string(&mut self, value: &str) -> usize {
        let at = self.bytes.len();
        self.bytes.extend_from_slice(value.as_bytes());
        self.bytes.push(0);
        at
    }
    fn node(&mut self, class: u32, size: usize) -> usize {
        let at = (self.bytes.len() + 4).next_multiple_of(16);
        self.bytes.resize(at + size, 0);
        self.u32(at - 4, class);
        at
    }
    fn rows(&mut self, descriptor: usize, class: u32, count: usize, stride: usize) -> usize {
        // Native typed pointers identify arrays by the marker immediately before the
        // header. Reserve its bytes even when the preceding node ends on an alignment.
        let header = (self.bytes.len() + 4).next_multiple_of(16);
        self.bytes.resize(header, 0);
        self.u32(header - 4, 0x8080_9FBD);
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
    /// Kind 0 always passes. The common probability and event routing still apply.
    fn unconditional(&mut self, ordinal: u8) -> usize {
        self.condition(0x8080_3E03, 0, 8, ordinal)
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
        ordinal: u8,
    ) -> usize {
        let at = self.condition(0x8080_3DE7, 2, 344, ordinal);
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
    /// Emits the removal and rearm lists. Returns whether a rearm timer was emitted.
    ///
    /// An always-active program has no removal list. Stock always-on actions leave that
    /// descriptor zeroed and its event mask clear, and the rearm ordinal follows directly.
    /// Kind 30 ends the action when the event context value equals the key.
    fn event_key_condition(&mut self, key: u32, ordinal: u8) -> usize {
        let at = self.condition(0x8080_3DEB, 30, 12, ordinal);
        self.u32(at + 8, key);
        at
    }
    /// Carry the complete native graph and its probability settings. Evaluation order
    /// and linked-state metadata are rebuilt after all nodes have been assembled.
    pub(crate) fn native_condition(
        &mut self,
        node: &NativeNode,
        ordinal: u8,
    ) -> Result<usize, String> {
        let class = crate::sandbox_perk::nodes::condition(node.kind)
            .map(|kind| kind.class)
            .filter(|class| *class != 0)
            .ok_or_else(|| format!("Condition kind {} has no native class.", node.kind))?;
        let at = self.condition(class, node.kind, node.bytes.len(), ordinal);
        self.bytes[at..at + node.bytes.len()].copy_from_slice(&node.bytes);
        self.bytes[at + 7] = ordinal;
        Ok(at)
    }

    /// The ending condition of a program, when it has one. The kind decides the removal
    /// event mask, so it is returned beside the node.
    fn removal(&mut self, program: &Program) -> Result<Option<(usize, u8)>, String> {
        Ok(match program.trigger {
            Trigger::Always | Trigger::Native => {
                if let Some(node) = &program.native_removal {
                    Some((self.native_condition(node, 1)?, node.kind))
                } else if let Some(key) = program.removal_key {
                    Some((self.event_key_condition(key, 1), 30))
                } else if program.trigger == Trigger::Native && program.duration_ms != 0 {
                    Some((self.timer(program.duration_ms, 1), 1))
                } else {
                    None
                }
            }
            Trigger::Equipped => Some((self.weapon_condition(15, 0x8080_3DF7, 1), 15)),
            Trigger::Drawn => Some((self.weapon_condition(17, 0x8080_3DDB, 1), 17)),
            _ => Some((self.timer(program.duration_ms, 1), 1)),
        })
    }

    /// Emits the removal and rearm lists. Returns whether a rearm timer was emitted.
    ///
    /// An always-active program has no removal list. Stock always-on actions leave that
    /// descriptor zeroed and its event mask clear, and the rearm ordinal follows directly.
    fn removal_and_rearm(&mut self, program: &Program) -> Result<bool, String> {
        let removal = self.removal(program)?;
        if let Some((removal, _)) = removal {
            self.nodes(0x48, CONDITION_ROWS, &[removal]);
        }
        let has_cooldown = program.trigger.supports_cooldown() && program.cooldown_ms != 0;
        if has_cooldown {
            let ordinal = if removal.is_some() { 2 } else { 1 };
            let rearm = self.timer(program.cooldown_ms, ordinal);
            self.nodes(0x58, CONDITION_ROWS, &[rearm]);
        }
        self.root_state(program, removal.map(|(_, kind)| kind), has_cooldown);
        Ok(has_cooldown)
    }

    /// Writes the compiled event masks and the retained-state and timer budgets.
    fn root_state(&mut self, program: &Program, removal_kind: Option<u8>, has_cooldown: bool) {
        let active_kind = match program.trigger {
            Trigger::Always => 0,
            Trigger::Equipped => 14,
            Trigger::Drawn => 16,
            Trigger::Native => program.native_trigger.as_ref().map_or(0, |node| node.kind),
            _ => 2,
        };
        self.u64(0x88, 1 << active_kind);
        self.u64(0x90, removal_kind.map_or(0, |kind| 1 << kind));
        self.u64(0x98, if has_cooldown { 2 } else { 1 });
        let extension_mask =
            program
                .actions
                .iter()
                .rev()
                .enumerate()
                .fold(0_u64, |mask, (index, action)| {
                    if matches!(action, Action::ExtendTimers { .. }) {
                        mask | (1 << index)
                    } else {
                        mask
                    }
                });
        self.u64(0xA0, extension_mask);
        self.bytes[0xCC] = 1 + program
            .actions
            .iter()
            .filter(|action| action.retained())
            .count() as u8;
        self.bytes[0xCD] =
            u8::from(active_kind == 1) + u8::from(removal_kind == Some(1)) + u8::from(has_cooldown);
    }
    fn action(
        &mut self,
        action: &Action,
        nested: Option<&Activation<'_>>,
    ) -> Result<usize, String> {
        let (class, kind, size) = match action {
            Action::Spawn { .. } => (0x8080_3E43, 3, 24),
            Action::Attach { .. } => (0x8080_3E45, 1, 64),
            Action::Pattern { .. } => (0x8080_3E12, 26, 24),
            Action::ExtendTimers { .. } => (0x8080_3E3B, 32, 40),
            Action::Property { .. } => (0x8080_29ED, 10, 80),
            Action::AddRounds { .. } => (0x8080_3E3F, 14, 136),
            Action::AddFraction { .. } => (0x8080_3E3E, 15, 136),
            Action::Native { node } => {
                let class = crate::sandbox_perk::nodes::effect(node.kind)
                    .map(|kind| kind.class)
                    .filter(|class| *class != 0)
                    .ok_or_else(|| format!("Effect kind {} has no native class.", node.kind))?;
                (class, node.kind, node.bytes.len())
            }
        };
        let at = self.node(class, size);
        self.bytes[at] = kind;
        self.bytes[at + 1] = u8::from(action.retained());
        match action {
            Action::Native { node } => {
                // Verbatim: the kind and retained bytes above already match the node.
                self.bytes[at..at + node.bytes.len()].copy_from_slice(&node.bytes);
            }
            Action::Spawn { position, .. } => {
                self.bytes[at + 4] = u8::from(*position == Position::Event);
            }
            Action::Attach {
                mode,
                keys,
                float_bits,
                ..
            } => {
                // The mode byte, the two keys and the four floats are carried verbatim from
                // the authored action. Their roles are not mapped, so nothing is derived here.
                self.bytes[at + 2] = *mode;
                self.u32(at + 0x18, keys[0]);
                self.u32(at + 0x1C, keys[1]);
                for (index, bits) in float_bits.iter().enumerate() {
                    self.u32(at + 0x20 + index * 4, *bits);
                }
                self.u32(at + 0x30, EMPTY_KEY);
            }
            Action::Pattern { .. } => {}
            Action::ExtendTimers { extend_ms, cap_ms } => {
                let nested = nested.ok_or("Extend Timers requires a kill trigger.")?;
                self.u32(at + 4, (*extend_ms as f32 / 1000.0).to_bits());
                self.u32(at + 8, (*cap_ms as f32 / 1000.0).to_bits());
                // The nested list repeats the trigger. Nested conditions carry ordinal 0xFF
                // and do not advance the action's ordinal sequence. The mask at +0x20 routes
                // the same event kind to the nested list.
                let condition = self.kill_condition(
                    nested.trigger,
                    nested.labels,
                    nested.mask,
                    nested.chance,
                    0xFF,
                );
                self.nodes(at + 0x10, CONDITION_ROWS, &[condition]);
                self.u64(at + 0x20, 1 << 2);
            }
            Action::Property {
                key,
                target,
                operation_byte,
                removal,
                value_bits,
                restore_bits,
                ability_mask,
                input,
                flag,
            } => {
                self.bytes[at + 2] = *target;
                self.bytes[at + 3] = *flag;
                self.u32(at + 4, *ability_mask);
                self.u32(at + 8, *key);
                self.constant_program(at + 0x18, *value_bits);
                self.bytes[at + 0x48] = *input;
                self.bytes[at + 0x49] = *operation_byte;
                self.bytes[at + 0x4A] = *removal;
                self.u32(at + 0x4C, *restore_bits);
            }
            Action::AddRounds {
                rounds,
                target,
                store,
                overflow,
                unit_scaled,
                action_scaled,
            } => {
                self.ammunition_filter(at);
                self.bytes[at + 0x68] = store.byte();
                self.bytes[at + 0x69] = u8::from(*overflow);
                self.bytes[at + 0x6A] = u8::from(*unit_scaled);
                self.bytes[at + 0x6B] = u8::from(*action_scaled);
                self.u32(at + 0x6C + target.index() * 4, *rounds as u32);
            }
            Action::AddFraction {
                fraction_bits,
                target,
                store,
                capacity,
                overflow,
                action_scaled,
            } => {
                self.ammunition_filter(at);
                self.bytes[at + 0x68] = store.byte();
                self.bytes[at + 0x69] = u8::from(*overflow);
                self.bytes[at + 0x6A] = capacity.byte();
                self.bytes[at + 0x6B] = u8::from(*action_scaled);
                self.u32(at + 0x6C + target.index() * 4, *fraction_bits);
            }
        }
        if let Some(asset) = action.asset() {
            if !asset.path.is_empty() {
                let path = self.string(&asset.path);
                self.pointer(at + 8, path);
            }
            self.u64(at + 0x10, u64::from(asset.graph));
        }
        Ok(at)
    }

    /// The empty source label filter every label-free stock ammunition node carries at
    /// `+0x08`: zeroed lists, the label globals reference at `+0x48` and the `0xFF` byte at
    /// `+0x58`, the same shape the kill condition's filter takes.
    fn ammunition_filter(&mut self, at: usize) {
        self.label_reference(at + 0x48);
        self.bytes[at + 0x58] = 0xFF;
    }

    /// Writes the value program stock nodes use for a constant: `34 00 3E 00` loads constant
    /// vector zero and stores output zero, and the single constant row holds the value in all
    /// four lanes. The retained metadata words and the zero fast-path selector match every
    /// surveyed constant program.
    fn constant_program(&mut self, at: usize, value_bits: u32) {
        let code = self.rows(at, 0x8080_0009, 4, 1);
        self.bytes[code..code + 4].copy_from_slice(&[0x34, 0x00, 0x3E, 0x00]);
        let constants = self.rows(at + 0x10, 0x8080_0090, 1, 16);
        for lane in 0..4 {
            self.u32(constants + lane * 4, value_bits);
        }
        self.u64(at + 0x20, 1);
        self.u32(at + 0x28, 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attach_node(action: &Action) -> Vec<u8> {
        let mut out = Payload::new();
        let at = out.action(action, None).unwrap();
        out.bytes[at..at + 64].to_vec()
    }

    #[test]
    fn a_named_property_node_matches_the_surveyed_constant_shape() {
        let mut out = Payload::new();
        let at = out
            .action(
                &Action::Property {
                    key: 0x5EE2_66FC,
                    target: 2,
                    operation_byte: 0,
                    removal: 1,
                    value_bits: 1.0_f32.to_bits(),
                    restore_bits: 0,
                    ability_mask: 0,
                    input: 0,
                    flag: 1,
                },
                None,
            )
            .unwrap();
        let node = &out.bytes[at..at + 80];
        assert_eq!(&node[..4], &[10, 1, 2, 1]);
        assert_eq!(
            u32::from_le_bytes(node[8..12].try_into().unwrap()),
            0x5EE2_66FC
        );
        assert_eq!(u64::from_le_bytes(node[0x18..0x20].try_into().unwrap()), 4);
        assert_eq!(u64::from_le_bytes(node[0x28..0x30].try_into().unwrap()), 1);
        assert_eq!(
            &node[0x38..0x48],
            &[1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(&node[0x48..0x4C], &[0, 0, 1, 0]);
        let decoded = crate::sandbox_perk::action::constant_program_value(&out.bytes, at + 0x18);
        assert_eq!(decoded, Some(1.0));
    }

    #[test]
    fn an_always_active_program_ends_on_its_event_key() {
        let program = Program {
            trigger: Trigger::Always,
            duration_ms: 0,
            cooldown_ms: 3_000,
            removal_key: Some(0xA628_8DD1),
            actions: vec![Action::attach(Asset {
                graph: 0x80BC_5810,
                path: String::new(),
                values: Vec::new(),
            })],
            ..Program::default()
        };
        let mut out = Payload::new();
        assert!(out.removal_and_rearm(&program).unwrap());
        // A pointer row stores an offset relative to itself, and the node precedes the row.
        let pointed = |row: usize| {
            let relative = i64::from_le_bytes(out.bytes[row..row + 8].try_into().unwrap());
            usize::try_from(row as i64 + relative).unwrap()
        };
        let (count, _, rows, class) = native_array_at(&out.bytes, 0x48).unwrap();
        assert_eq!((count, class), (1, CONDITION_ROWS));
        let node = pointed(rows);
        assert_eq!(
            &out.bytes[node..node + 12],
            &[0, 0, 0x80, 0x3F, 0xFF, 30, 0, 1, 0xD1, 0x8D, 0x28, 0xA6]
        );
        assert_eq!(
            u64::from_le_bytes(out.bytes[0x90..0x98].try_into().unwrap()),
            1 << 30
        );
        assert_eq!(
            u64::from_le_bytes(out.bytes[0x98..0xA0].try_into().unwrap()),
            2
        );
        assert_eq!(&out.bytes[0xCC..0xCE], &[2, 1]);
        let (rearm_count, _, rearm_rows, _) = native_array_at(&out.bytes, 0x58).unwrap();
        let rearm = pointed(rearm_rows);
        assert_eq!((rearm_count, out.bytes[rearm + 7]), (1, 2));
    }

    #[test]
    fn ammunition_nodes_match_the_label_free_stock_shape() {
        let mut out = Payload::new();
        let at = out.action(&Action::add_rounds(2), None).unwrap();
        let node = &out.bytes[at..at + 136];
        assert_eq!(&node[..4], &[14, 0, 0, 0]);
        assert!(node[4..0x48].iter().all(|byte| *byte == 0));
        assert_eq!(
            u64::from_le_bytes(node[0x50..0x58].try_into().unwrap()),
            u64::from(LABEL_GLOBALS)
        );
        assert_eq!(node[0x58], 0xFF);
        assert_eq!(&node[0x68..0x6C], &[1, 0, 0, 0]);
        assert_eq!(u32::from_le_bytes(node[0x6C..0x70].try_into().unwrap()), 2);
        assert!(node[0x70..].iter().all(|byte| *byte == 0));
        let at = out
            .action(
                &Action::AddFraction {
                    fraction_bits: 0.25_f32.to_bits(),
                    target: AmmunitionTarget::Category3,
                    store: AmmunitionStore::Reserves,
                    capacity: AmmunitionStore::Reserves,
                    overflow: true,
                    action_scaled: false,
                },
                None,
            )
            .unwrap();
        let node = &out.bytes[at..at + 136];
        assert_eq!(&node[..2], &[15, 0]);
        assert_eq!(&node[0x68..0x6C], &[0, 1, 0, 0]);
        assert!(node[0x6C..0x84].iter().all(|byte| *byte == 0));
        assert_eq!(
            f32::from_le_bytes(node[0x84..0x88].try_into().unwrap()),
            0.25
        );
        // The decoder reads the compiled node back as the same facts a stock node gives.
        let decoded = crate::sandbox_perk::action::decode(&{
            let mut program = Program {
                trigger: Trigger::Always,
                duration_ms: 0,
                actions: vec![Action::add_rounds(2)],
                ..Program::default()
            };
            program.name = "Ammo".into();
            let mut out = Payload::new();
            let activation = out.unconditional(0);
            out.nodes(0x20, CONDITION_ROWS, &[activation]);
            let effect = out.action(&program.actions[0], None).unwrap();
            out.nodes(0x38, EFFECT_ROWS, &[effect]);
            out.removal_and_rearm(&program).unwrap();
            out.u64(0, out.bytes.len() as u64);
            out.bytes
        })
        .unwrap();
        let effect = &decoded.groups[0].effects[0];
        assert_eq!(effect.kind, 14);
        assert!(
            effect
                .facts
                .iter()
                .any(|fact| fact.label == "Owning Slot Amount"),
            "{:?}",
            effect.facts
        );
        assert!(!effect.facts.iter().any(|fact| matches!(
            fact.value,
            crate::sandbox_perk::action::FactValue::Labels(_)
        )));
    }

    #[test]
    fn native_nodes_are_written_verbatim_under_a_compiler_owned_header() {
        let mut trigger = NativeNode::condition(6).unwrap();
        trigger.bytes[8] = 0x03;
        trigger.bytes[9] = 0x04;
        let mut ending = NativeNode::condition(29).unwrap();
        ending.bytes[8..12].copy_from_slice(&0xA628_8DD1_u32.to_le_bytes());
        let mut event = NativeNode::effect(43).unwrap();
        event.bytes[4..8].copy_from_slice(&0x5EE2_66FC_u32.to_le_bytes());
        let program = Program {
            trigger: Trigger::Native,
            native_trigger: Some(trigger),
            native_removal: Some(ending),
            duration_ms: 0,
            cooldown_ms: 2_000,
            actions: vec![Action::Native { node: event }, Action::native(30).unwrap()],
            ..Program::default()
        };
        program.validate().unwrap();
        let mut out = Payload::new();
        let activation = out
            .native_condition(program.native_trigger.as_ref().unwrap(), 0)
            .unwrap();
        assert_eq!(
            &out.bytes[activation..activation + 12],
            &[0, 0, 0x80, 0x3F, 0xFF, 6, 0, 0, 3, 4, 0, 0]
        );
        out.nodes(0x20, CONDITION_ROWS, &[activation]);
        let effects = program
            .actions
            .iter()
            .rev()
            .map(|action| out.action(action, None).unwrap())
            .collect::<Vec<_>>();
        let publish = effects[1];
        assert_eq!(
            &out.bytes[publish..publish + 8],
            &[43, 0, 0, 0, 0xFC, 0x66, 0xE2, 0x5E]
        );
        let count = effects[0];
        assert_eq!(&out.bytes[count..count + 3], &[30, 1, 0]);
        out.nodes(0x38, EFFECT_ROWS, &effects);
        assert!(out.removal_and_rearm(&program).unwrap());
        assert_eq!(
            u64::from_le_bytes(out.bytes[0x88..0x90].try_into().unwrap()),
            1 << 6
        );
        assert_eq!(
            u64::from_le_bytes(out.bytes[0x90..0x98].try_into().unwrap()),
            1 << 29
        );
        // One retained action, no duration timer, one cooldown timer.
        assert_eq!(&out.bytes[0xCC..0xCE], &[2, 1]);
        let json = serde_json::to_string(&program).unwrap();
        assert!(json.contains("\"trigger\":\"native\""), "{json}");
        assert!(json.contains("\"bytes\":\"0x2B000000FC66E25E\""), "{json}");
        assert_eq!(serde_json::from_str::<Program>(&json).unwrap(), program);
    }

    #[test]
    fn extend_timers_needs_a_kill_trigger_to_nest() {
        let mut out = Payload::new();
        let error = out
            .action(
                &Action::ExtendTimers {
                    extend_ms: 5_000,
                    cap_ms: 5_000,
                },
                None,
            )
            .unwrap_err();
        assert!(error.contains("kill trigger"));
    }

    #[test]
    fn a_default_attach_node_matches_the_shape_the_compiler_always_wrote() {
        let node = attach_node(&Action::attach(Asset {
            graph: 0x80BC_5810,
            path: String::new(),
            values: Vec::new(),
        }));
        assert_eq!(&node[..4], &[1, 1, 1, 0]);
        assert_eq!(
            u32::from_le_bytes(node[0x10..0x14].try_into().unwrap()),
            0x80BC_5810
        );
        for offset in [0x18, 0x1C, 0x30] {
            assert_eq!(
                u32::from_le_bytes(node[offset..offset + 4].try_into().unwrap()),
                EMPTY_KEY,
                "+0x{offset:X}"
            );
        }
        assert!(node[0x20..0x30].iter().all(|byte| *byte == 0));
        assert!(node[0x34..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn attach_technical_fields_are_written_verbatim() {
        let node = attach_node(&Action::Attach {
            asset: Asset {
                graph: 0x80BC_5810,
                path: String::new(),
                values: Vec::new(),
            },
            mode: 3,
            keys: [0x4113_6E32, 0x95E7_400C],
            float_bits: [1.0_f32.to_bits(); 4],
        });
        assert_eq!(node[2], 3);
        assert_eq!(
            u32::from_le_bytes(node[0x18..0x1C].try_into().unwrap()),
            0x4113_6E32
        );
        assert_eq!(
            u32::from_le_bytes(node[0x1C..0x20].try_into().unwrap()),
            0x95E7_400C
        );
        for offset in [0x20, 0x24, 0x28, 0x2C] {
            assert_eq!(
                f32::from_le_bytes(node[offset..offset + 4].try_into().unwrap()),
                1.0
            );
        }
        assert_eq!(
            u32::from_le_bytes(node[0x30..0x34].try_into().unwrap()),
            EMPTY_KEY
        );
    }
}
