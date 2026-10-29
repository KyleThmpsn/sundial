//! Source ability properties resolved by pattern identity and lowered into private bank rows.
use super::*;
use sundial::package_authoring::ability_modifier;
use sundial::package_authoring::sandbox_perk::program::{
    AbilityRow, AbilityRowModifier, PropertyOperation,
};
mod attack;
mod effect;
mod evade;
mod fixed;
mod group;
mod rate;
mod scalar;

// Root bank pairs of the source ability table. Each owns the same property array contract.
const BANKS: &[(u32, u32)] = &[
    (0x8080200F, 0x80802AC8),
    (0x808021B0, 0x808025C6),
    (0x808022AA, 0x80802B60),
    (0x808022E9, 0x8080259E),
    (0x80802343, 0x80802B06),
    (0x8080250F, 0x808024BC),
    (0x808025EB, 0x808025C9),
    (0x80802682, 0x80802467),
    (0x8080275C, 0x80802593),
    (0x808027BE, 0x808029B5),
    (0x80802883, 0x80802200),
    (0x808029ED, 0x80802979),
    (0x80802AAC, 0x80802ADC),
    (0x80802B75, 0x808028B0),
    (0x8080316B, 0x80803076),
    (0x808031E7, 0x80803210),
];

struct Ability {
    pattern: u32,
    bank: u32,
    data: Payload,
    rows: Vec<usize>,
    native: Option<u32>,
    native_graph: Option<u32>,
}

pub(super) struct Catalog {
    abilities: Vec<Ability>,
}

impl Catalog {
    pub(super) fn read(source: &Source, native: &mut Reader) -> Result<Self> {
        let tags = source.classes(0x80803432);
        ensure!(
            tags.len() == 1,
            "Source ability definitions are missing or ambiguous"
        );
        let definitions = source.read(tags[0])?;
        let assignments = source.runtime_assignments(&mut BTreeMap::new())?;
        let tags = native.classes(0x80805CA4);
        ensure!(
            tags.len() == 1,
            "Native ability definitions are missing or ambiguous"
        );
        let target = native.tag(tags[0], Some(0x80805CA4))?;
        let mut patterns = BTreeSet::new();
        for row in target.array(8, 28, Some(0x80805CAA))? {
            ensure!(
                patterns.insert(target.u32(row + 4)?),
                "Duplicate native ability pattern"
            );
        }
        let mut targets = BTreeMap::new();
        let root = native
            .manager
            .lookup
            .named_tags
            .iter()
            .find(|tag| tag.name == "investment_assets")
            .context("Native investment assets")?
            .hash
            .0;
        let root = native.tag(root, None)?;
        let map = native.tag(root.u32(16)?, Some(0x80809780))?;
        for row in map.array(8, 8, None)? {
            let pattern = map.u32(row)?;
            if !patterns.contains(&pattern) {
                continue;
            }
            ensure!(
                targets.insert(pattern, map.u32(row + 4)?).is_none(),
                "Ambiguous native ability assignment"
            );
        }
        let mut abilities = Vec::new();
        for row in definitions.array(8, 40, Some(0x8080343C))? {
            let pattern = definitions.u32(row)?;
            let tag = *assignments
                .get(&pattern)
                .context("Source ability has no runtime assignment")?;
            ensure!(
                source.reference(tag)? == 0x80800734,
                "Source ability is not a pattern"
            );
            let graph = source.read(tag)?;
            let mut found = None;
            for row in graph.array(32, 12, Some(0x80800715))? {
                let bank = graph.u32(row)?;
                ensure!(
                    source.reference(bank)? == 0x80800861,
                    "Source ability owner class differs"
                );
                let data = source.read(bank)?;
                let (runtime, settings) = (data.pointer(16)?, data.pointer(24)?);
                let pair = (data.u32(runtime - 4)?, data.u32(settings - 4)?);
                if !BANKS.contains(&pair) {
                    continue;
                }
                ensure!(
                    found.is_none(),
                    "Source ability has multiple property banks"
                );
                ensure!(
                    data.u32(runtime)? == bank
                        && data.u64(runtime + 8)? == settings as u64
                        && data.u32(runtime + 16)? == pair.1
                        && data.u32(settings)? == bank
                        && data.u64(settings + 8)? == runtime as u64
                        && data.u32(settings + 16)? == pair.0,
                    "Source ability bank pair is not reciprocal"
                );
                let rows = data.array(settings + 0xA0, 40, Some(0x80802A84))?;
                found = Some((bank, data, rows));
            }
            let (bank, data, rows) =
                found.context("Source ability has no recognized property bank")?;
            let target = if let Some(&tag) = targets.get(&pattern) {
                let graph = native.tag(tag, Some(0x80809C0F))?;
                ability_modifier::entity_bank(&graph.0).map_err(anyhow::Error::msg)?
            } else {
                None
            };
            abilities.push(Ability {
                pattern,
                bank,
                data,
                rows,
                native: target,
                native_graph: targets.get(&pattern).copied(),
            });
        }
        ensure!(!abilities.is_empty(), "Source ability table is empty");
        Ok(Self { abilities })
    }

