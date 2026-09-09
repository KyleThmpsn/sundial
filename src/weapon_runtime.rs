//! Typed discovery and validation for package-backed weapon runtime data.
//!
//! Weapon runtime entities are data-driven: an entity maps abstract binding hashes to concrete
//! resources, and each resource owner exposes instance and definition roots. Some roots use a
//! generated package schema while ordinary content roots use the client's native member registry.
//! This module joins both sources so authoring tools do not need weapon-specific offset tables.

#[cfg(test)]
mod tests;

mod registry;
use registry::*;

mod values;
pub use values::encode_weapon_runtime_value;
use values::*;

mod decode;
use decode::*;

mod compatibility;
pub use compatibility::{WeaponRuntimeResourceShape, load_weapon_runtime_resource_shape};

use crate::package_payload::{i64_at as read_i64, u32_at as read_u32, u64_at as read_u64};

use std::{
    collections::{BTreeMap, BTreeSet},
    mem::size_of,
    path::Path,
    sync::OnceLock,
};

use serde::{Deserialize, Serialize};
use tiger_pkg::{PackageManager, TagHash};

use crate::{
    hash::fnv1_name_hash,
    investment_schema::{GLOBALS_SANDBOX_PATTERN_TABLE_SLOT, investment_globals_table_tag},
    package_runtime::{open_shadowkeep_packages, resolve_live_named_tag},
    weapon_entity::{
        SANDBOX_PATTERN_ENTITY_ASSIGNMENT_TAG, SandboxPatternIdentity, WEAPON_BARREL_COMPONENT_KEY,
        WEAPON_CONTROLLER_COMPONENT_KEY, WEAPON_ENTITY_CLASS, WEAPON_INPUT_COMPONENT_KEY,
        WEAPON_MAGAZINE_COMPONENT_KEY, WEAPON_RELOAD_COMPONENT_KEY,
        WEAPON_STAT_TRANSLATOR_COMPONENT_KEY, WEAPON_TRIGGER_CHARGE_COMPONENT_KEY,
        WEAPON_TRIGGER_COMPONENT_KEY, sandbox_pattern_identity, sandbox_pattern_identity_at,
        weapon_component_binding_hashes, weapon_component_bindings, weapon_entity_assignment,
    },
};

const GENERATED_SCHEMA_CLASS: u32 = 0x8080_0000;
const STRUCTURED_RESOURCE_CLASS: u32 = 0x8080_9C36;
const OWNER_INSTANCE_POINTER: usize = 0x10;
const OWNER_DEFINITION_POINTER: usize = 0x18;
const EMPTY_NAME_HASH: u32 = 0x811C_9DC5;
const MAX_GENERATED_SCHEMA_NAME: usize = 160;
const MAX_RUNTIME_SCHEMA_DEPTH: usize = 32;
const MAX_TECHNICAL_RUNTIME_FIELD_BYTES: usize = 256;
const TECHNICAL_BYTES_PATH_HASH: u32 = 0x5048_4259;

/// Which native root inside a component-owner payload contains a runtime value.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeaponRuntimeRootKind {
    Instance,
    Definition,
    ComponentInstance,
    ComponentDefinition,
}

impl WeaponRuntimeRootKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Instance => "Instance",
            Self::Definition => "Definition",
            Self::ComponentInstance => "Component Instance",
            Self::ComponentDefinition => "Component Definition",
        }
    }
}

/// Stable semantic path element used to re-resolve a field after donor grafts.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponRuntimePathElement {
    pub name_hash: u32,
    pub type_handle: u32,
    /// Byte offset relative to the containing reflected value.
    pub byte_offset: u32,
}

/// Donor-independent locator for one runtime field.
///
/// The binding/resource pair selects the current component owner. The root schema and complete
/// reflected path then prove that a saved edit still means the same thing after donor changes.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponRuntimeFieldLocator {
    pub binding_hash: u32,
    pub resource_index: u16,
    pub root: WeaponRuntimeRootKind,
    pub root_schema: u32,
    pub path: Vec<WeaponRuntimePathElement>,
    pub type_handle: u32,
    /// Final byte offset relative to the selected instance, definition, or concrete resource.
    pub value_offset: u32,
    pub byte_size: u32,
}

