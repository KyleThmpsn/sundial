//! Real Reach caches through native conversion, portable reload and package staging.
#![cfg(feature = "d2-model-importer")]

use anyhow::{Context, Result, ensure};
use parhelion::{
    BatchBuildRequest, BatchBuildSnapshot, WeaponRecipe, build_and_stage_snapshot_with_progress,
};
use parhelion_import::tiger::{payload::Payload, reader::Reader};
use serde_json::{Value, json};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn configured(name: &str) -> Result<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("Set {name}"))
}

fn graph_bytes(root: &Path, graph: &Value, symbol: &str) -> Result<Payload> {
    let node = graph["nodes"]
        .as_array()
        .context("nodes")?
        .iter()
        .find(|node| node["symbol"] == symbol)
        .context("missing graph node")?;
    Ok(Payload(fs::read(
        root.join(node["file"].as_str().context("node file")?),
    )?))
}

#[test]
#[ignore = "Requires PARHELION_REACH_MAPS, PARHELION_REACH_PLAN, PARHELION_REACH_BARREL_WITNESS, SUNDIAL_STOCK_PACKAGES and fresh PARHELION_REACH_NATIVE_OUTPUT"]
fn translated_items_survive_portable_reload_and_native_staging() -> Result<()> {
    let maps = configured("PARHELION_REACH_MAPS")?;
    let native = configured("SUNDIAL_STOCK_PACKAGES")?;
    let output = configured("PARHELION_REACH_NATIVE_OUTPUT")?;
    ensure!(!output.exists(), "Use a fresh artifact directory");
    let barrel_witness: Value =
        serde_json::from_slice(&fs::read(configured("PARHELION_REACH_BARREL_WITNESS")?)?)?;
    let requested: Value = serde_json::from_slice(&fs::read(configured("PARHELION_REACH_PLAN")?)?)?;
    let report = parhelion_import::halo_reach::prepare(
        &configured("PARHELION_REACH_PLAN")?,
        &maps,
        &native,
        &output.join("converted"),
    )?;
    let entries = report["items"].as_array().context("prepared items")?;
    ensure!(!entries.is_empty(), "Empty required native corpus");
    let mut recipes = Vec::new();
    let mut evidence = Vec::new();
    for entry in entries {
        let path = PathBuf::from(entry["recipe"].as_str().context("prepared recipe")?);
        let recipe = WeaponRecipe::load_json(&path)?;
        let portable = recipe.to_json_pretty()?;
        let reloaded = WeaponRecipe::from_json_str(&portable)?;
        ensure!(
            recipe.identity == reloaded.identity,
            "Portable identity changed"
        );
        if recipe.kind.is_weapon() && recipe.icon_donor.is_none() {
            let icon = recipe
                .overrides
                .icon_edit
                .imported_image
                .as_ref()
                .context("Imported weapon has no inventory artwork")?;
            ensure!(
                reloaded.overrides.icon_edit.imported_image.as_ref() == Some(icon),
                "Generated artwork was lost during recipe reload"
            );
            let encoded = serde_json::to_value(icon)?;
            use base64::Engine as _;
            let png = base64::engine::general_purpose::STANDARD.decode(
                encoded["png_base64"]
                    .as_str()
                    .context("Embedded icon PNG")?,
            )?;
            let decoded = image::load_from_memory(&png)?.to_rgba8();
            ensure!(
                decoded.dimensions() == (96, 96),
                "Wrong inventory artwork size"
            );
            fs::write(
                output.join(format!(
                    "{}-icon.png",
                    entry["slug"].as_str().context("slug")?
                )),
                png,
            )?;
        }
        let reference = reloaded
            .overrides
            .imported_graph
            .as_ref()
            .context("imported graph")?;
        let graph: Value =
            serde_json::from_slice(&fs::read(reference.directory.join("asset-graph.json"))?)?;
        let request = requested
            .as_array()
            .context("Import plan")?
            .iter()
            .find(|request| request["slug"] == entry["slug"])
            .context("Prepared item absent from its plan")?;
        if request.get("first_person").is_some() {
            ensure!(
                graph["animation"]["first_person_status"] == "linked",
                "Requested arm motion was not linked"
            );
        }
        verify_loaded_inputs(&reference.directory, &graph)?;
        verify_sights(&reference.directory, &graph)?;
        verify_projectile_source(&path, &reference.directory, &graph)?;
        verify_prepared_geometry(entry, &recipe, &path, &reference.directory, &graph)?;
        fs::write(
            output.join(format!(
                "{}.parhelion.json",
                entry["slug"].as_str().context("slug")?
            )),
            portable,
        )?;
        evidence.push(
            json!({"recipe":path,"graph_sha256":reference.sha256,"native_geometry_verified":true}),
        );
        recipes.push(reloaded);
    }
    let snapshot = BatchBuildSnapshot::new(BatchBuildRequest {
        package_directory: native.clone(),
        staging_root: output.join("build"),
        ignore_installed_authored_overlays: true,
        recipes: recipes.clone(),
    })
    .map_err(anyhow::Error::msg)?;
    let built = build_and_stage_snapshot_with_progress(&snapshot, |p| println!("{:?}", p.phase))
        .map_err(anyhow::Error::msg)?;
    let view = output.join("view");
    let packages = view.join("packages");
    fs::create_dir_all(&packages)?;
    fs::create_dir_all(view.join("bin/x64"))?;
    for entry in fs::read_dir(&native)? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|e| e == "pkg") {
            fs::hard_link(entry.path(), packages.join(entry.file_name()))?;
        }
    }
    fs::copy(
        native
            .parent()
            .context("native root")?
            .join("bin/x64/oo2core_3_win64.dll"),
        view.join("bin/x64/oo2core_3_win64.dll"),
    )?;
    for artifact in &built.artifacts {
        let target = packages.join(&artifact.file_name);
        ensure!(
            !target.exists(),
            "Staged package would replace stock hard link"
        );
        fs::copy(built.run_directory.join(&artifact.file_name), target)?;
    }
    let mut staged = Reader::new(&packages, &output.join("readback"), false)?;
    let stock = Reader::discovery(&native, &output.join("stock-readback"), false)?;
    let globals = staged
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|n| n.name == "investment_globals")
        .context("globals")?
        .hash
        .0;
    let globals = staged.tag(globals, None)?;
    let root = staged.tag(globals.u32(16)?, None)?;
    let items = staged.tag(root.u32(8 + 48 * 16)?, None)?;
    for recipe in recipes {
        let hash = recipe.identity.item_hash.parse_u32()?;
        let row = items
            .array(8, 24, Some(0x80807be8))?
            .into_iter()
            .find(|&r| items.u32(r).ok() == Some(hash))
            .context("Authored item missing")?;
        let definition = staged.tag(items.u32(row + 16)?, Some(0x80807bea))?;
        ensure!(
            !definition
                .array(definition.pointer(0x88)?, 4, Some(0x808077b5))?
                .is_empty(),
            "Authored art is not registered"
        );
        let runtime =
            parhelion_import::tiger::rig::inspect(&mut staged, items.u32(row + 16)?, false)?;
        let reference = recipe
            .overrides
            .imported_graph
            .as_ref()
            .context("Imported graph")?;
        let graph: Value =
            serde_json::from_slice(&fs::read(reference.directory.join("asset-graph.json"))?)?;
        verify_staged_audio(
            &mut staged,
            &stock,
            items.u32(row + 16)?,
            &reference.directory,
            &graph,
        )?;
        verify_staged_projectile(
            &mut staged,
            &stock,
            &reference.directory,
            &graph,
            &runtime,
            &barrel_witness,
        )?;
        verify_staged_motion(&mut staged, &stock, &graph, &runtime)?;
        verify_staged_arms(&mut staged, &stock, &graph, &runtime)?;
        verify_staged_vehicle(&mut staged, &recipe, &runtime, &barrel_witness)?;
        verify_staged_barrel(&mut staged, &recipe, &runtime, &barrel_witness)?;
    }
    staged.finish()?;
    fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(
            &json!({"items":evidence,"staged":built.run_directory,"gameplay_verified":false}),
        )?,
    )?;
    Ok(())
}

