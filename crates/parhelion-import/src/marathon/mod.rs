//! Marathon static weapon presentations with isolated native Destiny carriers.
//! Native donors retain firing and reload behavior. Source animation is not copied.
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

mod geometry;
mod material;
use crate::tiger::{
    payload::Payload,
    reader::{Reader, outside, write_json},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path};

fn hash(v: &Value, k: &str) -> Result<u32> {
    crate::graph::hash(v, k)
}
use crate::presentation::{Graph, append, put};
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
    let mesh = geometry::read(
        source,
        entry["models"].as_array().context("Marathon models")?,
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
            h[1] = 0;
        }
        let hs = format!("{name}-header");
        let ds = format!("{name}-data");
        g.add(&hs, t, &h, Some(&ds), vec![])?;
        g.add(&ds, native.reference(t)?, &streams[i], Some(&hs), vec![])?;
    }
    material::build(
        source,
        native,
        &mut g,
        mesh.groups.iter().map(|g| g.0).collect(),
    )?;
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
    let result = json!({"item_hash":hash(entry,"target_item")?,"art_key":hash(entry,"art_key")?,"native_item":hash(entry,"native_item")?,"native_assignment":hash(parent,"assignment")?,"source_model":entry["models"][0]["model"],"source_models":entry["models"],"source_owner":owner_tag,"source_entity":entity_tag,"native_model":model_tag,"model_slot":model_slot,"plates_slot":plates_slot,"parent":"parent","companion":"parent-companion","nodes":g.nodes,"native_draw_parts":mesh.groups.len()*2,"vertices":mesh.positions.len(),"triangles":mesh.groups.iter().map(|g|g.1.len()).sum::<usize>(),"appearance":"Marathon geometry and original base-color/normal textures with native Destiny surface shading","animation":"Static source assemblies attached to native weapon root. Native donor gameplay and whole-weapon animation.","installable":false});
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
        obj.push_str(&format!("g {mat:08X}\n"));
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
        let settings = entry
            .get("icon")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        crate::graph::record_converter_revision(&working.join(slug).join("graph"))?;
        crate::artwork::prepare(&working.join(slug).join("graph"), native_packages, settings)?;
        if let Some(recipe) = entry.get("recipe") {
            let mut recipe = recipe.clone();
            apply_recipe_text(&mut recipe)?;
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
