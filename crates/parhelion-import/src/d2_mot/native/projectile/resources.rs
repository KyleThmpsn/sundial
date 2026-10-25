//! Resource compatibility is selected by native class and named slot identity.
use super::*;
use crate::d2_mot::native::{categories, effects::resources::programs};

pub(super) struct Prepared {
    pub movement: movement::Resources,
    pub namespace: categories::Namespace,
    pub report: Vec<Value>,
}

pub(super) fn prepare(
    source: &mut Reader,
    native: &mut Reader,
    movement: &Payload,
    category: &Payload,
    profile: &Payload,
    assets: &mut Assets,
    component_tags: &BTreeMap<u32, u32>,
) -> Result<Prepared> {
    let (mut pending, hashes) = references(source, movement)?;
    let (dictionary, namespace) = dictionary(source, native, &pending, category)?;
    let mut tags = component_tags.clone();
    let dictionary_tag = assets.reserve(native, "projectile-category-dictionary".into())?;
    assets.push(dictionary_tag, 0x80C70CA1, namespace.payload.clone(), None)?;
    tags.insert(dictionary, dictionary_tag);
    let (impact_table, collision, fallback_slot, sound) = compatibility(native, profile)?;
    let mut report = Vec::new();
    let mut programs_pending = BTreeMap::new();
    while let Some(tag) = pending.pop_first() {
        if tags.contains_key(&tag) {
            continue;
        }
        let class = source.reference(tag)?;
        let payload = source.tag(tag, None)?;
        if matches!(
            class,
            0x808031DE | 0x808031D8 | 0x808031C2 | 0x808031D3 | 0x808031C7
        ) {
            if programs_pending.contains_key(&tag) {
                continue;
            }
            let id = assets.reserve(native, format!("projectile-program-{tag:08X}"))?;
            tags.insert(tag, id);
            programs_pending.insert(tag, (class, (*payload).clone()));
            for at in (0..payload.0.len().saturating_sub(3)).step_by(4) {
                let dependency = payload.u32(at)?;
                if matches!(
                    source.reference(dependency),
                    Ok(0x808031DE | 0x808031D8 | 0x808031C2 | 0x808031D3 | 0x808031C7)
                ) {
                    pending.insert(dependency);
                }
            }
        } else if class == 0x8080873D {
            tags.insert(
                tag,
                named_slot(native, tag, &payload, fallback_slot, &mut report)?,
            );
        } else if class == 0x80809738 {
            tags.insert(tag, sound);
            report.push(
                json!({"source":format!("{tag:08X}"),"native":format!("{sound:08X}"),
                "difference":"Movement audio uses the native compatibility cue."}),
            );
        } else if class == 0x8080873F {
            tags.insert(tag, impact_table);
            report.push(
                json!({"source":format!("{tag:08X}"),"native":format!("{impact_table:08X}"),
                "difference":"Surface impact effects use the native compatibility profile."}),
            );
        } else if payload
            .pointer(16)
            .ok()
            .and_then(|i| payload.u32(i + 4).ok())
            == Some(0x80803F70)
        {
            tags.insert(tag, collision);
            report.push(
                json!({"source":format!("{tag:08X}"),"native":format!("{collision:08X}"),
                "difference":"Collision response uses the native compatibility profile."}),
            );
        }
    }
    for (tag, (class, payload)) in programs_pending {
        let converted = programs::emit(&payload, class, &tags)?;
        ensure!(
            converted.references.iter().all(|row| row.target.is_some()),
            "projectile trajectory resource is unbound"
        );
        let mut templates = native.classes(converted.class);
        templates.sort_unstable();
        let template = *templates
            .first()
            .context("native trajectory resource template")?;
        assets.push(tags[&tag], template, converted.payload, None)?;
    }
    Ok(Prepared {
        movement: movement::Resources {
            tags,
            hashes,
            categories: namespace.source_indices.clone(),
        },
        namespace,
        report,
    })
}

