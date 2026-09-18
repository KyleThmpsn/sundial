//! Reads a native perk action resource into the structure Parhelion explains and edits.
//!
//! The layout below was recovered from the supported client and checked against every
//! action resource in the installed packages. A decoded field records what the native
//! structure stores. It is not a claim that the gameplay meaning of that field is known.
//! `nodes` carries the per kind roles and their recorded limits.

use crate::package_payload::{bytes_at, i64_at, native_array_at, relative_offset, u32_at, u64_at};

use super::nodes::{self, Support};

mod facts;
pub use facts::{Fact, FactValue, label_name};
mod fields;
pub use fields::{
    ADDED_LABELS, CONSTANT_VALUE, CREATE_ENTITY_FLOAT_LABELS, CREATE_ENTITY_KEY_LABELS,
    CREATE_ENTITY_MODE_LABEL, EXTEND_TIMERS_MASK_LABEL, NAMED_PROPERTY_LABELS, NamedPropertyLabels,
    PROGRAM_WORDS, REQUIRED_LABELS, RESERVE_TRANSFER_PROGRAMS, constant_program_value,
};
#[cfg(test)]
pub(crate) mod fixtures;
pub mod layout;
pub mod native;
mod roles;
mod summary;
pub use roles::{ability_slot, component_target};
pub use summary::{ActionSummary, GroupSummary, SummaryLine};
#[cfg(test)]
mod tests;

/// Native class of an action root.
pub const ACTION_ROOT_CLASS: u32 = 0x8080_40B5;
/// Row class of a condition pointer list.
pub const CONDITION_ROW_CLASS: u32 = 0x8080_40BA;
/// Row class of an effect pointer list.
pub const EFFECT_ROW_CLASS: u32 = 0x8080_40AC;
/// Row class of one `All Subgroups` subgroup record.
pub const SUBGROUP_ROW_CLASS: u32 = 0x8080_3E06;
/// Row class of one source label entry.
pub const LABEL_ROW_CLASS: u32 = 0x8080_94B3;
/// Row class of the auxiliary record pointer list at the action root.
pub const AUXILIARY_ROW_CLASS: u32 = 0x8080_4083;

pub(crate) const GROUP_SIZE: usize = 0x48;
pub(crate) const PRIMARY_GROUP: usize = 0x20;
pub(crate) const ADDITIONAL_GROUPS: usize = 0x68;
/// Row class of the additional group array, and of the compiled routing records beside it.
pub(crate) const GROUP_ROW_CLASS: u32 = 0x8080_407D;
pub(crate) const GROUP_ROUTING_CLASS: u32 = 0x8080_407B;
pub(crate) const GROUP_ROUTING: usize = 0xA8;
pub(crate) const GROUP_ROUTING_SIZE: usize = 32;
pub(crate) const AUXILIARY_RECORDS: usize = 0x10;
pub(crate) const POLICY_CONFIGURATION: usize = 0x78;
pub(crate) const ROOT_KEY: usize = 0x80;
const ACTIVATION_EVENT_MASK: usize = 0x88;
const REMOVAL_EVENT_MASK: usize = 0x90;
const REARM_EVENT_MASK: usize = 0x98;
pub(crate) const POLICY_SELECTOR: usize = 0xB8;
const RETAINED_STATE_BUDGET: usize = 0xCC;
const TIMER_BUDGET: usize = 0xCD;
const ROOT_SIZE: usize = 0xD0;

pub(crate) const GROUP_ACTIVATION: usize = 0x00;
pub(crate) const GROUP_EFFECTS: usize = 0x18;
pub(crate) const GROUP_REMOVAL: usize = 0x28;
pub(crate) const GROUP_REARM: usize = 0x38;

const SUBGROUP_ROW_SIZE: usize = 0x20;
const SUBGROUP_HOLD: usize = 0x00;
const SUBGROUP_CONDITIONS: usize = 0x08;
const LABEL_ROW_SIZE: usize = 0x18;

