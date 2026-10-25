use super::*;
mod dynamics;

fn markers(p: &Payload, at: usize, class: u32) -> Result<Vec<Vec<u32>>> {
    p.array(at, 64, Some(class))?
        .into_iter()
        .map(|r| {
            [0x30, 0, 4, 0x10, 0x14, 0x18, 0x1C, 0x20, 0x24, 0x28, 0x2C]
                .into_iter()
                .map(|a| p.u32(r + a))
                .collect()
        })
        .collect()
}
fn values(p: &Payload, at: usize, stride: usize, class: u32) -> Result<Vec<u8>> {
    Ok(p.0[p.array_range(at, stride, Some(class))?].to_vec())
}
fn stage_mask(p: &Payload, native: bool) -> Result<u8> {
    let mut mask = 0;
    for (i, a) in [0x28, 0xC8, 0x168, 0x208, 0x2A8].into_iter().enumerate() {
        if !matches!(p.u32(a + if native { 0x20 } else { 0 })?, 0 | u32::MAX) {
            mask |= 1 << i;
        }
    }
    Ok(mask)
}

type Skeleton = BTreeMap<u32, (Option<u32>, Vec<f32>)>;

fn bones(p: &Payload, source: bool) -> Result<Skeleton> {
    let at = p.pointer(24)?;
    let class = p.u32(at - 4)?;
    let (names, poses) = if source {
        ensure!(class == 0x8080049A, "Source skeleton layout");
        (
            p.array(at + 0x98, 16, Some(0x808004F4))?,
            p.array(at + 0xB8, 32, None)?,
        )
    } else {
        ensure!(
            matches!(class, 0x808081D6 | 0x808081DE | 0x8080853E | 0x80808546),
            "Native skeleton layout"
        );
        (
            p.array(at + 0x80, 16, Some(0x80808A08))?,
            p.array(
                at + if matches!(class, 0x808081DE | 0x80808546) {
                    0xA0
                } else {
                    0x90
                },
                32,
                None,
            )?,
        )
    };
    ensure!(names.len() == poses.len(), "Bone transforms incomplete");
    let mut result = BTreeMap::new();
    for (&row, pose) in names.iter().zip(poses) {
        let parent = p.u32(row + 4)?;
        let parent = if parent == u32::MAX {
            None
        } else {
            Some(p.u32(*names.get(parent as usize).context("Bone parent")?)?)
        };
        let transform = (0..8)
            .map(|lane| p.f32(pose + lane * 4))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            result.insert(p.u32(row)?, (parent, transform)).is_none(),
            "Ambiguous bone name"
        );
    }
    Ok(result)
}

