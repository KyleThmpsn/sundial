//! Marathon weapon presentations with isolated native Destiny carriers.
//! Native carriers retain firing and reload gameplay. First-person animation comes from the
//! source controller, compiled onto private native first-person resources.
pub const FLAVOR_TEXT: &str = "I know who you are. You are DESTINY.";

/// Marathon presentations deliberately have no inherited Destiny lore tab.
pub fn apply_recipe_text(recipe: &mut Value) -> Result<()> {
    ensure!(recipe.is_object(), "Marathon recipe must be an object");
    recipe["flavor"] = json!(FLAVOR_TEXT);
    let overrides = recipe
        .as_object_mut()
        .unwrap()
        .entry("overrides")
        .or_insert_with(|| json!({}));
    let overrides = overrides
        .as_object_mut()
        .context("Marathon recipe overrides must be an object")?;
    overrides.remove("lore");
    overrides.insert("remove_lore".into(), json!(true));
    Ok(())
}

pub mod browse;
pub mod inputs;
pub use inputs::MaterialInputs;
mod animation;
pub use animation::inspect as inspect_animation;
mod gameplay;
mod geometry;
pub use gameplay::{apply_gameplay, author_gameplay};
mod material;
mod optic;
mod refresh;
mod rig;
use crate::tiger::{
    payload::Payload,
    reader::{Reader, outside, write_json},
};
use anyhow::{Context, Result, ensure};
pub use refresh::author_sights;
use serde_json::{Value, json};
use std::{fs, path::Path};

fn hash(v: &Value, k: &str) -> Result<u32> {
    crate::graph::hash(v, k)
}
use crate::presentation::{Graph, append, put};

/// The weapon's bone binding and animation section. Source animation compiles the source
/// controller onto private first-person resources. Otherwise the donor's own first-person
/// animation plays and source bones follow its equipment bones.
fn animate(
    source: &mut Reader,
    native: &mut Reader,
    parts: &[Value],
    item_tag: u32,
    root: &Path,
    source_animation: bool,
) -> Result<(rig::Binding, Value)> {
    if source_animation {
        let compiled = animation::compile(source, native, parts, item_tag, root)
            .context("Source first-person controller")?;
        let rig = rig::Binding::owned(source, parts, &compiled.palette)?;
        return Ok((rig, compiled.section));
    }
    let bones = animation::equipment_bones(native, item_tag)?;
    let rig = rig::Binding::donor(source, parts, &bones)?;
    Ok((
        rig,
        json!({"first_person_status":"native","controller":"donor",
            "reason":"Marathon animations are off, so the donor's own first-person animation plays.","gameplay_verified":false}),
    ))
}

