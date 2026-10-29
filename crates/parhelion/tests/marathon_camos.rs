//! Configured Marathon camo imports: each camo converts as an ornament of its weapon, and the
//! staged packages offer it in an unconditional Default Ornament socket with its own model. A
//! weapon whose camos an earlier case converted on the same donor copies them, and still offers
//! each one as its own ornament. Collections lists a weapon's camos on its weapon type's ornament
//! page, under Exotic for an Exotic weapon and under Weapons otherwise.
#![cfg(feature = "d2-model-importer")]

use anyhow::{Context, Result, ensure};
use parhelion::{BatchBuildRequest, BatchBuildSnapshot, WeaponRecipe};
use parhelion_import::{marathon, tiger::payload::Payload, tiger::reader::Reader};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::PathBuf,
    sync::Arc,
    time::Instant,
};

const RESET: u32 = 0xAEBAE371;

fn configured(name: &str) -> Result<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("Set {name}"))
}

fn word(value: &Value, what: &str) -> Result<u32> {
    let value = match value.as_str() {
        Some(text) => u64::from_str_radix(text.trim_start_matches("0x"), 16)?,
        None => value.as_u64().with_context(|| format!("{what} missing"))?,
    };
    Ok(u32::try_from(value)?)
}

/// The staged investment tables the readback follows.
struct Staged {
    reader: Reader,
    items: Arc<Payload>,
    rows: BTreeMap<u32, usize>,
    root: Arc<Payload>,
    metadata: Arc<Payload>,
    parents: BTreeMap<u32, u32>,
}

impl Staged {
    fn definition(&mut self, hash: u32) -> Result<Arc<Payload>> {
        let row = *self
            .rows
            .get(&hash)
            .with_context(|| format!("Emitted item {hash:08X} absent"))?;
        let tag = self.items.u32(row + 16)?;
        self.reader.tag(tag, Some(0x80807BEA))
    }

    fn hash_at(&self, index: u16) -> Result<u32> {
        let rows = self.items.array(8, 24, Some(0x80807BE8))?;
        self.items
            .u32(*rows.get(usize::from(index)).context("Item index")?)
    }

    /// Every model the item's selected art places, with its palette size.
    fn models(&mut self, definition: &Payload) -> Result<Vec<(u32, u32)>> {
        let art_rows = self.metadata.array(8, 32, Some(0x80805DFB))?;
        let mut models = Vec::new();
        for selection in definition.array(definition.pointer(0x88)?, 4, Some(0x808077B5))? {
            let art = *art_rows
                .get(usize::from(definition.u16(selection + 2)?))
                .context("Art row")?;
            for region in self.metadata.array(art + 16, 8, None)? {
                let slot = self.metadata.pointer(region)?;
                for key in self.metadata.array(slot + 8, 4, None)? {
                    let key = self.metadata.u32(key)?;
                    if [0, u32::MAX, 0x811C9DC5].contains(&key) {
                        continue;
                    }
                    let parent = *self.parents.get(&key).context("Art assignment")?;
                    let parent = self.reader.tag(parent, None)?;
                    let entity = self.reader.tag(parent.u32(16)?, Some(0x80809C0F))?;
                    for row in entity.array(16, 12, Some(0x80809C04))? {
                        let owner = self.reader.tag(entity.u32(row)?, Some(0x80809C36))?;
                        let data = owner.pointer(24)?;
                        if owner.u32(data - 4)? != 0x808072BD {
                            continue;
                        }
                        let tag = owner.u32(data + 0x1DC)?;
                        let model = self.reader.tag(tag, Some(0x808073A5))?;
                        models.push((tag, model.u32(0x40)?));
                    }
                }
            }
        }
        Ok(models)
    }