fn rigs(source: &Source, reader: &mut Reader, item: u32, graph: &Value) -> Result<Value> {
    if graph["kind"] != "weapon" {
        return Ok(Value::Null);
    }
    let original = &graph["rendering"][0]["source_runtime"];
    let runtime = parhelion_import::tiger::rig::inspect(reader, item, false)?;
    let character = &graph["animation"]["character"];
    if !character.is_null() {
        let lookups = runtime["components"]
            .as_array()
            .context("Runtime components")?
            .iter()
            .filter(|c| c["class"] == "8080344B" && c["entity"] != runtime["runtime_entity"])
            .collect::<Vec<_>>();
        let carrier = &graph["rendering"][0]["native_runtime"];
        let expected = carrier["components"]
            .as_array()
            .context("Native components")?
            .iter()
            .filter(|c| c["class"] == "8080344B" && c["entity"] != carrier["runtime_entity"])
            .count();
        ensure!(
            lookups.len() == expected && expected <= 1,
            "First-person lookup population changed"
        );
        if let Some(lookup) = lookups.first() {
            let lookup = reader.tag(
                u32::from_str_radix(lookup["owner"].as_str().context("Lookup owner")?, 16)?,
                Some(0x80809C36),
            )?;
            let parameters =
                reader.tag(lookup.u32(lookup.pointer(24)? + 0x94)?, Some(0x80808EE1))?;
            let group = parameters.array(8, 32, Some(0x80808EE5))?[parameters.u16(30)? as usize];
            let names = parameters.array(group + 16, 24, Some(0x80808EE8))?;
            let find = |name| -> Result<usize> {
                names
                    .iter()
                    .copied()
                    .find(|&r| parameters.u32(r + 16).ok() == Some(name))
                    .context("First-person profile lookup")
            };
            let requested =
                &graph["animation"]["first_person"]["attachment_profile"]["patch"]["after"];
            let parent = find(number(if requested.is_null() {
                &character["binding"]["parent"]
            } else {
                requested
            })?)?;
            let alias = find(number(&character["private"])?)?;
            ensure!(
                parameters.bytes::<16>(parent)? == parameters.bytes::<16>(alias)?
                    && parameters.bytes::<4>(parent + 20)? == parameters.bytes::<4>(alias + 20)?,
                "Shared attachment profile changes the first-person selector input"
            );
        }
    }
    let mut reports = Vec::new();
    for skeleton in original["skeletons"]
        .as_array()
        .context("Source skeletons")?
    {
        let first_person = skeleton["entity"] != original["runtime_entity"];
        let selected = runtime["skeletons"]
            .as_array()
            .context("Native skeletons")?
            .iter()
            .filter(|s| (s["entity"] != runtime["runtime_entity"]) == first_person)
            .collect::<Vec<_>>();
        ensure!(selected.len() == 1, "Native skeleton role is ambiguous");
        let owner =
            u32::from_str_radix(skeleton["owner"].as_str().context("Source skeleton")?, 16)?;
        let target = u32::from_str_radix(
            selected[0]["owner"].as_str().context("Native skeleton")?,
            16,
        )?;
        let expected = bones(&source.read(owner)?, true)?;
        let actual = bones(reader.tag(target, Some(0x80809C36))?.as_ref(), false)?;
        ensure!(!expected.is_empty(), "Source skeleton empty");
        for (name, (parent, transform)) in &expected {
            let (native_parent, native_transform) =
                actual.get(name).context("Source bone was lost")?;
            ensure!(
                parent == native_parent
                    && transform
                        .iter()
                        .zip(native_transform)
                        .all(|(a, b)| (a - b).abs() <= 1e-5),
                "Source hierarchy or inverse bind transform changed"
            );
        }
        reports.push(json!({"source":owner,"native":target,"first_person":first_person,"source_bones":expected.len(),"native_bones":actual.len()}));
    }
    ensure!(!reports.is_empty(), "No skeletons checked");
    Ok(json!(reports))
}