#[expect(
    clippy::cognitive_complexity,
    reason = "Preserve the audited converter while integrating the legacy rendering pipeline."
)]
fn build(source: &mut Reader, native: &mut Reader, entry: &Value, root: &Path) -> Result<Value> {
    let template: Value = serde_json::from_slice(&fs::read(
        Path::new(entry["native_template"].as_str().context("template path")?)
            .join("template-report.json"),
    )?)?;
    let mut selected = None;
    for model in template["models"].as_array().context("native models")? {
        let mt = hash(model, "model")?;
        let h = native.tag(mt, Some(0x808073a5))?;
        for m in h.array(16, 136, Some(0x80807378))? {
            let p = native.tag(h.u32(m)?, None)?;
            let a = native.tag(h.u32(m + 4)?, None)?;
            if p.u16(4)? != 8 || a.u16(4)? != 24 || h.u16(m + 42)? == 0 {
                continue;
            }
            let score = p.u32(0)?;
            if selected.as_ref().is_none_or(
                |(old, _, _, _): &(u32, Value, std::sync::Arc<Payload>, usize)| score > *old,
            ) {
                selected = Some((score, model.clone(), h.clone(), m));
            }
        }
    }
    let (_, model, header, mi) = selected.context("no native body carrier")?;
    let owner_tag = hash(&model, "owner")?;
    let entity_tag = hash(&model, "entity")?;
    let model_tag = hash(&model, "model")?;
    let parent = template["parents"]
        .as_array()
        .context("native parents")?
        .iter()
        .find(|p| {
            hex::decode(p["parent_bytes"].as_str().unwrap_or(""))
                .ok()
                .and_then(|b| {
                    b.get(16..20)
                        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
                })
                == Some(entity_tag)
        })
        .context("body parent")?;
    let parent_tag = hash(parent, "parent")?;
    let parts = entry
        .get("parts")
        .unwrap_or(&entry["models"])
        .as_array()
        .context("Marathon assembly parts")?;
    fs::create_dir_all(root)?;
    let (rig, animation) = animate(
        source,
        native,
        parts,
        hash(&template, "item_tag")?,
        root,
        entry["source_animation"] == true,
    )?;
    let mesh = geometry::read(
        source,
        entry["models"].as_array().context("Marathon models")?,
        &rig,
    )?;
    let geometry::Encoded {
        header: model_bytes,
        streams,
        patches,
    } = geometry::encode(&mesh, &header, mi)?;
    fs::create_dir_all(root)?;
    let mut g = Graph {
        root: root.to_path_buf(),
        nodes: vec![],
    };
    for (i, name) in ["positions", "attributes", "indices"].iter().enumerate() {
        let t = header.u32(mi + [0, 4, 16][i])?;
        let mut h = native.tag(t, None)?.0.clone();
        put(
            &mut h,
            if i == 2 { 8 } else { 0 },
            &(streams[i].len() as u32).to_le_bytes(),
        )?;
        if i == 2 {
            h[1] = 1;
        } else if i == 0 {
            put(&mut h, 4, &16u16.to_le_bytes())?;
        }
        let hs = format!("{name}-header");
        let ds = format!("{name}-data");
        g.add(&hs, t, &h, Some(&ds), vec![])?;
        g.add(&ds, native.reference(t)?, &streams[i], Some(&hs), vec![])?;
    }
    g.add("model", model_tag, &model_bytes, None, patches)?;
    let original = native.tag(owner_tag, Some(0x80809c36))?;
    let resource = original.pointer(24)?;
    ensure!(
        original.u32(resource - 4)? == 0x808072bd,
        "unsupported native model component"
    );
    let model_slot = resource + 0x1dc;
    let plates_slot = resource + 0x248;
    let plate_tag = original.u32(plates_slot)?;
    let plates = native.tag(plate_tag, None)?;
    g.add("plates", plate_tag, &plates.0, None, vec![])?;
    let mut owner = original.0.clone();
    let mut op = vec![
        json!({"offset":model_slot,"symbol":"model"}),
        json!({"offset":plates_slot,"symbol":"plates"}),
    ];
    for o in (0..owner.len() - 3).step_by(4) {
        if original.u32(o)? == owner_tag {
            op.push(json!({"offset":o,"symbol":"owner"}));
        }
    }
    for p in &op {
        put(
            &mut owner,
            p["offset"].as_u64().unwrap() as usize,
            &u32::MAX.to_le_bytes(),
        )?;
    }
    g.add("owner", owner_tag, &owner, None, op)?;
    let old_entity = native.tag(entity_tag, Some(0x80809c0f))?;
    let mut entity = old_entity.0.clone();
    let mut ep = vec![];
    for o in crate::tiger::entity::owner_slots(&old_entity, &original, owner_tag)? {
        put(&mut entity, o, &u32::MAX.to_le_bytes())?;
        ep.push(json!({"offset":o,"symbol":"owner"}));
    }
    crate::tiger::entity::reject_stale_owner(&Payload(entity.clone()), owner_tag)?;
    g.add("entity", entity_tag, &entity, None, ep)?;
    let materials = material::build(source, native, &mut g, &mesh)?;
    let mut parent_bytes = native.tag(parent_tag, None)?.0.clone();
    put(&mut parent_bytes, 16, &u32::MAX.to_le_bytes())?;
    g.add(
        "parent",
        parent_tag,
        &parent_bytes,
        None,
        vec![json!({"offset":16,"symbol":"entity"})],
    )?;
    let companion = native.tag(0x81a662de, None)?;
    g.add("parent-companion", 0x81a662de, &companion.0, None, vec![])?;
    let n = g.nodes.last_mut().unwrap();
    n["shared_owner"] = json!("parent");
    n["source_parent"] = json!(parent_tag);
    let parts = entry
        .get("parts")
        .unwrap_or(&entry["models"])
        .as_array()
        .context("Marathon assembly parts")?;
    let (sight, source_optics) = optic::author(
        source,
        native,
        &template,
        &mut g,
        parts,
        hash(entry, "target_item")?,
    )?;
    let result = json!({"item_hash":hash(entry,"target_item")?,"art_key":hash(entry,"art_key")?,"native_item":hash(entry,"native_item")?,"native_assignment":hash(parent,"assignment")?,"source_model":entry["models"][0]["model"],"source_models":entry["models"],"source_owner":owner_tag,"source_entity":entity_tag,"native_model":model_tag,"model_slot":model_slot,"plates_slot":plates_slot,"parent":"parent","companion":"parent-companion","nodes":g.nodes,"native_draw_parts":mesh.groups.len(),"vertices":mesh.positions.len(),"triangles":mesh.groups.iter().filter(|(key,_)|matches!(mesh.surfaces[key].stage,0|2|8|15)).map(|g|g.1.len()).sum::<usize>(),"appearance":"Translated Marathon pixel programs, authored material inputs and native render passes","materials":materials,"omitted_passes":mesh.omitted,"animation":"Static source assemblies attached to native weapon root. Native donor gameplay and whole-weapon animation.","installable":false});
    let mut result = result;
    result["kept_parts"] = sight;
    result["source_optics"] = source_optics;
    result["source_assembly"] = json!(parts);
    result["source"] = json!("marathon");
    result["source_motion"] = rig.report;
    result["live_displays"] = json!(true);
    result["reticle_drivers"] = json!(true);
    result["optics_revision"] = json!(1);
    result["animation"] = animation;
    result["material_inputs"] = json!(MaterialInputs::default());
    let item = native.tag(hash(&template, "item_tag")?, Some(0x80807bea))?;
    let slot =
        match item.u8(sundial::package_authoring::investment_schema::ITEM_INVENTORY_SLOT_OFFSET)? {
            0 => "kinetic",
            1 => "energy",
            2 => "power",
            _ => anyhow::bail!("Native weapon has an unsupported inventory slot"),
        };
    result["inventory_slot"] = match entry["recipe"]["overrides"].get("inventory_slot") {
        Some(Value::String(slot)) if matches!(slot.as_str(), "kinetic" | "energy" | "power") => {
            json!(slot)
        }
        None | Some(Value::Null) => json!(slot),
        _ => anyhow::bail!("Imported weapon has an unsupported inventory slot"),
    };
    write_json(&root.join("asset-graph.json"), &result)?;
    // Portable review mesh produced from exactly the geometry written to the graph.
    let mut obj = String::new();
    for p in &mesh.positions {
        obj.push_str(&format!("v {} {} {}\n", p[0], p[1], p[2]));
    }
    for a in &mesh.attributes {
        obj.push_str(&format!("vt {} {}\n", a[0], a[1]));
        obj.push_str(&format!("vn {} {} {}\n", a[2], a[3], a[4]));
    }
    for (mat, faces) in &mesh.groups {
        let surface = &mesh.surfaces[mat];
        obj.push_str(&format!(
            "g {mat:08X}-{:08X}-stage-{}\n",
            surface.material, surface.stage
        ));
        for f in faces {
            obj.push_str(&format!(
                "f {0}/{0}/{0} {1}/{1}/{1} {2}/{2}/{2}\n",
                f[0] as u32 + 1,
                f[1] as u32 + 1,
                f[2] as u32 + 1
            ));
        }
    }
    fs::write(root.join("assembled.obj"), obj)?;
    Ok(result)
}