fn verify_staged_arms(
    staged: &mut Reader,
    stock: &Reader,
    graph: &Value,
    runtime: &Value,
) -> Result<()> {
    let motion = &graph["animation"]["first_person"]["arm_motion"];
    if motion.is_null() {
        return Ok(());
    }
    let components = runtime["components"].as_array().context("Components")?;
    let lookup = components
        .iter()
        .filter(|row| row["entity"] != runtime["runtime_entity"] && row["class"] == "8080344B")
        .collect::<Vec<_>>();
    ensure!(lookup.len() == 1, "Missing first-person lookup");
    let owner = u32::from_str_radix(lookup[0]["owner"].as_str().context("Lookup owner")?, 16)?;
    ensure!(
        stock.manager.get_entry(tiger_pkg::TagHash(owner)).is_none(),
        "Imported arms still use the stock lookup"
    );
    let lookup = staged.tag(owner, Some(0x80809c36))?;
    let definition = lookup.pointer(24)?;
    let bank_tag = lookup.u32(definition + 0x90)?;
    let bank = staged.tag(bank_tag, Some(0x808036f6))?;
    let states_tag = lookup.u32(definition + 0x9c)?;
    let states = staged.tag(states_tag, Some(0x80803465))?;
    for tag in [bank_tag, states_tag] {
        ensure!(
            stock.manager.get_entry(tiger_pkg::TagHash(tag)).is_none(),
            "Imported reload retained a stock routing table"
        );
    }
    let mut receipts = Vec::new();
    for action in motion["actions"].as_array().context("Arm actions")? {
        let name = action["state"].as_str().context("Arm action name")?;
        let mut linked = Vec::new();
        for (descriptor, tag) in arm_routes(&states, &bank, name)? {
            ensure!(
                stock.manager.get_entry(tiger_pkg::TagHash(tag)).is_none(),
                "Reload state still selects a donor clip"
            );
            let clip = staged.tag(tag, Some(0x80808f49))?;
            let dynamic = clip.pointer(24)?;
            ensure!(
                clip.u16(dynamic)? == 0 && clip.u16(0x13c)? > 1,
                "Arm motion has no native frame stream"
            );
            let hand_slots = motion["hand_slots"].as_array().context("Hand slots")?;
            let rotations = clip.array(0xe8, 2, Some(0x8080000a))?;
            ensure!(
                hand_slots.iter().all(|slot| rotations
                    .iter()
                    .any(|&at| { clip.u16(at).ok().map(u64::from) == slot.as_u64() })),
                "Reload clip does not drive both hands"
            );
            linked.push(json!({"descriptor":descriptor,"tag":format!("{tag:08X}"),"frames":clip.u16(0x13c)?}));
        }
        ensure!(!linked.is_empty(), "Reload action has no clips");
        receipts.push(json!({"state":name,"clips":linked}));
    }
    ensure!(!receipts.is_empty(), "Empty required arm motion");
    fs::write(
        staged.output.join(format!("arms-{owner:08X}.json")),
        serde_json::to_vec_pretty(
            &json!({"lookup":owner,"bank":bank_tag,"states":states_tag,"actions":receipts,"gameplay_verified":false}),
        )?,
    )?;
    Ok(())
}

