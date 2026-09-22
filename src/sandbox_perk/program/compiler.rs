//! Emits the mapped single-group, policy-zero action format from authored nodes.
use super::*;
use crate::package_runtime::reader::PackageManager;
use crate::{
    investment_schema::NESTED_ARRAY_TRAILER,
    package_payload::u32_at,
    sandbox_perk::projectile,
    weapon_entity::{WEAPON_ENTITY_CLASS, validate_weapon_entity},
};
use tiger_pkg::TagHash;

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
enum Activation<'a> {
    Kill {
        trigger: Trigger,
        labels: &'a [u32],
        mask: [u8; 40],
        chance: u16,
    },
    Native(&'a NativeNode),
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

pub(super) fn assemble(program: &Program, label_mask: LabelMask) -> Result<Compiled, String> {
    program.validate()?;
    assemble_records(program, label_mask)
}

/// Serialize an editable draft without reading packages or validating asset residency.
/// Symbolic label lists remain authoritative. The normal compiler rebuilds their masks
/// against the selected installation before any package is written.
pub fn draft(program: &Program) -> Result<NativeProgram, String> {
    program.validate_structure()?;
    if let Some(native) = &program.native {
        return Ok(native.clone());
    }
    let mask = program
        .trigger
        .is_event()
        .then(|| (trigger_labels(program.trigger), [0; 40]));
    let compiled = assemble_records(program, mask)?;
    let mut native = NativeProgram::read(&compiled.payload)?;
    for asset in program
        .assets()
        .filter(|asset| !matches!(asset.graph, 0 | u32::MAX))
    {
        if program
            .assets()
            .any(|other| other.graph == asset.graph && other != asset)
        {
            return Err("This effect has different edits to the same asset. Keep editing those actions separately.".into());
        }
        if let Some(target) = native
            .assets
            .iter_mut()
            .find(|target| target.graph == asset.graph)
        {
            *target = asset.clone();
        }
    }
    Ok(native)
}

fn assemble_records(program: &Program, label_mask: LabelMask) -> Result<Compiled, String> {
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
    // The typed trigger is the first condition. Alternatives follow in their stock order.
    let mut activations = vec![activation];
    for node in &program.alternative_triggers {
        activations.push(out.native_condition(node, 0)?);
    }
    out.nodes(0x20, CONDITION_ROWS, &activations);
    let nested = label_mask
        .map(|(labels, mask)| Activation::Kill {
            trigger: program.trigger,
            labels,
            mask,
            chance: program.chance_permyriad,
        })
        .or_else(|| {
            program
                .native_trigger
                .as_ref()
                .filter(|node| program.trigger == Trigger::Native && node.kind == 2)
                .map(Activation::Native)
        });
    let mut effects = Vec::new();
    // The native initial dispatcher walks from last to first. Preserve authored execution order.
    for action in program.actions.iter().rev() {
        effects.push(out.action(action, nested.as_ref())?);
    }
    out.nodes(0x38, EFFECT_ROWS, &effects);
    out.removal_and_rearm(program)?;
    out.auxiliary(&program.auxiliary);
    out.policy(program.policy.as_ref());
    out.additional_groups(&program.additional_groups)?;
    out.u64(0, out.bytes.len() as u64);
    metadata::rebuild(&mut out.bytes)?;
    // Complex native graphs can carry state below their root node. Reserve an upper
    // bound for each node instead of reusing the scalar-only reservation calculation.
    let complex = program.native_nodes().any(|(condition, node)| {
        use crate::sandbox_perk::action::layout;
        if condition {
            layout::condition_layout(node.kind).is_none()
        } else {
            layout::effect_layout(node.kind).is_none()
        }
    });
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
        | Action::AdjustComponent { .. }
        | Action::UpdateAccumulator { .. }
        | Action::AbilityProperty { .. }
        | Action::TransmatContext { .. }
        | Action::OverrideHostKey { .. }
        | Action::SetDamageType { .. }
        | Action::WeaponReferenceCount { .. }
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

/// What preparing native nodes needs from an installation: the label registry, read once,
/// and proof that every resource a node references is live. Compilation uses the package
/// manager. Tests substitute a synthetic registry and a fixed set of live resources so the
/// traversal itself is checked without game packages.
pub(super) trait NativeResolver {
    fn registry(&mut self) -> Result<&[u8], String>;
    fn validate_resources(
        &self,
        graph: &crate::sandbox_perk::action::native::Graph,
    ) -> Result<(), String>;
}

struct Installed<'a> {
    manager: &'a PackageManager,
    registry: Option<Vec<u8>>,
}

impl NativeResolver for Installed<'_> {
    fn registry(&mut self) -> Result<&[u8], String> {
        if self.registry.is_none() {
            self.registry = Some(
                self.manager
                    .read_tag(TagHash(LABEL_GLOBALS))
                    .map_err(|error| format!("Could not read label globals: {error}"))?,
            );
        }
        Ok(self.registry.as_deref().expect("loaded registry"))
    }

    fn validate_resources(
        &self,
        graph: &crate::sandbox_perk::action::native::Graph,
    ) -> Result<(), String> {
        validate_native_resources(self.manager, graph)
    }
}

