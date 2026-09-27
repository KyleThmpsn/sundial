//! Explicit source-owned skeletons, independently validated against a native format carrier.
use super::*;

/// Geometry keeps its own bone indices only when both FK payloads convert successfully.
pub(super) fn mapping(config: &Value, source_item: &Value, native_item: &Value) -> Result<Value> {
    let source_path = Path::new(config["source"].as_str().context("source rig path")?);
    let native_path = Path::new(config["native"].as_str().context("native rig path")?);
    let source = load(&source_path.join("rig.json"))?;
    let native = load(&native_path.join("rig.json"))?;
    ensure!(
        &source["item_tag"] == source_item && &native["item_tag"] == native_item,
        "source-owned rig identity differs"
    );
    let from = selected(&source, &config["source_owner"])?;
    let to = selected(&native, &config["native_owner"])?;
    ensure!(
        from["bones"][0]["name_hash"] == to["bones"][0]["name_hash"],
        "weapon root differs"
    );
    let source_owner = config["source_owner"].as_str().context("source owner")?;
    let native_owner = config["native_owner"].as_str().context("native owner")?;
    skeleton(
        &raw(source_path, source_owner)?.0,
        &raw(native_path, native_owner)?.0,
        u32::from_str_radix(native_owner, 16)?,
    )?;
    let bones = from["bones"].as_array().context("source bones")?;
    let used = used_bones(source_path, source_item)?;
    ensure!(
        used.iter().all(|i| *i < bones.len()),
        "geometry exceeds source skeleton"
    );
    Ok(
        json!({"source_owned":true,"bone_map":(0..bones.len()).collect::<Vec<_>>(),
        "required_source_bones":used,"native_bone_count":bones.len(),
        "source_bones":bones,"native_bones":bones,"source_owner":source_owner,
        "native_owner":native_owner,"native_animation_donor":native_item,"gameplay_verified":false}),
    )
}

fn selected<'a>(rig: &'a Value, owner: &Value) -> Result<&'a Value> {
    let matches = rig["skeletons"]
        .as_array()
        .context("rig skeletons")?
        .iter()
        .filter(|s| &s["owner"] == owner)
        .collect::<Vec<_>>();
    ensure!(matches.len() == 1, "skeleton owner is missing or ambiguous");
    Ok(matches[0])
}

/// Require the shared arm prefix and weapon attachment boundary to agree. Weapon bones
/// beyond that boundary deliberately keep their source names and parent relationships.
pub(crate) fn validate(source: &Value, native: &Value) -> Result<Vec<(u32, u32, bool)>> {
    fn pair(rig: &Value, fp: bool) -> Result<(&Value, u32)> {
        let rows = rig["components"]
            .as_array()
            .context("rig components")?
            .iter()
            .filter(|c| (c["entity"] != rig["runtime_entity"]) == fp)
            .filter(|c| ["808081DE", "80808546"].contains(&c["class"].as_str().unwrap_or_default()))
            .collect::<Vec<_>>();
        ensure!(
            rows.len() == 1,
            "source-owned skeleton component is ambiguous"
        );
        Ok((
            selected(rig, &rows[0]["owner"])?,
            u32::from_str_radix(rows[0]["owner"].as_str().context("owner")?, 16)?,
        ))
    }
    let (sw, sw_tag) = pair(source, false)?;
    let (nw, nw_tag) = pair(native, false)?;
    let (sf, sf_tag) = pair(source, true)?;
    let (nf, nf_tag) = pair(native, true)?;
    let bones =
        |s: &Value| -> Result<Vec<Value>> { Ok(s["bones"].as_array().context("bones")?.clone()) };
    let (sw, nw, sf, nf) = (bones(sw)?, bones(nw)?, bones(sf)?, bones(nf)?);
    let source_start = sf
        .len()
        .checked_sub(sw.len())
        .context("source weapon section")?;
    let native_start = nf
        .len()
        .checked_sub(nw.len())
        .context("native weapon section")?;
    ensure!(
        source_start > 0 && source_start == native_start,
        "shared arm boundary differs"
    );
    for (s, n) in sf[..source_start].iter().zip(&nf[..native_start]) {
        ensure!(
            s["name_hash"] == n["name_hash"] && s["parent"] == n["parent"],
            "shared arm hierarchy differs"
        );
    }
    ensure!(
        sf[source_start]["parent"] == nf[native_start]["parent"],
        "weapon attachment parent differs"
    );
    for (weapon, full, start) in [(&sw, &sf, source_start), (&nw, &nf, native_start)] {
        for (i, bone) in weapon.iter().enumerate() {
            ensure!(
                bone["name_hash"] == full[start + i]["name_hash"],
                "weapon section names differ"
            );
            if i > 0 {
                ensure!(
                    bone["parent"].as_i64().context("weapon parent")? + start as i64
                        == full[start + i]["parent"].as_i64().context("full parent")?,
                    "weapon section hierarchy differs"
                );
            }
        }
    }
    Ok(vec![(sw_tag, nw_tag, false), (sf_tag, nf_tag, true)])
}

/// Convert both private owners and retain their native self tag until package allocation.
pub(crate) fn prepare(
    source: &Value,
    native: &Value,
    sr: &mut crate::d2_mot::reader::Reader,
    nr: &mut crate::d2_mot::reader::Reader,
    graph: &Path,
) -> Result<Value> {
    let mut result = Vec::new();
    for (from, to, first_person) in validate(source, native)? {
        let modern = sr.tag(from, Some(0x80809B06))?;
        let original = nr.tag(to, Some(0x80809C36))?;
        let (mut converted, report) = skeleton(&modern.0, &original.0, to)?;
        validate_native_instance(&converted.0)?;
        for offset in report["owner_patches"]
            .as_array()
            .context("FK self patches")?
        {
            let offset = usize::try_from(offset.as_u64().context("FK patch")?)?;
            converted.0[offset..offset + 4].copy_from_slice(&to.to_le_bytes());
        }
        let file = format!("animation/rig-{from:08X}.bin");
        let template_file = format!("animation/rig-{from:08X}-template.bin");
        fs::write(graph.join(&file), &converted.0)?;
        fs::write(graph.join(&template_file), &original.0)?;
        result.push(
            json!({"source":from,"native":to,"first_person":first_person,
            "file":file,"template_file":template_file,"conversion":report}),
        );
    }
    Ok(json!(result))
}
