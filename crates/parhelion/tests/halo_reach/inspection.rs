//! Follow the staged item art and measure the decoded mesh against its stock display.
use super::*;

/// A fixed imported optic must remain reachable when a native sight plug changes its index.
pub(super) fn sight_choices(
    staged: &mut Reader,
    stock: &mut Reader,
    item: u32,
    graph: &Value,
) -> Result<()> {
    let Some(sight) = graph.get("optic") else {
        return Ok(());
    };
    let selector = sight["placement"]["selector"]
        .as_u64()
        .context("Imported sight selector")?;
    let donor = u32::try_from(graph["native_item"].as_u64().context("Art donor")?)?;
    let original = parhelion_import::tiger::shadowkeep::extract(stock, donor)?;
    let art = parhelion_import::tiger::shadowkeep::extract(staged, item)?;
    let mut width = 1;
    for row in original["art_rows"]
        .as_array()
        .context("Original art rows")?
    {
        for slot in row["slots"].as_array().context("Original art slots")? {
            if slot["selector"].as_u64() == Some(selector) {
                width = width.max(
                    slot["assignments"]
                        .as_array()
                        .context("Sight choices")?
                        .len(),
                );
            }
        }
    }
    let mut choices = Vec::new();
    let mut optics = std::collections::BTreeSet::new();
    for row in art["art_rows"].as_array().context("Emitted art rows")? {
        let slots = row["slots"]
            .as_array()
            .context("Emitted art slots")?
            .iter()
            .filter(|slot| slot["selector"].as_u64() == Some(selector))
            .collect::<Vec<_>>();
        ensure!(slots.len() == 1, "Imported sight region disappeared");
        let keys = slots[0]["assignments"]
            .as_array()
            .context("Emitted sight choices")?;
        ensure!(
            keys.len() >= width,
            "Imported sight lost native alternatives"
        );
        for (position, key) in keys.iter().enumerate() {
            let key = u32::try_from(key.as_u64().context("Sight assignment")?)?;
            let assignment = format!("{key:08X}");
            let parents = art["parents"]
                .as_array()
                .context("Emitted art parents")?
                .iter()
                .filter(|parent| parent["assignment"].as_str() == Some(assignment.as_str()))
                .collect::<Vec<_>>();
            ensure!(
                parents.len() == 1,
                "Sight alternative {position} has no assigned part"
            );
            let entity_tag =
                u32::from_str_radix(parents[0]["entity"].as_str().context("Sight entity")?, 16)?;
            let entity = staged.tag(entity_tag, Some(0x80809C0F))?;
            let mut owners = Vec::new();
            for component in entity.array(16, 12, Some(0x80809C04))? {
                let tag = entity.u32(component)?;
                let owner = staged.tag(tag, Some(0x80809C36))?;
                let definition = usize::try_from(owner.u64(owner.pointer(16)? + 8)?)?;
                if owner.u32(definition - 4)? == 0x8080393B {
                    ensure!(
                        stock.manager.get_entry(tiger_pkg::TagHash(tag)).is_none(),
                        "Sight alternative still uses a stock optic"
                    );
                    owners.push(tag);
                }
            }
            ensure!(
                owners.len() == 1,
                "Sight alternative {position} has no unique optic"
            );
            optics.insert(owners[0]);
            choices.push(json!({"position":position,"assignment":format!("{key:08X}"),"entity":format!("{entity_tag:08X}"),"optic":format!("{:08X}",owners[0])}));
        }
    }
    ensure!(!choices.is_empty(), "Empty required sight alternatives");
    ensure!(
        optics.len() == 1,
        "Sight alternatives select different optics"
    );
    fs::write(
        staged.output.join(format!("sight-choices-{item:08X}.json")),
        serde_json::to_vec_pretty(
            &json!({"selector":selector,"native_alternatives":width,"choices":choices,
            "limit":"Every native sight index resolves a private optic. Native selection, ADS transitions and final camera placement require gameplay acceptance."}),
        )?,
    )?;
    Ok(())
}

