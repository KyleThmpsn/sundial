//! Component modifiers on checked private native component envelopes.
use super::*;
use sundial::package_authoring::{
    entity::weapon_component_bindings,
    sandbox_perk::program::{
        Asset, AttachmentTarget, NativeAssetComponent, NativeAssetPatch, NativeAssetResourceAppend,
        NativeAssetResourcePatch,
    },
};
mod connected;
mod dynamic;
mod nested;
mod network;
mod sequence;
mod status;

struct Modifier {
    component: u8,
    ability: u16,
    input: u16,
    operation: u8,
    amount: f32,
    change: f32,
}

// These layouts have one constant modifier bank, no event connections, and exactly
// the requested number of initialized rows. Every row is replaced privately.
const ENVELOPES: [(usize, u32); 19] = [
    (1, 0x80BB760E),
    (2, 0x80B7CB8B),
    (3, 0x80BC578B),
    (4, 0x80B81B7A),
    (5, 0x80BC579F),
    (6, 0x80BC57F3),
    (7, 0x80BC5798),
    (8, 0x80BC579D),
    (9, 0x80BC58BA),
    (10, 0x80BC579A),
    (11, 0x80BC58AA),
    (12, 0x80BC57B1),
    (13, 0x8161E71D),
    (14, 0x80B7CB8D),
    (15, 0x80BC57E0),
    (17, 0x80BC5796),
    (19, 0x80C19A7E),
    (21, 0x80BC57C3),
    (23, 0x80C19A8B),
];

fn envelope(count: usize) -> Result<u32> {
    ENVELOPES
        .iter()
        .find_map(|&(size, tag)| (size == count).then_some(tag))
        .context("Source modifier count has no checked native envelope")
}

fn input(component: u8, ability: u16, source: u16) -> Result<u16> {
    // Source talent operands and native stock modifier roles qualify these lanes.
    // Original compiled stat programs also qualify accuracy, both projectile-speed
    // inputs and the full recoil range. Barrel ordinals diverge after Damage.
    // Charge input 1 adds hold time to input 0. The target retains its own clock,
    // state transitions and progress outputs, including for negative hold values.
    // Ability consumers preserve recharge input 0 and active drain input 2. Melee
    // input 5 scales the first queried profile coefficient after parameter rows.
    // Its consumer is not qualified for other slots. Player Armor, Mobility and
    // Recovery need the additive tier-to-point conversion below.
    // Selected-weapon consumers read controller inputs 7/8 as one minus current.
    // Their view-input attenuation retains these ordinals in both native builds.
    Ok(match (component, source) {
        (9, 5) if ability == 2 => source,
        (0, 0..=15 | 17 | 19..=21)
        | (1, 0..=4)
        | (2, 0..=9 | 11 | 13..=16 | 22..=25 | 30 | 32)
        | (3, 0..=2)
        | (4, 0..=1)
        | (5, 0..=7 | 9)
        | (9, 0 | 2)
        | (10, 1 | 5 | 6)
        | (13, 0..=2)
        | (14, 0 | 1 | 3..=11 | 13..=15 | 17 | 18) => source,
        (2, 33) => 36,
        (2, 34) => 39,
        (2, 38) => 43,
        (2, 39) => 47,
        (2, 42..=52 | 60) => source + 12,
        (2, 57 | 58) => source + 13,
        _ => anyhow::bail!(
            "Source modifier component {component} input {source} needs a native input contract"
        ),
    })
}

fn modifiers(source: &Source, tag: u32) -> Result<Vec<Modifier>> {
    ensure!(
        source.reference(tag)? == 0x80800734,
        "Source attachment is not a pattern"
    );
    let graph = source.read(tag)?;
    let components = graph.array(0x20, 12, Some(0x80800715))?;
    ensure!(
        components.len() == 1 && graph.bytes::<32>(0x30)? == [0; 32] && graph.u64(0x88)? == 0,
        "Source modifier pattern has companions, connections or condition masks"
    );
    let owner = graph.u32(components[0])?;
    ensure!(
        graph
            .array(0x68, 40, Some(0x80800370))?
            .iter()
            .all(|&at| graph.u32(at + 16).ok() == Some(owner)),
        "Source modifier graph exports another owner"
    );
    modifier_owner(source, &graph, owner)
}

