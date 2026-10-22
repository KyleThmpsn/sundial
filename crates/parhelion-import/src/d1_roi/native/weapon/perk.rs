//! Bounded source action lowering. An unsupported node rejects the entire choice.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use sundial::package_authoring::sandbox_perk::program::{
    Action, AmmunitionStore, AmmunitionTarget, DamageMode, NativeNode, Program, Trigger,
};

/// Equipped stat contributions of one source choice on Shadowkeep stat definitions, as recipe
/// rows and report rows. A source row is a plain delta only when both scale factors are one
/// and its selector bytes are unset.
fn adjustments(
    record: &Payload,
    rows: &Value,
    definitions: &BTreeMap<u32, u16>,
) -> Result<(Vec<Value>, Vec<Value>)> {
    let rows = rows.as_array().map(Vec::as_slice).unwrap_or_default();
    ensure!(
        rows.len() == usize::try_from(record.u64(16)?)?,
        "Source stat adjustments disagree with the variant record"
    );
    let mut stats = Vec::new();
    let mut report = Vec::new();
    let mut indices = BTreeSet::new();
    for row in rows {
        let hash = u32::try_from(row["hash"].as_u64().context("Source stat hash")?)?;
        let bytes = Payload(hex::decode(
            row["record"]
                .as_str()
                .context("Source stat adjustment bytes")?,
        )?);
        ensure!(
            bytes.0.len() == 16
                && bytes.u32(4)? == 0x3F80_0000
                && bytes.u32(8)? == 0x3F80_0000
                && bytes.bytes::<3>(13)? == [0xFF, 0, 0],
            "Source stat {hash:08X} adjustment is scaled or selected and requires curve translation"
        );
        let value = i32::from_le_bytes(bytes.bytes::<4>(0)?);
        // Rise of Iron and Shadowkeep share these stat hashes, including Inventory Size, which
        // the modern stat group mapping treats as repurposed, so the lookup is direct.
        let index = *definitions
            .get(&hash)
            .with_context(|| format!("Source stat {hash:08X} has no Shadowkeep stat definition"))?;
        ensure!(
            u8::try_from(index).is_ok(),
            "Source stat {hash:08X} exceeds private plug stat capacity"
        );
        ensure!(
            indices.insert(index),
            "Source stat {hash:08X} is adjusted twice"
        );
        stats.push(json!({"definition_index":index,"value":value}));
        report.push(json!({"source_hash":hash,"source_index":row["index"],
            "definition_index":index,"value":value}));
    }
    Ok((stats, report))
}

struct Lowered {
    programs: Vec<Value>,
    raw: Vec<Program>,
    tags: Vec<u32>,
    auxiliary: usize,
    stats: Vec<Value>,
    adjustments: Vec<Value>,
    /// Sandbox perks of the stock plugs that stand in for source perks, and those plugs.
    stock: Vec<u16>,
    stock_plugs: Vec<String>,
}

/// Source talent perks, by identity, that run a stock Shadowkeep weapon perk instead of their
/// own action. Each source action attaches a talent pattern that sets weapon component fields,
/// which have no established Shadowkeep field mapping, while Shadowkeep ships the same perk:
/// Quickdraw `4E9855BC` (`quick_draw.pattern.tft`) runs Quickdraw `2A1CBFD4` and Snapshot
/// `5D32FF95` (`snapshot.pattern.tft`) runs Snapshot Sights `39169B67`. Rise of Iron and
/// Shadowkeep share no sandbox perk identities, so the pairs are listed rather than looked up.
const STOCK_EQUIVALENTS: [(u32, u32); 2] = [(0x4E98_55BC, 0x2A1C_BFD4), (0x5D32_FF95, 0x3916_9B67)];

