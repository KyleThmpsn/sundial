//! Follow the staged item art and measure the decoded mesh against its stock display.
use super::*;

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