fn named_slot(
    native: &mut Reader,
    tag: u32,
    payload: &Payload,
    fallback: u32,
    report: &mut Vec<Value>,
) -> Result<u32> {
    let name = payload.u32(8)?;
    let mut matches = Vec::new();
    for candidate in native.classes(0x80808BCB) {
        if native.tag(candidate, None)?.u32(8)? == name {
            matches.push(candidate);
        }
    }
    ensure!(
        matches.len() <= 1,
        "movement slot {name:08X} has multiple native counterparts"
    );
    let target = matches.first().copied().unwrap_or(fallback);
    if matches.is_empty() {
        report.push(json!({"source":format!("{tag:08X}"),"name":format!("{name:08X}"),
            "native":format!("{target:08X}"),
            "difference":"This named movement resource has no native counterpart and uses the compatibility movement profile's default resource."}));
    }
    Ok(target)
}

fn references(source: &Reader, payload: &Payload) -> Result<(BTreeSet<u32>, BTreeMap<u64, u32>)> {
    let mut hashes = BTreeMap::new();
    let mut pending = BTreeSet::new();
    for at in (0..payload.0.len().saturating_sub(3)).step_by(4) {
        let tag = payload.u32(at)?;
        if (0x80800001..=0x81FFFFFF).contains(&tag) && source.reference(tag).is_ok() {
            pending.insert(tag);
        }
        if at + 8 <= payload.0.len() {
            let hash = payload.u64(at)?;
            if let Some(entry) = source.manager.lookup.tag64_entries.get(&hash) {
                hashes.insert(hash, entry.hash32.0);
                pending.insert(entry.hash32.0);
            }
        }
    }
    Ok((pending, hashes))
}

fn dictionary(
    source: &mut Reader,
    native: &mut Reader,
    pending: &BTreeSet<u32>,
    category: &Payload,
) -> Result<(u32, categories::Namespace)> {
    // This dictionary is the native engine category namespace. The source counterpart
    // is identified by its validated dictionary format, not a weapon identity.
    let stock_dictionary = native.tag(0x80C70CA1, None)?;
    let mut dictionaries = Vec::new();
    for &tag in pending {
        let payload = source.tag(tag, None)?;
        if let Ok(namespace) =
            categories::emit_static(&payload, &stock_dictionary, &BTreeSet::new())
        {
            dictionaries.push((tag, namespace));
        }
    }
    ensure!(
        dictionaries.len() == 1,
        "movement must name one supported category dictionary"
    );
    let (tag, namespace) = dictionaries.remove(0);
    let mask = category.bytes::<56>(category.pointer(24)? + 0x68)?;
    let mut requested = BTreeSet::new();
    for index in 0..448 {
        if mask[index / 8] & (1 << (index % 8)) != 0 {
            requested.insert(
                *namespace
                    .source_names
                    .get(index)
                    .context("projectile category exceeds source dictionary")?,
            );
        }
    }
    let payload = source.tag(tag, None)?;
    Ok((
        tag,
        categories::emit_static(&payload, &stock_dictionary, &requested)?,
    ))
}

fn compatibility(native: &mut Reader, profile: &Payload) -> Result<(u32, u32, u32, u32)> {
    let mut native_tables = BTreeSet::new();
    for at in (0..profile.0.len().saturating_sub(3)).step_by(4) {
        let tag = profile.u32(at)?;
        if native.reference(tag).ok() == Some(0x80808BCD) {
            native_tables.insert(tag);
        }
    }
    ensure!(
        !native_tables.is_empty(),
        "native projectile profile has no impact resource table"
    );
    let impact_table = *native_tables.first().context("native impact table")?;
    let collision = profile.u32(profile.pointer(24)? + 0x118)?;
    let collision_payload = native.tag(collision, None)?;
    ensure!(
        collision_payload.u32(collision_payload.pointer(16)? + 4)? == 0x80804B02,
        "native projectile collision response class differs"
    );
    let movement = native.tag(MOVEMENT, None)?;
    let fallback_slot = unique_resource(native, &movement, 0x80808BCB)?;
    let sound = unique_resource(native, profile, 0x80809802)?;
    Ok((impact_table, collision, fallback_slot, sound))
}

fn unique_resource(native: &Reader, payload: &Payload, class: u32) -> Result<u32> {
    let mut slots = BTreeSet::new();
    for at in (0..payload.0.len().saturating_sub(3)).step_by(4) {
        let tag = payload.u32(at)?;
        if (0x80800001..=0x81FFFFFF).contains(&tag) && native.reference(tag).ok() == Some(class) {
            slots.insert(tag);
        }
    }
    ensure!(
        slots.len() == 1,
        "native template must name one {class:08X} compatibility resource"
    );
    Ok(*slots.first().context("native compatibility resource")?)
}