fn arm_routes(states: &Payload, bank: &Payload, name: &str) -> Result<Vec<(u32, u32)>> {
    let names = states.array(8, 8, Some(0x8080342E))?;
    let nodes = states.array(24, 16, Some(0x8080342F))?;
    let descriptors = bank.array(0x68, 32, Some(0x80809002))?;
    let clips = bank.array(8, 4, Some(0x80808F48))?;
    let hash = sundial::package_authoring::fnv1_name_hash(name);
    let row = names
        .iter()
        .find(|&&row| states.u32(row).ok() == Some(hash))
        .context("Reload state disappeared")?;
    let node = nodes[states.u32(row + 4)? as usize];
    let root = states.pointer(node + 8)?;
    let weighted = match states.u64(node)? {
        1 => vec![root],
        2 => states.array(root, 16, Some(0x80803437))?,
        _ => anyhow::bail!("Unsupported reload state"),
    };
    let mut linked = Vec::new();
    for choice in weighted {
        for entry in states.array(choice, 8, Some(0x80803439))? {
            let index = states.u32(entry)?;
            let descriptor = *descriptors
                .get(index as usize)
                .context("State descriptor outside bank")?;
            let slot = *clips
                .get(usize::from(bank.u16(descriptor + 24)?))
                .context("Descriptor clip outside bank")?;
            linked.push((index, bank.u32(slot)?));
        }
    }
    Ok(linked)
}

fn verify_loaded_inputs(root: &Path, graph: &Value) -> Result<()> {
    if graph["object_inputs"]
        .as_object()
        .is_none_or(|v| v.is_empty())
    {
        return Ok(());
    }
    for (symbol, fields) in [
        ("owner", &[(0x120, 96, 0x80809788)][..]),
        (
            "object-channels",
            &[(0x50, 16, 0x80800090), (0x90, 16, 0x8080979C)][..],
        ),
    ] {
        let stored = graph_bytes(root, graph, symbol)?;
        let start = stored.pointer(16)?;
        let end = stored.pointer(24)?;
        // Shadowkeep copies this span, not the complete stored resource. Resolve the
        // descriptors after the copy to exercise what binding and cleanup actually read.
        let loaded = Payload(stored.0[start..end].to_vec());
        if symbol == "object-channels" {
            verify_channel_receivers(&stored, &loaded)?;
        }
        for &(field, stride, class) in fields {
            let rows = loaded.array(field, stride, Some(class))?;
            ensure!(!rows.is_empty(), "Loaded {symbol} input storage is empty");
            if class == 0x8080979C {
                // Each pair starts unbound. Native unlink skips a handle of FFFFFFFF.
                ensure!(
                    rows.iter()
                        .all(|row| loaded.0[*row..row + stride].iter().all(|v| *v == 255)),
                    "Fresh channel state contains live cleanup handles"
                );
            }
        }
    }
    Ok(())
}

