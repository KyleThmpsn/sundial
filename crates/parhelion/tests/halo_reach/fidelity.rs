//! Follow staged health responses, visible-state conditions and private melee banks.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
mod geometry;
mod transfer;

const STATES: [u32; 5] = [0x2ca33bdb, 0xd09ad130, 0xa9d28bb0, 0x4a3554b4, 0x51033a3a];

fn hash(name: &str) -> u32 {
    name.bytes().fold(0x811c9dc5, |v: u32, b| {
        v.wrapping_mul(16777619) ^ u32::from(b)
    })
}

fn component(staged: &mut Reader, runtime: &Value, class: &str) -> Result<std::sync::Arc<Payload>> {
    let rows = runtime["components"]
        .as_array()
        .context("Runtime components")?
        .iter()
        .filter(|r| r["class"] == class)
        .collect::<Vec<_>>();
    ensure!(rows.len() == 1, "Expected one {class} component");
    staged.tag(
        u32::from_str_radix(rows[0]["owner"].as_str().context("Component owner")?, 16)?,
        Some(0x80809c36),
    )
}

fn number(value: &Value) -> Result<f32> {
    value
        .as_f64()
        .map(|v| v as f32)
        .context("Independent source number")
}

fn close(actual: f32, wanted: f32) -> Result<()> {
    ensure!(
        actual.is_finite() && (actual - wanted).abs() <= wanted.abs().max(1.) * 1e-5,
        "Native value {actual} differs from source {wanted}"
    );
    Ok(())
}

fn effect(staged: &mut Reader, stock: &Reader, tag: u32, looping: bool) -> Result<Value> {
    ensure!(
        stock.reference(tag).is_err(),
        "Damage effect still selects a stock entity"
    );
    let entity = staged.tag(tag, Some(0x80809c0f))?;
    let mut sequences = Vec::new();
    for row in entity.array(16, 12, None)? {
        let owner = entity.u32(row)?;
        let p = staged.tag(owner, Some(0x80809c36))?;
        let d = p.pointer(24)?;
        if p.u32(d - 4)? != 0x808084e9 {
            continue;
        }
        let controls = p.array(d + 0x158, 24, Some(0x808093e6))?;
        let events = p.array(d + 0x168, 24, Some(0x808093e6))?;
        ensure!(
            !controls.is_empty() && !events.is_empty(),
            "Damage effect has no scheduled content"
        );
        let mut repeats = 0;
        for row in &controls {
            let at = p.pointer(row + 16)?;
            repeats += usize::from(p.u8(at + 0x32)? != 0);
        }
        let mut retirements = 0;
        let mut classes = Vec::new();
        let mut particles = Vec::new();
        for row in &events {
            let at = p.pointer(row + 16)?;
            let class = p.u32(at - 4)?;
            retirements += usize::from(class == 0x80804ab0);
            classes.push(format!("{class:08X}"));
            if class == 0x80806cc6 {
                for system in p.array(at + 0x40, 24, Some(0x80806cc8))? {
                    ensure!(
                        p.u32(system + 0x14)? == u32::MAX,
                        "Damage particle requires unresolved system override"
                    );
                    let system_tag = p.u32(system + 0x10)?;
                    if system_tag == u32::MAX {
                        continue;
                    }
                    let system = staged.tag(system_tag, Some(0x80806e28))?;
                    let definition_tag = system.u32(0)?;
                    let definition = staged.tag(definition_tag, Some(0x80806e2c))?;
                    let source = definition.bytes::<2>(0x80 + 24 * 2)?;
                    let destination = definition.bytes::<2>(0x80 + 23 * 2)?;
                    // The frame consumer reads and writes this destination even
                    // when the optional initialization source is absent.
                    ensure!(
                        destination[0] <= 1,
                        "Particle angular update has no writable destination"
                    );
                    let rows = definition.u8(0x141 + usize::from(destination[0]))?;
                    ensure!(
                        usize::from(destination[1]) < usize::from(rows) * 4,
                        "Particle angular update destination exceeds its record"
                    );
                    particles.push(json!({"system":format!("{system_tag:08X}"),
                        "definition":format!("{definition_tag:08X}"),
                        "initial_source":source,"initial_destination":destination}));
                }
            }
        }
        ensure!(
            if looping {
                repeats > 0 && retirements == 0
            } else {
                repeats == 0 && retirements == 1
            },
            "Damage sequence has the wrong retirement policy"
        );
        sequences.push(json!({"owner":format!("{owner:08X}"),"controls":controls.len(),"events":classes,"repeats":repeats,"retirements":retirements,"particles":particles}));
    }
    ensure!(sequences.len() == 1, "Damage host has no unique sequence");
    Ok(json!({"entity":format!("{tag:08X}"),"looping":looping,"sequences":sequences}))
}

