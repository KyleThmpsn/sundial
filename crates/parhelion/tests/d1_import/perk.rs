//! Follow source talents through emitted sockets, private programs and stat contributions.
// Each readback walks one native record field by field, which reads best as one function.
#![allow(clippy::cognitive_complexity)]
use super::*;
mod direct;
mod event;
mod feedback;
mod fraction;
mod grant;
mod hud;
mod pattern;
mod timer;
mod transfer;
mod weighted;
type CheckResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[allow(clippy::cognitive_complexity)]
pub(super) fn check(
    source: &Source,
    stock: &mut Reader,
    readback: &mut Reader,
    item_tag: u32,
    recipe: &WeaponRecipe,
    receipt: &Value,
) -> CheckResult<Value> {
    let tag = readback
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|n| n.name == "investment_globals")
        .ok_or("Globals")?
        .hash
        .0;
    let globals = readback.tag(tag, None)?;
    let definition = readback.tag(item_tag, Some(0x80807BEA))?;
    // Follow each staged choice through its plug, finished-perk
    // row and runtime assignment. Source bytes, not the authored program,
    // are the independent oracle for the supported scalar effects.
    let root = readback.tag(globals.u32(16)?, None)?;
    let stat_definitions = readback.tag(root.u32(8 + 95 * 16)?, None)?;
    let stat_rows = stat_definitions.array(8, 32, Some(0x80807D09))?;
    let items = readback.tag(root.u32(8 + 48 * 16)?, None)?;
    let item_rows = items.array(8, 24, Some(0x80807BE8))?;
    let finished = readback.tag(globals.u32(16 + 71 * 16)?, Some(0x80805C97))?;
    let finished_rows = finished.array(8, 24, Some(0x80805C9D))?;
    let runtime = readback.tag(0x80EC3F60, None)?;
    let runtime_rows = runtime.array(8, 8, None)?;
    let sockets = definition.array(definition.pointer(0x68)?, 80, Some(0x808077C4))?;
    let source_directory =
        Path::new(receipt["folder"].as_str().ok_or("Source directory")?).join("source");
    let translated = receipt["graph"]["perk_translation"]["translated"]
        .as_array()
        .ok_or("Perk translation report")?;
    let mut behaviors = Vec::new();
    // Unsupported source talents must not leave an active default or a donor roll pool.
    // The same contract applies when the source grid contributes no supported choices.
    for (lane, column) in recipe.overrides.socket_columns.iter().enumerate() {
        let Some(column) = column else { continue };
        if column.choices.is_empty() {
            let socket = *sockets.get(lane).ok_or("Disabled socket lane")?;
            for offset in [0, 2, 12, 32] {
                assert_eq!(definition.u16(socket + offset)?, u16::MAX);
            }
            assert!(
                definition
                    .array(socket + 0x40, 32, Some(0x80802E03))?
                    .is_empty()
            );
            behaviors.push(json!({"disabled_socket":lane,
                "socket_type":definition.u16(socket)?,
                "default_plug":definition.u16(socket + 2)?,
                "reusable_set":definition.u16(socket + 12)?,
                "randomized_set":definition.u16(socket + 32)?,
                "embedded_choices":definition.array(socket + 0x40,32,Some(0x80802E03))?.len()}));
        }
    }
    let source_reader = source;
    // Discover mutually exclusive damage steps from the source payloads, independently of
    // the translator's report. Every alternative must resolve through one selectable socket.
    let source_grid: Value = serde_json::from_slice(&fs::read(source_directory.join(
        if receipt["graph"]["kind"] == "armor" {
            "armor.json"
        } else {
            "weapon.json"
        },
    ))?)?;
    let mut damage_steps = Vec::new();
    for node in source_grid["talents"]["nodes"]
        .as_array()
        .ok_or("Source talent nodes")?
    {
        for step in node["steps"].as_array().ok_or("Source talent steps")? {
            for variant in step["variants"].as_array().ok_or("Source variants")? {
                let Some(perks) = variant["perks"].as_array() else {
                    continue;
                };
                if perks.len() != 1 {
                    continue;
                }
                let Some(tag) = perks[0]["action"].as_u64() else {
                    continue;
                };
                let bytes = source.read(u32::try_from(tag)?)?;
                let effects = bytes.array(0x38, 8, None)?;
                if effects.len() != 1 {
                    continue;
                }
                let at = bytes.pointer(effects[0])?;
                if bytes.u32(at - 4)? == 0x80802AC6 && bytes.u8(at)? == 5 {
                    damage_steps.push((
                        node["index"].clone(),
                        step["index"].clone(),
                        bytes.u8(at + 2)?,
                    ));
                }
            }
        }
    }
    // Every damage effect any source talent carries, beside other perks and stat rows or not.
    // Rise of Iron applies an element only through such an effect.
    let mut damage_nodes = BTreeSet::new();
    let mut source_modes = BTreeSet::new();
    for node in source_grid["talents"]["nodes"]
        .as_array()
        .ok_or("Source talent nodes")?
    {
        for step in node["steps"].as_array().ok_or("Source talent steps")? {
            for variant in step["variants"].as_array().ok_or("Source variants")? {
                for perk in variant["perks"].as_array().into_iter().flatten() {
                    let Some(tag) = perk["action"].as_u64() else {
                        continue;
                    };
                    if perk["action_class"] != 0x80802711u32 {
                        continue;
                    }
                    let bytes = source.read(u32::try_from(tag)?)?;
                    for row in bytes.array(0x38, 8, None)? {
                        let at = bytes.pointer(row)?;
                        if bytes.u32(at - 4)? == 0x80802AC6 && bytes.u8(at)? == 5 {
                            assert_eq!(bytes.u8(at + 1)?, 1);
                            damage_nodes.insert(node["index"].as_u64().ok_or("Node index")?);
                            source_modes.insert(bytes.u8(at + 2)?);
                        }
                    }
                }
            }
        }
    }
    // Native damage markers: elemental perks and type-68 socket defaults, none for Kinetic.
    let native_damage = definition
        .array(definition.pointer(0x70)? + 16, 24, Some(0x808077BC))?
        .iter()
        .filter_map(|&at| match definition.u16(at).ok()? {
            83 | 449 => Some(1),
            84 | 450 => Some(2),
            85 | 451 => Some(3),
            _ => None,
        })
        .chain(sockets.iter().filter_map(|&at| {
            (definition.u16(at).ok()? == 68)
                .then(|| match definition.u16(at + 2).ok()? {
                    1505 => Some(1),
                    1506 => Some(2),
                    1507 => Some(3),
                    _ => None,
                })
                .flatten()
        }))
        .collect::<Vec<_>>();
    if receipt["graph"]["kind"] != "armor" && source_modes.len() <= 1 {
        // One source type, or none at all, fixes the weapon's damage. Source selectors are
        // Kinetic 0, Solar 1, Arc 2 and Void 3. Native markers number Arc 1, Solar 2, Void 3.
        let mode = match source_modes.first().copied().unwrap_or(0) {
            0 => 0,
            1 => 2,
            2 => 1,
            3 => 3,
            _ => return Err("Unsupported source damage selector".into()),
        };
        assert_eq!(
            native_damage,
            if mode == 0 { Vec::new() } else { vec![mode] },
            "Native weapon damage differs from the source talent effects"
        );
        behaviors.push(json!({"weapon_damage_mode":mode,
            "source_damage_effects":!source_modes.is_empty()}));
    }
    // A talent that sets the damage type never stands in for an exotic's trait.
    let report = &receipt["graph"]["perk_translation"];
    if report["intrinsic"] == "signature" {
        let socket = report["intrinsic_socket"]
            .as_u64()
            .ok_or("Intrinsic socket")?;
        let held = translated
            .iter()
            .filter(|r| r["socket"].as_u64() == Some(socket))
            .collect::<Vec<_>>();
        assert_eq!(held.len(), 1, "The intrinsic socket offers a choice");
        let node = held[0]["node"].as_u64().ok_or("Trait node")?;
        assert!(
            !damage_nodes.contains(&node),
            "A damage talent took the intrinsic socket"
        );
        behaviors.push(json!({"intrinsic_trait_node":node,"intrinsic_socket":socket}));
    }
    if damage_steps
        .iter()
        .map(|v| v.2)
        .collect::<BTreeSet<_>>()
        .len()
        > 1
    {
        let mut selected_socket = None;
        let mut choice_ids = BTreeSet::new();
        for (node, step, _) in &damage_steps {
            let matches = translated
                .iter()
                .filter(|r| r["node"] == *node && r["step"] == *step)
                .collect::<Vec<_>>();
            assert_eq!(matches.len(), 1, "A source damage alternative was lost");
            let slot = matches[0]["socket"]
                .as_u64()
                .ok_or("Damage choice socket")?;
            assert!(selected_socket.is_none_or(|previous| previous == slot));
            selected_socket = Some(slot);
            assert!(
                choice_ids.insert(
                    matches[0]["choice"]
                        .as_u64()
                        .ok_or("Damage choice ordinal")?
                )
            );
        }
        assert_eq!(choice_ids.len(), damage_steps.len());
    }
    for translated in translated {
        if let Some(kind) = translated["damage_type"].as_str() {
            let (source_mode, mode) = match kind {
                "kinetic" => (0, 0),
                "arc" => (2, 1),
                "solar" => (1, 2),
                "void" => (3, 3),
                _ => return Err("Unsupported damage contract".into()),
            };
            for tag in translated["source_actions"]
                .as_array()
                .ok_or("Source damage actions")?
            {
                let tag = tag.as_u64().ok_or("Source damage action tag")?;
                let source = source_reader.read(u32::try_from(tag)?)?;
                let effects = source.array(0x38, 8, None)?;
                assert_eq!(effects.len(), 1);
                assert_eq!(
                    source.bytes::<3>(source.pointer(effects[0])?)?,
                    [5, 1, source_mode]
                );
            }
            assert_eq!(
                native_damage,
                if mode == 0 { Vec::new() } else { vec![mode] },
                "Native weapon damage differs from source talent"
            );
            behaviors.push(json!({"damage_type":kind,"native_damage_mode":mode}));
            continue;
        }
        let slot = usize::try_from(translated["socket"].as_u64().ok_or("Translated socket")?)?;
        let variant = recipe
            .overrides
            .socket_plug_variants
            .iter()
            .find(|p| {
                usize::from(p.socket_index) == slot
                    && Some(u64::from(p.choice_index)) == translated["choice"].as_u64()
            })
            .ok_or("Private variant")?;
        let mut choices = vec![definition.u16(sockets[slot] + 2)?];
        for member in definition.array(sockets[slot] + 0x40, 32, Some(0x80802E03))? {
            let index = definition.u16(member)?;
            if index != u16::MAX && !choices.contains(&index) {
                choices.push(index);
            }
        }
        let sets = readback.tag(root.u32(8 + 51 * 16)?, None)?;
        let set_rows = sets.array(8, 24, None)?;
        if recipe.overrides.socket_columns[slot]
            .as_ref()
            .is_some_and(|column| column.reusable_choices.is_some())
        {
            let rolled = definition.u16(sockets[slot] + 32)?;
            assert_ne!(rolled, u16::MAX, "Rolled choices lost their pool");
            choices = sets
                .array(set_rows[usize::from(rolled)] + 8, 32, Some(0x80802E03))?
                .into_iter()
                .map(|row| sets.u16(row))
                .collect::<std::result::Result<Vec<_>, _>>()?;
        }
        let index = *choices
            .get(usize::from(variant.choice_index))
            .ok_or("Private choice is absent from the emitted socket")?;
        assert_ne!(index, u16::MAX);
        let plug_row = item_rows[usize::from(index)];
        let plug_hash = items.u32(plug_row)?;
        let plug = readback.tag(items.u32(plug_row + 16)?, Some(0x80807BEA))?;
        assert_ne!(plug_hash, variant.source_plug_hash.parse_u32()?);
        let perks = plug.array(plug.pointer(0x70)? + 16, 24, Some(0x808077BC))?;
        let sources = translated["source_actions"]
            .as_array()
            .ok_or("Source actions")?;
        grant::origins(source_reader, stock, &source_grid, translated)?;
        assert_eq!(
            perks.len(),
            sources.len()
                + usize::from(
                    translated["ability_grants"]
                        .as_array()
                        .is_some_and(|v| !v.is_empty())
                ),
            "Private choice inherited donor effects"
        );
        // Follow the source values and emitted stat identities independently of the
        // report. The Armor policy contributes ten native points per plain source unit.
        let stats = plug.array(plug.pointer(0x70)?, 0x28, Some(0x80803033))?;
        let adjustments = translated["stat_adjustments"]
            .as_array()
            .ok_or("Stat adjustments")?;
        assert_eq!(
            stats.len(),
            adjustments.len(),
            "Private choice stats differ from the source adjustments"
        );
        let mut actual_stats = stats
            .into_iter()
            .map(|row| {
                Ok((
                    u64::from(
                        stat_definitions.u32(
                            *stat_rows
                                .get(usize::from(plug.u8(row)?))
                                .ok_or("Private plug stat index leaves the definition table")?,
                        )?,
                    ),
                    i64::from(i32::from_le_bytes(plug.bytes::<4>(row + 4)?)),
                ))
            })
            .collect::<CheckResult<Vec<_>>>()?;
        let source_node = source_grid["talents"]["nodes"]
            .as_array()
            .ok_or("Source nodes")?
            .iter()
            .find(|n| n["index"] == translated["node"])
            .ok_or("Source node")?;
        let source_step = source_node["steps"]
            .as_array()
            .ok_or("Source steps")?
            .iter()
            .find(|s| s["index"] == translated["step"])
            .ok_or("Source step")?;
        let source_stats = source_step["variants"][0]["stat_adjustments"]
            .as_array()
            .ok_or("Source stats")?;
        assert_eq!(source_stats.len(), adjustments.len());
        let mut expected_stats = Vec::new();
        for (original, adjusted) in source_stats.iter().zip(adjustments) {
            let raw = Payload(hex::decode(
                original["record"].as_str().ok_or("Stat bytes")?,
            )?);
            let multiplier = if receipt["graph"]["kind"] == "armor" {
                assert_eq!(original["hash"].as_u64(), Some(0x1769266F));
                10i64
            } else {
                1
            };
            let expected = i64::from(i32::from_le_bytes(raw.bytes::<4>(0)?)) * multiplier;
            expected_stats.push((
                original["hash"].as_u64().ok_or("Source stat hash")?,
                expected,
            ));
            assert_eq!(adjusted["source_hash"], original["hash"]);
            assert_eq!(adjusted["value"].as_i64(), Some(expected));
        }
        actual_stats.sort_unstable();
        expected_stats.sort_unstable();
        assert_eq!(
            actual_stats, expected_stats,
            "Private choice changed a source stat delta"
        );
        let equivalents = translated["stock_perk_plugs"]
            .as_array()
            .ok_or("Stock equivalents")?;
        if !equivalents.is_empty() {
            let node = source_grid["talents"]["nodes"]
                .as_array()
                .ok_or("Source nodes")?
                .iter()
                .find(|n| n["index"] == translated["node"])
                .ok_or("Source node")?;
            let step = node["steps"]
                .as_array()
                .ok_or("Source steps")?
                .iter()
                .find(|s| s["index"] == translated["step"])
                .ok_or("Source step")?;
            let source_perks = step["variants"][0]["perks"]
                .as_array()
                .ok_or("Source perks")?;
            assert_eq!(equivalents.len(), source_perks.len());
            assert_eq!(perks.len(), source_perks.len());
            for (perk, source) in perks.iter().zip(source_perks) {
                let native_plug = match source["hash"].as_u64().ok_or("Source perk identity")? {
                    0x4E9855BC => 0x2A1CBFD4,
                    0x5D32FF95 => 0x39169B67,
                    _ => return Err("Unverified stock perk equivalent".into()),
                };
                let (_, bytes) = item(stock, native_plug)?;
                let donor = bytes;
                let donor_perks = donor.array(donor.pointer(0x70)? + 16, 24, Some(0x808077BC))?;
                assert_eq!(donor_perks.len(), 1);
                let expected_key =
                    finished.u32(finished_rows[usize::from(donor.u16(donor_perks[0])?)] + 4)?;
                let actual_key = finished.u32(finished_rows[usize::from(plug.u16(*perk)?)] + 4)?;
                assert_ne!(
                    actual_key, expected_key,
                    "Private choice reused the stock identity"
                );
                let assigned = runtime_rows
                    .iter()
                    .copied()
                    .filter(|&r| runtime.u32(r).ok() == Some(actual_key))
                    .collect::<Vec<_>>();
                assert_eq!(assigned.len(), 1);
                let tag = runtime.u32(assigned[0] + 4)?;
                let expected = runtime_rows
                    .iter()
                    .copied()
                    .filter(|&r| runtime.u32(r).ok() == Some(expected_key))
                    .collect::<Vec<_>>();
                assert_eq!(expected.len(), 1);
                assert_eq!(
                    tag,
                    runtime.u32(expected[0] + 4)?,
                    "Private choice lost its native equivalent"
                );
                assert_eq!(
                    readback.tag(tag, None)?.0,
                    stock.tag(tag, None)?.0,
                    "Stock equivalent action changed"
                );
                behaviors.push(
                    json!({"plug":plug_hash,"stock_equivalent":native_plug,"native_action":tag}),
                );
            }
            continue;
        }
        if translated["ability_grants"]
            .as_array()
            .is_some_and(|v| !v.is_empty())
        {
            let last = *perks.last().ok_or("Missing ability grant perk")?;
            let key = finished.u32(finished_rows[usize::from(plug.u16(last)?)] + 4)?;
            let assigned = runtime_rows
                .iter()
                .copied()
                .filter(|&r| runtime.u32(r).ok() == Some(key))
                .collect::<Vec<_>>();
            assert_eq!(assigned.len(), 1);
            let tag = runtime.u32(assigned[0] + 4)?;
            let action = readback.tag(tag, Some(0x808040B5))?;
            behaviors.push(grant::check(
                source_reader,
                stock,
                readback,
                &action,
                translated,
            )?);
        }
        for (perk, source_tag) in perks.into_iter().take(sources.len()).zip(sources) {
            let source_tag = source_tag.as_u64().ok_or("Source action tag")?;
            let source = source_reader.read(u32::try_from(source_tag)?)?;
            let key = finished.u32(finished_rows[usize::from(plug.u16(perk)?)] + 4)?;
            let assigned = runtime_rows
                .iter()
                .filter(|&&r| runtime.u32(r).ok() == Some(key))
                .copied()
                .collect::<Vec<_>>();
            assert_eq!(assigned.len(), 1);
            let action_tag = runtime.u32(assigned[0] + 4)?;
            let action = readback.tag(action_tag, Some(0x808040B5))?;
            if receipt["graph"]["kind"] == "armor" {
                for row in source.array(0x38, 8, None)? {
                    let at = source.pointer(row)?;
                    if source.u8(at)? == 34 {
                        assert_eq!(source.u8(at + 0x94)?, 0);
                    }
                }
                for row in action.array(0x38, 8, None)? {
                    let at = action.pointer(row)?;
                    if action.u8(at)? == 40 {
                        assert_eq!(action.u8(at + 0xB8)?, 0);
                    }
                }
            }
            check_action(source_reader, stock, readback, &source, &action)?;
            behaviors.push(
                json!({"plug":plug_hash,"source_action":source_tag,"native_action":action_tag,
                    "direct_profiles": direct::evidence(readback, &action)?,
                    "counters": event::counter_evidence(&action)?,
                    "ammunition_fractions": fraction::evidence(&action)?,
                    "attachment_modifiers": pattern::evidence(readback, &action)?,
                    "status_audio": pattern::audio_evidence(source_reader, readback, &action, recipe)?,
                    "timer_extensions": timer::evidence(&source, &action)?}),
            );
        }
    }
    Ok(json!(behaviors))
}