fn modifier_owner(source: &Source, graph: &Payload, owner: u32) -> Result<Vec<Modifier>> {
    let indices = graph
        .array(32, 12, Some(0x80800715))?
        .into_iter()
        .enumerate()
        .filter_map(|(index, at)| (graph.u32(at).ok() == Some(owner)).then_some(index))
        .collect::<Vec<_>>();
    ensure!(indices.len() == 1, "Source modifier owner is ambiguous");
    let component_index = indices[0] as u64;
    ensure!(
        source.reference(owner)? == 0x80800861,
        "Source modifier owner class differs"
    );
    let p = source.read(owner)?;
    let runtime = p.pointer(16)?;
    let settings = p.pointer(24)?;
    ensure!(
        p.u32(runtime - 4)? == 0x808021F1
            && p.u32(settings - 4)? == 0x808020DE
            && p.u32(runtime)? == owner
            && p.u64(runtime + 8)? == settings as u64
            && p.u32(runtime + 16)? == 0x808020DE
            && p.u32(settings)? == owner
            && p.u64(settings + 8)? == runtime as u64
            && p.u32(settings + 16)? == 0x808021F1
            && p.u64(settings + 0x80)? == 0
            && p.u64(settings + 0x98)? == 0
            && p.u64(settings + 0xA0)? == 1,
        "Source modifier bank has an unsupported pair or enable driver"
    );
    let rows = p.array(settings + 0x60, 96, Some(0x8080205F))?;
    let instances = p.array(runtime + 0x30, 120, Some(0x80802B7B))?;
    ensure!(
        rows.len() == instances.len(),
        "Source modifier count has no checked native envelope"
    );
    envelope(rows.len())?;
    let mut result = Vec::new();
    for (&at, &instance) in rows.iter().zip(&instances) {
        ensure!(
            p.u32(at)? == owner
                && p.u64(at + 8)? == instance as u64
                && p.u32(at + 16)? == 0x80802B7B
                && p.u32(instance)? == owner
                && p.u64(instance + 8)? == at as u64
                && p.u32(instance + 16)? == 0x8080205F
                && p.u64(at + 0x28)? == 0
                && p.u64(at + 0x48)? == 1
                && p.bytes::<3>(at + 0x35)? == [0; 3]
                && p.bytes::<3>(at + 0x55)? == [0; 3]
                && p.u32(at + 0x58)? == 0
                && p.u32(at + 0x5C)? == u32::MAX
                && p.f32(instance + 0x40)? == 1.0
                && p.bytes::<4>(instance + 0x44)? == [1, 0, 0, 0],
            "Source modifier has stateful weight, metadata or a nonreciprocal pair"
        );
        let component = p.u8(at + 0x54)?;
        let ability = p.u16(at + 0x50)?;
        let ability = if component == 9 {
            // Both dispatchers select an ability reference before its numeric input.
            // Grenade, Super and Melee retain slots 0, 1 and 2. The source controller
            // has fewer slots, so do not carry arbitrary selectors into the native bank.
            ensure!(
                ability <= 2,
                "Source modifier ability slot has no native contract"
            );
            ability
        } else {
            ensure!(
                ability == 0,
                "Source modifier has unexpected ability metadata"
            );
            u16::MAX
        };
        let input = input(component, ability, p.u16(at + 0x52)?)?;
        let operation = p.u8(at + 0x34)?;
        let mut value = p.f32(at + 0x30)?;
        ensure!(
            value.is_finite() && operation <= 1,
            "Unsupported source modifier operation"
        );
        if component == 10 {
            // Radius input 1 is added to the object's base radius before squaring.
            // Native objects can override that input. Timing inputs 5/6 multiply by
            // min(reviver, target). Preserve relative factors on native baselines.
            ensure!(
                operation == 1,
                "Source player interaction input requires a relative multiplier"
            );
        }
        if component == 13 {
            ensure!(
                operation == 0,
                "Source player stat multiplication has no native unit contract"
            );
            // Original player definitions give Armor, Mobility and Recovery one
            // source unit per tier and ten native points per tier. The MIDA banks
            // independently corroborate the Mobility scale with two and twenty.
            // Scale the delta only, never the host's baseline or a multiplier.
            value *= 10.0;
        }
        // Original D1 695020 applies a signed change to the multiplier. Native
        // F40810 takes the final factor. The two weight curves differ away from
        // unit weight. Dynamic translation supplies a reciprocal weight curve
        // for negative changes and proves a bounded nonnegative source output.
        let amount = if operation == 0 {
            value
        } else if value > 0.0 {
            1.0 + value
        } else {
            1.0 / (1.0 - value)
        };
        ensure!(
            amount.is_finite() && (operation == 0 || amount > 0.0),
            "Invalid translated modifier amount"
        );
        result.push(Modifier {
            component,
            ability,
            input,
            operation,
            amount,
            change: value,
        });
    }
    // These are compiled parameter exports, not programs. Every export must
    // point at the same bank or one of its paired constant rows.
    let exports = graph.array(0x68, 40, Some(0x80800370))?;
    let references = graph.array(0x78, 24, Some(0x80800790))?;
    ensure!(
        exports
            .iter()
            .filter(|&&at| graph.u32(at + 16).ok() == Some(owner))
            .count()
            == rows.len() * 2 + 2
            && references.len() == exports.len(),
        "Source modifier pattern exposes additional behavior"
    );
    let mut covered = BTreeSet::new();
    let mut expected_exports =
        BTreeSet::from([(runtime, settings + 0x70), (runtime, settings + 0x88)]);
    for (&row, &instance) in rows.iter().zip(&instances) {
        expected_exports.extend([(instance, row + 0x18), (instance, row + 0x38)]);
    }
    for (&export, &reference) in exports.iter().zip(&references) {
        if graph.u32(export + 16)? != owner {
            continue;
        }
        let class = graph.u32(export + 12)?;
        let offset = graph.u32(export + 4)? as usize;
        let input = graph.u32(export + 8)?;
        let target = usize::try_from(graph.u64(reference + 8)?)?;
        let parameter = usize::try_from(graph.u64(export + 24)?)?;
        let bank = class == 0x808021F1 && offset == 0 && input == 0 && target == runtime;
        let row = class == 0x80802B7B
            && input <= 1
            && instances.contains(&target)
            && target == runtime + offset;
        ensure!(
            graph.u32(export)? == 0
                && graph.u32(export + 16)? == owner
                && graph.u32(export + 20)? == 0
                && graph.u32(export + 36)? == 0
                && graph.u32(reference)? == owner
                && graph.u32(reference + 4)? == 0
                && graph.u64(reference + 16)? == component_index
                && (bank || row)
                && expected_exports.contains(&(target, parameter))
                && graph.u32(export + 32)? == p.u32(parameter + 8)?
                && covered.insert((target, parameter)),
            "Source pattern has an unsupported parameter export"
        );
    }
    ensure!(
        covered == expected_exports,
        "Source modifier parameter exports are incomplete"
    );
    Ok(result)
}