    pub(super) fn lower(
        &self,
        source: &Source,
        native: &mut Reader,
        variant: &Value,
        name: &str,
    ) -> Result<(Program, Vec<Value>)> {
        let mut program = Program {
            name: name.to_owned(),
            trigger: Trigger::Always,
            duration_ms: 0,
            ..Program::default()
        };
        let mut report = Vec::new();
        let mut keys = BTreeSet::new();
        for (index, bytes) in variant["ability_grants"]
            .as_array()
            .context("Source ability grants")?
            .iter()
            .enumerate()
        {
            let record = Payload(hex::decode(
                bytes.as_str().context("Source ability grant bytes")?,
            )?);
            ensure!(
                record.0.len() == 16
                    && record.bytes::<3>(5)? == [0; 3]
                    && record.bytes::<8>(8)? == [255, 255, 255, 255, 0, 0, 0, 0],
                "Source grant equips an ability or has additional selection metadata"
            );
            let key = record.u32(0)?;
            if key == 0x811C9DC5 {
                ensure!(
                    record.u8(4)? == 255
                        && self.abilities.iter().all(|ability| {
                            ability.rows.iter().all(|&at| {
                                ability.data.u32(at).is_ok_and(|candidate| candidate != key)
                            })
                        }),
                    "Empty source ability grant has a selection or a property definition"
                );
                // The empty-name key with no selected ability contributes no property.
                // Keep the original record index for later real grants in the same array.
                continue;
            }
            ensure!(
                ![0, u32::MAX].contains(&key),
                "Source ability grant has no property key"
            );
            let target = record.u8(4)?;
            ensure!(
                target == 255 || usize::from(target) < self.abilities.len(),
                "Source grant ability index exceeds its table"
            );
            let mut matched = false;
            for (ordinal, ability) in self.abilities.iter().enumerate() {
                if target != 255 && ordinal != usize::from(target) {
                    continue;
                }
                if ability.data.u32(ability.data.pointer(16)? - 4)? == 0x8080275C
                    && ability
                        .rows
                        .iter()
                        .any(|&at| ability.data.u32(at).ok() == Some(key))
                {
                    let bank = ability
                        .native
                        .context("Source Jump grant has no native bank")?;
                    let slot = ability_modifier::bank_slot(bank)
                        .context("Native Jump bank has no qualified property slot")?;
                    ensure!(
                        slot == sundial::package_authoring::ability_bank::AbilityTarget::Jump,
                        "Source Jump grant resolved another ability slot"
                    );
                    let (row, evidence) = crate::d1_roi::ability::movement::lower_group(
                        source,
                        native,
                        ability.bank,
                        bank,
                        key,
                    )?;
                    ensure!(
                        keys.insert(row.key),
                        "Source grants overlap one complete Jump group"
                    );
                    for mut record in evidence {
                        record["source_grant"] = json!(index);
                        record["source_ability"] = json!(ordinal);
                        record["source_pattern"] = json!(ability.pattern);
                        record["slot"] = json!(slot.byte());
                        record["parameter_index"] = Value::Null;
                        report.push(record);
                    }
                    program.actions.insert(
                        0,
                        Action::AbilityProperty {
                            target: slot,
                            key: row.key,
                            option: PropertyOperation::Apply,
                        },
                    );
                    program.ability_rows.push(row);
                    matched = true;
                    continue;
                }
                for &at in &ability.rows {
                    if ability.data.u32(at)? != key {
                        continue;
                    }
                    matched = true;
                    let bank = ability.native.with_context(|| {
                        format!(
                            "Source ability {:08X} has no matching native bank",
                            ability.pattern
                        )
                    })?;
                    let slot = ability_modifier::bank_slot(bank)
                        .context("Native ability bank has no qualified property slot")?;
                    let payload = native.tag(bank, Some(0x80809C36))?;
                    validate_row(&ability.data, at)?;
                    if ability.data.u32(ability.data.pointer(at + 32)? - 4)? == 0x80802AA8 {
                        ensure!(
                            slot == sundial::package_authoring::ability_bank::AbilityTarget::Melee,
                            "Contact-effect bridge requires the melee ability bank"
                        );
                        let (selected, effect) =
                            effect::lower(source, native, &ability.data, at, &payload)?;
                        ensure!(
                            keys.insert(selected),
                            "Source grants overlap one native ability property"
                        );
                        report.push(json!({"source_grant":index,"source_key":key,"source_ability":ordinal,
                            "source_pattern":ability.pattern,"source_bank":ability.bank,"source_property_offset":at,
                            "native_bank":bank,"slot":slot.byte(),"property_key":selected,
                            "parameter_index":null,"route":"native_melee_effect","balance":"native","effect":effect}));
                        program.actions.insert(
                            0,
                            Action::AbilityProperty {
                                target: slot,
                                key: selected,
                                option: PropertyOperation::Apply,
                            },
                        );
                        continue;
                    }
                    if ability.data.u32(ability.data.pointer(at + 32)? - 4)? == 0x80803198 {
                        let (modifier, attacks) =
                            attack::lower(source, native, &ability.data, at, &payload)?;
                        let row = AbilityRow::new(
                            bank,
                            slot,
                            u8::try_from(ability.data.u32(at + 16)?)?,
                            modifier,
                        );
                        row.check_bank(&payload.0).map_err(anyhow::Error::msg)?;
                        let selected = row.key;
                        ensure!(
                            keys.insert(selected),
                            "Source grants overlap one native ability property"
                        );
                        report.push(json!({"source_grant":index,"source_key":key,"source_ability":ordinal,
                            "source_pattern":ability.pattern,"source_bank":ability.bank,"source_property_offset":at,
                            "native_bank":bank,"slot":slot.byte(),"property_key":selected,
                            "parameter_index":null,"route":"private_attack_pair","balance":"source_amounts_native_feedback","attacks":attacks}));
                        program.actions.insert(
                            0,
                            Action::AbilityProperty {
                                target: slot,
                                key: selected,
                                option: PropertyOperation::Apply,
                            },
                        );
                        program.ability_rows.push(row);
                        continue;
                    }
                    if ability.data.u32(ability.data.pointer(at + 32)? - 4)? == 0x808030C8 {
                        let graph = ability.native_graph.context("Native movement graph")?;
                        let selected =
                            evade::lower(native, &ability.data, at, slot, &payload, graph)?;
                        ensure!(
                            keys.insert(selected),
                            "Source grants overlap one native ability property"
                        );
                        report.push(json!({"source_grant":index,"source_key":key,"source_ability":ordinal,
                            "source_pattern":ability.pattern,"source_bank":ability.bank,"source_property_offset":at,
                            "native_bank":bank,"native_graph":graph,"slot":slot.byte(),"property_key":selected,
                            "parameter_index":null,"route":"native_air_evade"}));
                        program.actions.insert(
                            0,
                            Action::AbilityProperty {
                                target: slot,
                                key: selected,
                                option: PropertyOperation::Apply,
                            },
                        );
                        continue;
                    }
                    for (modifier, parameter) in modifiers(source, &ability.data, at, &payload)? {
                        let row = AbilityRow::new(
                            bank,
                            slot,
                            u8::try_from(ability.data.u32(at + 16)?)?,
                            modifier,
                        );
                        row.check_bank(&payload.0).map_err(anyhow::Error::msg)?;
                        ensure!(
                            keys.insert(row.key),
                            "Source grants overlap one private ability property"
                        );
                        report.push(json!({"source_grant":index,"source_key":key,"source_ability":ordinal,
                            "source_pattern":ability.pattern,"source_bank":ability.bank,"source_property_offset":at,
                            "native_bank":bank,"slot":slot.byte(),"mode":row.mode,"property_key":row.key,"parameter_index":parameter}));
                        program.actions.insert(
                            0,
                            Action::AbilityProperty {
                                target: slot,
                                key: row.key,
                                option: PropertyOperation::Apply,
                            },
                        );
                        program.ability_rows.push(row);
                    }
                }
            }
            ensure!(
                matched,
                "Source ability grant {key:08X} has no property definition"
            );
        }
        if !program.actions.is_empty() {
            program.validate().map_err(anyhow::Error::msg)?;
        }
        Ok((program, report))
    }
}