const ACCUMULATOR_CHILDREN: usize = 0x08;
const ACCUMULATOR_ROW_CLASS: u32 = 0x8080_3E32;
const ACCUMULATOR_ROW_SIZE: usize = 0x20;
const NESTED_PREDICATE_POINTER: usize = 0x100;
const TIMER_EXTENSION_CONDITIONS: usize = 0x10;

/// Guards against a malformed payload driving unbounded recursion.
const MAX_NESTING: usize = 8;
/// Guards against a malformed count allocating an implausible list.
const MAX_LIST: usize = 256;

/// Literal probability marker stored in the condition header.
const LITERAL_PROBABILITY: u8 = 0xFF;

/// Which list of an action group a condition belongs to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConditionRole {
    /// Conditions that start the action.
    Activation,
    /// Conditions that end the action.
    Removal,
    /// Conditions that allow the action to start again.
    Rearm,
}

impl ConditionRole {
    /// Title-case heading for this list.
    #[must_use]
    pub const fn heading(self) -> &'static str {
        match self {
            Self::Activation => "Starts When",
            Self::Removal => "Ends When",
            Self::Rearm => "Ready Again When",
        }
    }
}

/// How a condition decides its probability roll.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Probability {
    /// The stored literal always passes.
    Always,
    /// The stored literal is rolled against a random number.
    Literal(f32),
    /// A native stat supplies the probability through the selector byte.
    NativeStat(u8),
}

impl Probability {
    fn read(payload: &[u8], node: usize) -> Result<Self, String> {
        let value = f32::from_bits(u32_at(payload, node)?);
        let selector = bytes_at::<1>(payload, node + 4)?[0];
        if selector != LITERAL_PROBABILITY {
            return Ok(Self::NativeStat(selector));
        }
        // The common checker skips the roll at exactly one and fails at or below zero.
        if value == 1.0 {
            Ok(Self::Always)
        } else {
            Ok(Self::Literal(value))
        }
    }

    /// Sentence fragment for a reader, or `None` when the condition always rolls through.
    #[must_use]
    pub fn describe(self) -> Option<String> {
        match self {
            Self::Always => None,
            Self::Literal(value) => Some(format!("{}% chance", facts::trim_number(value * 100.0))),
            Self::NativeStat(selector) => Some(format!("chance from native stat {selector}")),
        }
    }
}

/// What one accumulator child row contributes to the stored value.
///
/// The operation selectors add, replace or multiply on the inspected evaluator path.
/// Their exact numbering is recorded as stored rather than renamed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AccumulatorRow {
    /// Operation applied when the child condition passes.
    pub success_operation: u8,
    pub success_uses_event_value: bool,
    pub hold_seconds: f32,
    /// Value used by the success operation.
    pub success_value: f32,
    /// Operation applied when the child condition fails.
    pub failure_operation: u8,
    pub failure_uses_event_value: bool,
    /// Value used by the failure operation.
    pub failure_value: f32,
}

/// One subgroup of an `All Subgroups` condition.
#[derive(Clone, Debug, PartialEq)]
pub struct Subgroup {
    /// Hold value stored when the subgroup passes. A positive hold can keep it satisfied.
    pub hold: f32,
    /// Alternatives inside this subgroup. The first one that passes satisfies it.
    pub conditions: Vec<DecodedCondition>,
}

/// One decoded condition node.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedCondition {
    /// Complete relocatable data, including every declared nested allocation.
    pub native: Vec<u8>,
    /// Byte offset of the node inside the action payload.
    pub offset: usize,
    /// Native structure class.
    pub class: u32,
    /// Dispatch kind.
    pub kind: u8,
    /// Compiled evaluation ordinal, or `0xFF` inside a timer extension effect.
    pub ordinal: u8,
    /// Whether the evaluator supplies linked condition state to this node.
    pub linked_state: bool,
    /// Probability contract applied before the kind specific checker.
    pub probability: Probability,
    /// Mapped fields of this kind.
    pub facts: Vec<Fact>,
    /// Child conditions owned by an accumulator or a nested predicate.
    pub children: Vec<DecodedCondition>,
    /// Accumulator contribution of this node, set on the children of an accumulator.
    pub accumulator_row: Option<AccumulatorRow>,
    /// Subgroups owned by an `All Subgroups` condition.
    pub subgroups: Vec<Subgroup>,
}

