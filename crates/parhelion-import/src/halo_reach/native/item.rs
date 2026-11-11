//! One import entry's preparation: its source read with saved choices completed from the
//! defaults, its converted animation, audio and projectiles, its graph built and extended with
//! the native routes its source needs, and its graph and recipe written under the batch's
//! temporary folder.
use super::*;
use crate::halo_reach::animation::character;
mod armament;

/// What every entry of a batch shares.
pub(super) struct Batch<'a> {
    pub(super) maps: &'a Path,
    pub(super) native_packages: &'a Path,
    /// The batch's final folder, which the recipes name.
    pub(super) output: &'a Path,
    /// The folder the batch is prepared in before it moves to `output`.
    pub(super) temporary: &'a Path,
}

/// One entry's read sources and the folders its conversions write, shared by its later stages.
struct Sources<'a> {
    cache: &'a cache::Cache,
    behavior: &'a Value,
    projectiles: &'a Value,
    entry: &'a ImportRequest,
    /// The exported source folder.
    source: &'a Path,
    /// The native graph folder.
    graph: &'a Path,
}

/// Prepares `entry` and returns its row of the batch report.
pub(super) fn prepare(
    batch: &Batch<'_>,
    native: &mut Reader,
    pages: &mut resource::Pages,
    mut entry: ImportRequest,
) -> Result<Value> {
    let root = batch.temporary.join(&entry.slug);
    let source = root.join("source");
    let graph_root = root.join("graph");
    fs::create_dir_all(&source)?;
    let path = source_path(batch.maps, &entry.source.cache)?;
    pages.record(&path)?;
    let cache = cache::Cache::open(&path)?;
    let mut scene = scene(&cache, pages, &entry.source)?;
    character_defaults(&cache, &scene, native, &mut entry)?;
    gltf::write(&cache, pages, &scene, &source)?;
    write_json(&source.join("source.json"), &serde_json::to_value(&scene)?)?;
    let behavior = behavior::read(&cache, &scene)?;
    audio_defaults(batch.maps, &behavior, &scene, &mut entry)?;
    let (mut recipe, mut gameplay) = recipe(native, &entry, &scene, &behavior)?;
    let hud = crate::halo_reach::hud::prepare(&cache, pages, &scene, &mut recipe, &source)?;
    write_json(&source.join("behavior.json"), &behavior)?;
    let projectiles = crate::halo_reach::projectile::export(&cache, pages, &behavior, &source)?;
    let mut projectile_import =
        crate::halo_reach::projectile::select(&mut entry, &behavior, &projectiles)?;
    crate::halo_reach::animation::export(&cache, pages, &behavior, &scene, &source)?;
    first_person_defaults(&cache, &scene, native, &mut entry, &source, &gameplay)?;
    if scene.object.tag.group == "weap"
        && (entry.first_person.is_some()
            || crate::halo_reach::hud::scope::zooms(&scene.object.gameplay))
    {
        gameplay["aim"] = crate::halo_reach::hud::scope::aim(&cache, &scene.object.tag)?;
    }
    let source_audio = entry
        .source
        .audio
        .as_ref()
        .map(|options| {
            crate::halo_reach::audio::export(options, &behavior, pages, &source.join("audio"))
        })
        .transpose()?;
    crate::halo_reach::damage::expand(&cache, pages, &mut scene, &entry.source)?;
    if let Some(damage) = &scene.damage {
        write_json(
            &source.join("damage-geometry.json"),
            &serde_json::to_value(damage)?,
        )?;
    }
    let mut scene = scale::scene(scene, entry.scale)?;
    let layout = armament::Layout::read(&cache, native, &entry, &mut scene)?;
    let animation = first_person_actions(&cache, &scene, native, &entry, &graph_root)?;
    let sources = Sources {
        cache: &cache,
        behavior: &behavior,
        projectiles: &projectiles,
        entry: &entry,
        source: &source,
        graph: &graph_root,
    };
    let armament = armament::Armament::convert(&sources, pages, native, &mut scene, layout)?;
    let mut graph = build(
        &cache,
        pages,
        native,
        &scene,
        &entry,
        &graph_root,
        false,
        animation
            .as_ref()
            .and_then(|a| a.get("equipment_skeleton"))
            .filter(|s| !s.is_null()),
        armament.sections(),
    )
    .with_context(|| format!("Native Reach import {}", entry.slug))?;
    armament.place(&mut graph, &scene, &mut projectile_import, &entry)?;
    if let Some(audio) = source_audio {
        graph["source_audio"] = json!({
            "decoded_samples":audio["samples"].as_array().map_or(0, Vec::len),
            "unavailable":audio["unavailable"],
            "playback":"Decoded recordings are source evidence. The audio routes describe native playback coverage."
        });
    }
    inspection::build(
        &cache,
        pages,
        native,
        &scene,
        &entry,
        &graph_root,
        &mut graph,
    )?;
    reload(
        native,
        hash(&entry.recipe["donor"], "item_hash")?,
        animation.as_ref(),
        &mut gameplay,
    )?;
    graph["source_gameplay"] = gameplay;
    graph["source_scale"] =
        json!({"length":entry.scale,"mass":1.,"inertia":entry.scale * entry.scale});
    graph["import_settings"] = settings::record(&entry)?;
    graph["source_hud"] = hud;
    if let Some(animation) = animation {
        graph["animation"] = animation;
    }
    native_routes(
        &sources,
        &scene,
        pages,
        native,
        &mut graph,
        &mut recipe,
        projectile_import,
    )?;
    publish(batch, &entry, &scene, &root, &graph, recipe)
}