fn verify_channel_receivers(stored: &Payload, loaded: &Payload) -> Result<()> {
    let receivers = loaded.array(0x70, 80, None)?;
    let original = usize::try_from(loaded.u64(8)?)?;
    for definition in [original, stored.pointer(24)?] {
        for row in stored.array(definition + 0xD8, 112, Some(0x808097A1))? {
            // The native update dispatches every receiver in this inclusive byte range.
            // An inherited channel with no local receivers must select an empty range.
            let first = usize::from(stored.0[row + 0x68]);
            let last = usize::from(stored.0[row + 0x69]);
            ensure!(
                first > last || last < receivers.len(),
                "Channel {:08X} dispatches outside its loaded receiver array",
                stored.u32(row)?
            );
        }
    }
    Ok(())
}

fn verify_sights(root: &Path, graph: &Value) -> Result<()> {
    let Some(sight) = graph.get("optic") else {
        return Ok(());
    };
    let entity = graph_bytes(root, graph, "optic-entity")?;
    let components = entity.array(16, 12, Some(0x80809c04))?;
    let mut checked = 0;
    for (index, _) in components.iter().enumerate() {
        let p = graph_bytes(root, graph, &format!("optic-owner-{index}"))?;
        let definition = p.pointer(24)?;
        if p.u32(definition - 4)? != 0x8080393b {
            continue;
        }
        for (offset, name) in [(0x198, "rear"), (0x228, "front")] {
            let points = p.array(definition + offset, 64, Some(0x80809c00))?;
            ensure!(points.len() == 1, "Sight has no unique default marker");
            for axis in 0..3 {
                let expected = sight[name][axis].as_f64().context("Sight point")? as f32;
                ensure!(
                    (p.f32(points[0] + 32 + axis * 4)? - expected).abs() < 0.000001,
                    "Sight provider does not use the imported sight axis"
                );
            }
        }
        checked += 1;
    }
    ensure!(checked == 1, "Missing or ambiguous imported optic");
    Ok(())
}

fn verify_staged_motion(
    staged: &mut Reader,
    stock: &Reader,
    graph: &Value,
    runtime: &Value,
) -> Result<()> {
    let Some(motion) = graph.get("source_motion") else {
        return Ok(());
    };
    ensure!(
        motion["bindings"].as_array().is_some_and(|v| !v.is_empty()),
        "Requested source motion has no bindings"
    );
    let component = |class: &str| -> Result<u32> {
        let matches = runtime["components"]
            .as_array()
            .context("Runtime components")?
            .iter()
            .filter(|c| c["entity"] == runtime["runtime_entity"] && c["class"] == class)
            .collect::<Vec<_>>();
        ensure!(
            matches.len() == 1,
            "Source motion needs a unique runtime {class}"
        );
        Ok(u32::from_str_radix(
            matches[0]["owner"].as_str().context("Owner")?,
            16,
        )?)
    };
    let skeleton_tag = component("80808546")?;
    let lookup_tag = component("8080344B")?;
    for tag in [skeleton_tag, lookup_tag] {
        ensure!(
            stock.manager.get_entry(tiger_pkg::TagHash(tag)).is_none(),
            "Source motion retained a stock owner"
        );
    }
    let skeleton = staged.tag(skeleton_tag, Some(0x80809c36))?;
    let resource = skeleton.pointer(24)?;
    let bones = skeleton.array(resource + 0x80, 16, Some(0x80808a08))?;
    ensure!(
        Some(bones.len() as u64) == motion["bones"].as_u64(),
        "Staged source rig count changed"
    );
    let lookup = staged.tag(lookup_tag, Some(0x80809c36))?;
    let bank_tag = lookup.u32(lookup.pointer(24)? + 0x90)?;
    ensure!(
        stock
            .manager
            .get_entry(tiger_pkg::TagHash(bank_tag))
            .is_none(),
        "Source motion retained a stock bank"
    );
    let bank = staged.tag(bank_tag, Some(0x808036f6))?;
    let mut clips = Vec::new();
    for (index, row) in bank.array(8, 4, Some(0x80808f48))?.into_iter().enumerate() {
        let tag = bank.u32(row)?;
        ensure!(
            stock.manager.get_entry(tiger_pkg::TagHash(tag)).is_none(),
            "Extended source rig retained an unconverted stock clip"
        );
        let clip = staged.tag(tag, Some(0x80808f49))?;
        ensure!(
            usize::from(clip.u16(0x13e)?) == bones.len(),
            "Staged clip addresses another skeleton"
        );
        clips.push(json!({"ordinal":index,"tag":format!("{tag:08X}"),"frames":clip.u16(0x13c)?,"name":format!("{:08X}",clip.u32(0x120)?)}));
    }
    ensure!(!clips.is_empty(), "Extended source rig has an empty bank");
    fs::write(
        staged.output.join(format!(
            "motion-{}.json",
            runtime["runtime_entity"].as_str().context("Entity")?
        )),
        serde_json::to_vec_pretty(
            &json!({"entity":runtime["runtime_entity"],"skeleton":format!("{skeleton_tag:08X}"),"bank":format!("{bank_tag:08X}"),"clips":clips,"source":motion}),
        )?,
    )?;
    Ok(())
}