impl DecodedCondition {
    /// Source-backed gameplay description, also used by the complete program editor.
    pub fn description(&self) -> String {
        summary::describe_condition(self)
    }

    /// Catalog entry for this node, when the client registers the kind.
    #[must_use]
    pub fn catalog(&self) -> Option<&'static nodes::NodeKind> {
        nodes::condition(self.kind)
    }

    /// Title-case display name.
    #[must_use]
    pub fn name(&self) -> String {
        nodes::condition_name(self.kind)
    }
}

/// One decoded effect node.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedEffect {
    /// Complete relocatable data, including every declared nested allocation.
    pub native: Vec<u8>,
    /// Byte offset of the node inside the action payload.
    pub offset: usize,
    /// Native structure class.
    pub class: u32,
    /// Dispatch kind.
    pub kind: u8,
    /// Whether the caller supplies a retained effect-state handle.
    pub retained: bool,
    /// Mapped fields of this kind.
    pub facts: Vec<Fact>,
    /// Referenced entity graph or pattern, when this kind carries one.
    pub referenced_tag: Option<u32>,
    /// Native debug path stored beside the reference, when present.
    pub referenced_path: Option<String>,
    /// Conditions owned by a timer extension effect.
    pub conditions: Vec<DecodedCondition>,
}

impl DecodedEffect {
    /// Source-backed gameplay description, also used by the complete program editor.
    pub fn description(&self) -> String {
        summary::describe_effect(self)
    }

    /// Catalog entry for this node, when the client registers the kind.
    #[must_use]
    pub fn catalog(&self) -> Option<&'static nodes::NodeKind> {
        nodes::effect(self.kind)
    }

    /// Title-case display name.
    #[must_use]
    pub fn name(&self) -> String {
        nodes::effect_name(self.kind)
    }
}

/// One decoded action group.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedGroup {
    /// Byte offset of the 72-byte group record.
    pub offset: usize,
    /// Conditions that start the action. A flat list behaves as alternatives.
    pub activation: Vec<DecodedCondition>,
    /// Effects applied while the action is active, in authored order.
    pub effects: Vec<DecodedEffect>,
    /// Conditions that end the action.
    pub removal: Vec<DecodedCondition>,
    /// Conditions that allow the action to start again.
    pub rearm: Vec<DecodedCondition>,
}

impl DecodedGroup {
    /// The condition list for one role.
    #[must_use]
    pub fn conditions(&self, role: ConditionRole) -> &[DecodedCondition] {
        match role {
            ConditionRole::Activation => &self.activation,
            ConditionRole::Removal => &self.removal,
            ConditionRole::Rearm => &self.rearm,
        }
    }
}

/// A complete decoded action resource.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedAction {
    /// Declared payload size from the action root.
    pub byte_size: usize,
    /// Execution policy selected by the action root.
    pub policy: u8,
    /// The policy's configuration record, when the root links one.
    pub policy_configuration: Option<DecodedRecord>,
    /// Root byte +0xB9 beside the policy selector. Set in two stock actions, role unresolved.
    pub policy_modifier: u8,
    /// Root key at +0x80. The empty key in all but one stock action, role unresolved.
    pub root_key: u32,
    /// Retained effect-state slots reserved by the compiled action.
    pub retained_state_budget: u8,
    /// Timer slots reserved by the compiled action.
    pub timer_budget: u8,
    /// Compiled event mask gating the primary activation list.
    pub activation_event_mask: u64,
    /// Compiled event mask gating the primary removal list.
    pub removal_event_mask: u64,
    /// Compiled event mask gating the primary rearm list.
    pub rearm_event_mask: u64,
    /// Root records outside every group, in list order. Their role is not resolved.
    pub auxiliary: Vec<DecodedRecord>,
    /// The primary group first, then any additional groups.
    pub groups: Vec<DecodedGroup>,
}