fn visible_states(staged: &mut Reader, runtime: &Value, witness: &Value) -> Result<usize> {
    let render = component(staged, runtime, "808072BD")?;
    let groups = render.array(render.pointer(24)? + 0x38, 24, Some(0x80809c2c))?;
    let mut actual = BTreeSet::new();
    for &group in &groups {
        let pairs = render.array(group + 8, 8, Some(0x80809c31))?;
        if pairs.is_empty() {
            continue;
        }
        let mut region = None;
        let mut states = Vec::new();
        for pair in pairs {
            match render.u32(pair)? {
                0x51e7a18d => {
                    ensure!(
                        region.replace(render.u32(pair + 4)?).is_none(),
                        "Part has two region selectors"
                    );
                }
                0x6a807a1a => {
                    states.push(render.u32(pair + 4)?);
                }
                _ => anyhow::bail!("Unrecognized authored part condition"),
            }
        }
        states.sort_unstable();
        ensure!(
            actual.insert((region.context("Part has no region")?, states)),
            "Duplicate damage part condition"
        );
    }
    let mut expected = BTreeSet::new();
    for r in witness["regions"]
        .as_array()
        .context("Source region witness")?
    {
        let mut permutations = BTreeMap::<String, Vec<u32>>::new();
        for (i, p) in r["states"]
            .as_array()
            .context("Source state witness")?
            .iter()
            .enumerate()
        {
            let p = p.as_str().context("Source permutation")?;
            if !p.is_empty() {
                permutations
                    .entry(p.to_owned())
                    .or_default()
                    .push(STATES[i]);
            }
        }
        for mut states in permutations.into_values() {
            states.sort_unstable();
            expected.insert((hash(r["name"].as_str().context("Source region")?), states));
        }
    }
    ensure!(
        !expected.is_empty() && actual == expected,
        "Native visible-state conditions differ from source permutations"
    );
    Ok(actual.len())
}