fn prepare_native(manager: &PackageManager, program: &mut Program) -> Result<(), String> {
    let mut resolver = Installed {
        manager,
        registry: None,
    };
    prepare_native_nodes(program, &mut resolver)
}

/// Compiles the label masks of every native node against the current registry and proves
/// the resources it references are live, in every position the compiler emits verbatim:
/// trigger, ending, rearm, their alternatives, native actions and every further group. The
/// walk is `Program::native_nodes_mut`, so a node position the compiler emits cannot skip
/// this pass.
pub(super) fn prepare_native_nodes(
    program: &mut Program,
    resolver: &mut impl NativeResolver,
) -> Result<(), String> {
    use crate::sandbox_perk::action::native::{Graph, labels, schema};
    for (condition, node) in program.native_nodes_mut() {
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
            labels::compile(&mut graph, resolver.registry()?)?;
        }
        resolver.validate_resources(&graph)?;
        node.bytes = graph.emit()?;
    }
    Ok(())
}

/// Every resource tag a native node's records reference, with the class of the record.
/// Empty lanes are skipped.
pub(super) fn referenced_resources(
    graph: &crate::sandbox_perk::action::native::Graph,
) -> Result<Vec<(u32, u32)>, String> {
    use crate::sandbox_perk::action::native::schema;
    let mut result = Vec::new();
    for block in graph.blocks.iter().filter(|b| b.class != 0) {
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
                result.push((block.class, tag));
            }
        }
    }
    Ok(result)
}

fn validate_native_resources(
    manager: &PackageManager,
    graph: &crate::sandbox_perk::action::native::Graph,
) -> Result<(), String> {
    for block in graph.blocks.iter().filter(|b| b.class != 0) {
        validate_native_entity(manager, block)?;
    }
    for (_, tag) in referenced_resources(graph)? {
        let entry = manager
            .get_entry(TagHash(tag))
            .ok_or_else(|| format!("Native resource 0x{tag:08X} is missing."))?;
        if entry.reference == WEAPON_ENTITY_CLASS {
            projectile::residency::inspect(manager, tag)?;
        }
    }
    Ok(())
}

fn validate_native_entity(
    manager: &PackageManager,
    block: &crate::sandbox_perk::action::native::Block,
) -> Result<(), String> {
    let Some(tag) = super::native::entity_reference(block.class, &block.bytes)? else {
        return Ok(());
    };
    let tag = TagHash(tag);
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
    let labels = trigger_labels(trigger);
    let registry = manager
        .read_tag(TagHash(LABEL_GLOBALS))
        .map_err(|error| format!("Could not read label globals: {error}"))?;
    Ok(Some((labels, compile_labels(&registry, labels)?)))
}

fn trigger_labels(trigger: Trigger) -> &'static [u32] {
    match trigger {
        Trigger::PrecisionKill => &[0x962E_A19B],
        Trigger::MeleeKill => &[0xBF39_E12B, 0xE175_76C9, 0x5D3A_7C84],
        Trigger::GrenadeKill => &[0xC20D_D425],
        _ => &[],
    }
}

