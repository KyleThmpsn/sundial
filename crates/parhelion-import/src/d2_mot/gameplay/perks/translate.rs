//! Complete package-derived inputs for the supported sword controller translation.
//!
//! Extraction preserves every assigned controller and its auxiliary records. It does
//! not establish native event publication or map the auxiliary runtime consumer.
use std::{collections::BTreeSet, sync::Arc};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use super::{controller, lower, sword};
use crate::d2_mot::{payload::Payload, reader::Reader};

const EMPTY_KEY: u32 = 0x811C_9DC5;
const CONDITION_CLASSES: [u32; 5] = [
    0x8080_3086,
    0x8080_3060,
    0x8080_BDCF,
    0x8080_30BE,
    0x8080_3061,
];

pub struct Source {
    pub plug_hash: u32,
    pub item_tag: u32,
    pub perk_hash: u32,
    pub runtime_key: u32,
    pub action_tag: u32,
    pub controller: Arc<Payload>,
    pub routing: Routing,
    pub lunge: LungeAttachment,
    pub angular: AngularAttachment,
    pub movement_offset: usize,
}

#[derive(Debug, Serialize)]
pub struct Routing {
    pub draw_offset: usize,
    pub active_offset: usize,
    pub removals: Vec<usize>,
    pub rearm_offset: usize,
    pub cooldown_ms: u32,
    pub effects: Vec<usize>,
    /// Preserved source descriptors with an unmapped runtime consumer.
    pub auxiliary: Vec<controller::Auxiliary>,
}

pub struct LungeAttachment {
    pub effect_offset: usize,
    pub path: String,
    pub entity_tag: u32,
    pub owner_tag: u32,
    pub owner: Arc<Payload>,
    pub modifier_offset: usize,
}

pub struct AngularAttachment {
    pub effect_offset: usize,
    pub path: String,
    pub entity_tag: u32,
    pub owner_tag: u32,
    pub owner: Arc<Payload>,
    pub scales: sword::AngularScales,
}

fn rows(p: &Payload, at: usize, stride: usize, class: u32) -> Result<Vec<usize>> {
    ensure!(p.u64(at)? <= 256, "source list exceeds limit at {at:X}");
    array_rows(p, at, stride, class)
}

fn array_rows(p: &Payload, at: usize, stride: usize, class: u32) -> Result<Vec<usize>> {
    if p.u64(at)? == 0 {
        ensure!(
            p.u64(at + 8)? == 0,
            "empty source list has a pointer at {at:X}"
        );
        return Ok(Vec::new());
    }
    let header = p.pointer(at + 8)?;
    ensure!(
        header >= 4 && p.u32(header - 4)? == 0x8080_9FB8 && p.u64(header + 8)? == u64::from(class),
        "source list marker or class differs at {at:X}"
    );
    p.array(at, stride, Some(class))
}

fn neutral_state(p: &Payload, state: &controller::State) -> Result<()> {
    let at = state.offset;
    ensure!(
        p.u32(at + 4)? == 0
            && p.u64(at + 24)? == 0
            && p.u64(at + 48)? == 0
            && p.u64(at + 56)? == 0
            && p.u64(at + 72)? == 0
            && p.u64(at + 80)? == 0
            && p.u64(at + 88)? == u64::MAX
            && p.bytes::<40>(at + 96)? == [0; 40],
        "unsupported source state metadata at {at:X}"
    );
    ensure!(
        state.transitions.len() == 1,
        "supported source states need exactly one transition at {at:X}"
    );
    let transition = &state.transitions[0];
    let at = transition.offset;
    ensure!(
        transition.key == EMPTY_KEY
            && p.u32(at + 4)? == 0
            && p.bytes::<16>(at + 32)? == [0; 16]
            && transition
                .conditions
                .iter()
                .all(|condition| condition.children.is_empty()),
        "unsupported source transition metadata or nesting at {at:X}"
    );
    Ok(())
}

fn state_effects(p: &Payload, state: &controller::State) -> Result<Vec<usize>> {
    rows(p, state.offset + 8, 24, 0x8080_37AB)?
        .into_iter()
        .map(|row| {
            ensure!(
                p.u64(row)? != 0 && p.bytes::<16>(row + 8)? == [0; 16],
                "unsupported source effect row at {row:X}"
            );
            let effect = p.pointer(row)?;
            ensure!(effect >= 4, "source effect lacks a class at {row:X}");
            Ok(effect)
        })
        .collect()
}