impl WeaponRuntimeFieldLocator {
    /// Whether this locator has the compiler-supported shape needed for semantic re-resolution.
    #[must_use]
    pub fn is_buildable(&self) -> bool {
        !matches!(self.binding_hash, 0 | u32::MAX)
            && !matches!(self.root_schema, 0 | u32::MAX)
            && !matches!(self.type_handle, 0 | u32::MAX)
            && self.byte_size != 0
            && self.byte_size <= 0x10_0000
            && self.path.len() <= MAX_RUNTIME_SCHEMA_DEPTH
    }
}

/// Native value representation. Floating-point values retain their exact bit patterns.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum WeaponRuntimeValue {
    Boolean(bool),
    Signed(i64),
    Unsigned(u64),
    Float32Bits(u32),
    Vector4Float32Bits([u32; 4]),
    Bytes(Vec<u8>),
}

/// The editor and encoder appropriate for a runtime field.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WeaponRuntimeValueKind {
    Boolean,
    SignedInteger { bits: u8 },
    UnsignedInteger { bits: u8 },
    Enum { bits: u8 },
    BitFlags { bits: u8 },
    HexIdentifier { bits: u8 },
    Float32,
    Vector4Float32,
    FixedBytes { size: u32 },
}

impl WeaponRuntimeValueKind {
    #[must_use]
    pub const fn byte_size(&self) -> u32 {
        match self {
            Self::Boolean => 1,
            Self::SignedInteger { bits }
            | Self::UnsignedInteger { bits }
            | Self::Enum { bits }
            | Self::BitFlags { bits }
            | Self::HexIdentifier { bits } => (*bits as u32) / 8,
            Self::Float32 => 4,
            Self::Vector4Float32 => 16,
            Self::FixedBytes { size } => *size,
        }
    }

    #[must_use]
    pub const fn signed_range(&self) -> Option<(i64, i64)> {
        let Self::SignedInteger { bits } = self else {
            return None;
        };
        match bits {
            8 => Some((i8::MIN as i64, i8::MAX as i64)),
            16 => Some((i16::MIN as i64, i16::MAX as i64)),
            32 => Some((i32::MIN as i64, i32::MAX as i64)),
            64 => Some((i64::MIN, i64::MAX)),
            _ => None,
        }
    }

    #[must_use]
    pub const fn unsigned_maximum(&self) -> Option<u64> {
        let bits = match self {
            Self::UnsignedInteger { bits }
            | Self::Enum { bits }
            | Self::BitFlags { bits }
            | Self::HexIdentifier { bits } => *bits,
            _ => return None,
        };
        match bits {
            8 => Some(u8::MAX as u64),
            16 => Some(u16::MAX as u64),
            32 => Some(u32::MAX as u64),
            64 => Some(u64::MAX),
            _ => None,
        }
    }
}

/// Evidence used to discover a runtime field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WeaponRuntimeFieldSource {
    /// A named stored field from a generated package schema.
    GeneratedSchema,
    /// A named member from the native content registry.
    NativeMember,
    /// A fixed-size type whose internal member semantics are not published by the client.
    OpaqueNativeType,
}

/// One independently writable runtime value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeaponRuntimeField {
    pub locator: WeaponRuntimeFieldLocator,
    /// Resolved byte offset in the component-owner payload. This is diagnostic state rather than
    /// part of the stable recipe locator because different donors may place the same resource at
    /// different owner offsets.
    pub owner_offset: u32,
    pub name: String,
    pub path_label: String,
    pub kind: WeaponRuntimeValueKind,
    pub value: WeaponRuntimeValue,
    pub source: WeaponRuntimeFieldSource,
    /// Low byte of generated-schema metadata, when the path begins at such a field.
    pub generated_kind: Option<u8>,
}