/// Follow the selected art into the native aim providers and check camera clearance.
pub(super) fn sights(staged: &mut Reader, item: u32) -> Result<()> {
    let Some(path) = env::var_os("PARHELION_REACH_ADS_WITNESS") else {
        return Ok(());
    };
    let witness: Value = serde_json::from_slice(&fs::read(path)?)?;
    let required = witness["items"]
        .as_array()
        .context("Required aiming items")?;
    ensure!(!required.is_empty(), "Empty required aiming corpus");
    if !required
        .iter()
        .any(|value| value.as_u64() == Some(u64::from(item)))
    {
        return Ok(());
    }
    let art = parhelion_import::tiger::shadowkeep::extract(staged, item)?;
    let mut entities = std::collections::BTreeSet::new();
    let mut sights = Vec::new();
    for parent in art["parents"].as_array().context("Selected art parents")? {
        let tag = u32::from_str_radix(parent["entity"].as_str().context("Art entity")?, 16)?;
        if tag == u32::MAX || !entities.insert(tag) {
            continue;
        }
        let entity = staged.tag(tag, Some(0x80809c0f))?;
        for component in entity.array(16, 12, Some(0x80809c04))? {
            let owner_tag = entity.u32(component)?;
            let owner = staged.tag(owner_tag, Some(0x80809c36))?;
            let data = usize::try_from(owner.u64(owner.pointer(16)? + 8)?)?;
            if owner.u32(data - 4)? != 0x8080393b {
                continue;
            }
            let mut points = Vec::new();
            for descriptor in [0x198, 0x228] {
                let rows = owner.array(data + descriptor, 64, Some(0x80809c00))?;
                ensure!(rows.len() == 1, "Aiming provider has no unique default");
                let row = rows[0];
                ensure!(
                    owner.u64(row)? == 0 && owner.u64(row + 8)? == 0,
                    "Aim point still uses a donor joint"
                );
                let links = entity
                    .array(32, 72, Some(0x80809bc9))?
                    .into_iter()
                    .filter(|&at| {
                        entity.u32(at + 8).ok() == Some(owner_tag)
                            && entity.u32(at + 12).ok() == Some(0x80809bd7)
                            && entity.u64(at + 16).ok() == Some((data + descriptor - 64) as u64)
                    })
                    .collect::<Vec<_>>();
                ensure!(
                    links.len() == 1,
                    "Aim provider lost its movement connection"
                );
                points.push([
                    owner.f32(row + 32)?,
                    owner.f32(row + 36)?,
                    owner.f32(row + 40)?,
                ]);
            }
            sights.push((owner_tag, points));
        }
    }
    ensure!(sights.len() == 1, "Aimed item has no unique native sight");
    let (owner, points) = &sights[0];
    let rear = points[0];
    let front = points[1];
    ensure!(
        front[0] > rear[0]
            && (front[1] - rear[1]).abs() < 0.0001
            && (front[2] - rear[2]).abs() < 0.0001,
        "Sight does not aim along the source barrel axis"
    );
    let mut vertices = 0;
    for row in art["models"].as_array().context("Selected art geometry")? {
        let tag = u32::from_str_radix(row["model"].as_str().context("Art model")?, 16)?;
        let model = staged.tag(tag, Some(0x808073a5))?;
        for mesh in model.array(16, 136, Some(0x80807378))? {
            if model.array(mesh + 24, 32, Some(0x8080737e))?.is_empty() {
                continue;
            }
            let header = model.u32(mesh)?;
            let positions = staged.tag(staged.reference(header)?, None)?;
            for index in 0..positions.0.len() / 16 {
                let p = point(&model, &positions, index)?;
                ensure!(
                    p[0] > rear[0] && p[2] < rear[2],
                    "The aiming camera or its forward sightline intersects weapon geometry"
                );
                vertices += 1;
            }
        }
    }
    ensure!(vertices > 0, "Empty required aiming geometry");
    fs::write(
        staged.output.join(format!("sights-{item:08X}.json")),
        serde_json::to_vec_pretty(
            &json!({"owner":format!("{owner:08X}"),"rear":rear,"front":front,
            "vertices":vertices,"source_build":witness["source_build"],
            "limits":"Selected native aim providers and geometry clearance. Live aim transitions and perceived eye relief require gameplay acceptance."}),
        )?,
    )?;
    Ok(())
}