pub(super) fn armor_attachment(source: &Source, p: &Payload, at: usize) -> Result<()> {
    armor_attachment_inner(source, p, at, &mut Vec::new())
}

pub(super) fn armor_attachment_inner(
    source: &Source,
    p: &Payload,
    at: usize,
    ancestors: &mut Vec<u32>,
) -> Result<()> {
    match p.u8(at + 2)? {
        1 | 2 => Ok(()),
        0 => {
            // Stock armor uses the action owner for player component modifiers too.
            // A checked host stays in the same graph as its player modifier bank.
            // A weapon component on an armor owner still has no runtime contract.
            let tag = p.u32(at + 16)?;
            let graph = source.read(tag)?;
            let owners = graph.array(32, 12, Some(0x80800715))?;
            if owners.len() == 1 {
                let owner = source.read(graph.u32(owners[0])?)?;
                if owner.u32(owner.pointer(24)? - 4)? == 0x80802A94 {
                    let child = nested::source_action(source, &graph)?;
                    return Host::Armor.validate_action_inner(source, child, ancestors);
                }
            }
            let rows = if matches!(owners.len(), 2 | 3) {
                let owner = source.read(graph.u32(owners[0])?)?;
                let class = owner.u32(owner.pointer(24)? - 4)?;
                ensure!(
                    matches!(class, 0x80802698 | 0x808029C3)
                        && (owners.len() == 2 || class == 0x80802698),
                    "Source armor attachment needs another host contract"
                );
                status::modifier_bank(source, &graph)?
                    .context("Source armor host has no modifier bank")?
                    .1
            } else {
                modifiers(source, tag)?
            };
            ensure!(
                // Both modifier constructors query these interfaces from the enclosing
                // unit. Player Stats retains its Add-only tier conversion, and revival
                // selectors retain their relative-factor contract.
                rows.iter()
                    .all(|row| matches!(row.component, 5 | 9 | 10 | 13)),
                "Source armor owner attachment requires player component modifiers"
            );
            Ok(())
        }
        _ => anyhow::bail!("Source armor attachment has an unsupported target"),
    }
}