    /// The plug categories each socket type whitelists.
    fn whitelist(&mut self, socket_type: u16) -> Result<Vec<u32>> {
        let table = self.reader.tag(self.root.u32(8 + 94 * 16)?, None)?;
        let row = *table
            .array(8, 96, Some(0x80807ABB))?
            .get(usize::from(socket_type))
            .context("Socket type row")?;
        table
            .array(row + 0x28, 8, Some(0x80807AC0))?
            .into_iter()
            .map(|at| table.u32(at))
            .collect()
    }

    /// The Collections path of an item's single collectible, as presentation node hashes.
    fn collection_path(&mut self, hash: u32) -> Result<Vec<u32>> {
        let index = self
            .items
            .array(8, 24, Some(0x80807BE8))?
            .into_iter()
            .position(|row| self.items.u32(row).ok() == Some(hash))
            .context("Item index")?;
        let collectibles = self.reader.tag(self.root.u32(8 + 19 * 16)?, None)?;
        let nodes = self.reader.tag(self.root.u32(8 + 63 * 16)?, None)?;
        let node_rows = nodes.array(8, 0xA8, Some(0x80803056))?;
        let found = collectibles
            .array(8, 0xB8, Some(0x80803475))?
            .into_iter()
            .filter(|&row| collectibles.u16(row + 0x2C).ok() == Some(index as u16))
            .collect::<Vec<_>>();
        ensure!(found.len() == 1, "Camo {hash:08X} needs one collectible");
        let parents = |p: &Payload, row: usize| -> Result<Vec<u16>> {
            p.array(row + 0x18, 2, Some(0x80803962))?
                .into_iter()
                .map(|at| p.u16(at))
                .collect()
        };
        let mut path = Vec::new();
        let mut current = parents(&collectibles, found[0])?;
        while let [node] = current[..] {
            let row = *node_rows.get(usize::from(node)).context("Node row")?;
            path.push(nodes.u32(row + 0x28)?);
            current = parents(&nodes, row)?;
        }
        Ok(path)
    }
}