fn conditions(data: &Payload, descriptor: usize) -> Result<Vec<NativeNode>> {
    data.array(descriptor, 8, None)?
        .into_iter()
        .map(|at| {
            let node = data.pointer(at)?;
            let class = data.u32(node - 4)?;
            let kind = data.u8(node + 5)?;
            let size = match (class, kind) {
                (0x80803176, 0) => 8,
                (0x80802DEB, 1) => {
                    ensure!(data.f32(node + 8)? >= 0.0, "Negative source timer");
                    12
                }
                // Aiming down sights. The certain, unfiltered start and end records equal the
                // native kind 23 records of stock Tracking Module byte for byte.
                (0x80802192, 23) => {
                    let bytes = data.bytes::<12>(node)?;
                    ensure!(
                        bytes[..5] == [0, 0, 0x80, 0x3F, 0xFF]
                            && bytes[6] == 0
                            && bytes[7] <= 1
                            && bytes[8] <= 1
                            && bytes[9] == 0
                            && bytes[10] == 1
                            && bytes[11] == 0,
                        "Source aim condition is conditional or filtered"
                    );
                    12
                }
                _ => anyhow::bail!("Condition {class:08X} ({kind}) requires runtime translation"),
            };
            let bytes = data
                .0
                .get(node..node + size)
                .context("Truncated source condition")?
                .to_vec();
            Ok(NativeNode { kind, bytes })
        })
        .collect()
}

fn effect(data: &Payload, at: usize) -> Result<Action> {
    let class = data.u32(at - 4)?;
    let kind = data.u8(at)?;
    match (class, kind) {
        (0x80802AC6, 5) => {
            ensure!(
                data.u8(at + 1)? == 1 && data.u8(at + 2)? <= 3,
                "Unsupported source damage selector"
            );
            Ok(Action::SetDamageType {
                mode: DamageMode::from_byte(data.u8(at + 2)?),
                keep_after_removal: false,
            })
        }
        (0x8080221F, 14) => {
            ensure!(
                data.u8(at + 1)? == 0 && data.bytes::<2>(at + 6)? == [0; 2],
                "Unknown ammunition flags"
            );
            ensure!(
                (2..=5).all(|i| data.u8(at + i).is_ok_and(|v| v <= 1)),
                "Unknown ammunition selector"
            );
            ensure!(
                data.bytes::<12>(at + 12)? == [0; 12],
                "Source slot ammunition amounts require separate slot translation"
            );
            let _ = data.f32(at + 8)?;
            Ok(Action::AddFraction {
                fraction_bits: data.u32(at + 8)?,
                target: AmmunitionTarget::OwningWeapon,
                store: AmmunitionStore::from_byte(data.u8(at + 2)?).context("Ammunition store")?,
                capacity: AmmunitionStore::from_byte(data.u8(at + 4)?)
                    .context("Ammunition capacity")?,
                overflow: data.u8(at + 3)? != 0,
                action_scaled: data.u8(at + 5)? != 0,
            })
        }
        (0x80802840, 24) | (0x80802C04, 25) => {
            let size = if kind == 24 { 24 } else { 16 };
            ensure!(
                data.u8(at + 1)? == 1,
                "Unsupported source full-auto lifecycle"
            );
            let mut bytes = data
                .0
                .get(at..at + size)
                .context("Truncated source full-auto effect")?
                .to_vec();
            if kind == 24 {
                ensure!(
                    bytes[4..].iter().all(|v| *v == 0),
                    "Unknown source ability flags"
                );
            } else {
                ensure!(bytes[2..4] == [0, 0], "Unknown source timing flags");
                for offset in [4, 8, 12] {
                    let _ = data.f32(at + offset)?;
                }
            }
            bytes[0] = kind + 4;
            Ok(Action::Native {
                node: NativeNode {
                    kind: kind + 4,
                    bytes,
                },
            })
        }
        (0x80802755, 9) => {
            // The node is 60 bytes, the next node's class word follows it. The constant is stored
            // in four lanes at +10 rather than as a value program. A removal
            // policy of one restores zero when the program ends, as stock Cluster Bombs does
            // for its rocket channel.
            ensure!(
                data.u8(at + 1)? == 1 && data.u32(at + 12)? == 0,
                "Unknown source named property header"
            );
            let key = data.u32(at + 8)?;
            ensure!(
                ![0, u32::MAX, 0x811C_9DC5].contains(&key),
                "Source named property has no key"
            );
            let value_bits = data.u32(at + 16)?;
            ensure!(
                (1..4).all(|lane| data.u32(at + 16 + lane * 4).ok() == Some(value_bits))
                    && f32::from_bits(value_bits).is_finite()
                    && data.bytes::<28>(at + 32)? == [0; 28],
                "Source named property is not one constant"
            );
            Ok(Action::Property {
                key,
                target: data.u8(at + 2)?,
                operation_byte: 0,
                removal: 1,
                value_bits,
                restore_bits: 0,
                ability_mask: data.u32(at + 4)?,
                input: 0,
                flag: data.u8(at + 3)?,
            })
        }
        // Native kind 1 attaches a pattern the same way, but a talent pattern such as Quickdraw's
        // sets fields of the weapon's components through source expressions, and Shadowkeep's
        // components lay those fields out differently.
        (0x80802953, 0) => anyhow::bail!(
            "Source talent pattern {:08X} sets weapon component fields, which require a Shadowkeep field translation",
            data.u32(at + 0x10)?
        ),
        // Source kind 26 holds native kind 30's retained and operation bytes, the next node's
        // class word follows at +0C. Tracking `[1A 01 01]` equals stock Tracking Module `[1E 01 01]`.
        (0x80802B88, 26) => {
            ensure!(
                data.u8(at + 1)? == 1 && data.bytes::<9>(at + 3)? == [0; 9],
                "Unknown source weapon count fields"
            );
            Ok(Action::WeaponReferenceCount {
                selector: data.u8(at + 2)?,
            })
        }
        _ => anyhow::bail!("Effect {class:08X} ({kind}) requires runtime translation"),
    }
}