/// One typed recipe edit. The locator is re-resolved before the encoded value is written.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponRuntimeValueOverride {
    pub locator: WeaponRuntimeFieldLocator,
    pub value: WeaponRuntimeValue,
}

/// One abstract binding/resource edge from a weapon entity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeaponRuntimeBinding {
    pub binding_hash: u32,
    pub binding_label: String,
    pub resource_index: u16,
    pub resource_count: u16,
    pub owner_tag: u32,
    pub concrete_class: u32,
    pub resource_offset: u64,
}

/// One native root in a component-owner payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeaponRuntimeRoot {
    pub kind: WeaponRuntimeRootKind,
    pub schema: u32,
    pub owner_offset: u32,
    pub byte_size: u32,
    pub generated_schema: bool,
    pub fields: Vec<WeaponRuntimeField>,
}

/// One concrete resource selected by an abstract runtime-component binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeaponRuntimeResource {
    pub binding_hash: u32,
    pub binding_label: String,
    pub resource_index: u16,
    pub resource_count: u16,
    pub owner_tag: u32,
    pub concrete_class: u32,
    /// Other active bindings that select this exact owner/class/offset tuple.
    pub alias_bindings: Vec<(u32, u16)>,
    pub instance: WeaponRuntimeRoot,
    pub definition: Option<WeaponRuntimeRoot>,
}

/// One deduplicated component owner and the roots it contributes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeaponRuntimeOwner {
    pub owner_tag: u32,
    /// Deterministic binding used to address this owner after component donor grafts.
    pub anchor_binding_hash: u32,
    pub anchor_resource_index: u16,
    pub roots: Vec<WeaponRuntimeRoot>,
}

/// Complete package-backed runtime graph for one stock weapon pattern.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeaponRuntimeGraph {
    pub item_hash: u32,
    pub pattern_global_id_hash: u32,
    pub entity_tag: u32,
    pub bindings: Vec<WeaponRuntimeBinding>,
    pub resources: Vec<WeaponRuntimeResource>,
    pub owners: Vec<WeaponRuntimeOwner>,
}

/// Resolved stock runtime entity used when composing independent component donors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeaponRuntimeEntitySource {
    pub item_hash: u32,
    pub pattern_global_id_hash: u32,
    /// Selects this item's variant inside a shared weapon-content component.
    pub weapon_content_group_hash: u32,
    pub entity_tag: u32,
    pub payload: Vec<u8>,
}

impl WeaponRuntimeGraph {
    pub fn fields(&self) -> impl Iterator<Item = &WeaponRuntimeField> {
        self.resources
            .iter()
            .flat_map(|resource| {
                std::iter::once(&resource.instance)
                    .chain(resource.definition.iter())
                    .flat_map(|root| root.fields.iter())
            })
            .chain(
                self.owners
                    .iter()
                    .flat_map(|owner| owner.roots.iter())
                    .flat_map(|root| root.fields.iter()),
            )
    }

    #[must_use]
    pub fn field_count(&self) -> usize {
        self.fields().count()
    }
}

/// A locator resolved against the final, donor-grafted entity at build time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedWeaponRuntimeField {
    pub owner_tag: u32,
    pub owner_offset: usize,
    pub field: WeaponRuntimeField,
}

#[derive(Clone, Debug, Deserialize)]
struct RegistryMember {
    name_hash: u32,
    type_handle: u32,
    byte_offset: u32,
}

#[derive(Clone, Debug, Deserialize)]
struct NativeLayoutEntry {
    #[serde(rename = "byte_offset")]
    _byte_offset: u32,
    type_code: u32,
}

#[derive(Clone, Debug, Deserialize)]
struct RegistryRecord {
    handle: u32,
    #[serde(rename = "binding_hash")]
    _binding_hash: u32,
    base_type: u32,
    struct_size: u32,
    members: Vec<RegistryMember>,
    native_layout: Vec<NativeLayoutEntry>,
}