/// Fills `entry`'s character profile from the automatic defaults when it has none. A saved
/// weapon profile keeps its choices and gains only what the defaults add.
fn character_defaults(
    cache: &cache::Cache,
    scene: &Scene,
    native: &mut Reader,
    entry: &mut ImportRequest,
) -> Result<()> {
    if entry.character.is_none()
        && (scene.object.tag.group != "vehi" || entry.driver_pose.is_none())
    {
        entry.character = character::defaults(
            cache,
            scene,
            native,
            hash(&entry.recipe["donor"], "item_hash")?,
            entry.runtime_entity,
        )?;
    } else if scene.object.tag.group == "weap"
        && let Some(saved) = &mut entry.character
        && let Ok(Some(defaults)) = character::defaults(
            cache,
            scene,
            native,
            hash(&entry.recipe["donor"], "item_hash")?,
            entry.runtime_entity,
        )
    {
        complete_character(saved, defaults)?;
    } else if scene.object.tag.group == "vehi"
        && let Some(saved) = &mut entry.character
        && let Ok(Some(defaults)) = character::defaults(
            cache,
            scene,
            native,
            hash(&entry.recipe["donor"], "item_hash")?,
            entry.runtime_entity,
        )
    {
        // A vehicle saved before its seats rode keeps its driver choices and gains the rider roles
        // of seats added since. Without them every added seat kept the stock turret pose.
        for passenger in defaults.passengers {
            if !saved
                .passengers
                .iter()
                .any(|old| old.role == passenger.role)
            {
                saved.passengers.push(passenger);
            }
        }
    }
    Ok(())
}

/// Saved choices stay. A native state, stance or movement the automatic defaults gained later is
/// filled in, so a saved import does not keep the donor's motion for something it never chose.
fn complete_character(saved: &mut character::Options, defaults: character::Options) -> Result<()> {
    for (state, action) in defaults.actions {
        saved.actions.entry(state).or_insert(action);
    }
    for (stance, action) in defaults.stance {
        saved.stance.entry(stance).or_insert(action);
    }
    for (stance, directions) in defaults.locomotion {
        let saved = saved.locomotion.entry(stance).or_default();
        for (direction, candidates) in directions {
            let saved = saved.entry(direction).or_default();
            for candidate in candidates {
                let value = serde_json::to_value(&candidate)?;
                if !saved
                    .iter()
                    .any(|old| serde_json::to_value(old).ok().as_ref() == Some(&value))
                {
                    saved.push(candidate);
                }
            }
        }
    }
    Ok(())
}