fn dynamic_program(p: &Payload, at: usize) -> Result<(Vec<u8>, Vec<u8>)> {
    ensure!(
        p.u32(at - 4)? == 0x8080_3130 && p.bytes::<8>(at)? == [2, 1, 1, 0, 0, 0, 0, 0],
        "unsupported source dynamic attachment at {at:X}"
    );
    ensure!(
        p.u64(at + 32)? == 0
            && p.u64(at + 72)? == 1
            && p.u64(at + 80)? == 1
            && p.u64(at + 88)? == 0
            && p.u64(at + 96)? == 0
            && p.u16(at + 106)? == 0
            && p.u32(at + 108)? == EMPTY_KEY
            && p.u32(at + 112)? == 0
            && matches!(p.u8(at + 104)?, 0 | 1 | 255)
            && p.u8(at + 105)? <= 1,
        "unsupported source dynamic input metadata at {at:X}"
    );
    ensure!(
        p.u64(at + 40)? <= 4096 && p.u64(at + 56)? <= 256,
        "source dynamic program exceeds native limits at {at:X}"
    );
    let code = array_rows(p, at + 40, 1, 0x8080_0009)?
        .into_iter()
        .map(|row| p.u8(row))
        .collect::<Result<Vec<_>>>()?;
    let constants = array_rows(p, at + 56, 16, 0x8080_0090)?
        .into_iter()
        .map(|row| p.bytes::<16>(row))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    crate::d2_mot::native::effects::lower_program(&code, constants.len() / 16, 1)?;
    Ok((code, constants))
}

/// Recognize the complete supported draw, active and cooldown cycle by structure.
/// Node offsets are discovered from typed arrays and are never source tag identities.
pub fn routing(p: &Payload) -> Result<Routing> {
    ensure!(
        p.u64(8)? == 0
            && p.u64(16)? == 0
            && p.u32(64)? == EMPTY_KEY
            && p.bytes::<4>(68)? == [1, 3, 2, 0]
            && p.u32(72)? == 0,
        "unsupported source controller root metadata"
    );
    let decoded = controller::read(p)?;
    ensure!(
        decoded.states.len() == 3,
        "supported source controller needs three states"
    );
    let mut keys = BTreeSet::new();
    for state in &decoded.states {
        ensure!(
            keys.insert(state.key),
            "source controller repeats a state key"
        );
        neutral_state(p, state)?;
    }
    let idle = &decoded.states[0];
    let draw = &idle.transitions[0];
    ensure!(
        draw.conditions.len() == 1
            && draw.conditions[0].class == 0x8080_30AE
            && draw.conditions[0].kind == 16,
        "source initial state has no supported draw transition"
    );
    lower::draw_condition_prefix(p, draw.conditions[0].offset)?;
    let active = &decoded.states[1];
    let cooldown = &decoded.states[2];
    ensure!(
        draw.destination == active.offset
            && active.transitions[0].destination == cooldown.offset
            && cooldown.transitions[0].destination == idle.offset,
        "unsupported source draw, active and rearm topology"
    );
    ensure!(
        state_effects(p, idle)?.is_empty() && state_effects(p, cooldown)?.is_empty(),
        "source inactive states contain unsupported effects"
    );
    let removal = &active.transitions[0].conditions;
    ensure!(
        removal.len() == CONDITION_CLASSES.len()
            && removal
                .iter()
                .map(|condition| condition.class)
                .eq(CONDITION_CLASSES),
        "unsupported source active exit conditions"
    );
    for condition in removal {
        lower::condition(p, condition.offset)?;
    }
    let rearm = &cooldown.transitions[0].conditions;
    ensure!(
        rearm.len() == 1 && rearm[0].class == 0x8080_3060 && rearm[0].kind == 1,
        "unsupported source cooldown transition"
    );
    lower::timer_condition(p, rearm[0].offset)?;
    ensure!(
        p.u32(rearm[0].offset)? == 1.0f32.to_bits() && p.u8(rearm[0].offset + 6)? == 0,
        "source rearm probability or inversion cannot use native cooldown"
    );
    let seconds = p.f32(rearm[0].offset + 8)?;
    let milliseconds = (seconds * 1000.0).round();
    ensure!(
        milliseconds > 0.0
            && milliseconds <= 3_600_000.0
            && (milliseconds / 1000.0).to_bits() == seconds.to_bits(),
        "source cooldown cannot be represented exactly in milliseconds"
    );
    let effects = state_effects(p, active)?;
    ensure!(
        effects.len() == 3
            && p.u32(effects[0] - 4)? == 0x8080_3130
            && p.u32(effects[1] - 4)? == 0x8080_3130
            && p.u32(effects[2] - 4)? == 0x8080_30F3,
        "unsupported source active effects"
    );
    dynamic_program(p, effects[0])?;
    let (code, constants) = dynamic_program(p, effects[1])?;
    ensure!(
        code == [0x42, 0, 0x4C, 0]
            && constants.len() == 16
            && constants
                .chunks_exact(4)
                .all(|lane| lane == 1.0f32.to_le_bytes())
            && p.u8(effects[1] + 104)? == 255
            && p.u8(effects[1] + 105)? == 0,
        "source angular attachment must be the checked constant-one program"
    );
    lower::host_record(p, effects[2])?;
    ensure!(
        decoded.auxiliary.len() == 1,
        "unsupported source auxiliary count"
    );
    let auxiliary = &decoded.auxiliary[0];
    let at = auxiliary.offset;
    ensure!(
        auxiliary.class == controller::STATE_AUXILIARY
            && auxiliary.state_offsets == [active.offset]
            && auxiliary.state_keys == [active.key]
            && auxiliary.unknown_integer == Some(150)
            && auxiliary
                .key
                .is_some_and(|key| ![0, u32::MAX, EMPTY_KEY].contains(&key))
            && p.bytes::<8>(at)? == [0, 255, 0, 0, 0, 0, 0, 0]
            && p.u64(at + 32)? == 0
            && p.u32(at + 40)? == 255
            && p.u32(at + 44)? == EMPTY_KEY
            && p.u64(at + 48)? == 0,
        "unsupported source auxiliary descriptor"
    );
    Ok(Routing {
        draw_offset: draw.conditions[0].offset,
        active_offset: active.offset,
        removals: removal.iter().map(|condition| condition.offset).collect(),
        rearm_offset: rearm[0].offset,
        cooldown_ms: milliseconds as u32,
        effects,
        auxiliary: decoded.auxiliary,
    })
}