pub fn prepare(plan: &Path, packages: &Path, native_packages: &Path, out: &Path) -> Result<Value> {
    let out = outside(
        &outside(out, packages.parent().context("Marathon package parent")?)?,
        native_packages.parent().context("native package parent")?,
    )?;
    ensure!(!out.exists(), "Marathon output already exists");
    let plan: Value = serde_json::from_slice(&fs::read(plan)?)?;
    fs::create_dir_all(out.parent().context("Marathon output parent")?)?;
    let temporary = tempfile::Builder::new()
        .prefix("marathon-native-")
        .tempdir_in(out.parent().unwrap())?;
    let working = temporary.path();
    let mut source = Reader::marathon(packages, &working.join("source"))?;
    let mut native = Reader::new(native_packages, &working.join("native"), false)?;
    let mut reports = vec![];
    let entries = plan
        .as_array()
        .filter(|entries| !entries.is_empty())
        .context("Marathon plan is empty")?;
    let mut slugs = std::collections::BTreeSet::new();
    for entry in entries {
        let slug = entry["slug"].as_str().context("output slug")?;
        ensure!(
            !slug.is_empty()
                && slug.chars().all(|c| c.is_ascii_lowercase() || c == '-')
                && slugs.insert(slug),
            "invalid output slug"
        );
        let report = build(
            &mut source,
            &mut native,
            entry,
            &working.join(slug).join("graph"),
        )
        .with_context(|| format!("building {slug}"))?;
        let mut report = report;
        if let Some(identity) = entry["source_item_identity"].as_u64() {
            author_gameplay(
                &mut source,
                &mut native,
                &mut report,
                u32::try_from(identity)?,
            )?;
            write_json(&working.join(slug).join("graph/asset-graph.json"), &report)?;
        }
        let settings = entry
            .get("icon")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_else(|| crate::artwork::Settings::for_graph(&report, &entry["recipe"]));
        crate::graph::record_converter_revision(&working.join(slug).join("graph"))?;
        crate::artwork::prepare(&working.join(slug).join("graph"), native_packages, settings)?;
        if let Some(recipe) = entry.get("recipe") {
            let mut recipe = recipe.clone();
            apply_recipe_text(&mut recipe)?;
            apply_gameplay(&mut recipe, &report)?;
            let directory = working.join(slug).join("graph");
            let item = hash(&recipe["identity"], "item_hash")?;
            let mut reference = crate::GraphReference::new(&directory, item)?;
            crate::artwork::apply(&mut recipe, &directory)?;
            reference.directory = out.join(slug).join("graph");
            recipe["overrides"]["imported_graph"] = serde_json::to_value(reference)?;
            write_json(&working.join(slug).join("weapon.parhelion.json"), &recipe)?;
        }
        eprintln!(
            "{slug}: {} vertices, {} triangles",
            report["vertices"], report["triangles"]
        );
        reports.push(report);
    }
    source.finish()?;
    native.finish()?;
    write_json(&working.join("report.json"), &json!(reports))?;
    ensure!(!out.exists(), "Marathon output appeared concurrently");
    fs::rename(working, &out)?;
    Ok(json!({"output":out,"weapons":reports.len()}))
}