/// Discovers the source audio of a weapon or vehicle that names none, and the banks of a saved
/// import whose recorded bank is empty.
fn audio_defaults(
    maps: &Path,
    behavior: &Value,
    scene: &Scene,
    entry: &mut ImportRequest,
) -> Result<()> {
    if entry.source.audio.is_none() && matches!(scene.object.tag.group.as_str(), "weap" | "vehi") {
        entry.source.audio = crate::halo_reach::audio::defaults(maps, behavior)?;
    }
    if let Some(audio) = &mut entry.source.audio
        && audio.bank.as_os_str().is_empty()
    {
        let discovered = crate::halo_reach::audio::defaults(maps, behavior)?
            .context("Previously imported source audio is no longer available")?;
        audio.bank = discovered.bank;
        audio.additional_banks = discovered.additional_banks;
        audio.decoder = discovered.decoder;
    }
    Ok(())
}

/// Fills `entry`'s first-person actions from the donor's defaults when it chose none. A melee
/// weapon has both: its first-person actions and the character's strikes. The build composes
/// the two profiles on the weapon's attachment. A heat lock that the firing receipts made the
/// native magazine, with its vent as the reload, selects the source overheat motion for reloads.
fn first_person_defaults(
    cache: &cache::Cache,
    scene: &Scene,
    native: &mut Reader,
    entry: &mut ImportRequest,
    source: &Path,
    gameplay: &Value,
) -> Result<()> {
    use crate::halo_reach::animation::first_person::defaults;
    let vents =
        gameplay["heat"]["status"] == "partial" && gameplay["heat"]["vent_seconds"].is_number();
    let donor = hash(&entry.recipe["donor"], "item_hash")?;
    if let Some(saved) = &mut entry.first_person {
        // Saved choices stay. A heat-locked weapon saved before its reloads took the source vent
        // has neither reload, and gains the ones the defaults now select, so it does not keep the
        // donor's reload motion for a choice it never made.
        if vents
            && !["reload_full", "reload_empty"]
                .iter()
                .any(|state| saved.actions.contains_key(*state))
            && let Ok(Some(selected)) = defaults(cache, scene, native, donor, source, true)
            && selected.arms_model == saved.arms_model
        {
            for state in ["reload_full", "reload_empty"] {
                if let Some(action) = selected.actions.get(state) {
                    saved.actions.insert(state.to_owned(), action.clone());
                }
            }
        }
        return Ok(());
    }
    entry.first_person = defaults(cache, scene, native, donor, source, vents)?;
    if let Some(options) = &mut entry.first_person {
        options.handling_audio = entry.source.audio.is_some();
    }
    Ok(())
}

/// The source actions fitted to the donor's first-person arms, when the entry selects any.
fn first_person_actions(
    cache: &cache::Cache,
    scene: &Scene,
    native: &mut Reader,
    entry: &ImportRequest,
    graph: &Path,
) -> Result<Option<Value>> {
    entry
        .first_person
        .as_ref()
        .map(|options| {
            crate::halo_reach::animation::first_person::prepare(
                cache,
                scene,
                native,
                hash(&entry.recipe["donor"], "item_hash")?,
                options,
                &entry.bone_aliases,
                graph,
                entry.source.audio.is_some(),
            )
        })
        .transpose()
}