fn validate_row(data: &Payload, row: usize) -> Result<()> {
    let conditional = matches!(
        data.u32(data.pointer(row + 32)? - 4)?,
        0x8080209C | 0x808028B8 | 0x80803198 | 0x80802AA8
    );
    ensure!(
        data.u32(row + 20)? == 0
            && matches!(data.u32(row + 16)?, 1 | 255)
            && (conditional
                || (data.bytes::<4>(row + 8)? == [255, 0, 0, 0] && data.u64(row + 24)? == 0)),
        "Source ability property has a gate or unqualified flags"
    );
    Ok(())
}

fn modifiers(
    source: &Source,
    data: &Payload,
    row: usize,
    native: &Payload,
) -> Result<Vec<(AbilityRowModifier, Option<usize>)>> {
    let at = data.pointer(row + 32)?;
    match data.u32(at - 4)? {
        0x8080243C => rate::lower(data, row, native),
        0x8080296E => group::lower(source, data, row, native),
        0x8080251A | 0x808028BA | 0x808030A9 | 0x80802591 | 0x8080209C | 0x80802365
        | 0x808028B8 | 0x80802B72 | 0x808026D1 => fixed::lower(data, row, native),
        0x8080235E => scalar::lower(source, data, row, native),
        0x80802254 => Ok(vec![(
            AbilityRowModifier::Charges {
                count: u8::try_from(data.u32(at)?)?,
            },
            None,
        )]),
        0x80802974 => {
            ensure!(
                data.bytes::<16>(at)? == [0; 16],
                "Source ability parameter has an enable driver"
            );
            let settings = data.pointer(24)?;
            let parameters = data.array(settings + 0xB0, 16, Some(0x80802122))?;
            let start = usize::try_from(data.u32(at + 16)?)?;
            let count = usize::try_from(data.u32(at + 20)?)?;
            let end = start
                .checked_add(count)
                .context("Source ability parameter range overflow")?;
            ensure!(
                count > 0 && end <= parameters.len(),
                "Source ability parameter range is empty or exceeds its table"
            );
            parameters[start..end]
                .iter()
                .enumerate()
                .map(|(index, &at)| {
                    ensure!(
                        data.u32(at + 12)? <= 1,
                        "Source ability parameter operation differs"
                    );
                    Ok((
                        AbilityRowModifier::Parameter {
                            name: data.u32(at)?,
                            reset: data.u32(at + 4)?,
                            value: data.u32(at + 8)?,
                            add: data.u32(at + 12)? != 0,
                        },
                        Some(start + index),
                    ))
                })
                .collect()
        }
        class => anyhow::bail!("Source ability modifier {class:08X} requires translation"),
    }
}