/// One closed native record, carried by class and the bytes of its allocation graph.
///
/// Auxiliary records are self-contained leaves. A policy configuration may link a string,
/// which its bytes then include, so either kind can be preserved verbatim.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedRecord {
    pub class: u32,
    pub bytes: Vec<u8>,
}

impl DecodedAction {
    /// Every condition in the action, including nested children and subgroups.
    pub fn conditions(&self) -> Vec<&DecodedCondition> {
        let mut out = Vec::new();
        for group in &self.groups {
            for role in [
                ConditionRole::Activation,
                ConditionRole::Removal,
                ConditionRole::Rearm,
            ] {
                collect_conditions(group.conditions(role), &mut out);
            }
            for effect in &group.effects {
                collect_conditions(&effect.conditions, &mut out);
            }
        }
        out
    }

    /// Every effect in the action, in group order.
    pub fn effects(&self) -> impl Iterator<Item = &DecodedEffect> {
        self.groups.iter().flat_map(|group| group.effects.iter())
    }

    /// The weakest support level across the nodes and the supported program shape.
    /// Conversion fidelity must still be checked before claiming an exact rebuild.
    #[must_use]
    pub fn support(&self) -> Support {
        let conditions = self.conditions().into_iter().map(|node| {
            node.catalog()
                .map_or(Support::Unobserved, |entry| entry.support)
        });
        let effects = self.effects().map(|node| {
            node.catalog()
                .map_or(Support::Unobserved, |entry| entry.support)
        });
        let support = conditions.chain(effects).max().unwrap_or(Support::Readable);
        if support == Support::Authorable
            && super::program::decompile::decompile(self, "Custom Effect", |tag| tag).is_err()
        {
            Support::Readable
        } else {
            support
        }
    }

    /// Distinct entity graphs and patterns the action references.
    #[must_use]
    pub fn referenced_tags(&self) -> Vec<u32> {
        let mut tags = self
            .effects()
            .filter_map(|effect| effect.referenced_tag)
            .collect::<Vec<_>>();
        tags.sort_unstable();
        tags.dedup();
        tags
    }
}

fn collect_conditions<'a>(list: &'a [DecodedCondition], out: &mut Vec<&'a DecodedCondition>) {
    for condition in list {
        out.push(condition);
        collect_conditions(&condition.children, out);
        for subgroup in &condition.subgroups {
            collect_conditions(&subgroup.conditions, out);
        }
    }
}

/// Decodes one native action resource payload.
///
/// The payload must be a complete `0x808040B5` action resource as stored in a package.
pub fn decode(payload: &[u8]) -> Result<DecodedAction, String> {
    let byte_size = usize::try_from(u64_at(payload, 0)?)
        .map_err(|_| "Action size does not fit this platform")?;
    if byte_size != payload.len() {
        return Err(format!(
            "Action size field {byte_size} disagrees with payload size {}",
            payload.len()
        ));
    }
    if byte_size < ROOT_SIZE {
        return Err("Action payload is shorter than one action root".into());
    }
    let mut groups = vec![decode_group(payload, PRIMARY_GROUP)?];
    for offset in additional_groups(payload)? {
        groups.push(decode_group(payload, offset)?);
    }
    Ok(DecodedAction {
        byte_size,
        policy: bytes_at::<1>(payload, POLICY_SELECTOR)?[0],
        policy_configuration: policy_configuration(payload)?,
        policy_modifier: bytes_at::<1>(payload, POLICY_SELECTOR + 1)?[0],
        root_key: u32_at(payload, ROOT_KEY)?,
        retained_state_budget: bytes_at::<1>(payload, RETAINED_STATE_BUDGET)?[0],
        timer_budget: bytes_at::<1>(payload, TIMER_BUDGET)?[0],
        activation_event_mask: u64_at(payload, ACTIVATION_EVENT_MASK)?,
        removal_event_mask: u64_at(payload, REMOVAL_EVENT_MASK)?,
        rearm_event_mask: u64_at(payload, REARM_EVENT_MASK)?,
        auxiliary: auxiliary_records(payload)?,
        groups,
    })
}