/// Adds the native routes the source needs beyond its build, in the order they share state.
fn native_routes(
    sources: &Sources<'_>,
    scene: &Scene,
    pages: &mut resource::Pages,
    native: &mut Reader,
    graph: &mut Value,
    recipe: &mut Value,
    mut projectile_import: Value,
) -> Result<()> {
    let entry = sources.entry;
    character_profile(sources, pages, native, graph, recipe)?;
    fired_projectile(
        sources,
        pages,
        native,
        graph,
        recipe,
        &mut projectile_import,
    )?;
    graph["projectile_import"] = projectile_import;
    // A charged pair's charged barrel fires its own converted projectile through a private
    // copy of the donor's full-charge pattern override, written before the melee route
    // shares the intrinsic's private variant.
    if let Some(release) = crate::halo_reach::firing::release::prepare(
        sources.cache,
        pages,
        native,
        sources.behavior,
        sources.projectiles,
        entry,
        graph,
        recipe,
        sources.graph,
    )? {
        graph["source_gameplay"]["charged_release"] = release;
    }
    native_audio(sources, native, graph)?;
    weapon_effects(sources, scene, pages, native, graph)?;
    if entry
        .first_person
        .as_ref()
        .is_some_and(|options| !options.melee.is_empty())
    {
        let melee = crate::halo_reach::melee::prepare(
            native,
            sources.behavior,
            graph,
            recipe,
            sources.graph,
            entry.source.audio.is_some(),
        )?;
        graph["melee"] = melee;
    }
    // A heat lock that stands in for the magazine cools through a private refill on the same
    // intrinsic.
    let cooling = graph["source_gameplay"]["heat"]["cooling"].clone();
    if let Some(report) = crate::halo_reach::firing::heat::program(native, recipe, &cooling)? {
        graph["source_gameplay"]["heat"]["cooling"]["program"] = report;
    }
    handling_audio(sources, native, graph)?;
    if let Some(sword) = crate::halo_reach::melee::slam::prepare(
        native,
        scene,
        sources.behavior,
        recipe,
        sources.graph,
        entry.source.audio.is_some(),
    )? {
        graph["sword"] = sword;
    }
    // After the melee route and the charged release, which hold the tracking count.
    crate::halo_reach::firing::lock::prepare(native, sources.behavior, graph, recipe)?;
    report::behavior(graph);
    Ok(())
}

/// The character's body actions on the private weapon or driver profile.
fn character_profile(
    sources: &Sources<'_>,
    pages: &mut resource::Pages,
    native: &mut Reader,
    graph: &mut Value,
    recipe: &mut Value,
) -> Result<()> {
    if sources.entry.character.is_none() {
        return Ok(());
    }
    let profile = character::prepare(
        sources.cache,
        pages,
        native,
        sources.entry,
        graph,
        recipe,
        sources.graph,
    )?;
    if !graph["animation"].is_object() {
        graph["animation"] = json!({});
    }
    graph["animation"]["character"] = profile;
    Ok(())
}

/// The modeled source projectile on the entry's chosen native carrier, which the recipe fires.
/// The projectile's ricochet, release detonation, airstrike and EMP sit beside the weapon's other
/// gameplay receipts.
fn fired_projectile(
    sources: &Sources<'_>,
    pages: &mut resource::Pages,
    native: &mut Reader,
    graph: &mut Value,
    recipe: &mut Value,
    projectile_import: &mut Value,
) -> Result<()> {
    if sources.entry.projectile_entity.is_none() {
        return Ok(());
    }
    graph["projectile"] = crate::halo_reach::projectile::native(
        sources.cache,
        pages,
        native,
        sources.behavior,
        sources.projectiles,
        sources.entry,
        sources.graph,
    )?;
    select_projectile(recipe, &graph["projectile"])?;
    projectile_import["status"] = json!("imported");
    let ricochet = graph["projectile"]["conversion"]["responses"]["ricochet"].clone();
    if graph["source_gameplay"].is_object() && !ricochet.is_null() {
        graph["source_gameplay"]["ricochet"] = ricochet;
    }
    for key in ["remote_detonation", "airstrike", "emp"] {
        let receipt = graph["projectile"]["conversion"][key].clone();
        if graph["source_gameplay"].is_object() && !receipt.is_null() {
            graph["source_gameplay"][key] = receipt;
        }
    }
    Ok(())
}

/// Links the source audio of an item without a vehicle runtime to the donor's native audio.
/// Requested firing audio must link. Other source audio falls back to a source-only report.
fn native_audio(sources: &Sources<'_>, native: &mut Reader, graph: &mut Value) -> Result<()> {
    let entry = sources.entry;
    let Some(audio) = &entry.source.audio else {
        return Ok(());
    };
    if entry.runtime_entity.is_some() {
        return Ok(());
    }
    let template =
        crate::tiger::shadowkeep::extract(native, hash(&entry.recipe["donor"], "item_hash")?)?;
    let requested = audio.firing.is_some() || audio.require_firing;
    graph["audio"] = match crate::halo_reach::audio::native(
        native,
        hash(&template, "item_tag")?,
        sources.behavior,
        &sources.source.join("audio"),
        sources.graph,
        audio,
    ) {
        Ok(audio) => audio,
        Err(error) if requested => {
            return Err(error)
                .with_context(|| format!("Requested firing audio for {}", entry.slug));
        }
        Err(error) => json!({"status":"source_only","reason":format!("{error:#}")}),
    };
    if requested {
        ensure!(
            graph["audio"]["authoring_schema"] == 3,
            "Requested firing audio was not linked for {}: {}",
            entry.slug,
            graph["audio"]
        );
    }
    Ok(())
}