fn verify_staged_barrel(
    staged: &mut Reader,
    recipe: &WeaponRecipe,
    runtime: &Value,
    witness: &Value,
) -> Result<()> {
    let item = recipe.identity.item_hash.parse_u32()?;
    let Some(witness) = witness.get(format!("{item:08X}")) else {
        return Ok(());
    };
    let wanted = witness["projectiles"]
        .as_u64()
        .context("Independent source projectile count")?;
    let entity = u32::from_str_radix(
        runtime["runtime_entity"]
            .as_str()
            .context("Runtime entity")?,
        16,
    )?;
    let entity = staged.tag(entity, Some(0x80809c0f))?;
    let bindings = sundial::package_authoring::entity::weapon_component_bindings(
        &entity.0,
        sundial::package_authoring::entity::WEAPON_BARREL_COMPONENT_KEY,
    )
    .map_err(anyhow::Error::msg)?;
    ensure!(bindings.len() == 1, "Staged weapon has no unique Barrel");
    let binding = bindings[0];
    ensure!(
        binding.concrete_class == 0x80803889,
        "Unsupported native Barrel"
    );
    let owner = staged.tag(binding.owner_tag, Some(0x80809c36))?;
    let definition = usize::try_from(owner.u64(binding.resource_offset as usize + 8)?)?;
    ensure!(
        owner.u32(binding.resource_offset as usize + 4)? == 0x80803865,
        "Barrel definition class changed"
    );
    let slot = definition + 0xe60;
    let count = if owner.u64(slot)? == 0 {
        1
    } else {
        let pattern = owner.pointer(slot)?;
        ensure!(
            owner.u32(pattern - 4)? == 0x8080888d,
            "Spread pattern class changed"
        );
        let rows = owner.array(pattern + 0x48, 20, Some(0x8080888f))?;
        let total = rows.iter().try_fold(0u64, |sum, row| {
            owner.u32(row + 12).map(|count| sum + u64::from(count))
        })?;
        ensure!(
            total == u64::from(owner.u32(pattern + 0x60)?),
            "Spread rings and total disagree"
        );
        total
    };
    ensure!(
        count == wanted,
        "Native simultaneous projectile count differs from source"
    );
    Ok(())
}

fn verify_projectile_source(path: &Path, directory: &Path, graph: &Value) -> Result<()> {
    if let Some(projectile) = graph.get("projectile") {
        let projectile_graph = json!({"nodes":projectile["nodes"]});
        let root = projectile["root"].as_str().context("Projectile root")?;
        let entity = graph_bytes(directory, &projectile_graph, root)?;
        ensure!(
            entity.u16(0x96)? == 18,
            "Imported projectile lost its object type"
        );
        let source: Value = serde_json::from_slice(&fs::read(
            path.parent()
                .context("Item folder")?
                .join("source/projectiles.json"),
        )?)?;
        let datum = projectile["conversion"]["source"]["datum"]
            .as_u64()
            .context("Source projectile identity")?;
        let source = source
            .as_array()
            .context("Projectile source receipt")?
            .iter()
            .find(|p| p["tag"]["datum"] == datum)
            .context("Projectile source is absent")?;
        let scene: Value = serde_json::from_slice(&fs::read(
            path.parent()
                .context("Item folder")?
                .join("source")
                .join(
                    source["directory"]
                        .as_str()
                        .context("Projectile directory")?,
                )
                .join("source.json"),
        )?)?;
        ensure!(
            scene["models"]
                .as_array()
                .context("Projectile models")?
                .len()
                == 1,
            "Projectile witness requires one source model"
        );
        let wanted = scene["models"][0]["model"]["primitives"]
            .as_array()
            .context("Projectile primitives")?
            .iter()
            .flat_map(|p| p["vertices"].as_array().into_iter().flatten())
            .collect::<Vec<_>>();
        let positions = graph_bytes(
            directory,
            &projectile_graph,
            "reach-projectile-positions-data",
        )?;
        let model = graph_bytes(directory, &projectile_graph, "reach-projectile-model")?;
        ensure!(
            positions.0.len() == wanted.len() * 16,
            "Projectile vertex loss"
        );
        for (i, vertex) in wanted.iter().enumerate() {
            for axis in 0..3 {
                let actual = positions.i16(i * 16 + axis * 2)? as f32 / 32767. * model.f32(0x6c)?
                    + model.f32(0x60 + axis * 4)?;
                let expected = vertex["position"][axis]
                    .as_f64()
                    .context("Source projectile position")? as f32;
                ensure!(
                    (actual - expected).abs() <= model.f32(0x6c)? / 32767. + 0.00001,
                    "Projectile geometry changed"
                );
            }
        }
    }
    Ok(())
}