/// Reads the policy configuration record with everything it links.
fn policy_configuration(payload: &[u8]) -> Result<Option<DecodedRecord>, String> {
    let relative = i64_at(payload, POLICY_CONFIGURATION)?;
    if relative == 0 {
        return Ok(None);
    }
    let node = relative_offset(POLICY_CONFIGURATION, 0, relative)?;
    let class = node_class(payload, node)?;
    Ok(Some(DecodedRecord {
        class,
        bytes: native::capture(payload, node, class)?,
    }))
}

/// Reads each auxiliary record as one closed allocation, so a record that pointed at other
/// allocations is refused rather than copied without them.
fn auxiliary_records(payload: &[u8]) -> Result<Vec<DecodedRecord>, String> {
    pointer_rows(payload, AUXILIARY_RECORDS, AUXILIARY_ROW_CLASS)?
        .into_iter()
        .map(|node| {
            let class = node_class(payload, node)?;
            let graph = native::Graph::read(payload, node, class)?;
            let [block] = graph.blocks.as_slice() else {
                return Err(format!(
                    "Auxiliary record 0x{class:08X} points at other allocations"
                ));
            };
            Ok(DecodedRecord {
                class,
                bytes: block.bytes.clone(),
            })
        })
        .collect()
}

fn additional_groups(payload: &[u8]) -> Result<Vec<usize>, String> {
    let (count, rows, class) = optional_array(payload, ADDITIONAL_GROUPS)?;
    if count != 0 && class != GROUP_ROW_CLASS {
        return Err(format!("Action group list has class 0x{class:08X}"));
    }
    if count > MAX_LIST {
        return Err(format!("Action declares {count} additional groups"));
    }
    (0..count)
        .map(|index| {
            rows.checked_add(index * GROUP_SIZE)
                .ok_or_else(|| "Action group offset overflowed".to_owned())
        })
        .collect()
}

fn decode_group(payload: &[u8], offset: usize) -> Result<DecodedGroup, String> {
    bytes_at::<GROUP_SIZE>(payload, offset)?;
    Ok(DecodedGroup {
        offset,
        activation: condition_list(payload, offset + GROUP_ACTIVATION, 0)?,
        effects: effect_list(payload, offset + GROUP_EFFECTS)?,
        removal: condition_list(payload, offset + GROUP_REMOVAL, 0)?,
        rearm: condition_list(payload, offset + GROUP_REARM, 0)?,
    })
}

/// Reads an array descriptor, treating a zeroed descriptor as an empty list.
///
/// Action roots leave unused descriptors zeroed rather than pointing at an empty header,
/// so a strict read would reject valid stock actions.
fn optional_array(payload: &[u8], descriptor: usize) -> Result<(usize, usize, u32), String> {
    if u64_at(payload, descriptor)? == 0 && i64_at(payload, descriptor + 8)? == 0 {
        return Ok((0, 0, 0));
    }
    let (count, _, rows, class) = native_array_at(payload, descriptor)?;
    Ok((count, rows, class))
}

fn pointer_rows(
    payload: &[u8],
    descriptor: usize,
    expected_class: u32,
) -> Result<Vec<usize>, String> {
    let (count, rows, class) = optional_array(payload, descriptor)?;
    if count == 0 {
        return Ok(Vec::new());
    }
    if class != expected_class {
        return Err(format!(
            "Action node list at 0x{descriptor:X} has class 0x{class:08X}"
        ));
    }
    if count > MAX_LIST {
        return Err(format!("Action node list declares {count} entries"));
    }
    (0..count)
        .map(|index| {
            let row = rows
                .checked_add(index * size_of::<u64>())
                .ok_or("Action node row offset overflowed")?;
            relative_offset(row, 0, i64_at(payload, row)?)
        })
        .collect()
}

fn condition_list(
    payload: &[u8],
    descriptor: usize,
    depth: usize,
) -> Result<Vec<DecodedCondition>, String> {
    if depth > MAX_NESTING {
        return Err("Action conditions nest too deeply".into());
    }
    pointer_rows(payload, descriptor, CONDITION_ROW_CLASS)?
        .into_iter()
        .map(|node| decode_condition(payload, node, depth))
        .collect()
}