struct RuntimeRegistry {
    records: BTreeMap<u32, RegistryRecord>,
    names: BTreeMap<u32, Vec<String>>,
}

#[derive(Clone, Debug)]
struct GeneratedField {
    name: String,
    name_hash: u32,
    type_handle: u32,
    value_offset: u32,
    metadata: u32,
}

#[derive(Clone, Copy)]
struct OwnerRootDescriptor {
    kind: WeaponRuntimeRootKind,
    target: usize,
    schema: u32,
    limit: usize,
}

static RUNTIME_REGISTRY: OnceLock<Result<RuntimeRegistry, String>> = OnceLock::new();

/// Loads every package-backed runtime field for a stock weapon item.
pub fn load_weapon_runtime_graph(
    install_directory: &Path,
    item_hash: u32,
) -> Result<WeaponRuntimeGraph, String> {
    let manager = open_shadowkeep_packages(install_directory)?;
    load_weapon_runtime_graph_with_manager(&manager, item_hash)
}

/// Loads every package-backed runtime field while reusing an existing package manager.
pub fn load_weapon_runtime_graph_with_manager(
    manager: &PackageManager,
    item_hash: u32,
) -> Result<WeaponRuntimeGraph, String> {
    let source = load_weapon_runtime_entity_with_manager(manager, item_hash)?;
    load_weapon_runtime_graph_for_entity(
        manager,
        source.item_hash,
        source.pattern_global_id_hash,
        source.entity_tag,
        &source.payload,
    )
}

/// Resolves one stock item's sandbox pattern and complete runtime entity payload.
pub fn load_weapon_runtime_entity_with_manager(
    manager: &PackageManager,
    item_hash: u32,
) -> Result<WeaponRuntimeEntitySource, String> {
    let sandbox_payload = read_sandbox_pattern_table(manager)?;
    let pattern = sandbox_pattern_identity(&sandbox_payload, item_hash)?.ok_or_else(|| {
        format!("Item 0x{item_hash:08X} does not have a sandbox-pattern runtime row")
    })?;
    load_weapon_runtime_entity_from_pattern(manager, pattern)
}

/// Resolves the authoritative item and entity at an investment sandbox-pattern row index.
pub fn load_weapon_runtime_entity_at_pattern_index_with_manager(
    manager: &PackageManager,
    pattern_index: u16,
) -> Result<WeaponRuntimeEntitySource, String> {
    let sandbox_payload = read_sandbox_pattern_table(manager)?;
    let pattern = sandbox_pattern_identity_at(&sandbox_payload, usize::from(pattern_index))?
        .ok_or_else(|| {
            format!(
                "Weapon pattern index {pattern_index} is outside the installed sandbox-pattern table"
            )
        })?;
    if matches!(pattern.item_hash, 0 | EMPTY_NAME_HASH)
        || matches!(pattern.pattern_global_id_hash, 0 | EMPTY_NAME_HASH)
    {
        return Err(format!(
            "Weapon pattern index {pattern_index} has an inactive item or runtime identity"
        ));
    }
    load_weapon_runtime_entity_from_pattern(manager, pattern)
}

fn read_sandbox_pattern_table(manager: &PackageManager) -> Result<Vec<u8>, String> {
    let globals = resolve_live_named_tag(manager, "investment_globals", None)?;
    let globals_payload = manager
        .read_tag(globals)
        .map_err(|error| format!("Could not read investment globals {globals}: {error}"))?;
    let sandbox_tag = TagHash(investment_globals_table_tag(
        &globals_payload,
        GLOBALS_SANDBOX_PATTERN_TABLE_SLOT,
    )?);
    manager
        .read_tag(sandbox_tag)
        .map_err(|error| format!("Could not read sandbox-pattern table {sandbox_tag}: {error}"))
}