fn verify_skin(positions: &Payload, model: &Payload, expected: &[&Value]) -> Result<()> {
    for (i, vertex) in expected.iter().enumerate() {
        for axis in 0..3 {
            let actual = positions.i16(i * 16 + axis * 2)? as f32 / 32767. * model.f32(0x6c)?
                + model.f32(0x60 + axis * 4)?;
            let wanted = vertex["position"][axis]
                .as_f64()
                .context("source position")? as f32;
            ensure!(
                (actual - wanted).abs() <= model.f32(0x6c)? / 32767. + 0.00001,
                "Native quantized shape differs"
            );
        }
        ensure!(!vertex["weights"].is_null(), "Source skin witness absent");
        ensure!(
            positions.0[i * 16 + 8..i * 16 + 12]
                .iter()
                .map(|v| u32::from(*v))
                .sum::<u32>()
                == 255,
            "Native weights do not normalize"
        );
        for influence in 0..4 {
            let weight = vertex["weights"][influence]
                .as_f64()
                .context("Source weight")?;
            let stored = f64::from(positions.u8(i * 16 + 8 + influence)?) / 255.;
            ensure!(
                (stored - weight).abs() <= 3. / 255.,
                "Source influence was merged or lost"
            );
            if weight > 0. {
                ensure!(
                    u64::from(positions.u8(i * 16 + 12 + influence)?)
                        == vertex["joints"][influence]
                            .as_u64()
                            .context("Source joint")?,
                    "Source joint identity was replaced by a donor joint"
                );
            }
        }
    }
    Ok(())
}

fn verify_prepared_geometry(
    entry: &Value,
    recipe: &WeaponRecipe,
    path: &Path,
    directory: &Path,
    graph: &Value,
) -> Result<()> {
    let model = graph_bytes(directory, graph, "model")?;
    let positions = graph_bytes(directory, graph, "positions-data")?;
    let indices = graph_bytes(directory, graph, "indices-data")?;
    let source: Value = serde_json::from_slice(&fs::read(
        path.parent()
            .context("item folder")?
            .join("source/source.json"),
    )?)?;
    if entry["source"]["audio"].is_object()
        && recipe.kind == parhelion::ItemKind::Weapon
        && source["object"]["gameplay"]["barrels"]
            .as_array()
            .is_some_and(|b| !b.is_empty())
    {
        ensure!(
            graph["audio"]["authoring_schema"] == 3,
            "Source firing audio was not translated: {}",
            graph["audio"]
        );
    }
    ensure!(
        source["models"].as_array().context("source models")?.len() == 1,
        "Use unattached, identity-placement cases for independent native shape comparison"
    );
    let mut expected = Vec::new();
    for placed in source["models"].as_array().context("source models")? {
        for primitive in placed["model"]["primitives"]
            .as_array()
            .context("source draws")?
        {
            for vertex in primitive["vertices"]
                .as_array()
                .context("source vertices")?
            {
                expected.push(vertex);
            }
        }
    }
    ensure!(
        positions.0.len() == expected.len() * 16,
        "Native vertex loss"
    );
    for stage in [0, 7] {
        if model.u16(0xd8 + stage * 2)? != model.u16(0xda + stage * 2)? {
            ensure!(
                model.u16(0x108 + stage * 2)? == 28,
                "Weighted native layout missing"
            );
        }
    }
    verify_skin(&positions, &model, &expected)?;
    let draws = model.array(0xc8, 32, Some(0x8080737e))?;
    let mut triangles = 0;
    for (ordinal, row) in draws.into_iter().enumerate() {
        let visible = [0, 7].into_iter().any(|stage| {
            (usize::from(model.u16(0xd8 + stage * 2).unwrap())
                ..usize::from(model.u16(0xda + stage * 2).unwrap()))
                .contains(&ordinal)
        });
        ensure!(model.u8(row + 29)? > 0, "Native draw cannot advance");
        let start = model.u32(row + 8)? as usize;
        let count = model.u32(row + 12)? as usize;
        ensure!(
            (start + count) * 4 <= indices.0.len(),
            "Native draw escapes index buffer"
        );
        for at in (start..start + count).step_by(4) {
            for corner in 0..3 {
                ensure!(
                    (indices.u32((at + corner) * 4)? as usize) < expected.len(),
                    "Native face escapes vertices"
                );
            }
            ensure!(
                indices.u32((at + 3) * 4)? == u32::MAX,
                "Strip restart missing"
            );
            if visible {
                triangles += 1;
            }
        }
    }
    ensure!(
        triangles as u64 == expected_source_triangles(&source)?,
        "Native geometry coverage differs"
    );
    Ok(())
}