fn status_key(owner: &Payload, tag: u32) -> Result<u32> {
    let settings = owner.pointer(24)?;
    let live = owner.pointer(16)?;
    let duration = settings + 0x250;
    ensure!(
        owner.u32(settings - 4)? == 0x80802698
            && owner.u32(live - 4)? == 0x80802A75
            && owner.u32(duration + 0xA0)? == 9,
        "Source attachment cleanup key needs a qualified timed status"
    );
    status::pair(owner, tag, live + 0x50, duration, 0x8080255D, 0x8080223C)?;
    owner.u32(duration + 0xA4)
}

/// Admit a removal key only when this action also creates its status publisher.
/// The ordinary effect path still validates all companions, timer and private patches.
pub(super) fn duration_end(source: &Source, action: &Payload, key: u32) -> Result<()> {
    ensure!(key != 0x811C9DC5, "Source duration removal key is empty");
    for row in action.array(0x38, 8, None)? {
        let at = action.pointer(row)?;
        if action.u32(at - 4)? != 0x80802953
            || action.u8(at)? != 0
            || action.u8(at + 1)? != 1
            || action.u8(at + 2)? > 1
            || action.u32(at + 24)? != key
        {
            continue;
        }
        let tag = action.u32(at + 16)?;
        ensure!(
            source.reference(tag)? == 0x80800734,
            "Source status pattern"
        );
        let graph = source.read(tag)?;
        let owners = graph.array(32, 12, Some(0x80800715))?;
        let tag = graph.u32(*owners.first().context("Source status owner")?)?;
        ensure!(
            source.reference(tag)? == 0x80800861,
            "Source status owner class"
        );
        ensure!(
            status_key(&source.read(tag)?, tag)? == key,
            "Source duration removal key disagrees with its status publisher"
        );
        return Ok(());
    }
    anyhow::bail!("Source duration removal has no matching status publisher in this action")
}