fn load_weapon_runtime_entity_from_pattern(
    manager: &PackageManager,
    pattern: SandboxPatternIdentity,
) -> Result<WeaponRuntimeEntitySource, String> {
    let assignment_tag = TagHash(SANDBOX_PATTERN_ENTITY_ASSIGNMENT_TAG);
    let assignments = manager.read_tag(assignment_tag).map_err(|error| {
        format!("Could not read sandbox-pattern entity assignments {assignment_tag}: {error}")
    })?;
    let entity_tag = weapon_entity_assignment(&assignments, pattern.pattern_global_id_hash)?
        .ok_or_else(|| {
            format!(
                "Sandbox pattern 0x{:08X} does not resolve to a runtime weapon entity",
                pattern.pattern_global_id_hash
            )
        })?;
    let entity_tag = TagHash(entity_tag);
    let entry = manager
        .get_entry(entity_tag)
        .ok_or_else(|| format!("Runtime weapon entity {entity_tag} is not live"))?;
    if entry.reference != WEAPON_ENTITY_CLASS {
        return Err(format!(
            "Runtime weapon entity {entity_tag} has class 0x{:08X}, expected 0x{WEAPON_ENTITY_CLASS:08X}",
            entry.reference
        ));
    }
    let entity = manager
        .read_tag(entity_tag)
        .map_err(|error| format!("Could not read runtime weapon entity {entity_tag}: {error}"))?;
    Ok(WeaponRuntimeEntitySource {
        item_hash: pattern.item_hash,
        pattern_global_id_hash: pattern.pattern_global_id_hash,
        weapon_content_group_hash: pattern.weapon_content_group_hash,
        entity_tag: entity_tag.0,
        payload: entity,
    })
}

/// Decodes an already-resolved runtime entity. This is used after Parhelion applies component
/// donors in memory, ensuring the field UI reflects the effective graph rather than the base donor.
pub fn load_weapon_runtime_graph_for_entity(
    manager: &PackageManager,
    item_hash: u32,
    pattern_global_id_hash: u32,
    entity_tag: u32,
    entity: &[u8],
) -> Result<WeaponRuntimeGraph, String> {
    let registry = runtime_registry()?;
    let bindings = collect_runtime_bindings(entity, registry)?;
    let mut owner_anchors = BTreeMap::<u32, (u32, u16)>::new();
    for binding in &bindings {
        owner_anchors
            .entry(binding.owner_tag)
            .and_modify(|anchor| {
                if (binding.binding_hash, binding.resource_index) < *anchor {
                    *anchor = (binding.binding_hash, binding.resource_index);
                }
            })
            .or_insert((binding.binding_hash, binding.resource_index));
    }

    let mut owner_payloads = BTreeMap::new();
    for &owner_tag in owner_anchors.keys() {
        owner_payloads.insert(owner_tag, read_component_owner(manager, owner_tag)?);
    }

    let canonical_resources = canonical_runtime_resources(&bindings);
    let mut resources = Vec::with_capacity(canonical_resources.len());
    for (binding, alias_bindings) in canonical_resources {
        let payload = owner_payloads.get(&binding.owner_tag).ok_or_else(|| {
            format!(
                "Runtime component owner 0x{:08X} was not loaded",
                binding.owner_tag
            )
        })?;
        let mut resource = decode_component_resource(manager, payload, &binding, registry)?;
        resource.alias_bindings = alias_bindings;
        resources.push(resource);
    }
    resources.sort_by_key(|resource| (resource.binding_hash, resource.resource_index));

    let mut resource_ranges = BTreeMap::<u32, Vec<(usize, usize)>>::new();
    for resource in &resources {
        for root in std::iter::once(&resource.instance).chain(resource.definition.iter()) {
            let start = usize::try_from(root.owner_offset)
                .map_err(|_| "Runtime resource offset does not fit this platform")?;
            let end = start
                .checked_add(root.byte_size as usize)
                .ok_or("Runtime resource range overflowed")?;
            resource_ranges
                .entry(resource.owner_tag)
                .or_default()
                .push((start, end));
        }
    }

    let mut owners = Vec::with_capacity(owner_anchors.len());
    for (owner_tag, (anchor_binding_hash, anchor_resource_index)) in owner_anchors {
        let payload = owner_payloads
            .get(&owner_tag)
            .ok_or_else(|| format!("Runtime component owner 0x{owner_tag:08X} was not loaded"))?;
        let mut roots = decode_owner_roots(
            manager,
            payload,
            owner_tag,
            anchor_binding_hash,
            anchor_resource_index,
            registry,
        )?;
        enrich_component_field_labels(&mut resources, owner_tag, &roots);
        prepare_shared_owner_roots(
            payload,
            &mut roots,
            anchor_binding_hash,
            anchor_resource_index,
            resource_ranges.get(&owner_tag).map_or(&[], Vec::as_slice),
        )?;
        owners.push(WeaponRuntimeOwner {
            owner_tag,
            anchor_binding_hash,
            anchor_resource_index,
            roots,
        });
    }
    owners.sort_by_key(|owner| (owner.anchor_binding_hash, owner.anchor_resource_index));
    Ok(WeaponRuntimeGraph {
        item_hash,
        pattern_global_id_hash,
        entity_tag,
        bindings,
        resources,
        owners,
    })
}

