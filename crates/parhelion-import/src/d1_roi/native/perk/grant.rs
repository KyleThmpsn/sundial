//! Source ability properties resolved by pattern identity and lowered into private bank rows.
use super::*;
use crate::d1_roi::ability::movement;
use sundial::package_authoring::ability_modifier;
use sundial::package_authoring::sandbox_perk::program::{
    AbilityRow, AbilityRowModifier, PropertyOperation,
};
mod attack;
mod effect;
mod evade;
mod fixed;
mod group;
mod integer;
mod port;
mod property;
mod rate;
mod scalar;
mod successor;
pub(super) mod witness;
pub(in crate::d1_roi::native) use evade::Evades;

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

/// A source ability graph's one recognized property bank, its property rows and the graph's
/// component owners.
pub(super) fn property_bank(
    source: &Source,
    graph: u32,
) -> Result<(u32, Payload, Vec<usize>, Vec<u32>)> {
    let tag = graph;
    let graph = source.read(tag)?;
    let mut found = None;
    let mut owners = Vec::new();
    for row in graph.array(32, 12, Some(0x80800715))? {
        let bank = graph.u32(row)?;
        owners.push(bank);
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
    let (bank, data, rows) = found.context("Source ability has no recognized property bank")?;
    Ok((bank, data, rows, owners))
}

struct Ability {
    pattern: u32,
    bank: u32,
    data: Payload,
    rows: Vec<usize>,
    native: Option<u32>,
    native_graph: Option<u32>,
    /// The source graph's component owners.
    owners: Vec<u32>,
    /// The source glide's default horizontal burst factor, for a glide pattern.
    glide_default: Option<u32>,
}

pub(super) struct Catalog {
    abilities: Vec<Ability>,
    /// An item's private evades, when its translation can carry their human actions.
    evades: std::cell::RefCell<Option<Evades>>,
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
            let (bank, data, rows, owners) = property_bank(source, tag)?;
            let glide_default = if movement::Family::of(&data)? == Some(movement::Family::Glide) {
                movement::glide_default(source, &owners)?
            } else {
                None
            };
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
                owners,
                glide_default,
            });
        }
        ensure!(!abilities.is_empty(), "Source ability table is empty");
        Ok(Self {
            abilities,
            evades: std::cell::RefCell::new(None),
        })
    }

    /// Lets an airborne evade grant play the source's own human actions, converted under
    /// `symbol` for one item's graph. Without it the grant selects the native profile.
    pub(super) fn with_evades(self, symbol: String) -> Self {
        *self.evades.borrow_mut() = Some(Evades::new(symbol));
        self
    }

    /// The human actions the item's private evades play, for its graph.
    pub(super) fn take_human(&self) -> Option<crate::d1_roi::ability::animation::Human> {
        self.evades.borrow_mut().take()?.into_human()
    }

    /// The native graph assigned to the source ability row's pattern.
    pub(super) fn native_graph(&self, row: u8) -> Option<u32> {
        self.abilities.get(usize::from(row))?.native_graph
    }

    /// One script parameter that `key` sets in every source Super and in no other ability, each
    /// row its bank's only writer of that parameter, and the stock action writing the same value
    /// on the Super slot, as the program action and its evidence.
    fn super_property(
        &self,
        source: &Source,
        native: &mut Reader,
        key: u32,
    ) -> Result<Option<(Action, Value)>> {
        let mut parameter: Option<(u32, u32, u32)> = None;
        let mut holders = BTreeSet::new();
        for (ordinal, ability) in self.abilities.iter().enumerate() {
            for &at in &ability.rows {
                if ability.data.u32(at)? != key {
                    continue;
                }
                let Some(found) = witness::script_parameters(&ability.data, &ability.rows, at)?
                else {
                    return Ok(None);
                };
                let [(name, reset, value, _)] = found.as_slice() else {
                    return Ok(None);
                };
                // Add and replace agree from the reset while this row is the parameter's only
                // writer in its bank.
                for &other in ability.rows.iter().filter(|&&other| other != at) {
                    if witness::script_parameters(&ability.data, &ability.rows, other)?
                        .is_some_and(|other| other.iter().any(|p| p.0 == *name))
                    {
                        return Ok(None);
                    }
                }
                let current = (*name, *reset, *value);
                if parameter.is_some_and(|first| first != current) {
                    return Ok(None);
                }
                parameter = Some(current);
                holders.insert(ordinal);
            }
        }
        let Some((name, reset, value)) = parameter else {
            return Ok(None);
        };
        let supers = source
            .subclass_choices()?
            .iter()
            .map(|choice| usize::from(choice.ability_row))
            .collect::<BTreeSet<_>>();
        if holders != supers {
            return Ok(None);
        }
        let mask = 1u32 << sundial::package_authoring::ability_bank::AbilityTarget::Super.byte();
        let Some((action, tag)) = property::witness(native, name, mask, value, reset)? else {
            return Ok(None);
        };
        Ok(Some((
            action,
            json!({"parameter":name,"reset":reset,"value":value,
                "source_abilities":holders,"ability_mask":mask,"native_witness":tag}),
        )))
    }

    /// Lower one source ability property key an action's own effect applies on `slot`, as a
    /// grant of that key to every ability whose bank holds it. Returns the program carrying its
    /// native actions and private rows, and the grant report. Every action must reach `slot`.
    pub(super) fn lower_key(
        &self,
        source: &Source,
        native: &mut Reader,
        key: u32,
        slot: sundial::package_authoring::ability_bank::AbilityTarget,
        name: &str,
    ) -> Result<(Program, Vec<Value>)> {
        let mut record = key.to_le_bytes().to_vec();
        record.extend([255, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0]);
        let variant = json!({"ability_grants":[hex::encode(record)]});
        let (program, report) =
            self.lower(source, native, &variant, name, &BTreeMap::new(), false)?;
        ensure!(
            !program.actions.is_empty()
                && program.actions.iter().all(|action| {
                    matches!(action, Action::AbilityProperty { target, .. } if *target == slot)
                        || matches!(action, Action::Property { ability_mask, .. }
                            if *ability_mask == 1 << slot.byte())
                }),
            "Source ability property {key:08X} lowers to another ability slot"
        );
        Ok((program, report))
    }

    /// Lower a variant's property grants. A source row gated on another key lowers only when
    /// `gates` maps that key to the key the caller applies for it: the stock key a native entry
    /// applies, or the private key of a lowered foundation. With `defaults`, a glide group that
    /// leaves a source default in place restores it, for a choice that always applies alone
    /// beside its foundation.
    #[allow(clippy::cognitive_complexity)]
    pub(super) fn lower(
        &self,
        source: &Source,
        native: &mut Reader,
        variant: &Value,
        name: &str,
        gates: &BTreeMap<u32, u32>,
        defaults: bool,
    ) -> Result<(Program, Vec<Value>)> {
        let mut program = Program {
            name: name.to_owned(),
            trigger: Trigger::Always,
            duration_ms: 0,
            ..Program::default()
        };
        let mut report = Vec::new();
        let mut keys = BTreeSet::new();
        let mut dropped = Vec::<String>::new();
        let grants = variant["ability_grants"]
            .as_array()
            .context("Source ability grants")?;
        let grant_count = grants.len();
        for (index, bytes) in grants.iter().enumerate() {
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
            if target == 255
                && let Some((action, evidence)) = self.super_property(source, native, key)?
            {
                let Action::Property { key: name, .. } = &action else {
                    anyhow::bail!("Super property witness is not a Named Property write");
                };
                ensure!(
                    keys.insert(*name),
                    "Source grants overlap one native named property"
                );
                report.push(json!({
                    "source_grant": index,
                    "source_key": key,
                    "route": "native_named_property",
                    "balance": "native",
                    "evidence": evidence,
                }));
                program.actions.insert(0, action);
                continue;
            }
            let mut matched = false;
            for (ordinal, ability) in self.abilities.iter().enumerate() {
                if target != 255 && ordinal != usize::from(target) {
                    continue;
                }
                // Jump and Glide groups lower whole, under one applied key each.
                if movement::Family::of(&ability.data)?.is_some()
                    && ability
                        .rows
                        .iter()
                        .any(|&at| ability.data.u32(at).ok() == Some(key))
                {
                    let bank = ability
                        .native
                        .context("Source movement grant has no native bank")?;
                    let slot = ability_modifier::bank_slot(bank)
                        .context("Native movement bank has no qualified property slot")?;
                    ensure!(
                        slot == sundial::package_authoring::ability_bank::AbilityTarget::Jump,
                        "Source movement grant resolved another ability slot"
                    );
                    let glide =
                        movement::Family::of(&ability.data)? == Some(movement::Family::Glide);
                    ensure!(
                        !(defaults && glide && ability.glide_default.is_none()),
                        "Source glide has no readable default horizontal factor"
                    );
                    let (row, evidence) = movement::lower_group(
                        source,
                        native,
                        ability.bank,
                        bank,
                        key,
                        ability.glide_default.filter(|_| defaults),
                    )?;
                    ensure!(
                        keys.insert(row.key),
                        "Source grants overlap one complete movement group"
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
                if let Some(bank) = ability.native
                    && let Some((slot, evidence)) = port::witness(
                        source,
                        native,
                        &ability.data,
                        &ability.rows,
                        key,
                        bank,
                        gates,
                    )?
                {
                    ensure!(
                        keys.insert(key),
                        "Source grants overlap one native ability property"
                    );
                    let mut record = evidence;
                    record["source_grant"] = json!(index);
                    record["source_ability"] = json!(ordinal);
                    record["source_pattern"] = json!(ability.pattern);
                    report.push(record);
                    program.actions.insert(
                        0,
                        Action::AbilityProperty {
                            target: slot,
                            key,
                            option: PropertyOperation::Apply,
                        },
                    );
                    matched = true;
                    continue;
                }
                for &at in &ability.rows {
                    if ability.data.u32(at)? != key {
                        continue;
                    }
                    matched = true;
                    let Some(bank) = ability.native else {
                        let found = witness::find(native, &ability.data, &ability.rows, at)?;
                        if found.is_none()
                            && let Some((action, evidence)) =
                                successor::lower(native, &ability.data, &ability.rows, at, None)?
                        {
                            let Action::Property { key: name, .. } = &action else {
                                anyhow::bail!("Successor Super write is not a Named Property");
                            };
                            ensure!(
                                keys.insert(*name),
                                "Source grants overlap one native named property"
                            );
                            report.push(json!({"source_grant":index,"source_key":key,
                                "source_ability":ordinal,"source_pattern":ability.pattern,
                                "evidence":evidence}));
                            program.actions.insert(0, action);
                            continue;
                        }
                        let (bank, slot, selected) = found.with_context(|| {
                            format!(
                                "Source ability {:08X} has no matching native bank",
                                ability.pattern
                            )
                        })?;
                        ensure!(
                            keys.insert(selected),
                            "Source grants overlap one native ability property"
                        );
                        report.push(json!({"source_grant":index,"source_key":key,"source_ability":ordinal,
                            "source_pattern":ability.pattern,"source_bank":ability.bank,"source_property_offset":at,
                            "native_bank":bank,"slot":slot.byte(),"property_key":selected,
                            "parameter_index":null,"route":"native_row_witness","balance":"native"}));
                        program.actions.insert(
                            0,
                            Action::AbilityProperty {
                                target: slot,
                                key: selected,
                                option: PropertyOperation::Apply,
                            },
                        );
                        continue;
                    };
                    let slot = ability_modifier::bank_slot(bank)
                        .context("Native ability bank has no qualified property slot")?;
                    let payload = native.tag(bank, Some(0x80809C36))?;
                    let gate = validate_row(&ability.data, at, gates)?;
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
                    let class = ability.data.u32(ability.data.pointer(at + 32)? - 4)?;
                    // A movement attack pair, or a melee attack container under its gate.
                    if matches!(class, 0x80803198 | 0x808025AC) {
                        let (modifier, attacks) =
                            attack::lower(source, native, &ability.data, at, &payload, gate)?;
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
                            "source_modifier_class":class,"gate":gate,
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
                        let private = self.evades.borrow_mut().as_mut().map(|evades| {
                            evade::private(
                                source,
                                native,
                                &ability.owners,
                                &ability.data,
                                at,
                                slot,
                                (bank, &payload),
                                graph,
                                evades,
                            )
                        });
                        let fallback = match private {
                            Some(Ok((row, detail))) => {
                                ensure!(
                                    keys.insert(row.key),
                                    "Source grants overlap one private ability property"
                                );
                                report.push(json!({"source_grant":index,"source_key":key,"source_ability":ordinal,
                                    "source_pattern":ability.pattern,"source_bank":ability.bank,"source_property_offset":at,
                                    "native_bank":bank,"native_graph":graph,"slot":slot.byte(),"mode":row.mode,"property_key":row.key,
                                    "parameter_index":null,"route":"private_air_evade","evade":detail}));
                                program.actions.insert(
                                    0,
                                    Action::AbilityProperty {
                                        target: slot,
                                        key: row.key,
                                        option: PropertyOperation::Apply,
                                    },
                                );
                                program.ability_rows.push(row);
                                continue;
                            }
                            Some(Err(error)) => Some(format!("{error:#}")),
                            None => None,
                        };
                        let selected =
                            evade::lower(native, &ability.data, at, slot, &payload, graph)?;
                        ensure!(
                            keys.insert(selected),
                            "Source grants overlap one native ability property"
                        );
                        report.push(json!({"source_grant":index,"source_key":key,"source_ability":ordinal,
                            "source_pattern":ability.pattern,"source_bank":ability.bank,"source_property_offset":at,
                            "native_bank":bank,"native_graph":graph,"slot":slot.byte(),"property_key":selected,
                            "parameter_index":null,"route":"native_air_evade","private_refusal":fallback}));
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
                    let lowered = match private_rows(
                        source,
                        native,
                        &ability.data,
                        at,
                        (bank, slot, &payload),
                        gate,
                    ) {
                        Ok(lowered) => lowered,
                        Err(error) => {
                            // A script parameter the native bank cannot list, which the native
                            // ability's script still declares, takes a Named Property write.
                            if let Some(graph) = ability.native_graph
                                && let Some((action, evidence)) = successor::lower(
                                    native,
                                    &ability.data,
                                    &ability.rows,
                                    at,
                                    Some(graph),
                                )?
                            {
                                let Action::Property { key: name, .. } = &action else {
                                    anyhow::bail!(
                                        "Declared parameter write is not a Named Property"
                                    );
                                };
                                ensure!(
                                    keys.insert(*name),
                                    "Source grants overlap one native named property"
                                );
                                report.push(json!({"source_grant":index,"source_key":key,
                                    "source_ability":ordinal,"source_pattern":ability.pattern,
                                    "source_property_offset":at,"evidence":evidence}));
                                program.actions.insert(0, action);
                                continue;
                            }
                            // A motion modifier has no native route. The talent keeps its other
                            // grants and reports the motion as left out, but a grant set with
                            // nothing else still refuses.
                            if class == MOTION {
                                dropped.push(error.to_string());
                                report.push(json!({"source_grant":index,"source_key":key,
                                    "source_ability":ordinal,"source_pattern":ability.pattern,
                                    "source_property_offset":at,"source_modifier_class":class,
                                    "route":"left_out","reason":format!("{error:#}")}));
                                continue;
                            }
                            return Err(error);
                        }
                    };
                    for (row, parameter) in lowered {
                        // A contact effect keeps the native entity a hit attaches.
                        let entities = match &row.modifier {
                            AbilityRowModifier::Fixed { class, words, .. }
                                if *class == 0x8080442A =>
                            {
                                json!({"source_entity":ability.data.u32(ability.data.pointer(at + 32)? + 16)?,
                                    "native_entity":words[4]})
                            }
                            _ => Value::Null,
                        };
                        ensure!(
                            keys.insert(row.key),
                            "Source grants overlap one private ability property"
                        );
                        report.push(json!({"source_grant":index,"source_key":key,"source_ability":ordinal,
                            "source_pattern":ability.pattern,"source_bank":ability.bank,"source_property_offset":at,
                            "native_bank":bank,"slot":slot.byte(),"mode":row.mode,"property_key":row.key,"parameter_index":parameter,
                            "source_modifier_class":class,"gate":gate,"retained_native":entities}));
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
            if !matched && target != 255 && grant_count <= 16 {
                // A grant to one named ability whose source bank has no row for the key applies
                // nothing in the source either. It only takes one of the sixteen local key slots
                // the producer fills before bank lookup, so with no more than sixteen grants its
                // absence crowds out no other key. It emits nothing, never the native stock key.
                report.push(json!({
                    "source_grant": index,
                    "source_key": key,
                    "source_ability": target,
                    "route": "absent_in_source_bank",
                }));
                continue;
            }
            ensure!(
                matched,
                "Source ability grant {key:08X} has no property definition"
            );
        }
        if let Some(error) = dropped.first() {
            ensure!(!program.actions.is_empty(), "{error}");
        }
        if !program.actions.is_empty() {
            program.validate().map_err(anyhow::Error::msg)?;
        }
        Ok((program, report))
    }
}

/// A source row's private rows on `bank`, each checked against the bank's handlers and parameter
/// resets, with the parameter index each sets.
fn private_rows(
    source: &Source,
    native: &mut Reader,
    data: &Payload,
    at: usize,
    (bank, slot, payload): (
        u32,
        sundial::package_authoring::ability_bank::AbilityTarget,
        &Payload,
    ),
    gate: Option<u32>,
) -> Result<Vec<(AbilityRow, Option<usize>)>> {
    let mode = u8::try_from(data.u32(at + 16)?)?;
    modifiers(source, native, data, at, payload, gate)?
        .into_iter()
        .map(|(modifier, parameter)| {
            let row = AbilityRow::new(bank, slot, mode, modifier);
            row.check_bank(&payload.0).map_err(anyhow::Error::msg)?;
            Ok((row, parameter))
        })
        .collect()
}

/// The source motion modifier class (`8080286C`), the launch parameters of a Super's movement.
/// Shadowkeep's Fists of Havoc bank class cannot hold the native motion recipient (`808041D8`),
/// and no translation of its rows exists.
const MOTION: u32 = 0x8080_286C;

/// The row's gate key, when it has one the caller admits. Only a wildcard-priority script
/// parameter row can carry one. Other classes keep their own conditional routes.
/// The key the row's gate maps to, when it has one the caller admits. Script parameter rows,
/// numeric rows and melee attack containers can carry a gate, and the latter two also keep their
/// source priority. Other classes keep their own conditional routes.
fn validate_row(data: &Payload, row: usize, gates: &BTreeMap<u32, u32>) -> Result<Option<u32>> {
    let class = data.u32(data.pointer(row + 32)? - 4)?;
    let conditional = matches!(class, 0x8080209C | 0x808028B8 | 0x80803198 | 0x80802AA8);
    let gate = if data.u64(row + 24)? == 0 {
        None
    } else {
        let at = data.pointer(row + 24)?;
        // The source gate has one word. The next word may be another class marker.
        (at >= 4 && data.u32(at - 4)? == 0x80802981).then_some(data.u32(at)?)
    };
    let gateable = matches!(class, 0x80802974 | 0x8080235E | 0x808025AC);
    let prioritized = matches!(class, 0x8080235E | 0x808025AC);
    let mapped = gate.and_then(|key| gates.get(&key).copied());
    ensure!(
        data.u32(row + 20)? == 0
            && matches!(data.u32(row + 16)?, 1 | 255)
            && (conditional
                || ((data.bytes::<4>(row + 8)? == [255, 0, 0, 0]
                    || (prioritized && data.u32(row + 8)? <= 255))
                    && (data.u64(row + 24)? == 0 || (gateable && mapped.is_some())))),
        "Source ability property has a gate or unqualified flags"
    );
    Ok(mapped.filter(|_| gateable))
}

fn modifiers(
    source: &Source,
    reader: &mut Reader,
    data: &Payload,
    row: usize,
    native: &Payload,
    gate: Option<u32>,
) -> Result<Vec<(AbilityRowModifier, Option<usize>)>> {
    let at = data.pointer(row + 32)?;
    match data.u32(at - 4)? {
        0x8080243C => rate::lower(data, row, native),
        0x8080296E => group::lower(source, data, row, native),
        0x8080251A | 0x808028BA | 0x808030A9 | 0x80802591 | 0x8080209C | 0x80802365
        | 0x808028B8 | 0x80802B72 | 0x808026D1 | 0x8080241C | 0x8080206A | 0x80802650
        | 0x808027CB | 0x80802000 => fixed::lower(reader, data, row, native),
        0x8080235E => scalar::lower(source, data, row, native, gate),
        0x80802662 => integer::lower(data, row, native),
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
                            gate,
                        },
                        Some(start + index),
                    ))
                })
                .collect()
        }
        class => anyhow::bail!("Source ability modifier {class:08X} requires translation"),
    }
}