pub(super) fn damage(
    staged: &mut Reader,
    stock: &Reader,
    runtime: &Value,
    witness: &Value,
) -> Result<Value> {
    if witness.is_null() {
        return Ok(Value::Null);
    }
    let source = &witness["fields"];
    let health = component(staged, runtime, "80804B8A")?;
    let d = health.pointer(24)?;
    let i = health.pointer(16)?;
    let vitality = number(&source["Maximum Vitality@0xD0"])?;
    close(health.f32(d + 0x1a8)?, vitality)?;
    let sections = source["Damage Sections@0xE0"]
        .as_array()
        .context("Independent damage sections")?;
    ensure!(!sections.is_empty(), "Required damage witness is empty");
    let native = health.array(d + 0x1b0, 0x138, Some(0x80804c11))?;
    ensure!(
        native.len() == sections.len(),
        "Source health pools were lost"
    );
    let recoveries = health.array(d + 0x368, 0x50, Some(0x80804c5f))?;
    let recovery_instances = health.array(i + 0x340, 0x70, Some(0x80804c5e))?;
    let fractions = health.array(i + 0x310, 4, Some(0x80804c5c))?;
    ensure!(
        recoveries.len() == sections.len()
            && recovery_instances.len() == sections.len()
            && fractions.len() == sections.len(),
        "Source health pools lack initial or replicated state"
    );
    let outputs = health.array(d + 0x398, 56, Some(0x80804b92))?;
    ensure!(
        !outputs.is_empty()
            && health.array(i + 0x368, 32, Some(0x80804b91))?.len() == outputs.len(),
        "Vehicle health publications lack definitions or runtime storage"
    );
    let main = usize::try_from(
        source["Indirect Damage Section Index@0xD8"]
            .as_u64()
            .context("Independent main damage section")?,
    )?;
    let main_name = sections
        .get(main)
        .and_then(|s| s["fields"]["Name@0x0"].as_str())
        .context("Independent main damage section name")?;
    let hud = outputs
        .iter()
        .copied()
        .find(|&at| health.u32(at + 0x28).ok() == Some(0x657cb944))
        .context("Native vehicle HUD health publication is absent")?;
    let hud_recovery = *recoveries
        .get(usize::try_from(health.u32(hud + 0x30)?)?)
        .context("HUD health publication selects an absent recovery")?;
    let hud_section = *native
        .get(usize::try_from(health.u32(hud_recovery + 0x10)?)?)
        .context("HUD health publication selects an absent section")?;
    ensure!(
        health.u32(hud_section + 16)? == hash(main_name)
            && health.u8(hud + 0x34)? == 0
            && health.u8(hud + 0x35)? & 1 != 0,
        "Native vehicle HUD does not publish normalized primary health"
    );
    let slot_count = health.u32(d + 0x5a8)? as usize;
    ensure!(
        health.array(i + 0x3a8, 8, Some(0x8080949e))?.len() == slot_count,
        "Managed damage slots have no runtime storage"
    );
    let expected_slots = sections
        .iter()
        .filter(|s| {
            s["fields"]["Instant Responses@0x84"]
                .as_array()
                .is_some_and(|rs| {
                    rs.iter()
                        .any(|r| !r["fields"]["Looping Effect@0x48"].is_null())
                })
        })
        .count();
    ensure!(
        slot_count == expected_slots,
        "Source looping sections lost their managed slots"
    );
    let mut effects = BTreeMap::new();
    let mut readback = Vec::new();
    let mut death_stops = BTreeSet::new();
    for s in sections {
        let s = &s["fields"];
        let name = s["Name@0x0"].as_str().context("Source pool name")?;
        let section = *native
            .iter()
            .find(|&&at| health.u32(at + 16).ok() == Some(hash(name)))
            .context("Source health pool absent from emitted definition")?;
        close(
            health.f32(section + 20)?,
            number(&s["Vitality Percentage@0x8"])? * 100.,
        )?;
        let recovery = usize::try_from(health.u32(section + 0xd0)?)?;
        let rd = *recoveries.get(recovery).context("Section recovery")?;
        let ri = *recovery_instances
            .get(recovery)
            .context("Section recovery state")?;
        let initial = if s["Flags@0x4"].as_u64().context("Source section flags")? & 1 == 0 {
            1.
        } else {
            0.
        };
        close(health.f32(rd + 0x18)?, initial)?;
        close(health.f32(ri + 0x60)?, initial)?;
        close(health.f32(fractions[recovery])?, initial)?;
        close(health.f32(ri + 0x58)?, 1.)?;
        ensure!(
            health.u64(ri + 0x50)? == u64::MAX,
            "Fresh health has an active recovery timestamp"
        );
        let transfers = transfer::verify(&health, &native, section, s, sections)?;
        let thresholds = health.array(section + 0x78, 0xc8, Some(0x80804bb9))?;
        let mut by_fraction = BTreeMap::<u32, BTreeSet<(u32, u32)>>::new();
        for at in thresholds {
            let fraction = health.f32(at + 8)?;
            let mut transitions = BTreeSet::new();
            for row in health.array(at + 0xb0, 8, Some(0x80804c2c))? {
                let response = health.pointer(row)?;
                match health.u32(response - 4)? {
                    0x80804c3e => {
                        transitions.insert((health.u32(response + 4)?, health.u32(response + 8)?));
                    }
                    class @ (0x80804c34 | 0x80804c3c) => {
                        let tag = health.u32(response + 0x10)?;
                        if class == 0x80804c3c {
                            let slot = health.u32(response + 0x18)? as usize;
                            ensure!(
                                slot < slot_count,
                                "Damage response escapes its managed slots"
                            );
                            if fraction == 0. && tag == u32::MAX {
                                death_stops.insert(slot);
                            }
                        }
                        if tag != u32::MAX && !effects.contains_key(&tag) {
                            effects.insert(tag, effect(staged, stock, tag, class == 0x80804c3c)?);
                        }
                    }
                    _ => {}
                }
            }
            by_fraction
                .entry(fraction.to_bits())
                .or_default()
                .extend(transitions);
        }
        let mut verified = 0;
        let mut unbound = 0;
        for response in s["Instant Responses@0x84"]
            .as_array()
            .context("Source damage responses")?
        {
            let response = &response["fields"];
            if response["Flags@0x0"].as_u64().context("Response flags")? & 0x10000 != 0 {
                continue;
            }
            let fraction = number(&response["Damage Threshold@0x8"])?;
            let actual = by_fraction
                .get(&fraction.to_bits())
                .context("Source health crossing has no response")?;
            for region in response["Region Transitions@0x58"]
                .as_array()
                .context("Source region transitions")?
            {
                let r = &region["fields"];
                let key = hash(
                    r["Region@0x0"]
                        .as_str()
                        .context("Source transition region")?,
                );
                if r["Runtime Region Index@0x6"] == -1 {
                    ensure!(
                        !actual.iter().any(|&(region, _)| region == key),
                        "Unbound source region acquired a native transition"
                    );
                    unbound += 1;
                    continue;
                }
                let state = *STATES
                    .get(
                        r["New State@0x4"]
                            .as_u64()
                            .context("Source transition state")? as usize,
                    )
                    .context("Source state out of range")?;
                ensure!(
                    actual.contains(&(key, state)),
                    "Source damage crossing lost a region transition"
                );
                verified += 1;
            }
        }
        readback.push(
            json!({"name":name,"capacity":vitality*health.f32(section+20)?/100.,
            "initial_fraction":health.f32(ri+0x60)?,"recovery_ceiling":health.f32(ri+0x58)?,
            "region_transitions":verified,"unbound_region_transitions":unbound,"transfers":transfers}),
        );
    }
    ensure!(
        death_stops.len() == slot_count,
        "Vehicle death leaves a managed damage slot active"
    );
    let conditional_groups = visible_states(staged, runtime, witness)?;
    let geometry = geometry::verify(staged, runtime, witness)?;
    Ok(
        json!({"sections":readback,"hud_health_section":main_name,"health_outputs":outputs.len(),
        "conditional_groups":conditional_groups,"managed_slots":slot_count,
        "effects":effects.into_values().collect::<Vec<_>>(),"geometry":geometry,"gameplay_verified":false}),
    )
}

