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
pub use optic::restore_source_eye;
mod refresh;
mod rig;
mod tier;
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

/// A camo's bone binding. A camo is worn on its weapon, so it skins to the palette the weapon's
/// own geometry uses and plays the weapon's first-person animation instead of carrying one.
fn wear(
    source: &mut Reader,
    native: &mut Reader,
    parts: &[Value],
    item_tag: u32,
    source_animation: bool,
) -> Result<(rig::Binding, Value)> {
    let rig = if source_animation {
        let palette = animation::palette(source, native, parts, item_tag)?;
        rig::Binding::owned(source, parts, &palette)?
    } else {
        let bones = animation::equipment_bones(native, item_tag)?;
        rig::Binding::donor(source, parts, &bones)?
    };
    Ok((
        rig,
        json!({"first_person_status":"owner","controller":"owner",
            "reason":"A camo is worn on its weapon and plays the weapon's first-person animation.","gameplay_verified":false}),
    ))
}

#[expect(
    clippy::cognitive_complexity,
    reason = "Preserve the audited converter while integrating the legacy rendering pipeline."
)]
fn build(
    source: &mut Reader,
    native: &mut Reader,
    entry: &Value,
    root: &Path,
    step: &mut dyn FnMut(&str),
) -> Result<Value> {
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
    let item_tag = hash(&template, "item_tag")?;
    let source_animation = entry["source_animation"] == true;
    step(if source_animation {
        "Translating animations…"
    } else {
        "Fitting the donor's animations…"
    });
    let (rig, animation) = if entry.get("ornament").is_some() {
        wear(source, native, parts, item_tag, source_animation)?
    } else {
        animate(source, native, parts, item_tag, root, source_animation)?
    };
    step("Reading the model…");
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
    let mut g = Graph::new(root.to_path_buf(), vec![]);
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
    step(&match mesh.groups.len() {
        1 => "Translating 1 material and its shaders…".to_owned(),
        count => format!("Translating {count} materials and their shaders…"),
    });
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
    step("Placing sights and displays…");
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
    result["source_motion"]["rests"] = json!(
        mesh.rests
            .iter()
            .map(|(entity, rest)| json!({"entity":format!("{entity:08X}"),"rest":rest}))
            .collect::<Vec<_>>()
    );
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

/// Convert every plan entry, the weapon and then its camos, into `out`. `progress` hears each
/// entry's stages as they start.
pub fn prepare(
    plan: &Path,
    packages: &Path,
    native_packages: &Path,
    out: &Path,
    progress: &mut dyn FnMut(String),
) -> Result<Value> {
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
    let mut aims = std::collections::BTreeMap::new();
    // Each weapon's camos take ornament choices in build order. A camo that cannot be translated
    // takes none and is reported, so its weapon and the other camos still import.
    let mut choices = std::collections::BTreeMap::<String, usize>::new();
    let mut camos = vec![];
    let mut omitted = vec![];
    let planned = entries
        .iter()
        .filter(|entry| entry.get("ornament").is_some())
        .count();
    let mut started = 0;
    let reserved = reserved_lanes(entries)?;
    // A camo that another weapon converted on the same donor and packages is copied.
    let states = json!({"source":crate::tiger::reader::package_files_stamp(packages)?,
        "native":crate::tiger::reader::package_files_stamp(native_packages)?});
    for entry in entries {
        let slug = entry["slug"].as_str().context("output slug")?;
        ensure!(
            !slug.is_empty()
                && slug.chars().all(|c| c.is_ascii_lowercase() || c == '-')
                && slugs.insert(slug),
            "invalid output slug"
        );
        let mut entry = entry.clone();
        entry["reserved_lanes"] = json!(reserved.get(slug));
        let camo = if entry.get("ornament").is_some() {
            let owner = entry["wears"]
                .as_str()
                .context("A camo names the weapon it wears")?
                .to_owned();
            let choice = choices.get(&owner).copied().unwrap_or(0) + 1;
            browse::camo::number(&mut entry, choice)?;
            entry["conversion"] = json!(browse::camo::conversion(&entry, &states)?);
            Some((owner, choice))
        } else {
            None
        };
        // The importer names the weapon. A camo's stages name the camo.
        let label = if camo.is_some() {
            started += 1;
            format!(
                "Camo {started} of {planned} ({}): ",
                entry["name"].as_str().unwrap_or(slug)
            )
        } else {
            String::new()
        };
        let mut step = |stage: &str| progress(format!("{label}{stage}"));
        let (earlier, claim) = match entry["conversion"].as_str() {
            Some(digest) => match crate::store::claim(
                browse::camo::CONVERSION,
                digest.as_bytes(),
                |graph| browse::camo::holds(graph, digest),
                || step("Waiting for another weapon to finish this camo…"),
            )? {
                crate::store::Folder::Made(earlier) => (Some(earlier), None),
                crate::store::Folder::Make(claim) => (None, Some(claim)),
            },
            None => (None, None),
        };
        let mut prepared = prepare_entry(
            (&mut source, &mut native),
            &entry,
            (working, &out),
            native_packages,
            &mut aims,
            earlier.as_deref(),
            &mut step,
        );
        let mut reused = earlier.is_some();
        if reused && matches!(&prepared, Err(error) if !crate::cancellation::is_cancelled(error)) {
            // A copy that fails, such as one whose folder moved meanwhile, converts instead.
            if let Err(error) = &prepared {
                eprintln!("{slug}: copy failed, converting, {error:#}");
            }
            remove_partial(working, slug)?;
            reused = false;
            prepared = prepare_entry(
                (&mut source, &mut native),
                &entry,
                (working, &out),
                native_packages,
                &mut aims,
                None,
                &mut step,
            );
        }
        match (prepared, camo) {
            (Ok(report), camo) => {
                if let Some(claim) = claim {
                    claim.made(&working.join(slug).join("graph"));
                }
                if let Some((owner, choice)) = camo {
                    choices.insert(owner, choice);
                    camos.push(
                        json!({"slug":slug,"name":entry["name"],"ornament":entry["ornament"],
                        "conversion":entry["conversion"],"reused":reused}),
                    );
                }
                reports.push(report);
            }
            (Err(error), Some(_)) if !crate::cancellation::is_cancelled(&error) => {
                remove_partial(working, slug)?;
                eprintln!("{slug}: left out, {error:#}");
                omitted
                    .push(json!({"slug":slug,"name":entry["name"],"reason":format!("{error:#}")}));
            }
            (Err(error), _) => return Err(error),
        }
    }
    release_unused_lanes(entries, working, &choices)?;
    source.finish()?;
    native.finish()?;
    write_json(&working.join("report.json"), &json!(reports))?;
    ensure!(!out.exists(), "Marathon output appeared concurrently");
    crate::io::move_folder(working, &out)?;
    // Later imports copy each camo from its final place.
    for camo in &camos {
        if let (Some(slug), Some(digest)) = (camo["slug"].as_str(), camo["conversion"].as_str()) {
            crate::store::remember_folder(
                browse::camo::CONVERSION,
                digest.as_bytes(),
                &out.join(slug).join("graph"),
            );
        }
    }
    Ok(json!({"output":out,"weapons":reports.len(),"camos":camos,"omitted":omitted}))
}

/// Remove what a failed entry left in `working`.
fn remove_partial(working: &Path, slug: &str) -> Result<()> {
    let partial = working.join(slug);
    if partial.exists() {
        fs::remove_dir_all(&partial)?;
    }
    Ok(())
}

/// The lanes each weapon's camos are planned to take, by weapon slug. A camo's lane is fixed when
/// it is planned, since its private identities follow it.
fn reserved_lanes(
    entries: &[Value],
) -> Result<std::collections::BTreeMap<String, std::collections::BTreeSet<usize>>> {
    let mut reserved = std::collections::BTreeMap::<String, std::collections::BTreeSet<_>>::new();
    for entry in entries {
        if let (Some(owner), Some(lane)) = (
            entry["wears"].as_str(),
            entry["ornament"]["target_socket"].as_u64(),
        ) {
            reserved
                .entry(owner.to_owned())
                .or_default()
                .insert(usize::try_from(lane)?);
        }
    }
    Ok(reserved)
}

/// A weapon none of whose camos translated leaves the lanes its mod columns skipped for them
/// empty, so its later columns move down into them.
fn release_unused_lanes(
    entries: &[Value],
    working: &Path,
    choices: &std::collections::BTreeMap<String, usize>,
) -> Result<()> {
    for entry in entries.iter().filter(|entry| entry.get("wears").is_none()) {
        let slug = entry["slug"].as_str().context("output slug")?;
        let recipe_path = working.join(slug).join("weapon.parhelion.json");
        if choices.contains_key(slug) || !recipe_path.exists() {
            continue;
        }
        let graph: Value = serde_json::from_slice(&fs::read(
            working.join(slug).join("graph/asset-graph.json"),
        )?)?;
        let mut lanes = graph["source_perks"]["reserved_lanes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .map(usize::try_from)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if lanes.is_empty() {
            continue;
        }
        lanes.sort_unstable_by(|a, b| b.cmp(a));
        let mut recipe: Value = serde_json::from_slice(&fs::read(&recipe_path)?)?;
        for lane in lanes {
            gameplay::release_lane(&mut recipe, lane)?;
        }
        write_json(&recipe_path, &recipe)?;
    }
    Ok(())
}

/// Translate one plan entry into `working`, with its gameplay, framing, artwork and recipe, or
/// copy a camo from `earlier`, the same camo converted for another weapon. `step` hears each
/// stage as it starts. Returns its report.
fn prepare_entry(
    (source, native): (&mut Reader, &mut Reader),
    entry: &Value,
    (working, out): (&Path, &Path),
    native_packages: &Path,
    aims: &mut std::collections::BTreeMap<String, Value>,
    earlier: Option<&Path>,
    step: &mut dyn FnMut(&str),
) -> Result<Value> {
    let slug = entry["slug"].as_str().context("output slug")?;
    let mut report = match earlier {
        Some(earlier) => {
            step("Copying this camo from another weapon…");
            browse::camo::reuse(earlier, entry, &working.join(slug).join("graph"))
                .with_context(|| format!("copying {slug} from {}", earlier.display()))?
        }
        None => build(
            source,
            native,
            entry,
            &working.join(slug).join("graph"),
            step,
        )
        .with_context(|| format!("building {slug}"))?,
    };
    if let Some(identity) = entry["source_item_identity"].as_u64() {
        step("Reading stats and perks…");
        author_gameplay(source, native, &mut report, u32::try_from(identity)?)?;
        // A weapon whose mods cannot be read still imports, with the donor's own perks. Its mod
        // columns leave the lanes its camos are planned to take.
        let reserved: std::collections::BTreeSet<usize> = entry["reserved_lanes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .map(usize::try_from)
            .collect::<std::result::Result<_, _>>()?;
        if let Err(error) = gameplay::author_perks(
            source,
            native,
            &mut report,
            u32::try_from(identity)?,
            &reserved,
        ) {
            if crate::cancellation::is_cancelled(&error) {
                return Err(error);
            }
            report["source_perks"] =
                json!({"source":"marathon","status":"unsupported","reason":format!("{error:#}")});
        }
    }
    // A camo wears its weapon's sight, so it aims with the weapon's magnification.
    if let Some(owner) = entry["wears"].as_str() {
        report["source_gameplay"]["aim"] = aims
            .get(owner)
            .cloned()
            .context("A camo is planned before its weapon")?;
    }
    aims.insert(slug.to_owned(), report["source_gameplay"]["aim"].clone());
    write_json(&working.join(slug).join("graph/asset-graph.json"), &report)?;
    crate::graph::record_converter_revision(&working.join(slug).join("graph"))?;
    // A copied camo keeps the icon its conversion rendered.
    if earlier.is_none() {
        let settings = entry
            .get("icon")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_else(|| crate::artwork::Settings::for_graph(&report, &entry["recipe"]));
        step("Rendering the inventory icon…");
        crate::artwork::prepare(&working.join(slug).join("graph"), native_packages, settings)?;
    }
    if let Some(link) = entry.get("ornament") {
        // A camo binds to its weapon's ornament socket, and its own render is its icon. The
        // conversion's digest goes last, so a folder that records it is complete.
        let path = working.join(slug).join("graph/asset-graph.json");
        let mut graph: Value = serde_json::from_slice(&fs::read(&path)?)?;
        graph["ornament"] = link.clone();
        graph["ornament_icon_png"] = graph["source_icon_png"].clone();
        graph["conversion"] = entry["conversion"].clone();
        write_json(&path, &graph)?;
    }
    if let Some(recipe) = entry.get("recipe") {
        let mut recipe = recipe.clone();
        apply_recipe_text(&mut recipe)?;
        apply_gameplay(&mut recipe, &report)?;
        gameplay::apply_perks(&mut recipe, &report)?;
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
    Ok(report)
}
