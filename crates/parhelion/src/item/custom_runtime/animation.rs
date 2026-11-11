//! Link an imported model's converted first-person clips into the authored
//! runtime weapon entity through private copies of the donor's animation chain.
//!
//! The prepared graph carries the native attachment owner, first-person entity,
//! lookup owner and clip bank payloads plus converted clips keyed by the native
//! clip they replace. Each link in that chain is a plain tag word, so every copy
//! is rewritten to name the next private tag and the authored runtime entity is
//! retargeted from the native attachment owner to the private one.
use super::imports::Inputs;
use super::*;
use parhelion_import::d2_mot::rig_convert::animation::first_person::consumers;
use parhelion_import::d2_mot::rig_convert::animation::first_person::profile;
use serde_json::Value;
use std::collections::BTreeMap;
mod attachment;
pub(in crate::item) mod character;
pub(in crate::item) mod equipment;

const FIRST_PERSON_ENTITY_CLASS: u32 = 0x8080_9C0F;
const RESOURCE_OWNER_CLASS: u32 = 0x8080_9C36;
const CLIP_BANK_CLASS: u32 = 0x8080_36F6;
const CLIP_CLASS: u32 = 0x8080_8F49;

/// Imported first-person clips and the native chain they replace.
pub(in crate::item) struct ImportedAnimation {
    pub runtime_entity: u32,
    pub attachment_owner: (u32, Vec<u8>),
    pub entity: (u32, Vec<u8>),
    pub lookup_owner: (u32, Vec<u8>),
    pub bank: (u32, Vec<u8>),
    pub converted_bank: Option<Vec<u8>>,
    pub extra_clips: Vec<(u32, u32, Vec<u8>)>,
    pub bank_consumers: Vec<(u32, Vec<u8>)>,
    /// Bank consumers that gained inputs, parameters or layers, by stock owner.
    pub converted_consumers: BTreeMap<u32, ConvertedConsumer>,
    /// The first-person entity with the connections those consumers' added inputs read.
    pub converted_entity: Option<Vec<u8>>,
    /// Native clip tag and the converted payload replacing it, in bank order.
    pub clips: Vec<(u32, Vec<u8>)>,
    pub profile: Option<profile::Patch>,
    shared_profile: Option<character::Binding>,
    pub states: Option<ImportedStates>,
    pub rigs: Vec<ImportedRig>,
    pub rig_extension: bool,
    pub pose_layers: Option<ImportedPoseLayers>,
}

type ConvertedParameters = ((u32, Vec<u8>), Vec<u8>);

pub(in crate::item) struct ImportedPoseLayers {
    owner: u32,
    original: (u32, Vec<u8>),
    converted: Vec<u8>,
    /// The controller's parameter library, stock and extended, when its layers read added
    /// parameters.
    parameters: Option<ConvertedParameters>,
}

/// A bank consumer's converted payload and the allocation sized for it.
pub(in crate::item) struct ConvertedConsumer {
    payload: Vec<u8>,
    allocation: (u32, Vec<u8>),
    converted_allocation: Vec<u8>,
}