/// The same independent controller contract applies at the root and inside private wrappers.
fn check_action(
    source_reader: &Source,
    stock: &mut Reader,
    readback: &mut Reader,
    source: &Payload,
    action: &Payload,
) -> CheckResult<()> {
    feedback::check(source_reader, stock, readback, source, action)?;
    assert_eq!(source.u8(0xB0)?, action.u8(0xB8)?);
    assert_eq!(source.u8(0xB1)?, action.u8(0xB9)?);
    if source.u8(0xB0)? == 1 {
        let s = source.pointer(0x78)?;
        let n = action.pointer(0x78)?;
        assert_eq!(source.u32(s - 4)?, 0x808031C4);
        assert_eq!(action.u32(n - 4)?, 0x80803E07);
        assert_eq!(source.bytes::<8>(s)?, action.bytes::<8>(n)?);
    } else {
        assert_eq!(action.u64(0x78)?, 0);
    }
    let source_effects = source.array(0x38, 8, None)?;
    let effects = action.array(0x38, 8, Some(0x808040AC))?;
    assert!(effects.len() >= source_effects.len());
    let mut extensions = 0u64;
    for (index, &row) in effects.iter().enumerate() {
        if action.u8(action.pointer(row)?)? == 32 {
            extensions |= 1 << index;
        }
    }
    assert_eq!(action.u64(0xA0)?, extensions);
    let mut next_effect = 0;
    for source_at in source_effects {
        let s = source.pointer(source_at)?;
        if source.u8(s)? == 1 {
            next_effect += pattern::dynamic(
                source_reader,
                stock,
                readback,
                source,
                s,
                action,
                &effects[next_effect..],
            )?;
            continue;
        }
        let at = effects.get(next_effect).ok_or("Missing native effect")?;
        next_effect += 1;
        let n = if source.u8(s)? == 9 {
            let matching = effects
                .iter()
                .map(|&at| action.pointer(at))
                .collect::<std::result::Result<Vec<_>, _>>()?
                .into_iter()
                .filter(|&at| {
                    action.u8(at).ok() == Some(10)
                        && action.u8(at + 2).ok() == source.u8(s + 2).ok()
                        && action.u32(at + 8).ok() == source.u32(s + 8).ok()
                })
                .collect::<Vec<_>>();
            assert_eq!(
                matching.len(),
                1,
                "Source property lacks a unique native write"
            );
            matching[0]
        } else {
            action.pointer(*at)?
        };
        match source.u8(s)? {
            3 => direct::check(source_reader, stock, readback, source, s, action, n)?,
            12 => weighted::check(source, s, action, n)?,
            4 => {
                assert_eq!(action.u32(n - 4)?, 0x80803E42);
                assert_eq!(action.bytes::<4>(n)?, [5, 0, 0, 0]);
                assert_eq!(source.bytes::<8>(s + 4)?, action.bytes::<8>(n + 4)?);
                assert_eq!(action.bytes::<12>(n + 12)?, [0; 12]);
                if source.u32(s + 24)? != u32::MAX {
                    // A source default-orb alias must become the native service's
                    // default selection, never a dangling source package reference.
                    let configuration = source_reader.classes(0x80802257);
                    assert_eq!(configuration.len(), 1);
                    let globals = source_reader.read(configuration[0])?;
                    let default = source_reader.read(globals.u32(0xC8)?)?;
                    let selected = source_reader.read(source.u32(s + 24)?)?;
                    assert_eq!(default.u64(0x20)?, selected.u64(0x20)?);
                    let selected_owners = selected.array(0x20, 12, Some(0x80800715))?;
                    let default_owners = default.array(0x20, 12, Some(0x80800715))?;
                    for (selected_row, default_row) in
                        selected_owners.into_iter().zip(default_owners)
                    {
                        let selected = source_reader.read(selected.u32(selected_row)?)?;
                        let default = source_reader.read(default.u32(default_row)?)?;
                        assert_eq!(
                            selected.u32(selected.pointer(24)? - 4)?,
                            default.u32(default.pointer(24)? - 4)?
                        );
                    }
                }
                assert_eq!(action.u32(n + 24)?, u32::MAX);
                assert_eq!(action.u32(n + 28)?, 0);
            }
            28 => {
                event::incoming(source_reader, stock, source, s, action, n)?;
                let owner = readback.tag(action.u32(n + 0xA8)?, Some(0x808040B5))?;
                assert_eq!(
                    owner.0, action.0,
                    "Damage callback must resolve to its private action"
                );
            }
            0 => pattern::check(source_reader, stock, readback, source, s, action, n)?,
            10 | 11 => {
                // Compare the complete original numeric record, including every ammo
                // lane. A signature with both actions must retain both finished perks.
                let pickup = source.u8(s)? == 11;
                assert_eq!(action.u8(n)?, if pickup { 12 } else { 11 });
                assert_eq!(
                    action.u32(n - 4)?,
                    if pickup { 0x80803E37 } else { 0x80803E35 }
                );
                assert_eq!(source.bytes::<3>(s + 1)?, [0; 3]);
                assert_eq!(action.bytes::<3>(n + 1)?, [0; 3]);
                for offset in (4..if pickup { 28 } else { 48 }).step_by(4) {
                    assert_eq!(source.u32(s + offset)?, action.u32(n + offset)?);
                }
            }
            27 => {
                assert_eq!(action.u8(n)?, 32);
                assert_eq!(action.u32(n - 4)?, 0x80803E3B);
                assert_eq!(source.bytes::<8>(s + 4)?, action.bytes::<8>(n + 4)?);
                let original = source.array(s + 16, 8, None)?;
                let nested = action.array(n + 16, 8, Some(0x808040BA))?;
                assert!(!original.is_empty());
                assert_eq!(original.len(), nested.len());
                let (mut source_mask, mut mask) = (0u64, 0u64);
                for (s, n) in original.into_iter().zip(nested) {
                    let s = source.pointer(s)?;
                    let n = action.pointer(n)?;
                    assert!(matches!(source.u8(s + 5)?, 2 | 12 | 19));
                    if source.u8(s + 5)? != 2 {
                        assert_eq!(source.u8(s + 6)?, 0);
                    }
                    assert_eq!(source.bytes::<7>(s)?, action.bytes::<7>(n)?);
                    assert_eq!(action.u8(n + 7)?, 255);
                    source_mask |= 1 << source.u8(s + 5)?;
                    mask |= 1 << action.u8(n + 5)?;
                    event::check(source_reader, stock, source, s, action, n)?;
                }
                assert_eq!(source.u64(s + 32)?, source_mask);
                assert_eq!(action.u64(n + 32)?, mask);
            }
            19 => {
                assert_eq!(action.u8(n)?, 20);
                assert_eq!(action.u32(n - 4)?, 0x80803e24);
                assert_eq!(source.bytes::<2>(s + 1)?, action.bytes::<2>(n + 1)?);
            }
            34 => {
                assert_eq!(action.u8(n)?, 40);
                assert_eq!(action.u32(n - 4)?, 0x80802f16);
                assert_eq!(source.u8(s + 1)?, action.u8(n + 1)?);
                assert_eq!(source.u8(s + 0x94)?, action.u8(n + 0xb8)?);
                assert_eq!(action.u8(n + 0x148)?, 0);
                assert_eq!(action.bytes::<0x90>(n + 8)?, [0; 0x90]);
                assert_eq!(action.u8(n + 0xa0)?, 255);
                assert_eq!(action.bytes::<2>(n + 0xb0)?, [1; 2]);
                for (original, emitted) in [(0x170, 0x120), (0x180, 0x130)] {
                    let originals = source.array(s + original, 12, Some(0x80802069))?;
                    let rows = action.array(n + emitted, 12, Some(0x80803E22))?;
                    assert_eq!(originals.len(), rows.len());
                    for (original, row) in originals.into_iter().zip(rows) {
                        assert_eq!(source.bytes::<12>(original)?, action.bytes::<12>(row)?);
                        assert_eq!(action.u8(row)?, 0);
                        assert_eq!(action.u8(row + 8)?, 255);
                    }
                }
                let value = action.pointer(n + 0x140)?;
                assert_eq!(action.u32(value - 4)?, 0x80802f1a);
                assert_eq!(source.u32(s + 0x190)?, action.u32(value)?);
                assert_eq!(source.u32(s + 0x190)?, action.u32(value + 4)?);
                event::labels(source_reader, stock, source, s + 0x98, action, n + 0xc0)?;
            }
            7 => {
                assert_eq!(action.u8(n)?, 8);
                assert_eq!(action.u32(n - 4)?, 0x80803E4D);
                assert_eq!(source.bytes::<2>(s + 2)?, action.bytes::<2>(n + 2)?);
                assert_eq!(action.u8(n + 4)?, 0);
                assert_eq!(source.u32(s + 4)?, action.u32(n + 8)?);
                assert_eq!(source.u32(s + 8)?, action.u32(n + 12)?);
                let values = action.array(n + 0x28, 16, Some(0x80800090))?;
                let originals = source.array(s + 0x28, 16, Some(0x80800090))?;
                assert_eq!(values.len(), originals.len());
                for (original, emitted) in originals.into_iter().zip(values) {
                    assert_eq!(source.bytes::<16>(original)?, action.bytes::<16>(emitted)?);
                }
                let original_code = source.array(s + 0x18, 1, Some(0x80800009))?;
                let emitted_code = action.array(n + 0x18, 1, Some(0x80800009))?;
                assert_eq!(original_code.len(), emitted_code.len());
                for (original, emitted) in original_code.into_iter().zip(emitted_code) {
                    assert_eq!(source.u8(original)?, action.u8(emitted)?);
                }
                assert_eq!(source.bytes::<24>(s + 0x38)?, action.bytes::<24>(n + 0x38)?);
            }
            13 => {
                assert_eq!(action.u8(n)?, 14);
                assert_eq!(source.bytes::<4>(s + 2)?, action.bytes::<4>(n + 0x68)?);
                assert_eq!(source.u32(s + 8)?, action.u32(n + 0x6c)?);
                assert!(action.0[n + 0x70..n + 0x88].iter().all(|b| *b == 0));
            }
            5 => {
                assert_eq!(action.u8(n)?, 6);
                assert_eq!(source.u8(s + 2)?, action.u8(n + 2)?);
            }
            9 => {
                assert_eq!(action.u8(n)?, 10);
                assert_eq!(action.u32(n - 4)?, 0x808029ED);
                assert_eq!(source.u8(s + 1)?, action.u8(n + 1)?);
                assert_eq!(source.u8(s + 2)?, action.u8(n + 2)?);
                assert_eq!(source.u8(s + 3)?, action.u8(n + 3)?);
                assert_eq!(source.u32(s + 4)?, action.u32(n + 4)?);
                assert_eq!(source.u32(s + 8)?, action.u32(n + 8)?);
                let code = action.array(n + 0x18, 1, Some(0x80800009))?;
                assert_eq!(
                    code.into_iter()
                        .map(|at| action.u8(at))
                        .collect::<std::result::Result<Vec<_>, _>>()?,
                    [0x34, 0, 0x3E, 0]
                );
                let values = action.array(n + 0x28, 16, Some(0x80800090))?;
                assert_eq!(values.len(), 1);
                assert_eq!(source.bytes::<16>(s + 16)?, action.bytes::<16>(values[0])?);
                assert_eq!(action.u8(n + 0x4A)?, 1);
                assert_eq!(action.u32(n + 0x4C)?, 0);
            }
            14 => fraction::check(readback, source, s, action, n)?,
            15 => transfer::check(readback, source, s, action, n)?,
            24 | 25 => {
                assert_eq!(action.u8(n)?, source.u8(s)? + 4);
                let length = if source.u8(s)? == 24 { 24 } else { 16 };
                assert_eq!(source.0[s + 1..s + length], action.0[n + 1..n + length]);
            }
            26 => {
                assert_eq!(action.u8(n)?, 30);
                // Only the retained and operation bytes belong to this node.
                // Neighbor alignment and the next object's class are not fields.
                assert_eq!(source.bytes::<2>(s + 1)?, action.bytes::<2>(n + 1)?);
            }
            _ => {
                return Err("An unsupported source effect was presented as translated".into());
            }
        }
    }
    assert_eq!(
        next_effect,
        effects.len(),
        "Unexpected extra native effects"
    );
    for group in [0x20, 0x48, 0x58] {
        let source_conditions = source.array(group, 8, None)?;
        let conditions = action.array(group, 8, Some(0x808040BA))?;
        if group == 0x20 && source_conditions.is_empty() {
            // The original controller admits an empty primary list only on event
            // zero with its routing bit set. The authored Always condition must
            // preserve that event, certainty and lack of retained predicate state.
            assert_eq!(source.u64(0x80)?, 1);
            assert_eq!(action.u64(0x88)?, 1);
            assert_eq!(conditions.len(), 1);
            let n = action.pointer(conditions[0])?;
            assert_eq!(action.u32(n - 4)?, 0x80803E03);
            assert_eq!(action.bytes::<7>(n)?, [0, 0, 0x80, 0x3F, 0xFF, 0, 0]);
            continue;
        }
        assert_eq!(conditions.len(), source_conditions.len());
        for (s, n) in source_conditions.into_iter().zip(conditions) {
            let s = source.pointer(s)?;
            let n = action.pointer(n)?;
            assert_eq!(source.bytes::<5>(s)?, action.bytes::<5>(n)?);
            let kind = source.u8(s + 5)?;
            assert_eq!(
                action.u8(n + 5)?,
                match kind {
                    14..=18 | 30..=36 => kind - 1,
                    _ => kind,
                }
            );
            assert_eq!(source.u8(s + 6)?, action.u8(n + 6)?);
            if source.u8(s + 5)? == 1 {
                assert_eq!(source.u32(s + 8)?, action.u32(n + 8)?);
            }
            event::check(source_reader, stock, source, s, action, n)?;
            if kind == 34 {
                let mask = match group {
                    0x20 => 0x88,
                    0x48 => 0x90,
                    0x58 => 0x98,
                    _ => return Err("Unexpected condition group".into()),
                };
                assert_ne!(action.u64(mask)? & (1u64 << 33), 0);
            }
            if matches!(kind, 30 | 31) {
                assert_eq!(group, if kind == 30 { 0x20 } else { 0x48 });
                if source.u32(s + 8)? == 0x3B210AE5 {
                    event::duration_stock(source_reader, stock, readback, action.u32(n + 8)?)?;
                } else {
                    assert_eq!(kind, 31, "Unqualified external Duration start key");
                    event::duration_end(readback, action, n)?;
                }
                let mask = if group == 0x20 { 0x88 } else { 0x90 };
                assert_ne!(action.u64(mask)? & (1u64 << (kind - 1)), 0);
            }
        }
    }
    Ok(())
}