/// A plain weapon draw or holster condition: certain, unfiltered and bound to the shared
/// label dictionary. Source kinds 17 and 18 hold the same record as native draw and holster
/// kinds 16 and 17, which stock Cluster Bombs uses for its rocket channel.
fn draw_condition(data: &Payload, descriptor: usize, class: u32, kind: u8) -> Result<bool> {
    const LABEL_PATH: &[u8] = b"content/common/native/sandbox/label_globals.label_globals.tft";
    let rows = data.array(descriptor, 8, None)?;
    let [row] = rows.as_slice() else {
        return Ok(false);
    };
    let node = data.pointer(*row)?;
    if data.u32(node - 4)? != class {
        return Ok(false);
    }
    let bytes = data.bytes::<0x70>(node)?;
    ensure!(
        bytes[..4] == 1f32.to_le_bytes()
            && bytes[4] == 0xFF
            && bytes[5] == kind
            && bytes[6] == 0
            && bytes[8] == 1
            && bytes[9..0x50].iter().all(|byte| *byte == 0)
            && bytes[0x5C..].iter().all(|byte| *byte == 0),
        "Source draw condition is conditional or filtered"
    );
    let path = data.pointer(node + 0x50)?;
    ensure!(
        data.0.get(path..path + LABEL_PATH.len()) == Some(LABEL_PATH),
        "Source draw condition names another label dictionary"
    );
    Ok(true)
}

fn program(source: &Source, tag: u32, name: &str) -> Result<(Program, usize)> {
    ensure!(
        source.reference(tag)? == 0x80802711,
        "Source perk is not an action controller"
    );
    let data = source.read(tag)?;
    ensure!(
        data.u64(0x68)? == 0,
        "Additional source action groups require translation"
    );
    ensure!(
        data.u64(0x78)? == 0 && data.bytes::<4>(0xB0)? == [0; 4],
        "Source execution policy requires translation"
    );
    let actions = data
        .array(0x38, 8, None)?
        .into_iter()
        // Both engines activate stored effects from last to first. Program actions
        // are execution ordered, and the native compiler reverses them for storage.
        .rev()
        .map(|at| effect(&data, data.pointer(at)?))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        !actions.is_empty(),
        "Source choice has no executable effects"
    );
    if draw_condition(&data, 0x20, 0x808031AC, 17)?
        && draw_condition(&data, 0x48, 0x80802E9B, 18)?
        && data.u64(0x58)? == 0
    {
        let program = Program {
            name: name.to_owned(),
            trigger: Trigger::Drawn,
            duration_ms: 0,
            cooldown_ms: 0,
            actions,
            ..Program::default()
        };
        program.validate_structure().map_err(anyhow::Error::msg)?;
        return Ok((program, usize::try_from(data.u64(0x10)?)?));
    }
    let mut activation = conditions(&data, 0x20)?.into_iter();
    let mut removal = conditions(&data, 0x48)?.into_iter();
    let mut rearm = conditions(&data, 0x58)?.into_iter();
    let program = Program {
        name: name.to_owned(),
        trigger: Trigger::Native,
        duration_ms: 0,
        cooldown_ms: 0,
        native_trigger: Some(
            activation
                .next()
                .context("Source action has no activation condition")?,
        ),
        alternative_triggers: activation.collect(),
        native_removal: removal.next(),
        alternative_removals: removal.collect(),
        native_rearm: rearm.next(),
        alternative_rearms: rearm.collect(),
        actions,
        ..Program::default()
    };
    program.validate_structure().map_err(anyhow::Error::msg)?;
    Ok((program, usize::try_from(data.u64(0x10)?)?))
}

