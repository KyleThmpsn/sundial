//! Read wheel controls, effect selections and loading scopes from emitted packages.
use super::*;
use std::collections::BTreeSet;

fn selected(tag: u32) -> bool {
    ![0, u32::MAX, 0x811c9dc5].contains(&tag)
}

// Independent dense/sparse readback, without the dependency-index writer or
// the production prerequisite collector deciding which entries should exist.
fn dependencies(payload: &Payload) -> Result<BTreeSet<u32>> {
    let mut entries = BTreeSet::new();
    for row in payload.array(0x10, 40, Some(0x80809efb))? {
        let package = u16::try_from(payload.u64(row)?)?;
        for (word, at) in payload
            .array(row + 8, 4, Some(0x8080000b))?
            .into_iter()
            .enumerate()
        {
            for bit in 0..32 {
                if payload.u32(at)? & (1 << bit) != 0 {
                    entries.insert(tiger_pkg::TagHash::new(package, (word * 32 + bit) as u16).0);
                }
            }
        }
        for at in payload.array(row + 24, 2, Some(0x8080000a))? {
            entries.insert(tiger_pkg::TagHash::new(package, payload.u16(at)?).0);
        }
    }
    ensure!(!entries.is_empty(), "Empty required loading index");
    Ok(entries)
}

fn scope(staged: &mut Reader, stock: &Reader, owner: u32) -> Result<(u32, BTreeSet<u32>)> {
    let mut matches = Vec::new();
    for tag in staged.classes(0x80809ef9) {
        if stock.manager.get_entry(tiger_pkg::TagHash(tag)).is_some() {
            continue;
        }
        let payload = staged.tag(tag, Some(0x80809ef9))?;
        if payload.u32(12)? == owner {
            ensure!(payload.u32(8)? == tag, "Loading companion identity differs");
            matches.push((tag, dependencies(&payload)?));
        }
    }
    ensure!(
        matches.len() == 1,
        "Missing or ambiguous vehicle loading scope"
    );
    Ok(matches.remove(0))
}

fn effects(staged: &mut Reader, stock: &mut Reader, table: u32) -> Result<BTreeSet<u32>> {
    let payload = staged.tag(table, Some(0x80808bcd))?;
    ensure!(
        payload.0 == stock.tag(table, Some(0x80808bcd))?.0,
        "Wheel contact changed its stock impact table"
    );
    let mut targets = BTreeSet::new();
    for descriptor in [0x80, 0x98, 0xb0] {
        for event in payload.array(descriptor, 24, Some(0x80808bd5))? {
            for material in payload.array(event + 8, 32, Some(0x80808bd7))? {
                for at in payload.array(material + 8, 4, Some(0x80800014))? {
                    let tag = payload.u32(at)?;
                    if selected(tag) {
                        targets.insert(tag);
                    }
                }
            }
        }
    }
    let mut required = BTreeSet::from([table]);
    for target in targets {
        let effect = staged.tag(target, None)?;
        ensure!(
            effect.0 == stock.tag(target, None)?.0,
            "Wheel contact changed a retained effect"
        );
        required.insert(target);
        if staged.reference(target)? == 0x80809c0f {
            let replication = effect.u32(0x88)?;
            if selected(replication) {
                required.insert(replication);
            }
            for row in effect.array(0x10, 12, Some(0x80809c04))? {
                let owner = effect.u32(row)?;
                staged.tag(owner, Some(0x80809c36))?;
                required.insert(owner);
            }
        }
    }
    Ok(required)
}