fn effect_list(payload: &[u8], descriptor: usize) -> Result<Vec<DecodedEffect>, String> {
    pointer_rows(payload, descriptor, EFFECT_ROW_CLASS)?
        .into_iter()
        .map(|node| decode_effect(payload, node))
        .collect()
}

fn node_class(payload: &[u8], node: usize) -> Result<u32, String> {
    let class_offset = node
        .checked_sub(size_of::<u32>())
        .ok_or("Action node begins before its class")?;
    u32_at(payload, class_offset)
}

/// Read a standalone condition through the same decoder used for complete actions.
pub fn decode_condition_node(payload: &[u8]) -> Result<DecodedCondition, String> {
    let kind = bytes_at::<8>(payload, 0)?[5];
    let class = nodes::condition(kind)
        .filter(|entry| entry.observed())
        .ok_or_else(|| format!("Condition kind {kind} has no recovered native layout."))?
        .class;
    // Standalone nodes omit the allocation's preceding class tag. Relative pointers
    // keep their meaning when the entire captured graph moves by the same amount.
    let mut allocation = vec![0; 8];
    allocation[4..8].copy_from_slice(&class.to_le_bytes());
    allocation.extend_from_slice(payload);
    decode_condition(&allocation, 8, 0)
}

fn decode_condition(payload: &[u8], node: usize, depth: usize) -> Result<DecodedCondition, String> {
    if depth > MAX_NESTING {
        return Err("Action conditions nest too deeply".into());
    }
    let header = bytes_at::<8>(payload, node)?;
    let kind = header[5];
    let class = validate_node(payload, node, nodes::condition(kind), "Condition", kind)?;
    let mut decoded = DecodedCondition {
        native: Vec::new(),
        offset: node,
        class,
        kind,
        ordinal: header[7],
        linked_state: header[6] != 0,
        probability: Probability::read(payload, node)?,
        facts: fields::condition_facts(payload, node, kind)?,
        children: Vec::new(),
        accumulator_row: None,
        subgroups: Vec::new(),
    };
    decode_condition_children(payload, node, kind, depth, &mut decoded)?;
    decoded.native = native::capture(payload, node, class)?;
    Ok(decoded)
}

fn decode_condition_children(
    payload: &[u8],
    node: usize,
    kind: u8,
    depth: usize,
    decoded: &mut DecodedCondition,
) -> Result<(), String> {
    match kind {
        26 => {
            decoded.children = accumulator_children(payload, node, depth + 1)?;
        }
        31 => {
            decoded.subgroups = subgroup_lists(payload, node, depth + 1)?;
        }
        35 => {
            let pointer = node + NESTED_PREDICATE_POINTER;
            let child = relative_offset(pointer, 0, i64_at(payload, pointer)?)?;
            if child != pointer {
                decoded.children = vec![decode_condition(payload, child, depth + 1)?];
            }
        }
        _ => {}
    }
    Ok(())
}

fn accumulator_children(
    payload: &[u8],
    node: usize,
    depth: usize,
) -> Result<Vec<DecodedCondition>, String> {
    if depth > MAX_NESTING {
        return Err("Action conditions nest too deeply".into());
    }
    let (count, rows, class) = optional_array(payload, node + ACCUMULATOR_CHILDREN)?;
    if count == 0 {
        return Ok(Vec::new());
    }
    if class != ACCUMULATOR_ROW_CLASS {
        return Err(format!("Accumulator child list has class 0x{class:08X}"));
    }
    if count > MAX_LIST {
        return Err(format!("Accumulator child list declares {count} entries"));
    }
    (0..count)
        .map(|index| {
            let row = rows
                .checked_add(index * ACCUMULATOR_ROW_SIZE)
                .ok_or("Accumulator row offset overflowed")?;
            let child = relative_offset(row, 0, i64_at(payload, row)?)?;
            let mut condition = decode_condition(payload, child, depth)?;
            condition.accumulator_row = Some(AccumulatorRow {
                success_operation: bytes_at::<1>(payload, row + 0x08)?[0],
                success_uses_event_value: bytes_at::<1>(payload, row + 0x09)?[0] != 0,
                hold_seconds: f32::from_bits(u32_at(payload, row + 0x10)?),
                success_value: f32::from_bits(u32_at(payload, row + 0x0C)?),
                failure_operation: bytes_at::<1>(payload, row + 0x14)?[0],
                failure_uses_event_value: bytes_at::<1>(payload, row + 0x15)?[0] != 0,
                failure_value: f32::from_bits(u32_at(payload, row + 0x18)?),
            });
            Ok(condition)
        })
        .collect()
}