/// Intrinsic plugs (`67FBA961`) set the weapon's frame. A source grid's first column holds the
/// weapon's own perk, which takes that socket.
const INTRINSIC_CATEGORY: u64 = 0x67FB_A961;
/// Weapon perk plugs, as opposed to shaders (14), masterworks and trackers (13) and mods (37).
const PERK_ITEM_KIND: u64 = 15;

/// The damage type a source choice gives the weapon, when setting it is all the choice does.
fn damage_type(programs: &[Program]) -> Option<&'static str> {
    let [program] = programs else {
        return None;
    };
    let [Action::SetDamageType { mode, .. }] = program.actions.as_slice() else {
        return None;
    };
    match mode {
        DamageMode::Kinetic => Some("kinetic"),
        DamageMode::Solar => Some("solar"),
        DamageMode::Arc => Some("arc"),
        DamageMode::Void => Some("void"),
        DamageMode::Unknown(_) => None,
    }
}

/// Only an unambiguous, standalone source damage choice constrains automatic donor selection.
/// Unsupported actions retain their normal per-choice translation report.
pub(super) fn requires_kinetic(source: &Source, weapon: &Value) -> Result<bool> {
    let mut kinds = BTreeSet::new();
    for node in weapon["talents"]["nodes"]
        .as_array()
        .context("Source talent nodes")?
    {
        for step in node["steps"].as_array().context("Source talent steps")? {
            let variants = step["variants"]
                .as_array()
                .context("Source talent variants")?;
            let [variant] = variants.as_slice() else {
                continue;
            };
            let record = Payload(hex::decode(
                variant["record"].as_str().context("Source variant bytes")?,
            )?);
            let rows = variant["stat_adjustments"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default();
            if record.u64(16)? != 0 || !rows.is_empty() {
                continue;
            }
            let perks = variant["perks"].as_array().context("Source talent perks")?;
            let [perk] = perks.as_slice() else {
                continue;
            };
            let Some(tag) = perk["action"]
                .as_u64()
                .and_then(|tag| u32::try_from(tag).ok())
            else {
                continue;
            };
            if let Ok((program, _)) = program(source, tag, "Damage")
                && let Some(kind) = damage_type(&[program])
            {
                kinds.insert(kind);
            }
        }
    }
    Ok(kinds.len() == 1 && kinds.contains("kinetic"))
}

/// A native perk socket: its index, socket type and perk plugs, the default first.
struct PerkSocket<'a> {
    index: usize,
    socket_type: &'a Value,
    plugs: Vec<u32>,
}