pub(super) fn read(
    source: &Source,
    native: &mut Reader,
    p: &Payload,
    at: usize,
    ancestors: &mut Vec<u32>,
    omitted_feedback: &mut usize,
) -> Result<Vec<(Action, NativeAssetPatch)>> {
    if (p.u32(at - 4)?, p.u8(at)?) == (0x80802B8D, 1) {
        return dynamic::read(source, native, p, at);
    }
    ensure!(
        p.u8(at + 1)? == 1
            && p.u8(at + 2)? <= 2
            && p.bytes::<5>(at + 3)? == [0; 5]
            && p.u32(at + 20)? == 0
            && p.u32(at + 28)? == 0x811C9DC5
            && p.bytes::<16>(at + 32)? == [0; 16],
        "Source pattern needs target, retained-state or cleanup translation"
    );
    let source_tag = p.u32(at + 16)?;
    ensure!(
        source.reference(source_tag)? == 0x80800734,
        "Source attachment is not a pattern"
    );
    let source_graph = source.read(source_tag)?;
    let source_owners = source_graph.array(32, 12, Some(0x80800715))?;
    ensure!(
        matches!(source_owners.len(), 1..=3),
        "Source attachment has additional components"
    );
    let owner_tag = source_graph.u32(source_owners[0])?;
    ensure!(
        source.reference(owner_tag)? == 0x80800861,
        "Source attachment owner class differs"
    );
    let owner = source.read(owner_tag)?;
    ensure!(
        source_owners.len() != 3 || owner.u32(owner.pointer(24)? - 4)? == 0x80802698,
        "Source three-owner attachment requires a qualified status host"
    );
    if p.u32(at + 24)? != 0x811C9DC5 {
        // A nonempty first key suppresses the action's automatic retirement in
        // both runtimes. Retain it only on a separately qualified timed status
        // whose matching Duration identity agrees. The second key names a removal
        // write and remains closed. Status validation qualifies every companion.
        ensure!(
            status_key(&owner, owner_tag)? == p.u32(at + 24)?,
            "Source attachment cleanup key needs a qualified timed status"
        );
    }
    if owner.u32(owner.pointer(24)? - 4)? == 0x80802A94 {
        if p.u8(at + 2)? == 1 {
            // A player attachment cannot supply an owning weapon to the child.
            // Apply the same player-host restrictions used by an armor action.
            Host::Armor.validate_action(source, nested::source_action(source, &source_graph)?)?;
        }
        let (tag, program, omitted) = nested::read(source, native, &source_graph, ancestors)?;
        *omitted_feedback = omitted_feedback
            .checked_add(omitted)
            .context("Nested feedback record count overflow")?;
        return Ok(vec![(
            attachment(p, at, tag, None)?,
            NativeAssetPatch {
                action_index: 0,
                source_graph: tag,
                patches: Vec::new(),
                appends: Vec::new(),
                remove_owners: Vec::new(),
                components: Vec::new(),
                programs: vec![program],
            },
        )]);
    }
    if owner.u32(owner.pointer(24)? - 4)? == 0x808029C3 {
        let edit = connected::read(source, native, &source_graph)?;
        return Ok(vec![(attachment(p, at, edit.source_graph, None)?, edit)]);
    }
    if owner.u32(owner.pointer(24)? - 4)? == 0x80800610 {
        ensure!(
            p.u8(at + 2)? == 1,
            "HUD sequence requires a player attachment"
        );
        let (tag, patches) = sequence::read(source, native, &source_graph)?;
        return Ok(vec![(
            attachment(p, at, tag, None)?,
            NativeAssetPatch {
                action_index: 0,
                source_graph: tag,
                patches,
                appends: Vec::new(),
                remove_owners: Vec::new(),
                components: Vec::new(),
                programs: Vec::new(),
            },
        )]);
    }
    if owner.u32(owner.pointer(24)? - 4)? == 0x80802698 {
        if p.u8(at + 2)? == 0 {
            armor_attachment(source, p, at)?;
        }
        ensure!(
            p.u8(at + 2)? <= 1,
            "Source status must attach to a qualified player host"
        );
        let (edit, hud_status) = status::read(source, native, &source_graph, &owner, owner_tag)?;
        return Ok(vec![(
            attachment(p, at, edit.source_graph, hud_status)?,
            edit,
        )]);
    }
    let (tag, patches) = if source_owners.len() == 2
        && source.reference(owner.u32(owner.pointer(24)? - 4)?).ok() == Some(0x80800000)
        && source.read(owner.u32(owner.pointer(24)? - 4)?)?.u32(16)? == 0x80800362
    {
        network::modifier(source, native, &source_graph)?
    } else {
        modifier_patch(native, &modifiers(source, source_tag)?)?
    };
    let action = attachment(p, at, tag, None)?;
    Ok(vec![(
        action,
        NativeAssetPatch {
            action_index: 0,
            source_graph: tag,
            patches,
            appends: Vec::new(),
            remove_owners: Vec::new(),
            components: Vec::new(),
            programs: Vec::new(),
        },
    )])
}