pub(in crate::item) struct ImportedRig {
    original: (u32, Vec<u8>),
    converted: Vec<u8>,
    first_person: bool,
    kind: RigKind,
    relocation: Option<(usize, usize)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RigKind {
    Skeleton,
    Controls,
    Markers,
}

pub(in crate::item) struct ImportedStates {
    parameters: (u32, Vec<u8>),
    states: (u32, Vec<u8>),
    converted_parameters: Vec<u8>,
    converted_states: Vec<u8>,
}

fn tag(section: &Value, key: &str) -> AuthoringResult<u32> {
    u32::try_from(
        section[key]
            .as_u64()
            .ok_or_else(|| invalid(format!("Imported animation lacks {key}")))?,
    )
    .map_err(|_| invalid(format!("Imported animation {key} is not a tag")))
}

fn payload(graph: &Inputs, file: &Value) -> AuthoringResult<Vec<u8>> {
    let name = file
        .as_str()
        .ok_or_else(|| invalid("Imported animation file name missing"))?;
    graph.read(name)
}

/// A converted clip as its graph stores it. The build re-encodes it like the stock clip whose slot
/// it fills, see `like_stock`. No weighted layer row records these clips' spans.
fn clip_payload(graph: &Inputs, file: &Value) -> AuthoringResult<Vec<u8>> {
    payload(graph, file)
}

/// An imported clip stored as stock stores the clip whose slot it fills.
///
/// It takes that clip's animated codec: codec 1 curves only where stock uses them, codec 2
/// samples otherwise, including for a clip a graph saved as codec 1. Stock chose per motion:
/// near-rest motion such as the airborne additive `80BC5F40` and the idle `80BC5F49` is codec 2,
/// where codec 1's derived `z` cannot hold it. Imported copies of them in codec 1 shook the
/// weapon on screen while jumping and aiming (Dawn 1.7.2, 2026-11-08).
///
/// An additive clip that is the stock clip's motion at another strength also plays at the stock
/// strength, as Shadowkeep's landings are Rise of Iron's at a tenth. The first-person state
/// machine that triggers the slot is tuned for the stock clip.
fn like_stock(manager: &PackageManager, stock: u32, payload: &[u8]) -> AuthoringResult<Vec<u8>> {
    use parhelion_import::tiger::{animation::clip, payload::Payload};
    let imported = Payload(payload.to_vec());
    let reference =
        Payload(manager.read_tag(TagHash(stock)).map_err(|error| {
            invalid(format!("Could not read stock clip 0x{stock:08X}: {error}"))
        })?);
    let failed = |error: anyhow::Error| {
        invalid(format!(
            "Imported clip for 0x{stock:08X} could not take its stock form: {error:#}"
        ))
    };
    let curves = clip::animated_codec(&reference).map_err(failed)? == Some(0x8080_8F72);
    let clip = match clip::stock_weight(&imported, &reference).map_err(failed)? {
        Some(weight) => clip::scale_additive(&imported, weight, curves),
        None => clip::compact_as(&imported, curves),
    }
    .map_err(failed)?;
    Ok(clip.0)
}

/// Read a graph's linked first-person animation, if it prepared one.
pub(in crate::item) fn load(graph: &Inputs) -> AuthoringResult<Option<ImportedAnimation>> {
    let value = graph.value();
    let animation = &value["animation"];
    if animation["first_person_status"] != "linked" {
        return Ok(None);
    }
    let first_person = &animation["first_person"];
    let files = &first_person["files"];
    let mut clips = Vec::new();
    for clip in first_person["clips"]
        .as_array()
        .ok_or_else(|| invalid("Imported animation clips missing"))?
    {
        clips.push((tag(clip, "native")?, clip_payload(graph, &clip["file"])?));
    }
    let state_status = first_person["state_conversion"]["status"].as_str();
    let states = if matches!(state_status, Some("source_states" | "native_states")) {
        let section = &first_person["state_conversion"];
        let states = ImportedStates {
            parameters: (
                tag(section, "parameters")?,
                payload(graph, &files["parameters_template"])?,
            ),
            states: (
                tag(section, "states")?,
                payload(graph, &files["states_template"])?,
            ),
            converted_parameters: payload(graph, &files["parameters"])?,
            converted_states: payload(graph, &files["states"])?,
        };
        if state_status == Some("native_states")
            && (states.parameters.1 != states.converted_parameters
                || states.states.1 != states.converted_states)
        {
            return Err(invalid("Preserved native animation selectors changed"));
        }
        Some(states)
    } else {
        None
    };
    let result = ImportedAnimation {
        runtime_entity: tag(animation, "runtime_entity")?,
        attachment_owner: (
            tag(first_person, "attachment_owner")?,
            payload(graph, &files["attachment_owner"])?,
        ),
        entity: (
            tag(first_person, "entity")?,
            payload(graph, &files["entity"])?,
        ),
        lookup_owner: (
            tag(first_person, "lookup_owner")?,
            payload(graph, &files["lookup_owner"])?,
        ),
        bank: (tag(first_person, "bank")?, payload(graph, &files["bank"])?),
        converted_bank: files
            .get("converted_bank")
            .map(|file| payload(graph, file))
            .transpose()?,
        extra_clips: first_person["extra_clips"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|clip| {
                Ok((
                    tag(clip, "source")?,
                    tag(clip, "template")?,
                    clip_payload(graph, &clip["file"])?,
                ))
            })
            .collect::<AuthoringResult<_>>()?,
        bank_consumers: first_person["bank_consumers"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|consumer| {
                let key = consumer["file_key"]
                    .as_str()
                    .ok_or_else(|| invalid("Imported animation consumer file key missing"))?;
                Ok((tag(consumer, "owner")?, payload(graph, &files[key])?))
            })
            .collect::<AuthoringResult<_>>()?,
        converted_consumers: first_person["bank_consumers"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|consumer| consumer.get("converted_file_key").is_some())
            .map(|consumer| {
                let key = consumer["converted_file_key"]
                    .as_str()
                    .ok_or_else(|| invalid("Imported animation converted consumer key missing"))?;
                let allocation = &consumer["allocation"];
                Ok((
                    tag(consumer, "owner")?,
                    ConvertedConsumer {
                        payload: payload(graph, &files[key])?,
                        allocation: (
                            tag(allocation, "tag")?,
                            payload(graph, &allocation["template_file"])?,
                        ),
                        converted_allocation: payload(graph, &allocation["file"])?,
                    },
                ))
            })
            .collect::<AuthoringResult<_>>()?,
        converted_entity: files
            .get("entity_converted")
            .map(|file| payload(graph, file))
            .transpose()?,
        clips,
        states,
        shared_profile: if value["animation"]["character"]
            .get("first_person_parameters")
            .is_some()
        {
            character::load(graph)?
        } else {
            None
        },
        pose_layers: first_person
            .get("pose_layers")
            .filter(|value| !value.is_null())
            .map(|layers| {
                Ok(ImportedPoseLayers {
                    owner: tag(layers, "owner")?,
                    original: (
                        tag(layers, "tag")?,
                        payload(graph, &layers["template_file"])?,
                    ),
                    converted: payload(graph, &layers["file"])?,
                    parameters: layers
                        .get("parameters")
                        .map(|parameters| {
                            Ok::<_, crate::AuthoringError>((
                                (
                                    tag(parameters, "tag")?,
                                    payload(graph, &parameters["template_file"])?,
                                ),
                                payload(graph, &parameters["file"])?,
                            ))
                        })
                        .transpose()?,
                })
            })
            .transpose()?,
        rig_extension: first_person["rig_extension"].as_bool().unwrap_or(false),
        rigs: first_person["rigs"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|rig| {
                Ok(ImportedRig {
                    relocation: rig
                        .get("relocation")
                        .filter(|v| !v.is_null())
                        .map(|v| {
                            let field = |key| -> AuthoringResult<usize> {
                                usize::try_from(v[key].as_u64().ok_or_else(|| {
                                    invalid("Imported rig relocation is incomplete")
                                })?)
                                .map_err(|_| {
                                    invalid("Imported rig relocation exceeds address limits")
                                })
                            };
                            Ok::<_, crate::AuthoringError>((field("cut")?, field("delta")?))
                        })
                        .transpose()?,
                    kind: match rig.get("kind").and_then(Value::as_str) {
                        None | Some("skeleton") => RigKind::Skeleton,
                        Some("controls") => RigKind::Controls,
                        Some("markers") => RigKind::Markers,
                        Some(_) => return Err(invalid("Imported rig owner kind is unsupported")),
                    },
                    original: (tag(rig, "native")?, payload(graph, &rig["template_file"])?),
                    converted: payload(graph, &rig["file"])?,
                    first_person: rig["first_person"]
                        .as_bool()
                        .ok_or_else(|| invalid("Imported rig target missing"))?,
                })
            })
            .collect::<AuthoringResult<_>>()?,
        profile: first_person["attachment_profile"]
            .get("patch")
            .map(|patch| {
                serde_json::from_value(patch.clone())
                    .map_err(|error| invalid(format!("Imported animation profile: {error}")))
            })
            .transpose()?,
    };
    if result.clips.is_empty() && result.extra_clips.is_empty() {
        return Ok(None);
    }
    if let Some(bank) = &result.converted_bank {
        use parhelion_import::d2_mot::{
            payload::Payload, rig_convert::animation::first_person::dispatch,
        };
        if let Some(layers) = &result.pose_layers {
            dispatch::validate(&Payload(layers.converted.clone()), &Payload(bank.clone()))
                .map_err(|error| invalid(format!("Imported animation dispatch: {error:#}")))?;
        } else if read_u64(bank, 0x68)? != read_u64(&result.bank.1, 0x68)? {
            return Err(invalid(
                "Imported animation adds actions without a matching pose dispatch table. Refresh this import before building.",
            ));
        }
    }
    Ok(Some(result))
}

/// Rewrite every aligned word equal to `old` as `new`, requiring at least one.
fn relink(payload: &mut [u8], old: u32, new: u32, what: &str) -> AuthoringResult<usize> {
    let mut count = 0;
    for word in payload.chunks_exact_mut(4) {
        if u32::from_le_bytes([word[0], word[1], word[2], word[3]]) == old {
            word.copy_from_slice(&new.to_le_bytes());
            count += 1;
        }
    }
    if count == 0 {
        return Err(invalid(format!(
            "Imported animation {what} does not reference 0x{old:08X}"
        )));
    }
    Ok(count)
}

/// The offsets to write when the consumer field at `at` is retargeted. Growing a consumer seals
/// its runtime span (`tiger::instance::seal_span`), which copies the whole record stream after
/// the original: the schema view reads the copy and the instance view the original, so a field
/// written in one copy alone leaves the other naming the stock resource. A sealed consumer's field
/// must be a reference its stock owner's layout `declared` before any write, and its copy, at the
/// distance the seal placed it, must name the same stock tag. The stock layout reaches only the
/// original view, so the copy is found from the seal's own layout.
fn retarget_fields(
    payload: &parhelion_import::tiger::payload::Payload,
    declared: &[(usize, u32)],
    at: usize,
    sealed: bool,
) -> AuthoringResult<Vec<usize>> {
    let old = read_u32(&payload.0, at)?;
    if !sealed {
        return Ok(vec![at]);
    }
    if !declared.contains(&(at, old)) {
        return Err(invalid(format!(
            "Converted animation consumer field 0x{at:X} is not a declared reference to 0x{old:08X}"
        )));
    }
    let copy = parhelion_import::tiger::instance::sealed_copy(payload, at)
        .map_err(|error| invalid(format!("Converted animation consumer: {error:#}")))?
        .ok_or_else(|| {
            invalid(format!(
                "Converted animation consumer field 0x{at:X} has no sealed copy"
            ))
        })?;
    if read_u32(&payload.0, copy)? != old {
        return Err(invalid(format!(
            "Converted animation consumer's sealed copy of 0x{at:X} at 0x{copy:X} does not name 0x{old:08X}"
        )));
    }
    Ok(vec![at, copy])
}

/// Writes the private bank over bank consumer `original`'s stock bank and, on the pose layer
/// controller, its private layers and parameters, in both views of a sealed consumer. Every write
/// is planned and checked against the unwritten payload before any is made. `push` appends a
/// private payload as `author` does.
fn retarget_consumer(
    manager: &PackageManager,
    (original, payload, sealed): (u32, &mut parhelion_import::tiger::payload::Payload, bool),
    animation: &ImportedAnimation,
    (bank_tag, pose_layer_tag): (u32, Option<u32>),
    (allocator, tags): (AppendedTagAllocator, &mut Vec<NewTagSpec>),
    push: &mut impl FnMut(
        AppendedTagAllocator,
        &mut Vec<NewTagSpec>,
        u32,
        Vec<u8>,
        &str,
    ) -> AuthoringResult<u32>,
) -> AuthoringResult<()> {
    let declared = if sealed {
        sundial::package_authoring::declared_payload_references(manager, original, &payload.0)
            .map_err(|error| invalid(format!("Converted animation consumer: {error}")))?
    } else {
        Vec::new()
    };
    let mut writes = Vec::new();
    let at = consumers::bank_field(payload)
        .map_err(|error| invalid(format!("Animation bank consumer: {error:#}")))?;
    for offset in retarget_fields(payload, &declared, at, sealed)? {
        writes.push((offset, bank_tag));
    }
    if let Some(layers) = &animation.pose_layers
        && layers.owner == original
    {
        let field = payload
            .pointer(24)
            .map_err(|error| invalid(format!("Pose layer controller: {error:#}")))?
            + 0x11C;
        let layer_tag = pose_layer_tag.ok_or_else(|| invalid("Private pose layer tag missing"))?;
        for offset in retarget_fields(payload, &declared, field, sealed)? {
            writes.push((offset, layer_tag));
        }
        if let Some(((library, _), extended)) = &layers.parameters {
            let parameters = field - 0x11C + 0x110;
            let offsets = retarget_fields(payload, &declared, parameters, sealed)?;
            let private = push(
                allocator,
                tags,
                *library,
                extended.clone(),
                "pose parameters",
            )?;
            for offset in offsets {
                writes.push((offset, private));
            }
        }
    }
    for (offset, tag) in writes {
        payload.0[offset..offset + 4].copy_from_slice(&tag.to_le_bytes());
    }
    Ok(())
}

fn stock(
    manager: &PackageManager,
    (tag, payload): &(u32, Vec<u8>),
    class: u32,
    what: &str,
) -> AuthoringResult<()> {
    let entry = manager
        .get_entry(TagHash(*tag))
        .ok_or_else(|| invalid(format!("Imported animation {what} 0x{tag:08X} is not live")))?;
    if entry.reference != class {
        return Err(invalid(format!(
            "Imported animation {what} 0x{tag:08X} has class 0x{:08X}, expected 0x{class:08X}",
            entry.reference
        )));
    }
    unchanged(manager, *tag, payload, what)
}

/// The prepared copy of a live tag still matches it.
fn unchanged(
    manager: &PackageManager,
    tag: u32,
    payload: &[u8],
    what: &str,
) -> AuthoringResult<()> {
    let live = read_tag(manager, TagHash(tag), what)?;
    if live != payload {
        let first = live
            .iter()
            .zip(payload)
            .position(|(a, b)| a != b)
            .unwrap_or(live.len().min(payload.len()));
        return Err(invalid(format!(
            "Imported animation {what} 0x{tag:08X} changed since the graph was prepared ({} stock bytes, {} prepared bytes, first difference at 0x{first:X})",
            live.len(),
            payload.len()
        )));
    }
    Ok(())
}

/// Private animation payloads already appended, keyed by asset class and content.
///
/// Weapons of one source family share their clip banks, so the same converted
/// payload is requested by many weapons in a project. Sharing immutable payloads
/// avoids repeating their storage and loading work across asset packages.
pub(in crate::item) type AnimationTagCache = BTreeMap<(u32, [u8; 32]), u32>;

/// Source-owned rigs need both skeletons, one first-person controller at most per view and
/// the converted selectors that route to them. Each converted rig must still parse.
fn validate_rigs(manager: &PackageManager, animation: &ImportedAnimation) -> AuthoringResult<()> {
    if animation.rigs.is_empty() {
        return Ok(());
    }
    if animation
        .rigs
        .iter()
        .filter(|rig| rig.kind == RigKind::Skeleton)
        .count()
        != 2
        || animation
            .rigs
            .iter()
            .filter(|rig| rig.kind == RigKind::Skeleton && rig.first_person)
            .count()
            != 1
        || [false, true].into_iter().any(|first_person| {
            animation
                .rigs
                .iter()
                .filter(|rig| rig.kind == RigKind::Controls && rig.first_person == first_person)
                .count()
                > 1
        })
        || animation.states.is_none()
        || (!animation.rig_extension && animation.profile.is_none())
    {
        return Err(invalid(
            "Source-owned rigs require both skeletons and converted animation selectors",
        ));
    }
    for rig in &animation.rigs {
        // A runtime marker owner is replaced row by row and has no range layout to extend.
        if animation.rig_extension && rig.kind != RigKind::Markers {
            parhelion_import::tiger::animation::rig::validate_extension(
                &parhelion_import::tiger::payload::Payload(rig.original.1.clone()),
                &parhelion_import::tiger::payload::Payload(rig.converted.clone()),
                rig.relocation,
            )
            .map_err(|error| invalid(format!("Imported rig extension: {error:#}")))?;
        } else if rig.kind == RigKind::Skeleton {
            parhelion_import::tiger::animation::sections::validate_source_relocation(
                &rig.original.1,
                &rig.converted,
                rig.relocation,
            )
            .map_err(|error| invalid(format!("Imported source rig relocation: {error:#}")))?;
        } else if rig.relocation.is_some() {
            return Err(invalid("Imported rig relocation requires an FK skeleton"));
        }
        match rig.kind {
            RigKind::Controls => {
                parhelion_import::tiger::animation::rig::controls::validate(&rig.converted)
                    .map_err(|error| invalid(format!("Imported rig controls: {error:#}")))?;
            }
            RigKind::Skeleton => {
                parhelion_import::tiger::animation::sections::validate_native_instance(
                    &rig.converted,
                )
                .map_err(|error| invalid(format!("Imported skeleton instance: {error:#}")))?;
            }
            RigKind::Markers => {
                if rig.first_person
                    || animation
                        .rigs
                        .iter()
                        .filter(|r| r.kind == RigKind::Markers)
                        .count()
                        != 1
                {
                    return Err(invalid("Imported runtime marker owner is ambiguous"));
                }
                parhelion_import::tiger::markers::validate(&rig.converted)
                    .map_err(|error| invalid(format!("Imported runtime markers: {error:#}")))?;
            }
        }
        stock(manager, &rig.original, RESOURCE_OWNER_CLASS, "rig owner")?;
    }
    if animation.rig_extension || animation.rigs.iter().any(|rig| rig.relocation.is_some()) {
        use parhelion_import::tiger::{animation::rig::validate_weapon_pair, payload::Payload};
        let skeleton = |fp| {
            animation
                .rigs
                .iter()
                .find(|r| r.kind == RigKind::Skeleton && r.first_person == fp)
                .unwrap()
        };
        let equipment = skeleton(false);
        let first_person = skeleton(true);
        let before = validate_weapon_pair(
            &Payload(equipment.original.1.clone()),
            &Payload(first_person.original.1.clone()),
        )
        .map_err(|error| invalid(format!("Native weapon palette: {error:#}")))?;
        let after = validate_weapon_pair(
            &Payload(equipment.converted.clone()),
            &Payload(first_person.converted.clone()),
        )
        .map_err(|error| invalid(format!("Extended weapon palette: {error:#}")))?;
        if before != after
            || [false, true].into_iter().any(|fp| {
                animation
                    .rigs
                    .iter()
                    .filter(|r| r.kind == RigKind::Controls && r.first_person == fp)
                    .count()
                    != 1
            })
        {
            return Err(invalid(
                "Extended weapon palette requires both control rigs and an unchanged arm prefix",
            ));
        }
    }
    Ok(())
}

/// Append the private chain and retarget the authored entity. The clips go to the weapon's
/// asset group and the bank, owners and entity that route them to the runtime tags. Returns
/// the number of clips substituted.
#[allow(clippy::too_many_arguments)]
pub(in crate::item) fn author(
    manager: &PackageManager,
    animation: &ImportedAnimation,
    pattern_entity_tag: TagHash,
    pattern_entity: &mut [u8],
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
    (assets, asset_tags): (AppendedTagAllocator, &mut Vec<NewTagSpec>),
    cache: &mut AnimationTagCache,
) -> AuthoringResult<(usize, u32)> {
    if pattern_entity_tag.0 != animation.runtime_entity {
        return Err(invalid(format!(
            "Imported animation was prepared against runtime entity 0x{:08X} but this weapon's pattern uses {pattern_entity_tag}",
            animation.runtime_entity
        )));
    }
    stock(
        manager,
        &animation.attachment_owner,
        RESOURCE_OWNER_CLASS,
        "attachment owner",
    )?;
    stock(
        manager,
        &animation.entity,
        FIRST_PERSON_ENTITY_CLASS,
        "first-person entity",
    )?;
    stock(
        manager,
        &animation.lookup_owner,
        RESOURCE_OWNER_CLASS,
        "lookup owner",
    )?;
    stock(manager, &animation.bank, CLIP_BANK_CLASS, "clip bank")?;
    for consumer in &animation.bank_consumers {
        stock(manager, consumer, RESOURCE_OWNER_CLASS, "bank consumer")?;
        let payload = parhelion_import::tiger::payload::Payload(consumer.1.clone());
        let at = consumers::bank_field(&payload)
            .map_err(|error| invalid(format!("Animation bank consumer: {error:#}")))?;
        if read_u32(&consumer.1, at)? != animation.bank.0 {
            return Err(invalid("Imported animation consumer bank changed"));
        }
        if let Some(converted) = animation.converted_consumers.get(&consumer.0) {
            stock(
                manager,
                &converted.allocation,
                0x8080_9BBB,
                "consumer allocation",
            )?;
            let payload = parhelion_import::tiger::payload::Payload(converted.payload.clone());
            let at = consumers::bank_field(&payload)
                .map_err(|error| invalid(format!("Converted animation consumer: {error:#}")))?;
            if read_u32(&converted.payload, at)? != animation.bank.0
                || read_u32(&converted.payload, 0x44)? != converted.allocation.0
                || read_u64(&converted.payload, 0)? != converted.payload.len() as u64
            {
                return Err(invalid(
                    "Converted animation consumer differs from its stock owner",
                ));
            }
        }
    }
    if animation
        .converted_consumers
        .keys()
        .any(|owner| !animation.bank_consumers.iter().any(|(tag, _)| tag == owner))
    {
        return Err(invalid("Converted animation consumer has no stock owner"));
    }
    if let Some(layers) = &animation.pose_layers {
        stock(manager, &layers.original, 0x808034EB, "pose layers")?;
        let consumers = animation
            .bank_consumers
            .iter()
            .filter(|(owner, _)| *owner == layers.owner)
            .collect::<Vec<_>>();
        if consumers.len() != 1
            || layers.converted.len() < layers.original.1.len()
            || read_u64(&layers.converted, 0)? != layers.converted.len() as u64
        {
            return Err(invalid("Imported pose layer controller or payload differs"));
        }
        let consumer = parhelion_import::tiger::payload::Payload(consumers[0].1.clone());
        let base = consumer
            .pointer(24)
            .map_err(|error| invalid(format!("Pose layer controller: {error:#}")))?;
        if read_u32(&consumer.0, base - 4)? != 0x80803640
            || read_u32(&consumer.0, base + 0x11C)? != layers.original.0
        {
            return Err(invalid("Imported pose layer binding changed"));
        }
        if let Some(((library, original), _)) = &layers.parameters {
            unchanged(manager, *library, original, "pose parameters")?;
            if read_u32(&consumer.0, base + 0x110)? != *library
                || !animation.converted_consumers.contains_key(&layers.owner)
            {
                return Err(invalid("Imported pose parameter binding changed"));
            }
        }
    }
    if let Some(states) = &animation.states {
        stock(
            manager,
            &states.parameters,
            0x80808EE1,
            "animation parameter dictionary",
        )?;
        stock(manager, &states.states, 0x80803465, "animation state table")?;
    }
    validate_rigs(manager, animation)?;
    let mut push = |allocator: AppendedTagAllocator,
                    tags: &mut Vec<NewTagSpec>,
                    template: u32,
                    payload: Vec<u8>,
                    what: &str|
     -> AuthoringResult<u32> {
        // Key on the asset class rather than the template tag. Weapons of one
        // source family convert to the same payload while replacing different
        // native clips, so a template-keyed cache would never match.
        let class = manager
            .get_entry(TagHash(template))
            .ok_or_else(|| invalid(format!("Imported animation {what} template is not live")))?
            .reference;
        use sha2::Digest;
        let key = (class, sha2::Sha256::digest(&payload).into());
        if let Some(existing) = cache.get(&key) {
            return Ok(*existing);
        }
        allocator.reserve_payload(tags.len(), payload.len())?;
        let assigned = allocator.assigned_tag(tags.len(), "Imported animation", what)?;
        cache.insert(key, assigned.0);
        tags.push(NewTagSpec {
            template_tag: TagHash(template),
            payload,
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
        Ok(assigned.0)
    };
    let mut bank = animation
        .converted_bank
        .as_ref()
        .unwrap_or(&animation.bank.1)
        .clone();
    for (native, payload) in &animation.clips {
        let entry = manager.get_entry(TagHash(*native)).ok_or_else(|| {
            invalid(format!(
                "Imported animation clip 0x{native:08X} is not live"
            ))
        })?;
        if entry.reference != CLIP_CLASS {
            return Err(invalid(format!(
                "Imported animation clip 0x{native:08X} is not a native clip"
            )));
        }
        if payload.len() < 0x190 || read_u64(payload, 0)? != payload.len() as u64 {
            return Err(invalid(format!(
                "Imported animation clip for 0x{native:08X} has an invalid payload size"
            )));
        }
        let payload = like_stock(manager, *native, payload)?;
        let private = push(assets, asset_tags, *native, payload, "clip")?;
        // Several bank ordinals can name the same clip. Every alias must reach
        // the same private source clip while the controller keeps its ordinals.
        relink(&mut bank, *native, private, "clip bank")?;
    }
    for (source, template, payload) in &animation.extra_clips {
        if manager
            .get_entry(TagHash(*template))
            .is_none_or(|entry| entry.reference != CLIP_CLASS)
            || payload.len() < 0x190
            || read_u64(payload, 0)? != payload.len() as u64
        {
            return Err(invalid(
                "Imported source clip has an invalid template or payload",
            ));
        }
        let payload = like_stock(manager, *template, payload)?;
        let private = push(assets, asset_tags, *template, payload, "source clip")?;
        relink(&mut bank, *source, private, "source clip bank")?;
    }
    let bank_tag = push(allocator, tags, animation.bank.0, bank, "clip bank")?;
    let mut lookup = animation.lookup_owner.1.clone();
    relink(&mut lookup, animation.bank.0, bank_tag, "lookup owner")?;
    if let Some(states) = &animation.states {
        let mut dictionary = states.converted_parameters.clone();
        if let Some(binding) = animation.shared_profile {
            let parent = animation
                .profile
                .as_ref()
                .map_or(binding.parent, |patch| patch.after);
            dictionary = parhelion_import::tiger::animation::state::parameter::alias(
                &parhelion_import::tiger::payload::Payload(dictionary),
                3,
                parent,
                binding.private,
            )
            .map_err(|error| invalid(format!("Shared animation profile: {error:#}")))?
            .0;
        }
        let parameters = push(
            allocator,
            tags,
            states.parameters.0,
            dictionary,
            "animation parameter dictionary",
        )?;
        let state_table = push(
            allocator,
            tags,
            states.states.0,
            states.converted_states.clone(),
            "animation state table",
        )?;
        let at = parhelion_import::tiger::payload::Payload(lookup.clone())
            .pointer(24)
            .map_err(|error| invalid(format!("Animation lookup: {error:#}")))?;
        if read_u32(&lookup, at + 0x94)? != states.parameters.0
            || read_u32(&lookup, at + 0x9C)? != states.states.0
        {
            return Err(invalid("Animation lookup tables changed"));
        }
        lookup[at + 0x94..at + 0x98].copy_from_slice(&parameters.to_le_bytes());
        lookup[at + 0x9C..at + 0xA0].copy_from_slice(&state_table.to_le_bytes());
    }
    let lookup_tag = allocator
        .assigned_tag(tags.len(), "Imported animation", "lookup owner")?
        .0;
    retarget_weapon_component_owner_payload(
        &mut lookup,
        &animation.entity.1,
        animation.lookup_owner.0,
        lookup_tag,
    )
    .map_err(invalid)?;
    tags.push(NewTagSpec {
        template_tag: TagHash(animation.lookup_owner.0),
        payload: lookup,
        storage: crate::NewTagStorageMode::InheritTemplate,
    });
    let mut entity = animation
        .converted_entity
        .clone()
        .unwrap_or_else(|| animation.entity.1.clone());
    for rig in &animation.rigs {
        let target: &mut [u8] = if rig.first_person {
            &mut entity
        } else {
            &mut *pattern_entity
        };
        let mut owner = rig.converted.clone();
        if let Some((cut, delta)) = rig.relocation {
            parhelion_import::tiger::animation::rig::rebase_entity(
                target,
                &parhelion_import::tiger::payload::Payload(rig.original.1.clone()),
                rig.original.0,
                cut,
                delta,
            )
            .map_err(|error| invalid(format!("Imported rig relocation: {error:#}")))?;
        }
        let assigned = allocator
            .assigned_tag(tags.len(), "Imported animation", "source rig owner")?
            .0;
        retarget_weapon_component_owner_payload(&mut owner, target, rig.original.0, assigned)
            .map_err(invalid)?;
        retarget_weapon_component_owner(target, rig.original.0, assigned).map_err(invalid)?;
        parhelion_import::tiger::animation::rig::retarget_rebased_references(
            target,
            owner.len(),
            rig.original.0,
            assigned,
        )
        .map_err(|error| invalid(format!("Imported rig references: {error:#}")))?;
        tags.push(NewTagSpec {
            template_tag: TagHash(rig.original.0),
            payload: owner,
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
    }
    let pose_layer_tag = animation
        .pose_layers
        .as_ref()
        .map(|layers| {
            push(
                allocator,
                tags,
                layers.original.0,
                layers.converted.clone(),
                "pose layers",
            )
        })
        .transpose()?;
    for (original, bytes) in &animation.bank_consumers {
        let converted = animation.converted_consumers.get(original);
        let mut payload = parhelion_import::tiger::payload::Payload(
            converted
                .map_or(bytes, |converted| &converted.payload)
                .clone(),
        );
        retarget_consumer(
            manager,
            (*original, &mut payload, converted.is_some()),
            animation,
            (bank_tag, pose_layer_tag),
            (allocator, tags),
            &mut push,
        )?;
        if let Some(converted) = converted {
            let private = push(
                allocator,
                tags,
                converted.allocation.0,
                converted.converted_allocation.clone(),
                "consumer allocation",
            )?;
            payload.0[0x44..0x48].copy_from_slice(&private.to_le_bytes());
        }
        let assigned = allocator
            .assigned_tag(tags.len(), "Imported animation", "bank consumer")?
            .0;
        retarget_weapon_component_owner_payload(
            &mut payload.0,
            &animation.entity.1,
            *original,
            assigned,
        )
        .map_err(invalid)?;
        tags.push(NewTagSpec {
            template_tag: TagHash(*original),
            payload: payload.0,
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
        retarget_weapon_component_owner(&mut entity, *original, assigned).map_err(invalid)?;
    }
    retarget_weapon_component_owner(&mut entity, animation.lookup_owner.0, lookup_tag)
        .map_err(invalid)?;
    if entity
        .chunks_exact(4)
        .any(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]) == animation.lookup_owner.0)
    {
        return Err(invalid(
            "Imported animation first-person entity keeps a stale lookup owner reference",
        ));
    }
    let entity_tag = push(
        allocator,
        tags,
        animation.entity.0,
        entity,
        "first-person entity",
    )?;
    let attachment_tag =
        attachment::author(animation, entity_tag, pattern_entity, allocator, tags)?;
    Ok((
        animation.clips.len() + animation.extra_clips.len(),
        attachment_tag,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relink_rewrites_every_aligned_word_and_requires_one() {
        let mut bytes = vec![0u8; 20];
        bytes[0..4].copy_from_slice(&0x80C6_9D7Eu32.to_le_bytes());
        bytes[12..16].copy_from_slice(&0x80C6_9D7Eu32.to_le_bytes());
        // A misaligned coincidence is not a reference.
        bytes[5..9].copy_from_slice(&0x80C6_9D7Eu32.to_le_bytes());
        assert_eq!(
            relink(&mut bytes, 0x80C6_9D7E, 0x8253_0001, "bank").unwrap(),
            2
        );
        assert_eq!(read_u32(&bytes, 0).unwrap(), 0x8253_0001);
        assert_eq!(read_u32(&bytes, 12).unwrap(), 0x8253_0001);
        assert!(relink(&mut bytes, 0x80C6_9D7E, 0x8253_0002, "bank").is_err());
    }
}