fn expected_source_triangles(source: &Value) -> Result<u64> {
    source["models"]
        .as_array()
        .context("Source models")?
        .iter()
        .flat_map(|m| m["model"]["primitives"].as_array().into_iter().flatten())
        .try_fold(0u64, |count, p| {
            let indices = p["indices"]
                .as_array()
                .context("Source primitive indices")?;
            ensure!(
                indices.len().is_multiple_of(3),
                "Incomplete source triangle"
            );
            Ok(count + indices.len() as u64 / 3)
        })
}

fn verify_staged_audio(
    staged: &mut Reader,
    stock: &Reader,
    item: u32,
    directory: &Path,
    graph: &Value,
) -> Result<()> {
    if graph["audio"]["authoring_schema"] == 3 {
        let routed = parhelion_import::tiger::rig::inspect_with_audio(staged, item, false)?;
        let sounds = routed["audio"]["unnamed_groups"]
            .as_array()
            .context("Staged firing groups")?
            .iter()
            .flat_map(|g| g["sounds"].as_array().into_iter().flatten())
            .collect::<Vec<_>>();
        let mut private = Vec::new();
        for sound in &sounds {
            let bank = u32::from_str_radix(sound["bank"].as_str().context("Firing bank")?, 16)?;
            if stock.reference(bank).is_err() {
                staged.tag(bank, None)?;
                private.push(*sound);
            }
        }
        parhelion_import::io::write_json(
            &staged.output.join(format!("audio-{item:08X}.json")),
            &json!({"sounds":private,"expected":graph["audio"],"graph":directory}),
        )?;
        for media in graph["audio"]["transcoded_media"]
            .as_array()
            .context("Imported audio media")?
        {
            let expected = fs::read(directory.join(media["file"].as_str().context("Audio file")?))?;
            let mut found = false;
            for sound in &sounds {
                for tag in sound["media"].as_array().into_iter().flatten() {
                    let tag = u32::from_str_radix(tag.as_str().context("Staged audio tag")?, 16)?;
                    if stock.reference(tag).is_ok() {
                        continue;
                    }
                    found |= staged.tag(tag, None)?.0 == expected;
                }
            }
            ensure!(found, "Staged firing route does not reach source PCM");
        }
    }
    Ok(())
}

fn verify_staged_projectile(
    staged: &mut Reader,
    stock: &Reader,
    directory: &Path,
    graph: &Value,
    runtime: &Value,
    witness: &Value,
) -> Result<()> {
    if let Some(projectile) = graph.get("projectile") {
        let nodes = json!({"nodes":projectile["nodes"]});
        let expected = graph_bytes(directory, &nodes, "reach-projectile-positions-data")?;
        let mut matches = Vec::new();
        for tag in staged
            .classes(0x80809c0f)
            .into_iter()
            .filter(|t| stock.reference(*t).is_err())
        {
            let entity = staged.tag(tag, Some(0x80809c0f))?;
            if entity.u16(0x96)? != 18 {
                continue;
            }
            for row in entity.array(16, 12, Some(0x80809c04))? {
                let owner = staged.tag(entity.u32(row)?, Some(0x80809c36))?;
                let resource = owner.pointer(24)?;
                if owner.u32(resource - 4)? != 0x808072bd {
                    continue;
                }
                let model = staged.tag(owner.u32(resource + 0x1dc)?, Some(0x808073a5))?;
                for row in model.array(16, 136, Some(0x80807378))? {
                    let data = staged.tag(staged.reference(model.u32(row)?)?, None)?;
                    if data.0 == expected.0 {
                        matches.push(tag);
                    }
                }
            }
        }
        ensure!(
            matches.len() == 1,
            "Staged source projectile is absent or ambiguous"
        );
        let entity_tag = u32::from_str_radix(
            runtime["runtime_entity"]
                .as_str()
                .context("Runtime entity")?,
            16,
        )?;
        let entity = staged.tag(entity_tag, Some(0x80809c0f))?;
        let bindings =
            sundial::package_authoring::entity::weapon_component_bindings(&entity.0, 0x5f0dd954)
                .map_err(anyhow::Error::msg)?;
        let [binding] = bindings.as_slice() else {
            anyhow::bail!("Weapon content selection is absent or ambiguous");
        };
        ensure!(binding.concrete_class == 0x80803acb, "Weapon content class");
        let owner = staged.tag(binding.owner_tag, Some(0x80809c36))?;
        let definition = owner.u64(binding.resource_offset as usize + 8)? as usize;
        ensure!(
            owner.u32(definition - 4)? == 0x80803ac9,
            "Weapon content definition"
        );
        let content =
            u32::from_str_radix(runtime["content_key"].as_str().context("Content key")?, 16)?;
        let selected = owner
            .array(definition + 0x240, 0x1c0, Some(0x80803acf))?
            .into_iter()
            .find(|&row| owner.u32(row + 0x10).ok() == Some(content))
            .unwrap_or(definition + 0x80);
        ensure!(
            owner.u32(selected + 0xf0)? == matches[0],
            "Weapon runtime does not select the translated projectile"
        );
        // Follow the emitted projectile's Movement resource, independently of
        // the converter report. Its reset value and live initial value must agree.
        let fired = staged.tag(matches[0], Some(0x80809c0f))?;
        let movement =
            sundial::package_authoring::entity::weapon_component_bindings(&fired.0, 0x0437756d)
                .map_err(anyhow::Error::msg)?;
        let item = graph["item_hash"]
            .as_u64()
            .context("Imported item identity")?;
        let expected = witness[format!("{item:08X}")]["launch_multiplier"]
            .as_f64()
            .context("Configured projectile launch multiplier witness")?
            as f32;
        ensure!(
            expected.is_finite() && expected > 0.0,
            "Invalid launch multiplier witness"
        );
        let mut speeds = Vec::new();
        for binding in movement {
            if binding.concrete_class != 0x80803b73 {
                continue;
            }
            let payload = staged.tag(binding.owner_tag, Some(0x80809c36))?;
            let instance = usize::try_from(binding.resource_offset)?;
            let definition = usize::try_from(payload.u64(instance + 8)?)?;
            ensure!(
                payload.u32(definition - 4)? == 0x8080388f,
                "Projectile definition class"
            );
            let speed = payload.f32(instance + 0x144)?;
            ensure!(
                speed == payload.f32(definition + 0x88)?,
                "Projectile reset loses launch speed"
            );
            ensure!(
                speed == expected,
                "Imported projectile does not use the configured launch multiplier"
            );
            speeds.push(json!({"owner":binding.owner_tag,"instance":instance,
                "definition":definition,"multiplier":speed}));
        }
        ensure!(
            !speeds.is_empty(),
            "Imported projectile has no checked moving trajectory"
        );
        fs::write(
            staged
                .output
                .join(format!("projectile-{entity_tag:08X}.json")),
            serde_json::to_vec_pretty(&json!({"runtime_entity":entity_tag,
                "content_owner":binding.owner_tag,"content_key":content,
                "selected_property":selected,"projectile":matches[0],
                "speed":speeds,"geometry_verified":true,"gameplay_verified":false}))?,
        )?;
    }
    Ok(())
}