/// Follow the item's emitted art and apply an independently captured native selector table.
pub(super) fn details(staged: &mut Reader, item: u32) -> Result<()> {
    let Some(path) = env::var_os("PARHELION_REACH_DRAW_WITNESS") else {
        return Ok(());
    };
    let witness: Value = serde_json::from_slice(&fs::read(path)?)?;
    let details = witness["required_details"]
        .as_array()
        .context("Required native detail inputs")?;
    ensure!(!details.is_empty(), "Empty required detail coverage");
    let art = parhelion_import::tiger::shadowkeep::extract(staged, item)?;
    let mut models = std::collections::BTreeSet::new();
    let mut checked = Vec::new();
    for row in art["models"].as_array().context("Staged art models")? {
        let tag = u32::from_str_radix(row["model"].as_str().context("Model tag")?, 16)?;
        if !models.insert(tag) {
            continue;
        }
        let model = staged.tag(tag, Some(0x808073a5))?;
        let mut draws = 0;
        for mesh in model.array(16, 136, Some(0x80807378))? {
            for draw in model.array(mesh + 24, 32, Some(0x8080737e))? {
                let category = model.u8(draw + 27)?;
                let selection = &witness["categories"][category.to_string()];
                for detail in details {
                    let detail = usize::try_from(detail.as_u64().context("Native detail input")?)?;
                    ensure!(
                        selection[detail].as_u64().is_some_and(|value| value > 0),
                        "Art model {tag:08X} draw {} disappears at native detail {detail}",
                        model.u16(draw + 22)?
                    );
                }
                draws += 1;
            }
        }
        // A marker-only sight carrier legitimately contains no rendered geometry.
        if draws > 0 {
            checked.push(json!({"model":format!("{tag:08X}"),"draws":draws}));
        }
    }
    ensure!(!checked.is_empty(), "Empty required visible art");
    fs::write(
        staged.output.join(format!("art-details-{item:08X}.json")),
        serde_json::to_vec_pretty(&json!({
            "models":checked,"selector_witness":witness,
            "limits":"Emitted art accepted by the captured native detail selector. Live inspection and GPU submission remain unverified."
        }))?,
    )?;
    Ok(())
}

fn point(model: &Payload, positions: &Payload, index: usize) -> Result<[f32; 3]> {
    let mut result = [0.; 3];
    for (axis, value) in result.iter_mut().enumerate() {
        *value = f32::from(positions.i16(index * 16 + axis * 2)?) / 32767. * model.f32(0x6c)?
            + model.f32(0x60 + axis * 4)?;
    }
    Ok(result)
}