/// A weapon's firing effect built from its source behavior, or the donor's own where it does not
/// translate, and the report of its attachments.
fn weapon_effects(
    sources: &Sources<'_>,
    scene: &Scene,
    pages: &mut resource::Pages,
    native: &mut Reader,
    graph: &mut Value,
) -> Result<()> {
    let entry = sources.entry;
    if scene.object.tag.group != "weap" || entry.runtime_entity.is_some() {
        return Ok(());
    }
    let template =
        crate::tiger::shadowkeep::extract(native, hash(&entry.recipe["donor"], "item_hash")?)?;
    graph["firing_effect"] = firing_effect::build(
        sources.cache,
        pages,
        native,
        sources.behavior,
        scene,
        hash(&template, "item_tag")?,
        sources.graph,
    )
    .unwrap_or_else(|error| json!({"status":"native","reason":format!("{error:#}")}));
    attachment::report(scene, graph);
    Ok(())
}

/// The handling audio of first-person actions: their source cues, or the donor's cues on every
/// clip where the plan switches handling audio off.
fn handling_audio(sources: &Sources<'_>, native: &mut Reader, graph: &mut Value) -> Result<()> {
    let Some(options) = sources
        .entry
        .first_person
        .as_ref()
        .filter(|options| !options.actions.is_empty())
    else {
        return Ok(());
    };
    let handling = if options.handling_audio {
        crate::halo_reach::audio::clips::prepare(native, sources.behavior, graph, sources.graph)
            .with_context(|| format!("Handling audio for {}", sources.entry.slug))?
    } else {
        json!({"status":"source_only","reason":"Handling audio is switched off in the import plan, the donor's cues remain on every clip"})
    };
    graph["handling_audio"] = handling;
    Ok(())
}

/// Writes the entry's graph, its artwork and its recipe, which names the graph's place in the
/// batch output, and returns the entry's report row.
fn publish(
    batch: &Batch<'_>,
    entry: &ImportRequest,
    scene: &Scene,
    root: &Path,
    graph: &Value,
    mut recipe: Value,
) -> Result<Value> {
    let graph_root = root.join("graph");
    write_json(&root.join("graph/asset-graph.json"), graph)?;
    crate::graph::check_particle_bindings(&graph_root)?;
    crate::graph::record_converter_revision(&graph_root)?;
    crate::artwork::prepare(
        &graph_root,
        batch.native_packages,
        entry.icon.unwrap_or_else(|| {
            if recipe["kind"] == "weapon" && scene.object.gameplay["weapon_class"] == "sword" {
                crate::artwork::Settings::sword()
            } else {
                crate::artwork::Settings::for_recipe(&recipe)
            }
        }),
    )?;
    crate::artwork::apply(&mut recipe, &graph_root)?;
    report::compact(&root.join("graph/asset-graph.json"))?;
    let item = hash(&entry.recipe["identity"], "item_hash")?;
    let mut reference = crate::GraphReference::new(&graph_root, item)?;
    reference.directory = batch.output.join(&entry.slug).join("graph");
    if recipe.get("overrides").is_none() {
        recipe["overrides"] = json!({});
    }
    recipe["overrides"]["imported_graph"] = serde_json::to_value(reference)?;
    write_json(&root.join("weapon.parhelion.json"), &recipe)?;
    Ok(
        json!({"slug":entry.slug,"source":entry.source,"recipe":batch.output.join(&entry.slug).join("weapon.parhelion.json"),"vertices":graph["vertices"],"triangles":graph["triangles"],"native_skinning":graph["native_skinning"],"retained_native_behavior":graph["retained_native_behavior"],"limits":graph["limits"]}),
    )
}