fn attachment(
    p: &Payload,
    at: usize,
    tag: u32,
    hud_status: Option<sundial::package_authoring::sandbox_perk::program::HudStatus>,
) -> Result<Action> {
    Ok(Action::Attach {
        asset: Asset {
            graph: tag,
            hud_status,
            ..Asset::default()
        },
        mode: match p.u8(at + 2)? {
            0 => AttachmentTarget::ThisItem,
            1 => AttachmentTarget::Player,
            _ => AttachmentTarget::TriggeringWeapon,
        },
        keys: [p.u32(at + 24)?, p.u32(at + 28)?],
        float_bits: [0; 4],
    })
}

fn modifier_patch(
    native: &mut Reader,
    rows: &[Modifier],
) -> Result<(u32, Vec<NativeAssetResourcePatch>)> {
    // Each shipped graph contains only one modifier bank with the same number
    // of live rows. Private patches replace every row, retaining native allocation
    // and initialization without inheriting the carrier's numeric behavior.
    let tag = envelope(rows.len())?;
    let graph = native.tag(tag, Some(0x80809C0F))?;
    let owners = graph.array(16, 12, Some(0x80809C04))?;
    ensure!(
        owners.len() == 1 && graph.bytes::<32>(32)? == [0; 32],
        "Native modifier envelope has companions or connections"
    );
    let owner = graph.u32(owners[0])?;
    Ok((tag, modifier_patches(native, &graph, owner, rows)?))
}

fn modifier_patches(
    native: &mut Reader,
    graph: &Payload,
    owner: u32,
    rows: &[Modifier],
) -> Result<Vec<NativeAssetResourcePatch>> {
    let target = native.tag(owner, Some(0x80809C36))?;
    let settings = target.pointer(24)?;
    ensure!(
        target.u32(settings - 4)? == 0x80803AFE
            && target.u64(settings + 0x78)? == 0
            && target.u64(settings + 0x90)? == 0
            && target.u64(settings + 0x98)? == 1,
        "Native modifier bank default enable differs"
    );
    let settings_rows = target.array(settings + 88, 88, Some(0x80803B06))?;
    let bindings = weapon_component_bindings(&graph.0, 0x7330E39F).map_err(anyhow::Error::msg)?;
    ensure!(
        settings_rows.len() == rows.len() && bindings.len() == rows.len(),
        "Native modifier envelope has a different runtime row count"
    );
    let mut patches = Vec::new();
    for (row, to) in rows.iter().zip(settings_rows) {
        let instance = target.u64(to + 8)?;
        let binding = bindings
            .iter()
            .find(|b| {
                b.owner_tag == owner
                    && b.concrete_class == 0x80803B05
                    && b.resource_offset == instance
            })
            .context("Native modifier row binding")?;
        ensure!(
            target.u32(to)? == owner
                && target.u32(to + 4)? == 0x80803B05
                && target.u64(instance as usize + 8)? == to as u64
                && target.u64(to + 32)? == 0
                && target.u64(to + 64)? == 1
                && target.f32(instance as usize + 64)? == 1.0
                && target.u8(instance as usize + 68)? == 1,
            "Native modifier row has a different pair or weight"
        );
        let expected = target.bytes::<88>(to)?;
        let mut bytes = expected;
        bytes[40..44].copy_from_slice(&row.amount.to_le_bytes());
        bytes[44] = row.operation;
        bytes[72..74].copy_from_slice(&row.ability.to_le_bytes());
        bytes[74..76].copy_from_slice(&row.input.to_le_bytes());
        bytes[76] = row.component;
        patches.push(NativeAssetResourcePatch {
            binding_hash: binding.binding_hash,
            resource_index: u16::try_from(binding.resource_index)?,
            offset: u32::try_from(
                to.checked_sub(usize::try_from(instance)?)
                    .context("Native modifier definition precedes instance")?,
            )?,
            expected: expected.to_vec(),
            bytes: bytes.to_vec(),
            imported_particle: None,
        });
    }
    Ok(patches)
}