/// The base's perk sockets, the intrinsic first and then the rest in socket order.
fn perk_sockets(sockets: &[Value]) -> Result<Vec<PerkSocket<'_>>> {
    let mut intrinsic = Vec::new();
    let mut others = Vec::new();
    for (index, socket) in sockets.iter().enumerate() {
        if socket["socket_type"] == u16::MAX {
            continue;
        }
        let Some(choices) = socket["choices"].as_array() else {
            continue;
        };
        let Some(default) = choices.iter().find(|choice| choice["default"] == true) else {
            continue;
        };
        if default["item_kind"].as_u64() != Some(PERK_ITEM_KIND) {
            continue;
        }
        let mut plugs = Vec::new();
        for choice in std::iter::once(default).chain(choices.iter().filter(|choice| {
            choice["default"] != true && choice["item_kind"].as_u64() == Some(PERK_ITEM_KIND)
        })) {
            let hash = u32::try_from(choice["hash"].as_u64().context("Native plug identity")?)?;
            if !plugs.contains(&hash) {
                plugs.push(hash);
            }
        }
        let socket = PerkSocket {
            index,
            socket_type: &socket["socket_type"],
            plugs,
        };
        if default["category"].as_u64() == Some(INTRINSIC_CATEGORY) {
            intrinsic.push(socket);
        } else {
            others.push(socket);
        }
    }
    intrinsic.extend(others);
    Ok(intrinsic)
}

/// A translated source choice and where it sits in the source grid.
struct Choice {
    column: i64,
    row: i64,
    name: String,
    description: Value,
    icon: Option<String>,
    lowered: Lowered,
    report: Value,
}

/// A donor choice supplies only a finished-perk layout. Empty sight and magazine plugs are
/// valid choices, so search the remaining default and alternate choices before refusing it.
fn template(assembly: &mut Assembly<'_>, sockets: &[Value]) -> Result<(u32, u16)> {
    let mut visited = BTreeSet::new();
    for default in [true, false] {
        for choice in sockets
            .iter()
            .flat_map(|socket| socket["choices"].as_array().into_iter().flatten())
        {
            if choice["item_kind"].as_u64() != Some(PERK_ITEM_KIND)
                || (choice["default"] == true) != default
            {
                continue;
            }
            let hash = u32::try_from(choice["hash"].as_u64().context("Native plug identity")?)?;
            if !visited.insert(hash) {
                continue;
            }
            let tag = crate::d2_mot::assets::item::find_native(&mut assembly.native, hash)?;
            let item = assembly.native.tag(tag, Some(0x80807BEA))?;
            let perks = sundial::package_authoring::native_weapon::base_sandbox_perks(&item.0)
                .map_err(anyhow::Error::msg)?;
            if let Some(&perk) = perks.first() {
                return Ok((hash, perk));
            }
        }
    }
    anyhow::bail!("Native gameplay plug choices have no finished perk template")
}