fn enrich_component_field_labels(
    resources: &mut [WeaponRuntimeResource],
    owner_tag: u32,
    owner_roots: &[WeaponRuntimeRoot],
) {
    let mut labels =
        BTreeMap::<(u32, u32), (usize, String, String, WeaponRuntimeFieldSource, Option<u8>)>::new(
        );
    for field in owner_roots.iter().flat_map(|root| &root.fields) {
        let score = semantic_runtime_label_score(&field.path_label);
        if score == 0 {
            continue;
        }
        let key = (field.owner_offset, field.locator.byte_size);
        let replace = labels
            .get(&key)
            .is_none_or(|(existing_score, ..)| score > *existing_score);
        if replace {
            labels.insert(
                key,
                (
                    score,
                    field.name.clone(),
                    field.path_label.clone(),
                    field.source,
                    field.generated_kind,
                ),
            );
        }
    }
    for field in resources
        .iter_mut()
        .filter(|resource| resource.owner_tag == owner_tag)
        .flat_map(|resource| {
            std::iter::once(&mut resource.instance)
                .chain(resource.definition.iter_mut())
                .flat_map(|root| root.fields.iter_mut())
        })
    {
        let key = (field.owner_offset, field.locator.byte_size);
        let Some((_, name, path_label, source, generated_kind)) = labels.get(&key) else {
            continue;
        };
        if semantic_runtime_label_score(path_label)
            > semantic_runtime_label_score(&field.path_label)
        {
            field.name.clone_from(name);
            field.path_label.clone_from(path_label);
            field.source = *source;
            field.generated_kind = *generated_kind;
        }
    }
}

fn semantic_runtime_label_score(label: &str) -> usize {
    label
        .split('›')
        .map(str::trim)
        .filter(|segment| {
            !segment.starts_with("Member 0x")
                && !segment.starts_with("Value 0x")
                && !segment.starts_with("Unreflected byte")
        })
        .count()
}