/// Follow the item's actual art selectors and compare against independently read source parts.
#[allow(clippy::cognitive_complexity)]
pub fn check(
    source: &Source,
    reader: &mut Reader,
    item: u32,
    graph: &Value,
    require_tessellation: bool,
) -> Result<Value> {
    let skeletons = rigs(source, reader, item, graph)?;
    let globals = reader
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|n| n.name == "investment_globals")
        .context("Globals")?
        .hash
        .0;
    let globals = reader.tag(globals, None)?;
    let art = reader.tag(globals.u32(16 + 66 * 16)?, None)?;
    let rows = art.array(8, 32, Some(0x80805DFB))?;
    let definition = reader.tag(item, Some(0x80807BEA))?;
    let selectors = definition.array(definition.pointer(0x88)?, 4, Some(0x808077B5))?;
    let source_rows = graph["gear_art"]["rows"]
        .as_array()
        .context("Source art rows")?;
    ensure!(
        selectors.len() == source_rows.len(),
        "Art selector row count changed"
    );
    // Authoring assigns private keys from the recipe identity. Follow each actual
    // selector position to its source assignment instead of comparing preparation keys.
    let mut selected = BTreeMap::new();
    let mut bind = |from: u32, to: u32| -> Result<()> {
        if matches!(from, 0 | u32::MAX | 0x811C9DC5) {
            ensure!(from == to, "Empty art selector changed");
        } else {
            ensure!(
                !matches!(to, 0 | u32::MAX | 0x811C9DC5),
                "Selected art part vanished"
            );
            if let Some(previous) = selected.insert(from, to) {
                ensure!(
                    previous == to,
                    "Source part has inconsistent private assignments"
                );
            }
        }
        Ok(())
    };
    for (r, source_row) in selectors.into_iter().zip(source_rows) {
        ensure!(
            i64::from(definition.u8(r)? as i8)
                == source_row["class"].as_i64().context("Art class")?
                && u32::from(definition.u8(r + 1)?) == number(&source_row["flags"])?,
            "Art class or flags changed"
        );
        let row = *rows
            .get(usize::from(definition.u16(r + 2)?))
            .context("Art row")?;
        let singles = source_row["singles"]
            .as_array()
            .context("Source direct art")?;
        ensure!(singles.len() == 2, "Source direct art width");
        for (i, from) in singles.iter().enumerate() {
            bind(number(from)?, art.u32(row + 8 + i * 4)?)?;
        }
        let slots = art.array(row + 16, 8, None)?;
        let source_slots = source_row["slots"].as_array().context("Source art slots")?;
        ensure!(slots.len() == source_slots.len(), "Art slot count changed");
        for (slot, source_slot) in slots.into_iter().zip(source_slots) {
            let at = art.pointer(slot)?;
            ensure!(
                Some(art.u64(at)?) == source_slot["selector"].as_u64(),
                "Art slot selector changed"
            );
            let choices = art.array(at + 8, 4, None)?;
            let source_choices = source_slot["assignments"]
                .as_array()
                .context("Source art choices")?;
            ensure!(
                choices.len() == source_choices.len(),
                "Art choice count changed"
            );
            for (choice, from) in choices.into_iter().zip(source_choices) {
                bind(number(from)?, art.u32(choice)?)?;
            }
        }
    }
    let parts = graph["gear_art"]["parts"]
        .as_array()
        .context("Prepared parts")?;
    let expected = parts
        .iter()
        .map(|p| number(&p["source_assignment"]))
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(
        !expected.is_empty()
            && selected.keys().copied().collect::<BTreeSet<_>>() == expected
            && selected.values().copied().collect::<BTreeSet<_>>().len() == expected.len(),
        "Source parts are missing from the item's selectors"
    );
    let assignments = reader.tag(0x80EC3F61, None)?;
    let assignments = assignments
        .array(8, 8, None)?
        .into_iter()
        .map(|r| Ok((assignments.u32(r)?, assignments.u32(r + 4)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let source_maps = source.classes(0x80803AAA);
    ensure!(source_maps.len() == 1, "Source assignment map is ambiguous");
    let source_map = source.read(source_maps[0])?;
    let source_map = source_map
        .array(8, 8, Some(0x80803AA9))?
        .into_iter()
        .map(|r| Ok((source_map.u32(r)?, source_map.u32(r + 4)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let optic = reader.tag(0x81529E1F, Some(0x80809C36))?;
    let optic_class = optic.u32(optic.pointer(24)? - 4)?;
    let mut reports = Vec::new();
    let mut tessellated = 0;
    for part in parts {
        let mut expected_markers = Vec::new();
        let mut actual_markers = Vec::new();
        let mut expected_draws = Vec::new();
        let mut actual_draws = Vec::new();
        let mut expected_optics = Vec::new();
        let mut actual_optics = Vec::new();
        let parent = *source_map
            .get(&number(&part["source_assignment"])?)
            .context("Source assignment")?;
        let parent = source.read(parent)?;
        let entity = source.read(parent.u32(16)?)?;
        let source_entity = entity.clone();
        for row in entity.array(0x20, 12, Some(0x80800715))? {
            let owner = source.read(entity.u32(row)?)?;
            let resource = owner.pointer(24)?;
            match owner.u32(resource - 4)? {
                0x808008FB => {
                    expected_markers.extend(markers(&owner, resource + 0xD8, 0x80800428)?)
                }
                0x808027FC => expected_optics.push((
                    values(&owner, resource + 0xB8, 1, 0x80800009)?,
                    values(&owner, resource + 0xC8, 16, 0x80800090)?,
                )),
                0x80801A9C | 0x80801BF6 => {
                    let model = source.read(owner.u32(resource + 0x1C4)?)?;
                    for mesh in model.array(16, 0xA0, Some(0x80801BBF))? {
                        let index_header = model.u32(mesh + 0x40)?;
                        let indices = source.read(source.reference(index_header)?)?;
                        let digest = hex::encode(Sha256::digest(&indices.0));
                        let draws = model.array(mesh + 0x48, 0x24, Some(0x80801AEF))?;
                        for stage in 0..19 {
                            let range = usize::from(model.u16(mesh + 0x58 + stage * 2)?)
                                ..usize::from(model.u16(mesh + 0x5A + stage * 2)?);
                            for &draw in draws.get(range).context("Source draw stage")? {
                                let material = source.read(model.u32(draw)?)?;
                                let mask = stage_mask(&material, false)?;
                                expected_draws.push((
                                    digest.clone(),
                                    stage,
                                    model.u16(draw + 6)?,
                                    model.u32(draw + 8)?,
                                    model.u32(draw + 12)?,
                                    model.u16(draw + 0x18)?,
                                    model.u8(draw + 0x1E)?,
                                    model.u8(draw + 0x1F)?,
                                    material.u32(8)?,
                                    mask,
                                ));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        let assignment = selected[&number(&part["source_assignment"])?];
        let parent = *assignments.get(&assignment).context("Native assignment")?;
        let parent = reader.tag(parent, Some(0x8080744A))?;
        let entity = reader.tag(parent.u32(16)?, Some(0x80809C0F))?;
        let dynamic = dynamics::check(source, reader, &source_entity, &entity)?;
        for row in entity.array(16, 12, Some(0x80809C04))? {
            let owner = reader.tag(entity.u32(row)?, Some(0x80809C36))?;
            let resource = owner.pointer(24)?;
            let class = owner.u32(resource - 4)?;
            if owner.u32(owner.pointer(16)? - 4)? == 0x80808506 {
                actual_markers.extend(markers(&owner, resource + 0xB0, 0x80808513)?);
            }
            if class == optic_class {
                actual_optics.push((
                    values(&owner, resource + 0xA0, 1, 0x80800009)?,
                    values(&owner, resource + 0xB0, 16, 0x80800090)?,
                ));
            }
            if !matches!(class, 0x808072BD | 0x80807286) {
                continue;
            }
            let model = reader.tag(owner.u32(resource + 0x1DC)?, Some(0x808073A5))?;
            for mesh in model.array(16, 136, Some(0x80807378))? {
                let header = model.u32(mesh + 16)?;
                let index = reader
                    .manager
                    .get_entry(tiger_pkg::TagHash(header))
                    .context("Native index header")?
                    .reference;
                let indices = reader.tag(index, None)?;
                let digest = hex::encode(Sha256::digest(&indices.0));
                let draws = model.array(mesh + 24, 32, Some(0x8080737E))?;
                for stage in 0..19 {
                    let range = usize::from(model.u16(mesh + 0x28 + stage * 2)?)
                        ..usize::from(model.u16(mesh + 0x2A + stage * 2)?);
                    for &draw in draws.get(range).context("Native draw stage")? {
                        let material = reader.tag(model.u32(draw)?, Some(0x808071E8))?;
                        let mask = stage_mask(&material, true)?;
                        if mask & 6 != 0 {
                            tessellated += 1;
                        }
                        for (a, subtype) in
                            [(0x48, 1), (0xE8, 4), (0x188, 5), (0x228, 2), (0x2C8, 0)]
                        {
                            let shader = material.u32(a)?;
                            if matches!(shader, 0 | u32::MAX) {
                                continue;
                            }
                            let header = reader
                                .manager
                                .get_entry(tiger_pkg::TagHash(shader))
                                .context("Native shader header")?;
                            ensure!(
                                (header.file_type, header.file_subtype) == (33, subtype),
                                "Shader is stored in the wrong stage"
                            );
                            let bytes = reader.tag(header.reference, None)?;
                            ensure!(bytes.0.starts_with(b"DXBC"), "Shader program absent");
                        }
                        actual_draws.push((
                            digest.clone(),
                            stage,
                            model.u16(draw + 6)?,
                            model.u32(draw + 8)?,
                            model.u32(draw + 12)?,
                            model.u16(draw + 20)?,
                            model.u8(draw + 26)?,
                            model.u8(draw + 27)?,
                            material.u32(8)?,
                            mask,
                        ));
                    }
                }
            }
        }
        expected_markers.sort();
        actual_markers.sort();
        expected_draws.sort();
        actual_draws.sort();
        expected_optics.sort();
        actual_optics.sort();
        ensure!(
            expected_markers == actual_markers,
            "Source marker transform or binding changed"
        );
        ensure!(
            !expected_draws.is_empty() && expected_draws == actual_draws,
            "Source index stream, draw coverage, material stages or LOD changed"
        );
        ensure!(
            expected_optics == actual_optics,
            "Source zoom procedure or constants changed"
        );
        reports.push(json!({"source_assignment":part["source_assignment"],"assignment":assignment,"draws":actual_draws.len(),"markers":actual_markers,"optics":actual_optics.len(),"dynamics":dynamic}));
    }
    ensure!(
        !require_tessellation || tessellated > 0,
        "Required hull/domain case was not exercised"
    );
    Ok(
        json!({"parts":reports,"skeletons":skeletons,"tessellated_draws":tessellated,"live_equipment_activation":false}),
    )
}