/// Translate the source talent grid onto the base's perk sockets. Each source column becomes one
/// socket: the first column takes the intrinsic socket and the rest take the remaining perk
/// sockets in order. A column's choices keep their source row order, and each clones a plug from
/// its native socket's pool as its layout template, distinct while the pool lasts. A choice whose
/// only effect sets a fixed damage type sets the recipe's damage type. Alternative damage steps
/// share a separate socket so their private effects follow the equipped choice.
/// Unused perk sockets are disabled so the base's own perks do not stand in for the source's.
#[allow(clippy::cognitive_complexity)]
pub(super) fn translate(
    assembly: &mut Assembly<'_>,
    source: &Value,
    donor: u32,
) -> Result<(Value, Value, Value, Option<&'static str>)> {
    let native = crate::d2_mot::ornaments::native(&mut assembly.native, donor)?;
    let definitions = crate::d2_mot::gameplay::stat_definitions(&mut assembly.native)?;
    let sockets = native["sockets"].as_array().context("Native sockets")?;
    let (template_hash, template_perk) = template(assembly, sockets)?;
    let perk_sockets = perk_sockets(sockets)?;
    let mut equivalents = BTreeMap::new();
    for (source, plug) in STOCK_EQUIVALENTS {
        let tag = crate::d2_mot::assets::item::find_native(&mut assembly.native, plug)
            .with_context(|| format!("Stock perk plug {plug:08X}"))?;
        let data = assembly.native.tag(tag, Some(0x80807BEA))?;
        let perks = sundial::package_authoring::native_weapon::base_sandbox_perks(&data.0)
            .map_err(anyhow::Error::msg)?;
        ensure!(
            !perks.is_empty(),
            "Stock perk plug {plug:08X} has no sandbox perk"
        );
        equivalents.insert(source, (plug, perks));
    }
    let mut columns = sockets.iter().map(|_| Value::Null).collect::<Vec<_>>();
    for socket in &perk_sockets {
        columns[socket.index] = json!({"choices":[],"socket_type":u16::MAX});
    }
    let mut choices = Vec::new();
    let mut unsupported = Vec::new();
    let mut damage = None;
    for node in source["talents"]["nodes"]
        .as_array()
        .context("Source talent nodes")?
    {
        let column = node["column"].as_i64().context("Source talent column")?;
        let row = node["row"].as_i64().context("Source talent row")?;
        for step in node["steps"].as_array().context("Source talent steps")? {
            let name = step["name"].as_str().context("Source talent name")?;
            let lowered = (|| -> Result<Lowered> {
                let variants = step["variants"]
                    .as_array()
                    .context("Source talent variants")?;
                ensure!(
                    variants.len() == 1,
                    "Talent step has multiple runtime variants"
                );
                let record = Payload(hex::decode(
                    variants[0]["record"]
                        .as_str()
                        .context("Source variant bytes")?,
                )?);
                let (stats, adjusted) =
                    adjustments(&record, &variants[0]["stat_adjustments"], &definitions)?;
                let perks = variants[0]["perks"]
                    .as_array()
                    .context("Source talent perks")?;
                // A choice with only stat rows becomes a stats-only private plug.
                ensure!(
                    !perks.is_empty() || !stats.is_empty(),
                    "Talent step has no standalone runtime perk"
                );
                let mut programs = Vec::new();
                let mut raw = Vec::new();
                let mut tags = Vec::new();
                let mut auxiliary = 0;
                let mut stock = Vec::new();
                let mut stock_plugs = Vec::new();
                for perk in perks {
                    let tag = u32::try_from(
                        perk["action"]
                            .as_u64()
                            .context("Source perk has no standalone action")?,
                    )?;
                    tags.push(tag);
                    let identity =
                        u32::try_from(perk["hash"].as_u64().context("Source perk identity")?)?;
                    if let Some((plug, indices)) = equivalents.get(&identity) {
                        stock.extend(indices.iter().copied());
                        stock_plugs.push(format!("{plug:08X}"));
                        continue;
                    }
                    let (program, omitted) = program(assembly.source, tag, name)?;
                    auxiliary += omitted;
                    programs.push(json!({"source_perk_index":template_perk,"program":program,"runtime_values":[],"action_float_values":[]}));
                    raw.push(program);
                }
                Ok(Lowered {
                    programs,
                    raw,
                    tags,
                    auxiliary,
                    stats,
                    adjustments: adjusted,
                    stock,
                    stock_plugs,
                })
            })();
            let lowered = match lowered {
                Ok(lowered) => lowered,
                Err(error) => {
                    unsupported.push(json!({"node":node["index"],"step":step["index"],"name":name,"reason":format!("{error:#}")}));
                    continue;
                }
            };
            let report = json!({"node":node["index"],"step":step["index"],"name":name,
                "column":column,"row":row,"source_actions":lowered.tags,
                "stat_adjustments":lowered.adjustments,"stock_perk_plugs":lowered.stock_plugs,
                "omitted_presentation_records":lowered.auxiliary});
            if let Some(kind) = damage_type(&lowered.raw)
                .filter(|_| lowered.stats.is_empty() && lowered.stock.is_empty())
            {
                damage.get_or_insert(kind);
                choices.push(Choice {
                    column: i64::MIN,
                    row,
                    name: name.to_owned(),
                    description: step["description"].clone(),
                    icon: None,
                    lowered,
                    report,
                });
                continue;
            }
            let container = u32::try_from(step["icon"].as_u64().unwrap_or(0))?;
            let icon = assembly
                .talent_icon(container)
                .with_context(|| format!("Source talent {name} icon"))?;
            choices.push(Choice {
                column,
                row,
                name: name.to_owned(),
                description: step["description"].clone(),
                icon,
                lowered,
                report,
            });
        }
    }
    let damage_kinds = choices
        .iter()
        .filter(|c| c.column == i64::MIN)
        .filter_map(|c| damage_type(&c.lowered.raw))
        .collect::<BTreeSet<_>>();
    if damage_kinds.len() > 1 {
        let damage_nodes = choices
            .iter()
            .filter(|c| c.column == i64::MIN)
            .map(|c| c.report["node"].as_u64().context("Damage talent node"))
            .collect::<Result<BTreeSet<_>>>()?;
        ensure!(
            damage_nodes.len() == 1,
            "Damage alternatives belong to different source nodes"
        );
        for choice in choices.iter_mut().filter(|c| c.column == i64::MIN) {
            choice.column = i64::MAX;
            choice.report["damage_choice"] = json!(damage_type(&choice.lowered.raw));
        }
    }
    let mut source_columns = choices
        .iter()
        .map(|choice| choice.column)
        .filter(|&column| column != i64::MIN)
        .collect::<Vec<_>>();
    source_columns.sort_unstable();
    source_columns.dedup();
    let mut variants = Vec::new();
    let mut translated = Vec::new();
    for choice in choices.iter().filter(|choice| choice.column == i64::MIN) {
        let mut report = choice.report.clone();
        report["damage_type"] = json!(damage);
        translated.push(report);
    }
    for (ordinal, &column) in source_columns.iter().enumerate() {
        let mut members = choices
            .iter()
            .filter(|choice| choice.column == column)
            .collect::<Vec<_>>();
        members.sort_by_key(|choice| choice.row);
        // The runtime base supplies socket layouts, not a limit on source talent columns.
        // Additional columns use a normal perk layout and still receive private plug identities.
        let extra;
        let socket = if let Some(socket) = perk_sockets.get(ordinal) {
            socket
        } else {
            let layout = perk_sockets
                .last()
                .context("Native base has no perk socket layout")?;
            extra = PerkSocket {
                index: columns.len(),
                socket_type: layout.socket_type,
                plugs: layout.plugs.clone(),
            };
            columns.push(Value::Null);
            &extra
        };
        let mut plugs = Vec::new();
        for (choice_index, choice) in members.into_iter().enumerate() {
            // The template only lends its layout. Each choice still becomes its own private item,
            // so a pool shorter than the column repeats its plugs.
            let plug = socket.plugs[choice_index % socket.plugs.len()];
            plugs.push(format!("0x{plug:08X}"));
            let mut variant = json!({"socket_index":socket.index,"choice_index":choice_index,
                "source_plug_hash":format!("0x{plug:08X}"),"name":choice.name,
                "description":choice.description,"replace_effects":true,
                "investment_stats":choice.lowered.stats,"additional_sandbox_perks":choice.lowered.stock,
                "sandbox_perks":choice.lowered.programs});
            if let Some(dependency) = choice
                .lowered
                .raw
                .iter()
                .find_map(|program| program.projectile_dependency())
            {
                variant["projectile"] = serde_json::to_value(dependency)?;
            }
            if let Some(icon) = &choice.icon {
                variant["icon"] =
                    json!({"source":"image","name":choice.name,"image":{"png_base64":icon}});
            }
            variants.push(variant);
            let mut report = choice.report.clone();
            report["socket"] = json!(socket.index);
            report["choice"] = json!(choice_index);
            report["template_plug"] = json!(format!("{plug:08X}"));
            translated.push(report);
        }
        if !plugs.is_empty() {
            columns[socket.index] = json!({"choices":plugs,"socket_type":socket.socket_type});
        }
    }
    ensure!(
        !translated.is_empty(),
        "No source talent behavior is currently supported for this weapon"
    );
    Ok((
        json!(columns),
        json!(variants),
        json!({"translated":translated,"unsupported":unsupported,"damage_type":damage,
        "finished_perk_template":{"plug":template_hash,"index":template_perk},
        "adaptation":"Source talent columns become private socket choices in row order, with additional perk sockets when needed. Supported talents are available immediately, with plain stat adjustments as equipped stat contributions. A fixed damage talent sets the weapon's damage type. Alternative damage steps share a separate socket, with the first source step as the initial damage type. Source progression, auxiliary status presentation and unsupported talents are not migrated.",
        "gameplay_verified":false}),
        damage,
    ))
}