/// Re-resolves and validates a saved locator against the final runtime entity.
pub fn resolve_weapon_runtime_field(
    manager: &PackageManager,
    entity: &[u8],
    locator: &WeaponRuntimeFieldLocator,
) -> Result<ResolvedWeaponRuntimeField, String> {
    let bindings = weapon_component_bindings(entity, locator.binding_hash)?;
    let binding = bindings
        .get(usize::from(locator.resource_index))
        .ok_or_else(|| {
            format!(
                "Runtime field selects resource {} of binding 0x{:08X}, but the effective entity has {} resources",
                locator.resource_index,
                locator.binding_hash,
                bindings.len()
            )
        })?;
    let payload = read_component_owner(manager, binding.owner_tag)?;
    let registry = runtime_registry()?;
    let field = if matches!(
        locator.root,
        WeaponRuntimeRootKind::ComponentInstance
            | WeaponRuntimeRootKind::ComponentDefinition
    ) {
        let runtime_binding = WeaponRuntimeBinding {
            binding_hash: locator.binding_hash,
            binding_label: runtime_binding_label(locator.binding_hash, registry),
            resource_index: locator.resource_index,
            resource_count: u16::try_from(binding.resource_count)
                .map_err(|_| "Runtime resource count does not fit 16 bits")?,
            owner_tag: binding.owner_tag,
            concrete_class: binding.concrete_class,
            resource_offset: binding.resource_offset,
        };
        if locator.root == WeaponRuntimeRootKind::ComponentInstance
            && runtime_binding.concrete_class != locator.root_schema
        {
            return Err(format!(
                "Runtime resource class changed from 0x{:08X} to 0x{:08X}; select the field again for the current donors",
                locator.root_schema, runtime_binding.concrete_class
            ));
        }
        let resource = decode_component_resource(manager, &payload, &runtime_binding, registry)?;
        let root = match locator.root {
            WeaponRuntimeRootKind::ComponentInstance => Some(resource.instance),
            WeaponRuntimeRootKind::ComponentDefinition => resource.definition,
            WeaponRuntimeRootKind::Instance | WeaponRuntimeRootKind::Definition => None,
        }
        .ok_or_else(|| {
            format!(
                "Runtime binding 0x{:08X} resource {} has no {}",
                locator.binding_hash,
                locator.resource_index,
                locator.root.label().to_ascii_lowercase()
            )
        })?;
        if root.schema != locator.root_schema {
            return Err(format!(
                "Runtime component schema changed from 0x{:08X} to 0x{:08X}; select the field again for the current donors",
                locator.root_schema, root.schema
            ));
        }
        root.fields
            .into_iter()
            .find(|field| field.locator == *locator)
    } else {
        let mut roots = decode_owner_roots(
            manager,
            &payload,
            binding.owner_tag,
            locator.binding_hash,
            locator.resource_index,
            registry,
        )?;
        if locator.path.first().is_some_and(|element| {
            element.name_hash == TECHNICAL_BYTES_PATH_HASH
        }) {
            // Technical shared-owner ranges are constructed after concrete resources are
            // excluded. Reconstruct that same boundary-aware view before matching a saved
            // locator, rather than accepting an arbitrary offset into the owner payload.
            let owner_bindings = collect_runtime_bindings(entity, registry)?
                .into_iter()
                .filter(|candidate| candidate.owner_tag == binding.owner_tag)
                .collect::<Vec<_>>();
            let mut ranges = Vec::new();
            for (candidate, _) in canonical_runtime_resources(&owner_bindings) {
                let resource = decode_component_resource(manager, &payload, &candidate, registry)?;
                for root in std::iter::once(&resource.instance).chain(resource.definition.iter()) {
                    let start = usize::try_from(root.owner_offset)
                        .map_err(|_| "Runtime resource offset does not fit this platform")?;
                    let end = start
                        .checked_add(root.byte_size as usize)
                        .ok_or("Runtime resource range overflowed")?;
                    ranges.push((start, end));
                }
            }
            prepare_shared_owner_roots(
                &payload,
                &mut roots,
                locator.binding_hash,
                locator.resource_index,
                &ranges,
            )?;
        }
        let root = roots
            .into_iter()
            .find(|root| root.kind == locator.root)
            .ok_or_else(|| {
                format!(
                    "Runtime component owner 0x{:08X} has no {} root",
                    binding.owner_tag,
                    locator.root.label().to_ascii_lowercase()
                )
            })?;
        if root.schema != locator.root_schema {
            return Err(format!(
                "Runtime field schema changed from 0x{:08X} to 0x{:08X}; select the field again for the current donors",
                locator.root_schema, root.schema
            ));
        }
        root.fields
            .into_iter()
            .find(|field| field.locator == *locator)
    }
    .ok_or_else(|| {
        format!(
            "Runtime field {} no longer exists at the saved reflected path and root-relative offset 0x{:X}",
            format_runtime_path(&locator.path, registry),
            locator.value_offset
        )
    })?;
    Ok(ResolvedWeaponRuntimeField {
        owner_tag: binding.owner_tag,
        owner_offset: usize::try_from(field.owner_offset)
            .map_err(|_| "Runtime field offset does not fit this platform")?,
        field,
    })
}