struct Attachment {
    path: String,
    entity_tag: u32,
    owner_tag: u32,
    owner: Arc<Payload>,
    modifiers: Vec<usize>,
}

fn attachment(r: &mut Reader, p: &Payload, at: usize) -> Result<Attachment> {
    let path = super::path(p, at + 8)?.context("source attachment has no path")?;
    ensure!(!path.is_empty(), "source attachment path is empty");
    let entity_tag = r.ref64(p, at + 16)?;
    let entity = r.tag(entity_tag, Some(0x8080_9AD8))?;
    ensure!(
        entity.u64(0)? == entity.0.len() as u64,
        "source entity declared size differs"
    );
    let components = rows(&entity, 8, 12, 0x8080_9ACD)?;
    ensure!(
        components.len() == 1,
        "source attachment needs one modifier component"
    );
    let owner_tag = entity.u32(components[0])?;
    let owner = r.tag(owner_tag, Some(0x8080_9B06))?;
    ensure!(
        owner.u64(0)? == owner.0.len() as u64,
        "source owner declared size differs"
    );
    let instance = owner.pointer(16)?;
    let settings = owner.pointer(24)?;
    ensure!(
        instance >= 4
            && settings >= 4
            && owner.u32(instance - 4)? == 0x8080_2D2A
            && owner.u32(settings - 4)? == 0x8080_2D2B
            && owner.u32(instance)? == owner_tag
            && owner.u32(instance + 4)? == 0x8080_2D2B
            && owner.u64(instance + 8)? == settings as u64
            && owner.u32(settings)? == owner_tag
            && owner.u32(settings + 4)? == 0x8080_2D2A
            && owner.u64(settings + 8)? == instance as u64,
        "source modifier component pair differs"
    );
    let modifiers = rows(&owner, settings + 88, 112, super::SETTINGS)?;
    let runtime = rows(&owner, instance + 80, 128, super::MODIFIER)?;
    ensure!(
        runtime.len() == modifiers.len(),
        "source modifier row counts differ"
    );
    // Shared tracing checks every live/settings pair and records its metadata tags.
    super::modifiers(r, owner_tag, &owner, instance, settings)?;
    Ok(Attachment {
        path,
        entity_tag,
        owner_tag,
        owner,
        modifiers,
    })
}