pub(super) fn verify(
    staged: &mut Reader,
    stock: &mut Reader,
    recipe: &WeaponRecipe,
    graph: &Value,
    runtime: &Value,
    directory: &Path,
    scale: f32,
) -> Result<()> {
    if recipe.kind != parhelion::ItemKind::Sparrow {
        return Ok(());
    }
    let source: Value = serde_json::from_slice(&fs::read(
        directory
            .parent()
            .context("Vehicle import directory")?
            .join("source/source.json"),
    )?)?;
    let contacts = &source["object"]["gameplay"]["contacts"];
    let source_wheels = contacts["wheels"].as_array().context("Source wheels")?;
    let maximum_mass = source_wheels.iter().try_fold(0f64, |maximum, wheel| {
        let mass = wheel["mass_source"].as_f64().context("Source wheel mass")?;
        ensure!(mass.is_finite() && mass >= 0., "Invalid source wheel mass");
        Ok::<_, anyhow::Error>(maximum.max(mass))
    })?;
    let donor_ratios = if source_wheels.is_empty() {
        Vec::new()
    } else {
        ensure!(maximum_mass > 0., "Source wheel masses are all zero");
        let donor = stock.tag(0x80bfc32a, Some(0x80809c36))?;
        let definition = donor.pointer(24)?;
        let rows = donor.array(definition + 0x130, 0x88, Some(0x80803d4f))?;
        let settings = donor.pointer(*rows.first().context("Donor wheel")? + 0x78)?;
        [0x20, 0x28, 0x34, 0x3c]
            .into_iter()
            .map(|field| donor.f32(settings + field).map(f64::from))
            .collect::<Result<Vec<_>>>()?
    };
    ensure!(scale.is_finite() && scale > 0., "Invalid vehicle scale");
    let entity_tag = u32::from_str_radix(
        runtime["runtime_entity"]
            .as_str()
            .context("Vehicle entity")?,
        16,
    )?;
    let entity = staged.tag(entity_tag, Some(0x80809c0f))?;
    let replication = entity.u32(0x88)?;
    ensure!(
        staged.tag(replication, Some(0x80809bb6))?.u32(8)? == entity_tag,
        "Vehicle replication does not own its entity"
    );
    let (companion, private) = scope(staged, stock, replication)?;
    let investment = dependencies(staged.tag(0x80ee8cbd, Some(0x80809ef9))?.as_ref())?;
    let mut wheels = Vec::new();
    let mut tables = BTreeSet::new();
    let mut required = BTreeSet::new();
    let mut slopes = Vec::new();
    for row in entity.array(0x10, 12, Some(0x80809c04))? {
        let tag = entity.u32(row)?;
        let owner = staged.tag(tag, Some(0x80809c36))?;
        if owner.u64(24)? == 0 {
            continue;
        }
        let definition = owner.pointer(24)?;
        if owner.u32(definition - 4)? != 0x80803d65 {
            continue;
        }
        if !source_wheels.is_empty() {
            for (field, name) in [
                (0xb4, "maximum_slope_cosine_0"),
                (0xb8, "maximum_slope_cosine_1"),
            ] {
                let expected = contacts[name].as_f64().context("Source slope cosine")?;
                let actual = f64::from(owner.f32(definition + field)?);
                ensure!(
                    (actual - expected).abs() < 1e-6,
                    "Emitted wheel slope differs from source {name}"
                );
                slopes.push(json!({"owner":format!("{tag:08X}"),"field":field,
                    "source":expected,"cosine":actual}));
            }
        }
        for wheel in owner.array(definition + 0x130, 0x88, Some(0x80803d4f))? {
            ensure!(owner.u64(wheel + 0x78)? != 0, "Wheel has no settings");
            let settings = owner.pointer(wheel + 0x78)?;
            ensure!(
                owner.u32(settings - 4)? == 0x80803d52,
                "Wheel settings class"
            );
            let source_wheel = source_wheels
                .get(wheels.len())
                .context("Emitted wheel has no source contact")?;
            let mut slip = Vec::new();
            for (field, name) in [
                (0x2c, "friction_velocity_source"),
                (0x40, "brake_friction_velocity_source"),
            ] {
                let source_speed = source_wheel[name].as_f64().context("Source slip speed")?;
                // Source length units are ten feet, independently of the writer.
                let expected = source_speed * 3.048 * f64::from(scale);
                let actual = f64::from(owner.f32(settings + field)?);
                ensure!(
                    actual.is_finite()
                        && actual >= 0.
                        && (actual - expected).abs() <= 1e-5 * expected.abs().max(1.),
                    "Emitted wheel slip threshold differs from source {name}"
                );
                slip.push(json!({"field":field,"source_speed":source_speed,
                    "metres_per_second":actual,"expected_metres_per_second":expected}));
            }
            let table = owner.u32(settings + 0x50)?;
            let mass = source_wheel["mass_source"]
                .as_f64()
                .context("Source wheel mass")?;
            let mut lateral = Vec::new();
            for ((rolling, ratio, multiplier), donor_ratio) in [
                (0x1c, 0x20, 0xa8),
                (0x24, 0x28, 0xac),
                (0x30, 0x34, 0xa8),
                (0x38, 0x3c, 0xac),
            ]
            .into_iter()
            .zip(&donor_ratios)
            {
                let actual = f64::from(owner.f32(settings + ratio)?);
                ensure!(
                    actual.is_finite() && actual >= 0.,
                    "Invalid lateral tire ratio"
                );
                ensure!(
                    (actual * maximum_mass - donor_ratio * mass).abs()
                        <= 1e-5 * (donor_ratio * maximum_mass).abs().max(1.),
                    "Emitted lateral tire response loses source wheel-mass weighting"
                );
                let coefficient = actual
                    * f64::from(owner.f32(settings + rolling)?)
                    * f64::from(owner.f32(definition + multiplier)?);
                lateral.push(json!({"ratio_field":ratio,"ratio":actual,"coefficient":coefficient}));
            }
            wheels.push(json!({"owner":format!("{tag:08X}"),"wheel":wheel,
                "settings":settings,"slip":slip,"source_mass":mass,"lateral":lateral,
                "impact_table":format!("{table:08X}")}));
            if selected(table) {
                tables.insert(table);
            }
        }
    }
    let expected = graph["vehicle"]["contacts"]["wheels"]
        .as_array()
        .map_or(0, Vec::len);
    ensure!(
        wheels.len() == expected && wheels.len() == source_wheels.len(),
        "Emitted wheels differ from the portable input or source scene"
    );
    for table in &tables {
        required.extend(effects(staged, stock, *table)?);
    }
    let private_missing = required.difference(&private).copied().collect::<Vec<_>>();
    let investment_missing = required
        .difference(&investment)
        .copied()
        .collect::<Vec<_>>();
    fs::write(
        staged.output.join(format!(
            "wheel-loading-{:08X}.json",
            recipe.identity.item_hash.parse_u32()?
        )),
        serde_json::to_vec_pretty(&json!({"entity":format!("{entity_tag:08X}"),
            "companion":format!("{companion:08X}"),"wheels":wheels,"tables":tables,
            "slopes":slopes,"source_scale":scale,
            "required":required,"private_missing":private_missing,
            "investment_missing":investment_missing,"nonwheeled":expected == 0,
            "gameplay_verified":false}))?,
    )?;
    ensure!(
        private_missing.is_empty(),
        "Vehicle scope omits wheel effects: {private_missing:X?}"
    );
    ensure!(
        investment_missing.is_empty(),
        "Investment scope omits wheel effects: {investment_missing:X?}"
    );
    Ok(())
}