pub(crate) fn compile_labels(registry: &[u8], labels: &[u32]) -> Result<[u8; 40], String> {
    crate::package_runtime::labels::Registry::read(registry)?.mask(labels)
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
    /// Root records preserved from a stock action. Each is a self-contained leaf, so it is
    /// copied as it was read and listed at the root in the same order.
    /// The execution policy preserved from a stock action. `Payload::new` wrote the default
    /// policy, so only a selected one changes the root.
    fn policy(&mut self, policy: Option<&Policy>) {
        use crate::sandbox_perk::action::{POLICY_CONFIGURATION, POLICY_SELECTOR, ROOT_KEY};
        let Some(policy) = policy else {
            return;
        };
        self.bytes[POLICY_SELECTOR] = policy.selector;
        self.bytes[POLICY_SELECTOR + 1] = policy.modifier;
        self.u32(ROOT_KEY, policy.key);
        if let Some(record) = &policy.configuration {
            let at = self.node(record.class, record.bytes.len());
            self.bytes[at..at + record.bytes.len()].copy_from_slice(&record.bytes);
            self.pointer(POLICY_CONFIGURATION, at);
        }
    }
    /// Further programs preserved from a stock action, each list emitted in its native order,
    /// with one zeroed routing record per group for `metadata::rebuild` to fill.
    fn additional_groups(&mut self, groups: &[NativeGroup]) -> Result<(), String> {
        use crate::sandbox_perk::action::{
            ADDITIONAL_GROUPS, GROUP_ACTIVATION, GROUP_EFFECTS, GROUP_REARM, GROUP_REMOVAL,
            GROUP_ROUTING, GROUP_ROUTING_CLASS, GROUP_ROUTING_SIZE, GROUP_ROW_CLASS, GROUP_SIZE,
        };
        if groups.is_empty() {
            return Ok(());
        }
        let rows = self.rows(ADDITIONAL_GROUPS, GROUP_ROW_CLASS, groups.len(), GROUP_SIZE);
        for (index, group) in groups.iter().enumerate() {
            let base = rows + index * GROUP_SIZE;
            for (offset, list) in [
                (GROUP_ACTIVATION, &group.activation),
                (GROUP_REMOVAL, &group.removal),
                (GROUP_REARM, &group.rearm),
            ] {
                if list.is_empty() {
                    continue;
                }
                let mut nodes = Vec::with_capacity(list.len());
                for node in list {
                    nodes.push(self.native_condition(node, 0)?);
                }
                self.nodes(base + offset, CONDITION_ROWS, &nodes);
            }
            if !group.effects.is_empty() {
                let mut nodes = Vec::with_capacity(group.effects.len());
                for node in &group.effects {
                    let action = Action::Native { node: node.clone() };
                    nodes.push(self.action(&action, None)?);
                }
                self.nodes(base + GROUP_EFFECTS, EFFECT_ROWS, &nodes);
            }
        }
        self.rows(
            GROUP_ROUTING,
            GROUP_ROUTING_CLASS,
            groups.len(),
            GROUP_ROUTING_SIZE,
        );
        Ok(())
    }
    fn auxiliary(&mut self, records: &[NativeRecord]) {
        if records.is_empty() {
            return;
        }
        let nodes = records
            .iter()
            .map(|record| {
                let at = self.node(record.class, record.bytes.len());
                self.bytes[at..at + record.bytes.len()].copy_from_slice(&record.bytes);
                at
            })
            .collect::<Vec<_>>();
        use crate::sandbox_perk::action::{AUXILIARY_RECORDS, AUXILIARY_ROW_CLASS};
        self.nodes(AUXILIARY_RECORDS, AUXILIARY_ROW_CLASS, &nodes);
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
        if let Some(node) = &program.native_removal {
            return Ok(Some((self.native_condition(node, 1)?, node.kind)));
        }
        Ok(match program.trigger {
            Trigger::Always | Trigger::Native => {
                if let Some(key) = program.removal_key {
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
        let mut removals = removal.iter().map(|(at, _)| *at).collect::<Vec<_>>();
        for node in &program.alternative_removals {
            removals.push(self.native_condition(node, 1)?);
        }
        if !removals.is_empty() {
            self.nodes(0x48, CONDITION_ROWS, &removals);
        }
        let has_cooldown = program.trigger.supports_cooldown()
            && program.cooldown_ms != 0
            && program.native_rearm.is_none();
        let ordinal = if removal.is_some() { 2 } else { 1 };
        let mut rearms = Vec::new();
        if let Some(node) = &program.native_rearm {
            rearms.push(self.native_condition(node, ordinal)?);
        } else if has_cooldown {
            rearms.push(self.timer(program.cooldown_ms, ordinal));
        }
        for node in &program.alternative_rearms {
            rearms.push(self.native_condition(node, ordinal)?);
        }
        if !rearms.is_empty() {
            self.nodes(0x58, CONDITION_ROWS, &rearms);
        }
        self.root_state(program, removal.map(|(_, kind)| kind), has_cooldown)?;
        Ok(has_cooldown)
    }

    /// Writes the compiled event masks and the retained-state and timer budgets.
    ///
    /// Both budgets are one byte. The reservation pass in `assemble` refuses a program that
    /// needs more slots than a byte holds, by name, so counting them here refuses it the same
    /// way rather than wrapping into a budget smaller than the state the action keeps.
    fn root_state(
        &mut self,
        program: &Program,
        removal_kind: Option<u8>,
        has_cooldown: bool,
    ) -> Result<(), String> {
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
        let group_retained = program
            .additional_groups
            .iter()
            .flat_map(|group| &group.effects)
            .filter(|node| node.bytes.get(1).is_some_and(|byte| *byte != 0))
            .count();
        let retained = 1
            + program
                .actions
                .iter()
                .filter(|action| action.retained())
                .count()
            + group_retained;
        self.bytes[0xCC] = u8::try_from(retained)
            .map_err(|_| "The program exceeds its retained-state reservation limit.")?;
        let alternative_timers = program
            .alternative_triggers
            .iter()
            .chain(&program.alternative_removals)
            .chain(&program.native_rearm)
            .chain(&program.alternative_rearms)
            .chain(
                program
                    .additional_groups
                    .iter()
                    .flat_map(NativeGroup::conditions),
            )
            .filter(|node| node.kind == 1)
            .count();
        let timers = usize::from(active_kind == 1)
            + usize::from(removal_kind == Some(1))
            + usize::from(has_cooldown)
            + alternative_timers;
        self.bytes[0xCD] =
            u8::try_from(timers).map_err(|_| "The program exceeds its timer reservation limit.")?;
        Ok(())
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
            Action::AdjustComponent { .. } => (0x8080_3E4D, 8, 80),
            Action::UpdateAccumulator { .. } => (0x8080_3E2F, 42, 8),
            Action::AbilityProperty { .. } => (0x8080_3E1D, 7, 12),
            Action::TransmatContext { .. } => (0x8080_3E2E, 47, 8),
            Action::OverrideHostKey { .. } => (0x8080_3E1C, 35, 12),
            Action::SetDamageType { .. } => (0x8080_3E41, 6, 4),
            Action::WeaponReferenceCount { .. } => (0x8080_3E0D, 30, 3),
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
                let condition = match nested {
                    Activation::Kill {
                        trigger,
                        labels,
                        mask,
                        chance,
                    } => self.kill_condition(*trigger, labels, *mask, *chance, 0xFF),
                    Activation::Native(node) => self.native_condition(node, 0xFF)?,
                };
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
            Action::AdjustComponent {
                target,
                flag,
                option,
                scale_bits,
                limit_bits,
                value_bits,
                input,
            } => {
                self.bytes[at + 2] = *target;
                self.bytes[at + 3] = *flag;
                self.bytes[at + 4] = *option;
                self.u32(at + 8, *scale_bits);
                self.u32(at + 0x0C, *limit_bits);
                self.constant_program(at + 0x18, *value_bits);
                // Two words every one of the 179 stock nodes sets to one. Their role is not
                // mapped, so they are written as the stock template does.
                self.u32(at + 0x38, 1);
                self.u32(at + 0x40, 1);
                self.bytes[at + 0x48] = *input;
            }
            Action::UpdateAccumulator { mode, value_bits } => {
                self.bytes[at + 2] = *mode;
                self.u32(at + 4, *value_bits);
            }
            Action::AbilityProperty {
                target,
                key,
                option,
            } => {
                self.bytes[at + 2] = *target;
                self.u32(at + 4, *key);
                self.bytes[at + 8] = *option;
            }
            Action::TransmatContext { key } => self.u32(at + 4, *key),
            Action::OverrideHostKey {
                target,
                interface,
                key,
                apply_to_player,
            } => {
                self.bytes[at + 2] = *target;
                self.bytes[at + 3] = *interface;
                self.u32(at + 4, *key);
                self.bytes[at + 8] = u8::from(*apply_to_player);
            }
            Action::SetDamageType {
                mode,
                keep_after_removal,
            } => {
                self.bytes[at + 2] = *mode;
                self.bytes[at + 3] = u8::from(*keep_after_removal);
            }
            Action::WeaponReferenceCount { selector } => self.bytes[at + 2] = *selector,
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
    use crate::package_payload::native_array_at;
    use crate::sandbox_perk::action::native::{Graph, labels};
    use crate::sandbox_perk::nodes;

    /// A synthetic installation: the fixture label registry and a fixed set of live
    /// resource tags. Nothing here reads a game package.
    struct Synthetic {
        registry: Vec<u8>,
        live: Vec<u32>,
    }

    impl Synthetic {
        /// The label globals are always live: every event condition refers to them.
        fn new(live: &[u32]) -> Self {
            let mut live = live.to_vec();
            live.push(LABEL_GLOBALS);
            Self {
                registry: crate::package_runtime::labels::fixture::registry(),
                live,
            }
        }
    }

    impl NativeResolver for Synthetic {
        fn registry(&mut self) -> Result<&[u8], String> {
            Ok(&self.registry)
        }

        fn validate_resources(&self, graph: &Graph) -> Result<(), String> {
            for (_, tag) in referenced_resources(graph)? {
                if !self.live.contains(&tag) {
                    return Err(format!("Native resource 0x{tag:08X} is missing."));
                }
            }
            Ok(())
        }
    }

    const KILL: u8 = 2;
    const PRECISION: u32 = 0x962E_A19B;
    const DEAD_RESOURCE: u32 = 0x8ABC_0001;

    #[test]
    fn editable_draft_preserves_symbolic_filters_extra_groups_and_native_bits() {
        let mut node = NativeNode::effect(47).unwrap();
        node.bytes[4..8].copy_from_slice(&0xFEEDABCDu32.to_le_bytes());
        let program = Program {
            trigger: Trigger::PrecisionKill,
            actions: vec![
                Action::UpdateAccumulator {
                    mode: 0,
                    value_bits: 0x80000000,
                },
                Action::Native { node: node.clone() },
            ],
            additional_groups: vec![NativeGroup {
                effects: vec![node],
                ..NativeGroup::default()
            }],
            ..Program::default()
        };
        let mut native = draft(&program).unwrap();
        let registry = crate::package_runtime::labels::fixture::registry();
        labels::compile(&mut native.graph, &registry).unwrap();
        let expected = assemble(
            &program,
            Some((
                trigger_labels(program.trigger),
                compile_labels(&registry, &[PRECISION]).unwrap(),
            )),
        )
        .unwrap();
        assert!(
            super::super::decompile::native_fidelity(
                &expected.payload,
                &native.graph.emit().unwrap()
            )
            .unwrap()
            .is_empty()
        );
        assert_eq!(native.assets.len(), 0);
    }

    #[test]
    fn draft_keeps_component_overrides_and_refuses_to_merge_distinct_edits() {
        use crate::weapon_runtime::{
            WeaponRuntimeFieldLocator, WeaponRuntimeRootKind, WeaponRuntimeValue,
            WeaponRuntimeValueOverride,
        };
        let first = Asset {
            graph: 0x815282E1,
            path: "content/projectile.pattern.tft".into(),
            values: vec![WeaponRuntimeValueOverride {
                locator: WeaponRuntimeFieldLocator {
                    graph_tag: Some(0x815282E1),
                    binding_hash: 1,
                    resource_index: 0,
                    root: WeaponRuntimeRootKind::ComponentInstance,
                    root_schema: 0x80803B73,
                    path: vec![],
                    type_handle: 2,
                    value_offset: 0x144,
                    byte_size: 8,
                },
                value: WeaponRuntimeValue::Bytes(
                    [0x7FC12345_u32.to_le_bytes(), 0x80000000_u32.to_le_bytes()].concat(),
                ),
            }],
        };
        let program = Program {
            actions: vec![Action::Pattern {
                asset: first.clone(),
            }],
            ..Program::default()
        };
        assert_eq!(draft(&program).unwrap().assets, [first.clone()]);
        let mut conflicting = program;
        let mut second = first;
        second.values.clear();
        conflicting.actions.push(Action::attach(second));
        assert!(draft(&conflicting).is_err());
    }

    fn kill_class() -> u32 {
        nodes::condition(KILL).unwrap().class
    }

    /// A Kill Event whose first source label list names a precision kill while its compiled
    /// masks are still those of the template, exactly as a label edit leaves a node before
    /// it is compiled.
    fn kill_with_stale_masks() -> NativeNode {
        let mut node = NativeNode::condition(KILL).unwrap();
        let mut graph = Graph::read(&node.bytes, 0, kill_class()).unwrap();
        let (source, _) = labels::bindings(kill_class()).unwrap()[0];
        graph
            .create_target(0, source + 8, 0x808094B3, true)
            .unwrap();
        let rows = graph.blocks[0].links[&(source + 8)];
        graph.resize_array(rows, 1).unwrap();
        graph.blocks[rows].bytes[..4].copy_from_slice(&PRECISION.to_le_bytes());
        node.bytes = graph.emit().unwrap();
        node
    }

    /// A Kill Event whose resource lane names a tag no installation holds.
    fn kill_with_dead_resource() -> NativeNode {
        let mut node = NativeNode::condition(KILL).unwrap();
        node.bytes[80..84].copy_from_slice(&DEAD_RESOURCE.to_le_bytes());
        node
    }

    fn compiled_precision_mask(node: &NativeNode) -> [u8; 40] {
        let graph = Graph::read(&node.bytes, 0, kill_class()).unwrap();
        let (_, predicate) = labels::bindings(kill_class()).unwrap()[0];
        labels::effective(&graph, 0, predicate).unwrap()[0]
    }

    type Read = fn(&Program) -> &NativeNode;

    /// Every position that carries a verbatim condition, each holding one copy of the node,
    /// with a way to read that copy back after preparation. The primary trigger comes first
    /// and is the reference the other positions must match.
    fn condition_positions(node: &NativeNode) -> Vec<(String, Program, Read)> {
        let base = Program {
            trigger: Trigger::Always,
            duration_ms: 1000,
            actions: vec![Action::native(43).unwrap()],
            ..Program::default()
        };
        let group = |group: NativeGroup| Program {
            additional_groups: vec![group],
            ..base.clone()
        };
        let positions: Vec<(&str, Program, Read)> = vec![
            (
                "native_trigger",
                Program {
                    trigger: Trigger::Native,
                    native_trigger: Some(node.clone()),
                    ..base.clone()
                },
                |program| program.native_trigger.as_ref().unwrap(),
            ),
            (
                "alternative_triggers",
                Program {
                    alternative_triggers: vec![node.clone()],
                    ..base.clone()
                },
                |program| &program.alternative_triggers[0],
            ),
            (
                "native_removal",
                Program {
                    native_removal: Some(node.clone()),
                    ..base.clone()
                },
                |program| program.native_removal.as_ref().unwrap(),
            ),
            (
                "alternative_removals",
                Program {
                    alternative_removals: vec![node.clone()],
                    ..base.clone()
                },
                |program| &program.alternative_removals[0],
            ),
            (
                "native_rearm",
                Program {
                    native_rearm: Some(node.clone()),
                    ..base.clone()
                },
                |program| program.native_rearm.as_ref().unwrap(),
            ),
            (
                "alternative_rearms",
                Program {
                    alternative_rearms: vec![node.clone()],
                    ..base.clone()
                },
                |program| &program.alternative_rearms[0],
            ),
            (
                "additional_groups.activation",
                group(NativeGroup {
                    activation: vec![node.clone()],
                    ..NativeGroup::default()
                }),
                |program| &program.additional_groups[0].activation[0],
            ),
            (
                "additional_groups.removal",
                group(NativeGroup {
                    removal: vec![node.clone()],
                    ..NativeGroup::default()
                }),
                |program| &program.additional_groups[0].removal[0],
            ),
            (
                "additional_groups.rearm",
                group(NativeGroup {
                    rearm: vec![node.clone()],
                    ..NativeGroup::default()
                }),
                |program| &program.additional_groups[0].rearm[0],
            ),
        ];
        positions
            .into_iter()
            .map(|(name, program, read)| (name.to_string(), program, read))
            .collect()
    }

    #[test]
    fn label_edits_compile_in_every_native_condition_position() {
        let node = kill_with_stale_masks();
        assert_eq!(
            compiled_precision_mask(&node),
            [0; 40],
            "fixture masks are stale"
        );
        let registry = crate::package_runtime::labels::fixture::registry();
        let expected = compile_labels(&registry, &[PRECISION]).unwrap();
        assert_ne!(expected, [0; 40]);
        let mut reference = None;
        for (position, mut program, read) in condition_positions(&node) {
            let mut resolver = Synthetic::new(&[]);
            prepare_native_nodes(&mut program, &mut resolver)
                .unwrap_or_else(|error| panic!("{position}: {error}"));
            let prepared = read(&program);
            assert_eq!(
                compiled_precision_mask(prepared),
                expected,
                "{position} kept its stale masks"
            );
            match &reference {
                None => reference = Some(prepared.bytes.clone()),
                Some(bytes) => assert_eq!(
                    &prepared.bytes, bytes,
                    "{position} compiled differently from the primary trigger"
                ),
            }
        }
    }

    #[test]
    fn missing_resources_are_rejected_in_every_native_condition_position() {
        let node = kill_with_dead_resource();
        let graph = Graph::read(&node.bytes, 0, kill_class()).unwrap();
        assert_eq!(
            referenced_resources(&graph).unwrap(),
            vec![(kill_class(), DEAD_RESOURCE), (kill_class(), LABEL_GLOBALS)]
        );
        for (position, program, _) in condition_positions(&node) {
            let mut missing = Synthetic::new(&[]);
            let error =
                prepare_native_nodes(&mut program.clone(), &mut missing).expect_err(&position);
            assert!(error.contains("0x8ABC0001"), "{position}: {error}");
            let mut live = Synthetic::new(&[DEAD_RESOURCE]);
            prepare_native_nodes(&mut program.clone(), &mut live)
                .unwrap_or_else(|error| panic!("{position} with the resource live: {error}"));
        }
    }

    #[test]
    fn missing_resources_are_rejected_in_group_effects_like_native_actions() {
        let mut node = NativeNode::effect(13).unwrap();
        node.bytes[16..20].copy_from_slice(&DEAD_RESOURCE.to_le_bytes());
        let graph = Graph::read(&node.bytes, 0, nodes::effect(13).unwrap().class).unwrap();
        assert!(
            referenced_resources(&graph)
                .unwrap()
                .iter()
                .any(|(_, tag)| *tag == DEAD_RESOURCE)
        );
        let as_action = Program {
            trigger: Trigger::Always,
            actions: vec![Action::Native { node: node.clone() }],
            ..Program::default()
        };
        let as_group_effect = Program {
            trigger: Trigger::Always,
            actions: vec![Action::native(43).unwrap()],
            additional_groups: vec![NativeGroup {
                effects: vec![node],
                ..NativeGroup::default()
            }],
            ..Program::default()
        };
        for (position, program) in [("actions", as_action), ("group effects", as_group_effect)] {
            let error = prepare_native_nodes(&mut program.clone(), &mut Synthetic::new(&[]))
                .expect_err(position);
            assert!(error.contains("0x8ABC0001"), "{position}: {error}");
            prepare_native_nodes(&mut program.clone(), &mut Synthetic::new(&[DEAD_RESOURCE]))
                .unwrap_or_else(|error| panic!("{position}: {error}"));
        }
    }

    #[test]
    fn native_nodes_reach_every_verbatim_position() {
        let condition = NativeNode::condition(KILL).unwrap();
        let effect = NativeNode::effect(43).unwrap();
        let program = Program {
            trigger: Trigger::Native,
            native_trigger: Some(condition.clone()),
            native_removal: Some(condition.clone()),
            native_rearm: Some(condition.clone()),
            alternative_triggers: vec![condition.clone()],
            alternative_removals: vec![condition.clone()],
            alternative_rearms: vec![condition.clone()],
            actions: vec![Action::Native {
                node: effect.clone(),
            }],
            additional_groups: vec![NativeGroup {
                activation: vec![condition.clone()],
                effects: vec![effect.clone()],
                removal: vec![condition.clone()],
                rearm: vec![condition.clone()],
            }],
            ..Program::default()
        };
        let (conditions, effects): (Vec<_>, Vec<_>) = program
            .native_nodes()
            .partition(|(condition, _)| *condition);
        assert_eq!(conditions.len(), 9);
        assert_eq!(effects.len(), 2);
        assert!(conditions.iter().all(|(_, node)| **node == condition));
        assert!(effects.iter().all(|(_, node)| **node == effect));
    }

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
    fn explicit_end_conditions_override_trigger_defaults_and_rebuild_event_masks() {
        for trigger in [Trigger::Drawn, Trigger::Equipped, Trigger::WeaponKill] {
            let mut ending = NativeNode::condition(1).unwrap();
            ending.bytes[8..12].copy_from_slice(&2.75f32.to_le_bytes());
            let program = Program {
                trigger,
                native_removal: Some(ending),
                actions: vec![Action::add_rounds(1)],
                ..Program::default()
            };
            program.validate_structure().unwrap();
            let mut out = Payload::new();
            let (at, kind) = out.removal(&program).unwrap().unwrap();
            assert_eq!(kind, 1);
            assert_eq!(&out.bytes[at + 8..at + 12], &2.75f32.to_le_bytes());
            out.removal_and_rearm(&program).unwrap();
            assert_eq!(
                u64::from_le_bytes(out.bytes[0x90..0x98].try_into().unwrap()),
                1 << 1
            );
        }
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

    /// The retained byte belongs to the effect kind, not to the authored action: every stock
    /// node of a kind agrees on it. The compiler writes it from `Action::retained`, so each
    /// typed action has to agree with the captured stock template of the kind it compiles to.
    /// A disagreement makes the compiled node unlike every stock one, and makes the decompile
    /// that reproduces the kind refuse every stock node of it, which is how `TransmatContext`
    /// became unreachable while still compiling.
    #[test]
    fn every_typed_action_writes_the_stock_retained_byte() {
        let asset = || Asset {
            graph: 0x80BC_2F21,
            path: String::new(),
            values: Vec::new(),
        };
        let kill = Activation::Kill {
            trigger: Trigger::WeaponKill,
            labels: &[],
            mask: [0; 40],
            chance: 10_000,
        };
        let typed = [
            Action::Spawn {
                asset: asset(),
                position: Position::default(),
            },
            Action::attach(asset()),
            Action::Pattern { asset: asset() },
            Action::ExtendTimers {
                extend_ms: 5_000,
                cap_ms: 5_000,
            },
            Action::property(0x5EE2_66FC),
            Action::adjust_component(0),
            Action::update_accumulator(1.0),
            Action::ability_property(0),
            Action::transmat_context(0x1234_5678),
            Action::override_host_key(0x1234_5678),
            Action::set_damage_type(1),
            Action::weapon_reference_count(0),
            Action::add_rounds(1),
            Action::add_fraction(0.5),
        ];
        for action in typed {
            let mut out = Payload::new();
            let at = out.action(&action, Some(&kill)).unwrap();
            let kind = out.bytes[at];
            let stock = crate::sandbox_perk::action::native::template(false, kind)
                .and_then(|template| template.get(1).copied())
                .unwrap_or_else(|| panic!("{} has no stock template", action.label()));
            assert_eq!(
                out.bytes[at + 1],
                stock,
                "{} (effect kind {kind}) writes a retained byte no stock node of the kind carries",
                action.label()
            );
        }
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
    fn recovered_kill_triggers_preserve_filters_and_chance_in_timer_extensions() {
        use crate::sandbox_perk::action::{self, native::Graph};
        let mut source = Payload::new();
        let at = source.kill_condition(Trigger::PrecisionKill, &[0x962E_A19B], [0; 40], 3750, 0);
        // Retain an additional opaque native requirement and an untouched float lane.
        source.bytes[at + 0x140] = 1;
        source.u32(at + 0x154, (-0.0f32).to_bits());
        let bytes = Graph::read(&source.bytes, at, 0x80803DE7)
            .unwrap()
            .emit()
            .unwrap();
        let program = Program {
            trigger: Trigger::Native,
            native_trigger: Some(NativeNode {
                kind: 2,
                bytes: bytes.clone(),
            }),
            actions: vec![
                Action::ExtendTimers {
                    extend_ms: 3000,
                    cap_ms: 7000,
                },
                Action::Spawn {
                    asset: Asset {
                        graph: 1,
                        ..Default::default()
                    },
                    position: Position::Event,
                },
            ],
            ..Default::default()
        };
        let compiled = assemble(&program, None).unwrap();
        let decoded = action::decode(&compiled.payload).unwrap();
        let kills = decoded
            .conditions()
            .into_iter()
            .filter(|condition| condition.kind == 2)
            .collect::<Vec<_>>();
        assert_eq!(kills.len(), 2);
        for condition in kills {
            let mut actual = condition.native.clone();
            actual[7] = bytes[7]; // Only the compiled ordinal belongs to the receiving program.
            assert_eq!(actual, bytes);
        }
        let spawn = decoded.effects().find(|effect| effect.kind == 3).unwrap();
        assert_eq!(spawn.native[4], 1);
        let mut no_kill = program;
        no_kill.native_trigger = NativeNode::condition(6);
        assert!(no_kill.validate().is_err());
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