fn collect_runtime_bindings(
    entity: &[u8],
    registry: &RuntimeRegistry,
) -> Result<Vec<WeaponRuntimeBinding>, String> {
    let mut bindings = Vec::new();
    for binding_hash in weapon_component_binding_hashes(entity)? {
        for binding in weapon_component_bindings(entity, binding_hash)? {
            bindings.push(WeaponRuntimeBinding {
                binding_hash,
                binding_label: runtime_binding_label(binding_hash, registry),
                resource_index: u16::try_from(binding.resource_index).map_err(|_| {
                    format!(
                        "Runtime binding 0x{binding_hash:08X} resource index does not fit 16 bits"
                    )
                })?,
                resource_count: u16::try_from(binding.resource_count).map_err(|_| {
                    format!(
                        "Runtime binding 0x{binding_hash:08X} resource count does not fit 16 bits"
                    )
                })?,
                owner_tag: binding.owner_tag,
                concrete_class: binding.concrete_class,
                resource_offset: binding.resource_offset,
            });
        }
    }
    bindings.sort_by_key(|binding| (binding.binding_hash, binding.resource_index));
    Ok(bindings)
}

fn canonical_runtime_resources(
    bindings: &[WeaponRuntimeBinding],
) -> Vec<(WeaponRuntimeBinding, Vec<(u32, u16)>)> {
    let mut aliases = BTreeMap::<(u32, u64, u32), Vec<&WeaponRuntimeBinding>>::new();
    for binding in bindings {
        aliases
            .entry((
                binding.owner_tag,
                binding.resource_offset,
                binding.concrete_class,
            ))
            .or_default()
            .push(binding);
    }
    aliases
        .into_values()
        .map(|mut candidates| {
            candidates.sort_by_key(|binding| runtime_resource_binding_priority(binding));
            let canonical = (*candidates[0]).clone();
            let mut alias_bindings = candidates
                .into_iter()
                .skip(1)
                .map(|binding| (binding.binding_hash, binding.resource_index))
                .collect::<Vec<_>>();
            alias_bindings.sort_unstable();
            (canonical, alias_bindings)
        })
        .collect()
}

fn runtime_resource_binding_priority(binding: &WeaponRuntimeBinding) -> (u8, u16, u32, u16) {
    let semantic = matches!(
        binding.binding_hash,
        WEAPON_INPUT_COMPONENT_KEY
            | WEAPON_TRIGGER_COMPONENT_KEY
            | WEAPON_BARREL_COMPONENT_KEY
            | WEAPON_CONTROLLER_COMPONENT_KEY
            | WEAPON_MAGAZINE_COMPONENT_KEY
            | WEAPON_RELOAD_COMPONENT_KEY
            | WEAPON_TRIGGER_CHARGE_COMPONENT_KEY
    );
    (
        if semantic {
            0
        } else if binding.resource_count == 1 {
            1
        } else {
            2
        },
        binding.resource_count,
        binding.binding_hash,
        binding.resource_index,
    )
}

fn relative_target(data: &[u8], pointer: usize) -> Result<usize, String> {
    let relative = read_i64(data, pointer)?;
    let target = if relative >= 0 {
        pointer.checked_add(relative as usize)
    } else {
        pointer.checked_sub(relative.unsigned_abs() as usize)
    }
    .ok_or("Native self-relative pointer overflowed")?;
    if target >= data.len() {
        return Err(format!(
            "Native self-relative pointer at 0x{pointer:X} resolves outside its payload"
        ));
    }
    Ok(target)
}