pub(super) fn verify(
    staged: &mut Reader,
    stock: &mut Reader,
    recipe: &WeaponRecipe,
    graph: &Value,
    source: &Value,
    display_scale: f32,
) -> Result<Option<Value>> {
    if graph.get("inspection").is_none() {
        return Ok(None);
    }
    let owner = stock.tag(
        u32::try_from(
            graph["inspection"]["source_owner"]
                .as_u64()
                .context("Gear owner")?,
        )?,
        Some(0x80809c36),
    )?;
    let carrier_tag = owner.u32(owner.pointer(24)? + 0x1dc)?;
    let carrier = stock.tag(carrier_tag, Some(0x808073a5))?;
    let center = [carrier.f32(0x80)?, carrier.f32(0x84)?, carrier.f32(0x88)?];
    let radius = carrier.f32(0x8c)? * display_scale;
    ensure!(radius > 0., "Empty native display bounds");
    // The runtime mesh includes broken permutations. Inventory must match the
    // separately exported intact source, not that expanded runtime mesh.
    let source_models = source["models"].as_array().context("Source models")?;
    ensure!(
        source_models.len() == 1 && source_models[0]["parent"].is_null(),
        "Inspection witness requires a single unparented source model"
    );
    let source_positions = source_models[0]["model"]["primitives"]
        .as_array()
        .context("Intact source primitives")?
        .iter()
        .map(|primitive| primitive["vertices"].as_array().context("Source vertices"))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .map(|vertex| serde_json::from_value::<[f32; 3]>(vertex["position"].clone()))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    ensure!(!source_positions.is_empty(), "Empty intact source geometry");
    let source_center = std::array::from_fn::<_, 3, _>(|axis| {
        let lo = source_positions
            .iter()
            .map(|p| p[axis])
            .fold(f32::INFINITY, f32::min);
        let hi = source_positions
            .iter()
            .map(|p| p[axis])
            .fold(f32::NEG_INFINITY, f32::max);
        (lo + hi) * 0.5
    });
    let source_radius = source_positions
        .iter()
        .map(|p| {
            p.iter()
                .zip(source_center)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f32>()
                .sqrt()
        })
        .fold(0f32, f32::max);
    ensure!(source_radius > 0., "Degenerate intact source geometry");
    let item = recipe.identity.item_hash.parse_u32()?;
    let art = parhelion_import::tiger::shadowkeep::extract(staged, item)?;
    let mut models = std::collections::BTreeSet::new();
    let mut readback = Vec::new();
    for row in art["models"].as_array().context("Staged gear models")? {
        let tag = u32::from_str_radix(row["model"].as_str().context("Gear model tag")?, 16)?;
        if !models.insert(tag) {
            continue;
        }
        ensure!(
            stock.manager.get_entry(tiger_pkg::TagHash(tag)).is_none(),
            "Inventory art still selects stock geometry"
        );
        let model = staged.tag(tag, Some(0x808073a5))?;
        let scale = radius / source_radius;
        ensure!(scale.is_finite() && scale > 0., "Invalid inventory scale");
        let owner_tag = u32::from_str_radix(row["owner"].as_str().context("Gear owner")?, 16)?;
        let owner = staged.tag(owner_tag, Some(0x80809c36))?;
        let groups = owner.array(owner.pointer(24)? + 0x38, 24, Some(0x80809c2c))?;
        let expected_group = parhelion_import::tiger::draws::drawn_group(
            &carrier,
            carrier.array(16, 136, Some(0x80807378))?[0],
        )?;
        let mut measured_radius = 0f32;
        let mut vertices = 0;
        for mesh in model.array(16, 136, Some(0x80807378))? {
            for draw in model.array(mesh + 24, 32, Some(0x8080737e))? {
                let group = model.u16(draw + 0x14)?;
                ensure!(
                    usize::from(group) < groups.len() && group == expected_group,
                    "Inventory draw selects a damage group outside its display carrier"
                );
            }
            let header_tag = model.u32(mesh)?;
            let header = staged.tag(header_tag, None)?;
            ensure!(header.u16(4)? == 16, "Unexpected inventory vertex format");
            let positions = staged.tag(staged.reference(header_tag)?, None)?;
            ensure!(
                positions.0.len() / 16 == source_positions.len(),
                "Inventory art differs from intact source vertices"
            );
            for (index, &original) in source_positions
                .iter()
                .enumerate()
                .take(positions.0.len() / 16)
            {
                let actual = point(&model, &positions, index)?;
                for axis in 0..3 {
                    let expected = (original[axis] - source_center[axis]) * scale + center[axis];
                    ensure!(
                        (actual[axis] - expected).abs() < 0.0001,
                        "Inventory art distorts or displaces source geometry"
                    );
                }
                measured_radius = measured_radius.max(
                    actual
                        .iter()
                        .zip(center)
                        .map(|(a, b)| (a - b).powi(2))
                        .sum::<f32>()
                        .sqrt(),
                );
                vertices += 1;
            }
        }
        ensure!(vertices > 0, "Empty required inventory geometry");
        ensure!(
            measured_radius <= radius + 0.0001,
            "Inventory model exceeds the native display envelope"
        );
        ensure!(
            measured_radius >= radius * 0.99,
            "Inventory model does not occupy the requested display size"
        );
        readback.push(json!({"model":format!("{tag:08X}"),"vertices":vertices,
            "scale":scale,"measured_radius":measured_radius}));
    }
    ensure!(
        !readback.is_empty(),
        "Required inventory model was not selected"
    );
    Ok(Some(
        json!({"item":format!("{item:08X}"),"carrier":format!("{carrier_tag:08X}"),
        "native_center":center,"native_radius":radius,"models":readback,"gameplay_verified":false}),
    ))
}