fn lunge_modifier(owner: &Payload, at: usize) -> Result<()> {
    owner.bytes::<112>(at)?;
    ensure!(
        owner.u8(at + 96)? == 12 && owner.u16(at + 90)? == 3,
        "source lunge modifier destination differs"
    );
    lower::modifier_input(owner.u8(at + 96)?, owner.u16(at + 90)?)?;
    ensure!(
        owner.u8(at + 20)? <= 1
            && owner.bytes::<3>(at + 21)? == [0; 3]
            && i64::from_le_bytes(owner.bytes(at + 24)?) == -24
            && owner.u32(at + 36)? == 0
            && owner.u64(at + 40)? == 0
            && owner.u64(at + 48)? == 0
            && i64::from_le_bytes(owner.bytes(at + 56)?) == -56
            && owner.u32(at + 68)? == 0
            && owner.u64(at + 72)? == 1
            && owner.u64(at + 80)? == 0
            && owner.i16(at + 88)? == -1
            && owner.u32(at + 92)? == EMPTY_KEY
            && owner.bytes::<3>(at + 97)? == [0; 3]
            && owner.u32(at + 100)? == 0
            && owner.u32(at + 104)? == u32::MAX
            && owner.u32(at + 108)? == 0,
        "unsupported source lunge modifier metadata or operation"
    );
    owner.f32(at + 16)?;
    Ok(())
}

/// Extract every supported runtime controller assigned to a modern source plug.
/// Enhanced plugs can own multiple actions, which remain separate source records.
pub fn extract_all(r: &mut Reader, plug_hash: u32) -> Result<Vec<Source>> {
    let (item_tag, item) = super::item(r, plug_hash)?;
    let block = super::resource(&item, 0x68, 0x8080_7381)?;
    let identities = super::table(r, 0x8080_76AA)?;
    let indices = array_rows(&identities, 8, 12, 0x8080_76AE)?;
    let finished = super::table(r, 0x8080_542D)?;
    let definitions = array_rows(&finished, 8, 40, 0x8080_5433)?;
    let perks = rows(&item, block + 16, 32, 0x8080_7387)?;
    ensure!(!perks.is_empty(), "source plug has no sandbox perks");
    let mut result = Vec::with_capacity(perks.len());
    let mut assigned = BTreeSet::new();
    for source in perks {
        let index = usize::try_from(item.u32(source)?)?;
        let identity = *indices
            .get(index)
            .context("perk index outside source catalog")?;
        let perk_hash = identities.u32(identity)?;
        let matches = definitions
            .iter()
            .copied()
            .filter(|at| finished.u32(*at).ok() == Some(perk_hash))
            .collect::<Vec<_>>();
        ensure!(
            matches.len() == 1,
            "missing or ambiguous source perk {perk_hash:08X}"
        );
        let runtime_key = finished.u32(matches[0] + 4)?;
        let action_tag = super::action(r, runtime_key)?
            .with_context(|| format!("source perk {perk_hash:08X} has no standalone action"))?;
        ensure!(
            assigned.insert(action_tag),
            "source plug repeats an assigned controller"
        );
        let controller = r.tag(action_tag, Some(super::CONTROLLER))?;
        let routing =
            routing(&controller).with_context(|| format!("source controller {action_tag:08X}"))?;
        let lunge = attachment(r, &controller, routing.effects[0])?;
        ensure!(
            lunge.modifiers.len() == 1,
            "source lunge component needs one modifier"
        );
        let modifier_offset = lunge.modifiers[0];
        lunge_modifier(&lunge.owner, modifier_offset)?;
        let angular = attachment(r, &controller, routing.effects[1])?;
        let scales = sword::angular_scales(&angular.owner)?;
        let movement_offset = routing.effects[2];
        result.push(Source {
            plug_hash,
            item_tag,
            perk_hash,
            runtime_key,
            action_tag,
            controller,
            lunge: LungeAttachment {
                effect_offset: routing.effects[0],
                path: lunge.path,
                entity_tag: lunge.entity_tag,
                owner_tag: lunge.owner_tag,
                owner: lunge.owner,
                modifier_offset,
            },
            angular: AngularAttachment {
                effect_offset: routing.effects[1],
                path: angular.path,
                entity_tag: angular.entity_tag,
                owner_tag: angular.owner_tag,
                owner: angular.owner,
                scales,
            },
            movement_offset,
            routing,
        });
    }
    Ok(result)
}

/// Extract a plug only when its complete runtime contains one supported controller.
/// Multi-controller plugs require an explicit adapter for all their source actions.
pub fn extract(r: &mut Reader, plug_hash: u32) -> Result<Source> {
    let mut sources = extract_all(r, plug_hash)?;
    ensure!(
        sources.len() == 1,
        "source plug {plug_hash:08X} has {} controllers and needs a combined adapter",
        sources.len()
    );
    sources
        .pop()
        .context("source plug has no translated controller")
}