#[test]
#[ignore = "Requires PARHELION_MARATHON_PACKAGES, SUNDIAL_STOCK_PACKAGES, PARHELION_MARATHON_CAMO_CASES and SUNDIAL_TEST_ARTIFACTS without a marathon-camos folder"]
#[allow(clippy::cognitive_complexity)]
fn marathon_camos_stage_as_unconditional_ornaments_with_their_own_models() -> Result<()> {
    let source = configured("PARHELION_MARATHON_PACKAGES")?.canonicalize()?;
    let native = configured("SUNDIAL_STOCK_PACKAGES")?.canonicalize()?;
    let output = parhelion_import::tiger::reader::outside(
        &configured("SUNDIAL_TEST_ARTIFACTS")?.join("marathon-camos"),
        source.parent().context("Source root")?,
    )?;
    let output =
        parhelion_import::tiger::reader::outside(&output, native.parent().context("Native root")?)?;
    ensure!(!output.exists(), "Use a fresh camo artifact directory");
    let cases: Vec<Value> =
        serde_json::from_slice(&fs::read(configured("PARHELION_MARATHON_CAMO_CASES")?)?)?;
    ensure!(!cases.is_empty(), "Required camo corpus is empty");
    fs::create_dir_all(&output)?;
    // The run's own store, as the app opens one in its models folder.
    parhelion_import::store::open(&output);
    let mut timings = serde_json::Map::new();
    let started = Instant::now();
    let listing = marathon::browse::browse(&source, &output.join("browse"), |_, _| {})?;
    timings.insert(
        "listing_seconds".into(),
        json!(started.elapsed().as_secs_f64()),
    );
    let mut recipes = Vec::new();
    let mut imported = Vec::new();
    // The camos earlier cases converted, by donor and cosmetic entity.
    let mut converted = BTreeSet::new();
    for (index, case) in cases.iter().enumerate() {
        let item = word(&case["source_item"], "Source item")?;
        let entry = listing
            .entries
            .iter()
            .find(|entry| entry.item == item)
            .context("Required source weapon is absent")?;
        // The listing's camos must match the independent package survey, in node order.
        let expected = case["expected_camos"]
            .as_array()
            .context("Expected camos")?
            .iter()
            .map(|camo| {
                Ok((
                    word(&camo["item"], "Camo item")?,
                    camo["name"].as_str().context("Camo name")?.to_owned(),
                    word(&camo["cosmetic"], "Camo cosmetic")?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(!expected.is_empty(), "A case needs camos to check");
        let listed = entry
            .camos
            .iter()
            .map(|camo| (camo.item, camo.name.clone(), camo.cosmetic))
            .collect::<Vec<_>>();
        ensure!(
            listed == expected,
            "{} lists camos {listed:?}, the survey {expected:?}",
            entry.name
        );
        ensure!(
            entry.cosmetic == Some(word(&case["default_cosmetic"], "Default cosmetic")?),
            "{} lost its own look",
            entry.name
        );
        let started = Instant::now();
        let path = marathon::browse::import(
            entry,
            &[(
                word(&case["donor"], "Donor")?,
                case["donor_name"]
                    .as_str()
                    .context("Donor name")?
                    .to_owned(),
            )],
            marathon::browse::Options {
                source_animation: false,
                ornaments: true,
            },
            &source,
            &native,
            &output.join(format!("import-{index}")),
            &mut |_| {},
        )?;
        timings.insert(
            format!("import_{index}_seconds"),
            json!(started.elapsed().as_secs_f64()),
        );
        let recipe = WeaponRecipe::load_json(&path)?;
        let hash = recipe.identity.item_hash.parse_u32()?;
        let graph = recipe
            .overrides
            .imported_graph
            .as_ref()
            .context("Imported graph")?;
        graph.validate(hash)?;
        let camo_report: Value =
            serde_json::from_slice(&fs::read(path.with_file_name("ornaments.json"))?)?;
        ensure!(
            camo_report["omitted"].as_array().is_none_or(Vec::is_empty),
            "{} left camos out: {}",
            entry.name,
            camo_report["omitted"]
        );
        let donor = word(&case["donor"], "Donor")?;
        let shared = expected
            .iter()
            .filter(|(_, _, cosmetic)| converted.contains(&(donor, *cosmetic)))
            .count();
        ensure!(
            camo_report["reused"].as_u64() == Some(shared as u64),
            "{} copied {} camos, {shared} were converted by an earlier case",
            entry.name,
            camo_report["reused"]
        );
        converted.extend(expected.iter().map(|(_, _, cosmetic)| (donor, *cosmetic)));
        ensure!(
            graph.attachments.len() == expected.len(),
            "{} carries {} camo graphs for {} camos",
            entry.name,
            graph.attachments.len(),
            expected.len()
        );
        let value = serde_json::to_value(&recipe)?;
        let columns = value["overrides"]["socket_columns"]
            .as_array()
            .context("Socket columns")?;
        let lanes = columns
            .iter()
            .enumerate()
            .filter(|(_, column)| column["choices"][0].as_str() == Some("0xAEBAE371"))
            .collect::<Vec<_>>();
        ensure!(
            lanes.len() == 1,
            "One ornament column starts with Default Ornament"
        );
        let (lane, column) = lanes[0];
        let choices = column["choices"]
            .as_array()
            .context("Ornament choices")?
            .iter()
            .map(|choice| word(choice, "Ornament choice"))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            choices.len() == expected.len() + 1
                && choices.iter().collect::<BTreeSet<_>>().len() == choices.len(),
            "Ornament choices are not one distinct layout plug per camo"
        );
        ensure!(
            column["choice_conditions"]
                .as_array()
                .is_none_or(|conditions| conditions
                    .iter()
                    .all(|c| c.as_array().is_none_or(Vec::is_empty))),
            "Ornament choices carry unlock conditions"
        );
        let mut plugs = BTreeMap::new();
        for attachment in &graph.attachments {
            let child: Value =
                serde_json::from_slice(&fs::read(attachment.directory.join("asset-graph.json"))?)?;
            let link = &child["ornament"];
            ensure!(
                word(&link["target_weapon"], "Camo owner")? == hash
                    && link["target_socket"].as_u64() == Some(lane as u64),
                "Camo graph belongs to another weapon or socket"
            );
            let choice = usize::try_from(link["choice_index"].as_u64().context("Camo choice")?)?;
            let (_, name, cosmetic) = expected.get(choice - 1).context("Camo choice range")?;
            ensure!(
                link["name"] == json!(name),
                "Camo choice {choice} is not {name}"
            );
            ensure!(
                word(&link["native_plug"], "Layout plug")? == choices[choice],
                "Camo choice {choice} names another layout plug"
            );
            // The camo converts its own cosmetic entity, never the weapon's default look.
            let entities = child["source_models"]
                .as_array()
                .context("Camo source models")?
                .iter()
                .map(|model| word(&model["entity"], "Camo model entity"))
                .collect::<Result<BTreeSet<_>>>()?;
            ensure!(
                entities.contains(cosmetic)
                    && !entities.contains(&word(&case["default_cosmetic"], "Default cosmetic")?),
                "Camo {name} does not wear its own cosmetic entity"
            );
            ensure!(
                child["ornament_icon_png"].is_string(),
                "Camo {name} has no icon"
            );
            plugs.insert(
                choice,
                (
                    word(&child["item_hash"], "Camo plug")?,
                    name.clone(),
                    child["vertices"].clone(),
                ),
            );
        }
        let variants = value["overrides"]["socket_plug_variants"]
            .as_array()
            .context("Plug variants")?;
        for (choice, (_, name, _)) in &plugs {
            ensure!(
                variants
                    .iter()
                    .any(|variant| variant["socket_index"] == json!(lane)
                        && variant["choice_index"] == json!(choice)
                        && variant["name"] == json!(name)
                        && variant["sandbox_perks"]
                            .as_array()
                            .is_some_and(Vec::is_empty)),
                "Camo {name} has no effect-free private plug"
            );
        }
        imported.push((hash, lane, column["socket_type"].clone(), plugs));
        recipes.push(recipe);
    }
    // A copied camo is bound to its own weapon's plug, never the plug it was copied from.
    let plugs = imported
        .iter()
        .flat_map(|(_, _, _, plugs)| plugs.values().map(|(plug, _, _)| *plug))
        .collect::<Vec<_>>();
    ensure!(
        plugs.iter().collect::<BTreeSet<_>>().len() == plugs.len(),
        "Two camos share a private plug"
    );
    let started = Instant::now();
    let snapshot = BatchBuildSnapshot::new(BatchBuildRequest {
        package_directory: native.clone(),
        staging_root: output.join("build"),
        ignore_installed_authored_overlays: true,
        maps: Vec::new(),
        recipes: recipes.clone(),
    })
    .map_err(anyhow::Error::msg)?;
    let built =
        parhelion::build_and_stage_snapshot_with_progress(&snapshot, |p| println!("{:?}", p.phase))
            .map_err(anyhow::Error::msg)?;
    timings.insert(
        "build_seconds".into(),
        json!(started.elapsed().as_secs_f64()),
    );
    let view = output.join("view");
    let packages = view.join("packages");
    fs::create_dir_all(&packages)?;
    fs::create_dir_all(view.join("bin/x64"))?;
    let manifest: Value = serde_json::from_slice(&fs::read(
        built.run_directory.join("parhelion-manifest.json"),
    )?)?;
    let ignored = manifest["ignored_authored_files"]
        .as_array()
        .context("Ignored authored files")?
        .iter()
        .map(|row| row.as_str().context("Ignored authored filename"))
        .collect::<Result<BTreeSet<_>>>()?;
    for entry in fs::read_dir(&native)? {
        let entry = entry?;
        if ignored.contains(entry.file_name().to_string_lossy().as_ref())
            || built
                .artifacts
                .iter()
                .any(|artifact| entry.file_name().to_string_lossy() == artifact.file_name)
        {
            continue;
        }
        if entry.path().extension().is_some_and(|e| e == "pkg") {
            fs::hard_link(entry.path(), packages.join(entry.file_name()))?;
        }
    }
    fs::copy(
        native
            .parent()
            .context("Native root")?
            .join("bin/x64/oo2core_3_win64.dll"),
        view.join("bin/x64/oo2core_3_win64.dll"),
    )?;
    for artifact in &built.artifacts {
        let destination = packages.join(&artifact.file_name);
        ensure!(!destination.exists(), "Never overwrite a stock link");
        fs::copy(built.run_directory.join(&artifact.file_name), destination)?;
    }
    let reader = Reader::new(&packages, &output.join("readback"), false)?;
    let names = reader
        .manager
        .lookup
        .named_tags
        .iter()
        .map(|e| (e.name.clone(), e.hash.0))
        .collect::<BTreeMap<_, _>>();
    let mut reader = reader;
    let globals = reader.tag(*names.get("investment_globals").context("Globals")?, None)?;
    let root = reader.tag(globals.u32(16)?, None)?;
    let items = reader.tag(root.u32(8 + 48 * 16)?, None)?;
    let rows = items
        .array(8, 24, Some(0x80807BE8))?
        .into_iter()
        .map(|row| Ok((items.u32(row)?, row)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let metadata = reader.tag(globals.u32(16 + 66 * 16)?, None)?;
    let assets = reader.tag(*names.get("investment_assets").context("Assets")?, None)?;
    let assignments = reader.tag(assets.u32(0x20)?, None)?;
    let parents = assignments
        .array(8, 8, None)?
        .into_iter()
        .map(|at| Ok((assignments.u32(at)?, assignments.u32(at + 4)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut staged = Staged {
        reader,
        items,
        rows,
        root,
        metadata,
        parents,
    };
    let mut evidence = Vec::new();
    for ((hash, lane, socket_type, plugs), case) in imported.iter().zip(&cases) {
        let weapon = staged.definition(*hash)?;
        let base_models = staged.models(&weapon)?;
        ensure!(!base_models.is_empty(), "Weapon {hash:08X} places no model");
        let sockets = weapon.array(weapon.pointer(0x68)?, 0x50, Some(0x808077C4))?;
        let row = *sockets.get(*lane).context("Ornament socket absent")?;
        ensure!(
            u64::from(weapon.u16(row)?) == socket_type.as_u64().context("Socket type")?,
            "Ornament socket type changed"
        );
        ensure!(
            staged.hash_at(weapon.u16(row + 2)?)? == RESET,
            "Ornament socket does not start with Default Ornament"
        );
        let whitelist = staged.whitelist(weapon.u16(row)?)?;
        let members = weapon.array(row + 0x40, 0x20, Some(0x80802E03))?;
        ensure!(
            members.len() == plugs.len() + 1,
            "Ornament socket offers {} choices for {} camos",
            members.len(),
            plugs.len()
        );
        // As stock ornaments are, an Exotic's camos are under Exotic and Weapon Ornaments, and an
        // ordinary weapon's under Weapons and Weapon Ornaments, all on one weapon-type page.
        let exotic =
            weapon.u8(sundial::package_authoring::investment_schema::ITEM_RARITY_OFFSET)? == 5;
        let ornaments: &[u32] = if exotic {
            &[0x47D9E328, 0x3FB0E331]
        } else {
            &[0xBC18ECA7, 0x5B219F74]
        };
        let mut pages = BTreeSet::new();
        let mut camos = Vec::new();
        for (choice, &member) in members.iter().enumerate() {
            ensure!(
                weapon.u64(member + 8)? == 0,
                "Ornament choice {choice} has an unlock condition"
            );
            let plug = staged.hash_at(weapon.u16(member)?)?;
            if choice == 0 {
                ensure!(
                    plug == RESET,
                    "First ornament choice is not Default Ornament"
                );
                continue;
            }
            let (expected, name, vertices) = plugs.get(&choice).context("Camo choice")?;
            ensure!(
                plug == *expected,
                "Choice {choice} offers {plug:08X}, not {name}"
            );
            let definition = staged.definition(plug)?;
            let block = definition.pointer(0x40)?;
            ensure!(
                definition.u32(block - 4)? == 0x808077E3,
                "Camo {name} has no plug block"
            );
            let category = definition.u32(block)?;
            ensure!(
                whitelist.contains(&category),
                "Camo {name} category {category:08X} is outside its socket type"
            );
            ensure!(
                definition.u64(block + 0x40)? == 0 && definition.u64(block + 0x50)? == 0,
                "Camo {name} keeps unlock requirements"
            );
            // The survey maps each camo's Marathon tier to the Destiny rarity it should carry.
            let tier = case["expected_camos"][choice - 1]["rarity"]
                .as_str()
                .context("Expected rarity")?;
            let rarity =
                ["common", "uncommon", "rare", "legendary", "exotic"]
                    .get(
                        usize::from(definition.u8(
                            sundial::package_authoring::investment_schema::ITEM_RARITY_OFFSET,
                        )?)
                        .wrapping_sub(1),
                    )
                    .copied();
            ensure!(
                rarity == Some(tier),
                "Camo {name} is {rarity:?}, its Marathon tier maps to {tier}"
            );
            let models = staged.models(&definition)?;
            ensure!(
                !models.is_empty()
                    && models
                        .iter()
                        .all(|model| !base_models.iter().any(|base| base.0 == model.0)),
                "Camo {name} does not place its own model"
            );
            // The weapon places its body and its sight, each with its own palette, and the camo
            // places its own of both.
            let palettes = |models: &[(u32, u32)]| {
                let mut palettes = models.iter().map(|model| model.1).collect::<Vec<_>>();
                palettes.sort_unstable();
                palettes
            };
            ensure!(
                palettes(&models) == palettes(&base_models),
                "Camo {name} skins to palettes {:?}, its weapon to {:?}",
                palettes(&models),
                palettes(&base_models)
            );
            let path = staged.collection_path(plug)?;
            ensure!(
                path.get(1..3) == Some(ornaments),
                "Camo {name} is on {path:08X?}, not under {ornaments:08X?}"
            );
            pages.insert(path[0]);
            camos.push(json!({"choice":choice,"name":name,"plug":format!("{plug:08X}"),"rarity":tier,
                "category":format!("{category:08X}"),"models":models.iter().map(|m| format!("{:08X}", m.0)).collect::<Vec<_>>(),
                "palettes":palettes(&models),"graph_vertices":vertices,"collections":path.iter().map(|h| format!("{h:08X}")).collect::<Vec<_>>()}));
        }
        ensure!(
            pages.len() <= 1,
            "Weapon {hash:08X}'s camos are on {} pages",
            pages.len()
        );
        evidence.push(json!({"weapon":case["weapon"],"item":format!("{hash:08X}"),"lane":lane,"exotic":exotic,
            "socket_type":socket_type,"whitelist":whitelist.iter().map(|h| format!("{h:08X}")).collect::<Vec<_>>(),
            "base_models":base_models.iter().map(|m| format!("{:08X}", m.0)).collect::<Vec<_>>(),"camos":camos}));
    }
    staged.reader.finish()?;
    fs::write(
        output.join("verified-camos.json"),
        serde_json::to_vec_pretty(&json!({"cases":cases,"items":evidence,"timings":timings,
            "staged_directory":built.run_directory,"gameplay_verified":false}))?,
    )?;
    Ok(())
}