fn subgroup_lists(payload: &[u8], node: usize, depth: usize) -> Result<Vec<Subgroup>, String> {
    if depth > MAX_NESTING {
        return Err("Action conditions nest too deeply".into());
    }
    let (count, rows, class) = optional_array(payload, node + ACCUMULATOR_CHILDREN)?;
    if count == 0 {
        return Ok(Vec::new());
    }
    if class != SUBGROUP_ROW_CLASS {
        return Err(format!("Action subgroup list has class 0x{class:08X}"));
    }
    if count > MAX_LIST {
        return Err(format!("Action subgroup list declares {count} entries"));
    }
    (0..count)
        .map(|index| {
            let row = rows
                .checked_add(index * SUBGROUP_ROW_SIZE)
                .ok_or("Action subgroup row offset overflowed")?;
            Ok(Subgroup {
                hold: f32::from_bits(u32_at(payload, row + SUBGROUP_HOLD)?),
                conditions: condition_list(payload, row + SUBGROUP_CONDITIONS, depth)?,
            })
        })
        .collect()
}

fn decode_effect(payload: &[u8], node: usize) -> Result<DecodedEffect, String> {
    let header = bytes_at::<2>(payload, node)?;
    let kind = header[0];
    let class = validate_node(payload, node, nodes::effect(kind), "Effect", kind)?;
    let reference = fields::effect_reference(payload, node, kind)?;
    let conditions = if kind == 32 {
        condition_list(payload, node + TIMER_EXTENSION_CONDITIONS, 1)?
    } else {
        Vec::new()
    };
    Ok(DecodedEffect {
        native: native::capture(payload, node, class)?,
        offset: node,
        class,
        kind,
        retained: header[1] != 0,
        facts: fields::effect_facts(payload, node, kind)?,
        referenced_tag: reference.0,
        referenced_path: reference.1,
        conditions,
    })
}

fn validate_node(
    payload: &[u8],
    node: usize,
    entry: Option<&nodes::NodeKind>,
    family: &str,
    kind: u8,
) -> Result<u32, String> {
    let class = node_class(payload, node)?;
    let entry = entry.ok_or_else(|| format!("{family} kind {kind} is not registered"))?;
    if !entry.observed() {
        return Err(format!(
            "{family} kind {kind} has no recovered native layout"
        ));
    }
    if class != entry.class {
        return Err(format!(
            "{family} kind {kind} at 0x{node:X} has class 0x{class:08X}, expected 0x{:08X}",
            entry.class
        ));
    }
    let end = node
        .checked_add(entry.struct_size as usize)
        .ok_or("Action node size overflowed")?;
    if payload.get(node..end).is_none() {
        return Err(format!("{family} kind {kind} at 0x{node:X} is truncated"));
    }
    Ok(class)
}

/// Reads the source label hashes stored in a label array descriptor.
pub(super) fn label_hashes(payload: &[u8], descriptor: usize) -> Result<Vec<u32>, String> {
    let (count, rows, class) = optional_array(payload, descriptor)?;
    if count == 0 {
        return Ok(Vec::new());
    }
    if class != LABEL_ROW_CLASS || count > MAX_LIST {
        return Ok(Vec::new());
    }
    (0..count)
        .map(|index| {
            let row = rows
                .checked_add(index * LABEL_ROW_SIZE)
                .ok_or("Action label row offset overflowed")?;
            u32_at(payload, row)
        })
        .collect()
}