pub(super) fn melee(
    staged: &mut Reader,
    stock: &mut Reader,
    recipe: &WeaponRecipe,
    witness: &Value,
) -> Result<Value> {
    if witness.is_null() {
        return Ok(Value::Null);
    }
    let amount = number(&witness["fields"]["Damage Lower Bound@0x20"])?;
    let key = hash(&format!("{}.contact-feedback", recipe.namespace));
    let mut profiles = BTreeSet::new();
    let mut banks = Vec::new();
    let mut excluded = Vec::new();
    use sundial::package_authoring::ability_bank::{MELEE_SLOT, slot_banks};
    for &tag in slot_banks(MELEE_SLOT) {
        let bank = staged.tag(tag, Some(0x80809c36))?;
        let stock_bank = stock.tag(tag, Some(0x80809c36))?;
        let old_rows = stock_bank.array(stock_bank.pointer(24)? + 0x88, 56, None)?;
        let stock_attacks = old_rows
            .iter()
            .try_fold(0usize, |count, &row| -> Result<usize> {
                let modifier = stock_bank.pointer(row + 0x30)?;
                Ok(count + usize::from(stock_bank.u32(modifier + 4)? == 0x80804427))
            })?;
        if stock_attacks == 0 {
            ensure!(
                bank.0 == stock_bank.0,
                "An auxiliary bank without an attack handler changed"
            );
            excluded.push(json!({"bank":format!("{tag:08X}"),"reason":"Stock bank has no attack handler","unchanged":true}));
            continue;
        }
        let rows = bank.array(bank.pointer(24)? + 0x88, 56, None)?;
        let mut attacks = 0;
        for row in rows {
            if bank.u32(row + 16)? != key {
                continue;
            }
            let modifier = bank.pointer(row + 0x30)?;
            if bank.u32(modifier + 4)? != 0x80804427 {
                continue;
            }
            for field in [0x18, 0x20] {
                if bank.u64(modifier + field)? == 0 {
                    continue;
                }
                let attack = bank.pointer(modifier + field)?;
                ensure!(
                    bank.u32(attack - 4)? == 0x80804464,
                    "Weapon melee attack has another record type"
                );
                let contact = bank.pointer(attack + 8)?;
                ensure!(
                    bank.u32(contact - 4)? == 0x8080444c,
                    "Weapon melee contact has another type"
                );
                profiles.insert(bank.u32(contact + 0x38)?);
                attacks += 1;
            }
        }
        ensure!(
            attacks > 0,
            "Private weapon melee is absent from a subclass bank"
        );
        ensure!(
            old_rows
                .iter()
                .all(|&r| stock_bank.u32(r + 16).ok() != Some(key)),
            "Private melee key already exists in stock"
        );
        banks.push(json!({"bank":format!("{tag:08X}"),"private_attacks":attacks}));
    }
    ensure!(
        profiles.len() == 1 && !banks.is_empty(),
        "Subclass banks disagree on the private melee contact profile"
    );
    let tag = *profiles.first().context("No private contact profile")?;
    ensure!(
        stock.reference(tag).is_err(),
        "Melee modifies a stock contact profile"
    );
    let p = staged.tag(tag, Some(0x80809c36))?;
    let d = p.pointer(24)?;
    ensure!(
        p.u32(d - 4)? == 0x80804b02 && p.u8(d + 0x10)? == 0,
        "Melee is not an absolute outgoing damage profile"
    );
    close(p.f32(d + 0x14)?, amount)?;
    close(p.f32(d + 0x18)?, amount)?;
    close(p.f32(d + 0x1c)?, 0.)?;
    Ok(
        json!({"key":format!("{key:08X}"),"profile":format!("{tag:08X}"),"contact_amount":amount,
        "banks":banks,"excluded_banks":excluded,"gameplay_verified":false}),
    )
}