fn verify_staged_vehicle(
    staged: &mut Reader,
    recipe: &WeaponRecipe,
    runtime: &Value,
    witness: &Value,
) -> Result<()> {
    if recipe.kind == parhelion::ItemKind::Sparrow {
        let reference = recipe
            .overrides
            .imported_graph
            .as_ref()
            .context("Vehicle graph")?;
        let graph: Value =
            serde_json::from_slice(&fs::read(reference.directory.join("asset-graph.json"))?)?;
        let expected = graph_bytes(&reference.directory, &graph, "positions-data")?;
        let carrier = graph["native_model"].as_u64().context("Native model")?;
        let visibility = &witness["vehicle_visibility"][format!("{carrier:08X}")];
        let visible = hex::decode(
            visibility["enabled_groups_hex"]
                .as_str()
                .context("Vehicle carrier requires an independent visibility witness")?,
        )?;
        ensure!(!visible.is_empty(), "Empty vehicle visibility witness");
        let mut matched = false;
        let mut draws = Vec::new();
        for component in runtime["components"]
            .as_array()
            .context("Runtime components")?
        {
            if component["class"] != "808072BD" {
                continue;
            }
            let owner = staged.tag(
                u32::from_str_radix(component["owner"].as_str().context("Runtime owner")?, 16)?,
                Some(0x80809c36),
            )?;
            let model = staged.tag(owner.u32(owner.pointer(24)? + 0x1dc)?, Some(0x808073a5))?;
            for row in model.array(16, 136, Some(0x80807378))? {
                let header = model.u32(row)?;
                let bytes = staged.tag(staged.reference(header)?, None)?;
                if bytes.0 != expected.0 {
                    continue;
                }
                matched = true;
                let parts = model.array(row + 24, 32, Some(0x8080737E))?;
                ensure!(!parts.is_empty(), "Imported vehicle has no draw records");
                for part in parts {
                    let group = usize::from(model.u16(part + 20)?);
                    ensure!(
                        visible
                            .get(group / 8)
                            .is_some_and(|b| b & (1 << (group % 8)) != 0),
                        "Imported vehicle draw uses inactive native visibility group {group}"
                    );
                    draws.push(json!({"index":model.u16(part + 22)?,"group":group}));
                }
            }
        }
        ensure!(
            matched,
            "Summoned vehicle does not reference the translated geometry"
        );
        fs::write(
            staged.output.join(format!(
                "vehicle-visibility-{:08X}.json",
                recipe.identity.item_hash.parse_u32()?
            )),
            serde_json::to_vec_pretty(
                &json!({"carrier":carrier,"witness":visibility,"draws":draws,"gameplay_verified":false}),
            )?,
        )?;
    }
    Ok(())
}
