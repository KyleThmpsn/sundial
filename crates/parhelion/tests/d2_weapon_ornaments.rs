//! Configured modern weapon ornament imports for stock weapons whose ornament socket offers a
//! shared plug set: each converts on its stock weapon into a mod, which joins that weapon's set
//! without a condition and places a model of its own on the weapon's art. The mod takes its
//! source's rarity, shows its artwork under the build's watermark and has a collectible of its
//! own on its stock ornament's Collections page. The stock weapon's own definition stays as it was.
//! `PARHELION_ORNAMENT_RUNTIMES` names runtime DLLs to build for, each in a temporary installation
//! over the clean packages, so Dawn, Sunrise before 0.6 and the Sunrise 0.6 mod folder are each
//! read back. Builds for one brand must draw one watermark, and each brand its own.
#![cfg(feature = "d2-model-importer")]

use anyhow::{Context, Result, anyhow, ensure};
use parhelion::{BatchBuildRequest, BatchBuildSnapshot, BuildReport, ItemKind, WeaponRecipe};
use parhelion_import::{
    d2_mot::service,
    tiger::{payload::Payload, reader::Reader},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use sundial::{investment::InvestmentCatalog, package_authoring::RuntimeBrand};
use tiger_pkg::{Package, PackageD2PreBL, manager::PackagePath};

/// Default Ornament, which every stock weapon ornament socket offers first.
const RESET: u32 = 0xAEBAE371;

fn configured(name: &str) -> Result<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("Set {name}"))
}

/// The staged investment tables the readback follows.
struct Staged {
    reader: Reader,
    items: Arc<Payload>,
    rows: BTreeMap<u32, usize>,
    root: Arc<Payload>,
    metadata: Arc<Payload>,
    strings: Arc<Payload>,
    icons: Arc<Payload>,
    parents: BTreeMap<u32, u32>,
}

/// A collectible row: its position, hash and presentation parents.
#[derive(Clone)]
struct Collectible {
    index: usize,
    hash: u32,
    parents: Vec<u16>,
}

impl Staged {
    fn index(&self, hash: u32) -> Result<usize> {
        let row = *self
            .rows
            .get(&hash)
            .with_context(|| format!("Item {hash:08X} absent"))?;
        let rows = self.items.array(8, 24, Some(0x80807BE8))?;
        rows.iter()
            .position(|&at| at == row)
            .context("Item row outside its table")
    }

    fn definition_tag(&self, hash: u32) -> Result<u32> {
        let row = *self
            .rows
            .get(&hash)
            .with_context(|| format!("Item {hash:08X} absent"))?;
        self.items.u32(row + 16)
    }

    fn definition(&mut self, hash: u32) -> Result<Arc<Payload>> {
        let tag = self.definition_tag(hash)?;
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
                    let Some(&parent) = self.parents.get(&key) else {
                        continue;
                    };
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

    /// The shared set a weapon's ornament socket offers, the socket whose default is Default
    /// Ornament.
    fn ornament_set(&mut self, weapon: u32) -> Result<u16> {
        let definition = self.definition(weapon)?;
        for row in definition.array(definition.pointer(0x68)?, 0x50, Some(0x808077C4))? {
            let initial = definition.u16(row + 2)?;
            let embedded = if definition.u64(row + 0x40)? == 0 {
                Vec::new()
            } else {
                definition.array(row + 0x40, 0x20, Some(0x80802E03))?
            };
            let offers_reset = (initial != u16::MAX && self.hash_at(initial)? == RESET)
                || embedded
                    .iter()
                    .map(|&at| self.hash_at(definition.u16(at)?))
                    .collect::<Result<Vec<_>>>()?
                    .contains(&RESET);
            let set = definition.u16(row + 0x0C)?;
            if offers_reset && set != u16::MAX {
                return Ok(set);
            }
        }
        Err(anyhow!(
            "Weapon {weapon:08X} has no ornament socket with a shared set"
        ))
    }

    /// The members of a shared plug set: item index and condition count.
    fn set_members(&mut self, set: u16) -> Result<Vec<(u16, u64)>> {
        let table = self.reader.tag(self.root.u32(8 + 51 * 16)?, None)?;
        let rows = table.array(8, 0x18, Some(0x80802DFF))?;
        let row = *rows.get(usize::from(set)).context("Plug set row")?;
        table
            .array(row + 8, 0x20, Some(0x80802E03))?
            .into_iter()
            .map(|member| Ok((table.u16(member)?, table.u64(member + 8)?)))
            .collect()
    }

    /// Every collectible of an item index.
    fn collectibles(&mut self, item: u16) -> Result<Vec<Collectible>> {
        let table = self.reader.tag(self.root.u32(8 + 19 * 16)?, None)?;
        let mut found = Vec::new();
        for (index, row) in table
            .array(8, 0xB8, Some(0x80803475))?
            .into_iter()
            .enumerate()
        {
            if table.u16(row + 0x2C)? != item {
                continue;
            }
            let parents = table
                .array(row + 0x18, 2, Some(0x80803962))?
                .into_iter()
                .map(|at| table.u16(at))
                .collect::<Result<Vec<_>>>()?;
            found.push(Collectible {
                index,
                hash: table.u32(row + 0x28)?,
                parents,
            });
        }
        Ok(found)
    }

    /// A presentation node's hash, parents and collectible children.
    fn node(&mut self, index: u16) -> Result<(u32, Vec<u16>, Vec<u16>)> {
        let table = self.reader.tag(self.root.u32(8 + 63 * 16)?, None)?;
        let row = *table
            .array(8, 0xA8, Some(0x80803056))?
            .get(usize::from(index))
            .context("Presentation node row")?;
        let parents = table
            .array(row + 0x18, 2, None)?
            .into_iter()
            .map(|at| table.u16(at))
            .collect::<Result<Vec<_>>>()?;
        let children = table
            .array(row + 0x78, 4, Some(0x80803068))?
            .into_iter()
            .map(|at| table.u16(at))
            .collect::<Result<Vec<_>>>()?;
        Ok((table.u32(row + 0x28)?, parents, children))
    }

    /// The watermark layer an item's icon draws.
    fn watermark(&mut self, hash: u32) -> Result<u32> {
        let index = self.index(hash)?;
        let row = *self
            .strings
            .array(8, 24, Some(0x80805CDF))?
            .get(index)
            .context("Item strings row")?;
        let strings = self.reader.tag(self.strings.u32(row + 16)?, None)?;
        let icon = usize::from(strings.u16(0x80)?);
        let row = *self
            .icons
            .array(8, 0x18, Some(0x80802957))?
            .get(icon)
            .context("Icon row")?;
        let container = self.reader.tag(self.icons.u32(row + 0x10)?, None)?;
        container.u32(0x20)
    }
}

#[test]
#[ignore = "Requires PARHELION_IMPORT_MODERN_PACKAGES, SUNDIAL_STOCK_PACKAGES, SUNDIAL_TEST_ARTIFACTS without a d2-weapon-ornaments folder, and PARHELION_WEAPON_ORNAMENTS"]
#[allow(clippy::cognitive_complexity)]
fn modern_weapon_ornaments_join_their_stock_weapons_shared_sets() -> Result<()> {
    let modern = configured("PARHELION_IMPORT_MODERN_PACKAGES")?.canonicalize()?;
    let native = configured("SUNDIAL_STOCK_PACKAGES")?.canonicalize()?;
    let output = parhelion_import::tiger::reader::outside(
        &configured("SUNDIAL_TEST_ARTIFACTS")?.join("d2-weapon-ornaments"),
        modern.parent().context("Modern root")?,
    )?;
    let output =
        parhelion_import::tiger::reader::outside(&output, native.parent().context("Native root")?)?;
    ensure!(
        !output.exists(),
        "Use a fresh weapon ornament artifact directory"
    );
    // Comma-separated source names, chosen for the mounted package version.
    let names = env::var("PARHELION_WEAPON_ORNAMENTS")?
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    ensure!(!names.is_empty(), "Required weapon ornament list is empty");
    fs::create_dir_all(&output)?;
    let catalog = InvestmentCatalog::load_with_cache_path(
        native.parent().context("Native root")?,
        &output.join("catalog-cache.json"),
        true,
        |_| {},
    )
    .map_err(|error| anyhow!(error))?;
    // The importer's templates: clean stock packages hold no authored items.
    let templates = catalog
        .shared_weapon_ornaments()
        .into_iter()
        .filter(|ornament| ornament.art_rows == 1)
        .collect::<Vec<_>>();
    ensure!(
        !templates.is_empty(),
        "No stock weapon offers a shared ornament set"
    );
    let donors = json!({"weapon_ornaments": templates});
    let items = service::scan_cached(&modern, &native, &output.join("catalog"), false, |_| {})?;
    let mut recipes = Vec::new();
    let mut imported = Vec::new();
    // An ornament that fails is reported with the others' readback, so one run shows them all.
    let mut failed = Vec::new();
    for name in &names {
        let source = items
            .iter()
            .find(|item| {
                item.family() == service::Family::WeaponOrnament
                    && &item.name == name
                    && !item.present_in_native
            })
            .with_context(|| format!("{name} is not a listed weapon ornament Shadowkeep lacks"))?;
        let path = service::prepare_with_progress(
            source,
            &modern,
            &native,
            &donors,
            &output.join(format!("import-{:08X}", source.hash)),
            &mut |step| println!("{name}: {step}"),
        );
        let path = match path {
            Ok(path) => path,
            Err(error) => {
                println!("{name}: failed, {error:#}");
                failed.push(json!({"name":name,"reason":format!("{error:#}")}));
                continue;
            }
        };
        let mut recipe = WeaponRecipe::load_json(&path).map_err(|error| anyhow!("{error}"))?;
        ensure!(recipe.kind == ItemKind::Mod, "{name} is not a mod");
        let weapon = recipe
            .overrides
            .ornament_weapon
            .as_ref()
            .context("The mod names no weapon")?
            .parse_u32()
            .map_err(|error| anyhow!("{error}"))?;
        let base = recipe
            .donor
            .item_hash
            .parse_u32()
            .map_err(|error| anyhow!("{error}"))?;
        let template = templates
            .iter()
            .find(|template| template.hash == base && template.weapon == weapon)
            .with_context(|| format!("{name} is built on no shared ornament of its weapon"))?;
        ensure!(
            Some(template.plug_category) == source.plug_category,
            "{name} is built on another ornament category"
        );
        ensure!(
            !recipe.is_imported_ornament() && recipe.has_collections(),
            "{name} is not a weapon ornament with a Collections entry"
        );
        // The artwork is the item's icon, which the plug shows, not a perk icon over its
        // template's watermark.
        ensure!(
            recipe.overrides.icon_edit.imported_image.is_some()
                && recipe
                    .overrides
                    .socket_plug_variants
                    .iter()
                    .all(|perk| perk.icon.is_none()),
            "{name}'s artwork is not its item icon"
        );
        let rarity = source.rarity.context("The source has no rarity")?;
        let graph_path = recipe
            .overrides
            .imported_graph
            .as_ref()
            .context("Imported graph")?
            .directory
            .join("asset-graph.json");
        let graph: Value = serde_json::from_slice(&fs::read(graph_path)?)?;
        ensure!(
            graph["kind"] == "ornament"
                && graph.get("ornament").is_none()
                && graph["native_item"].as_u64() == Some(u64::from(weapon)),
            "{name}'s graph is not an ornament of its stock weapon"
        );
        // Renaming changes the mod's identity, which the build binds the graph to.
        recipe
            .rename_authored_item(format!("{} Copy", recipe.name))
            .map_err(|error| anyhow!("{error}"))?;
        let item = recipe
            .identity
            .item_hash
            .parse_u32()
            .map_err(|error| anyhow!("{error}"))?;
        let collectible = recipe
            .identity
            .collectible_hash
            .parse_u32()
            .map_err(|error| anyhow!("{error}"))?;
        // The recipe's source card shows the ornament.
        ensure!(
            graph["source_icon_png"].is_string()
                && graph["source_item"].as_u64() == Some(u64::from(source.hash)),
            "{name}'s graph does not name its source ornament"
        );
        // A saved recipe names its pinned graph folder and keeps its weapon, and the build uses the
        // copy saving and loading makes.
        let saved = recipe
            .to_json_pretty()
            .map_err(|error| anyhow!("{error}"))?;
        let document: Value = serde_json::from_str(&saved)?;
        ensure!(
            document["overrides"]["imported_graph"]["directory"].is_string(),
            "{name}'s saved recipe does not name its graph folder"
        );
        let reopened = WeaponRecipe::from_json_str(&saved).map_err(|error| anyhow!("{error}"))?;
        ensure!(
            reopened.identity == recipe.identity
                && reopened.overrides.ornament_weapon == recipe.overrides.ornament_weapon
                && reopened.overrides.imported_graph == recipe.overrides.imported_graph,
            "{name}'s saved recipe lost its identity, weapon or graph"
        );
        imported.push(json!({"name":name,"source":format!("{:08X}",source.hash),
            "item":item,"collectible":collectible,"rarity":rarity,"weapon":weapon,
            "weapon_name":template.weapon_name,"template":base,"category":template.plug_category,
            "graph_vertices":graph["vertices"]}));
        recipes.push(reopened);
    }
    ensure!(
        !recipes.is_empty(),
        "No weapon ornament imported: {}",
        serde_json::to_string(&failed)?
    );
    // Each configured runtime builds the recipes in a temporary installation that runs it, so the
    // readback covers every install layout and watermark: Dawn and Sunrise before 0.6 append to the
    // stock packages, and Sunrise 0.6 builds for its mod folder. Without one, the clean packages
    // build alone.
    let runtimes = env::var("PARHELION_ORNAMENT_RUNTIMES")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(|path| Some(PathBuf::from(path)))
        .collect::<Vec<_>>();
    let runtimes = if runtimes.is_empty() {
        vec![None]
    } else {
        runtimes
    };
    let mut builds = Vec::new();
    // The watermark pixels each runtime brand's builds drew.
    let mut watermarks = BTreeMap::<&str, BTreeSet<Vec<u8>>>::new();
    for (position, runtime) in runtimes.iter().enumerate() {
        let folder = output.join(format!("runtime-{position}"));
        let packages = match runtime {
            Some(dll) => install(&native, dll, &folder.join("install"))?,
            None => native.clone(),
        };
        let (brand, loader) = match sundial::package_authoring::installed_runtime_kind(
            packages.parent().context("Install root")?,
        ) {
            Some((RuntimeBrand::Dawn, loader)) => ("Dawn", loader),
            Some((RuntimeBrand::Sunrise, loader)) => ("Sunrise", loader),
            None => ("none", false),
        };
        println!("Building for {brand}, mod loader {loader}");
        let snapshot = BatchBuildSnapshot::new(BatchBuildRequest {
            package_directory: packages.clone(),
            staging_root: folder.join("build"),
            ignore_installed_authored_overlays: true,
            maps: Vec::new(),
            recipes: recipes.clone(),
        })
        .map_err(anyhow::Error::msg)?;
        let built = parhelion::build_and_stage_snapshot_with_progress(&snapshot, |p| {
            println!("{:?}", p.phase);
        })
        .map_err(anyhow::Error::msg)?;
        let readback = read_back(&native, &built, &folder, &imported)?;
        // A mod loader build puts its records in a package of their own and leaves each stock
        // package's entry count as it was. Other runtimes append the records to the stock host.
        ensure!(
            readback.records == loader,
            "The build for {brand} with mod loader {loader} staged a records package: {}",
            readback.records
        );
        if loader {
            for patch in &readback.packages {
                ensure!(
                    patch["stock_entries"].is_null() || patch["entries"] == patch["stock_entries"],
                    "{} changes its stock package's entry count",
                    patch["file"]
                );
            }
        }
        if runtime.is_some() {
            watermarks
                .entry(brand)
                .or_default()
                .insert(readback.watermark.clone());
        }
        let watermark = hex::encode(Sha256::digest(&readback.watermark));
        builds.push(json!({"runtime":runtime,"brand":brand,"mod_loader":loader,
            "staged_directory":built.run_directory,"packages":readback.packages,
            "watermark_sha256":watermark,"ornaments":readback.ornaments}));
    }
    // The builds for one runtime brand draw one watermark, and each brand draws its own.
    let drawn = watermarks
        .iter()
        .map(|(brand, pixels)| {
            let digests = pixels
                .iter()
                .map(|pixels| hex::encode(Sha256::digest(pixels)))
                .collect::<Vec<_>>();
            (*brand, digests)
        })
        .collect::<Vec<_>>();
    ensure!(
        watermarks.values().all(|pixels| pixels.len() == 1)
            && watermarks.values().flatten().collect::<BTreeSet<_>>().len() == watermarks.len(),
        "Watermarks by runtime brand: {drawn:?}"
    );
    fs::write(
        output.join("verified-weapon-ornaments.json"),
        serde_json::to_vec_pretty(&json!({"builds":builds,"watermarks":drawn,"failed":failed,
            "gameplay_verified":false}))?,
    )?;
    ensure!(
        failed.is_empty(),
        "{} of {} weapon ornaments failed to import: {}",
        failed.len(),
        names.len(),
        serde_json::to_string(&failed)?
    );
    Ok(())
}

/// A temporary installation over the clean packages that runs `runtime`, as a game folder holds
/// one: the packages, Oodle and the runtime's `steam_api64.dll`.
fn install(native: &Path, runtime: &Path, root: &Path) -> Result<PathBuf> {
    let packages = root.join("packages");
    fs::create_dir_all(&packages)?;
    fs::create_dir_all(root.join("bin/x64"))?;
    for entry in fs::read_dir(native)? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|e| e == "pkg") {
            fs::hard_link(entry.path(), packages.join(entry.file_name()))?;
        }
    }
    let bin = native.parent().context("Native root")?.join("bin/x64");
    fs::copy(
        bin.join("oo2core_3_win64.dll"),
        root.join("bin/x64/oo2core_3_win64.dll"),
    )?;
    fs::copy(runtime, root.join("bin/x64/steam_api64.dll"))?;
    fs::write(root.join("destiny2.exe"), [])?;
    Ok(packages)
}

/// What one build's readback found.
struct Readback {
    ornaments: Vec<Value>,
    /// The pixels of the watermark every ornament's icon draws.
    watermark: Vec<u8>,
    /// Whether the build staged a package of its own investment records.
    records: bool,
    /// Each staged package's id, patch and entry count, beside its stock package's.
    packages: Vec<Value>,
}

/// The id, patch and entry count of the package file at `path`.
fn package_header(path: &Path) -> Result<(u16, u16, usize)> {
    let package = PackageD2PreBL::open(path.to_str().context("Package path")?)
        .map_err(|error| anyhow!("{}: {error}", path.display()))?;
    Ok((
        package.pkg_id(),
        package.patch_id(),
        package.entries().len(),
    ))
}

/// Stages `built` over the clean packages in a view under `folder` and reads each imported
/// ornament back from it.
#[allow(clippy::cognitive_complexity)]
fn read_back(
    native: &Path,
    built: &BuildReport,
    folder: &Path,
    imported: &[Value],
) -> Result<Readback> {
    let view = folder.join("view");
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
    // The staged package files, by the name a packages folder holds them under.
    let staged_files = built
        .artifacts
        .iter()
        .map(|artifact| built.run_directory.join(&artifact.file_name))
        .filter(|path| path.extension().is_some_and(|e| e == "pkg"))
        .map(|path| {
            let name = path
                .file_name()
                .context("Staged package name")?
                .to_string_lossy()
                .into_owned();
            Ok((name, path))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    ensure!(!staged_files.is_empty(), "The build staged no packages");
    // The newest stock patch of each package the build stages.
    let staged_ids = staged_files
        .values()
        .map(|path| Ok(package_header(path)?.0))
        .collect::<Result<BTreeSet<_>>>()?;
    let mut stock_headers = BTreeMap::<u16, (u16, usize)>::new();
    for entry in fs::read_dir(native)? {
        let entry = entry?;
        let path = entry.path();
        if ignored.contains(entry.file_name().to_string_lossy().as_ref())
            || !path.extension().is_some_and(|e| e == "pkg")
        {
            continue;
        }
        let id = PackagePath::parse(path.to_str().context("Stock package path")?)
            .and_then(|parsed| u16::from_str_radix(&parsed.id, 16).ok());
        if !id.is_some_and(|id| staged_ids.contains(&id)) {
            continue;
        }
        let (id, patch, entries) = package_header(&path)?;
        if stock_headers
            .get(&id)
            .is_none_or(|(newest, _)| patch > *newest)
        {
            stock_headers.insert(id, (patch, entries));
        }
    }
    let mut staged_packages = Vec::new();
    for (name, path) in &staged_files {
        let (id, patch, entries) = package_header(path)?;
        let stock = stock_headers.get(&id);
        staged_packages.push(json!({"file":name,"id":format!("{id:04X}"),"patch":patch,
            "entries":entries,"stock_patch":stock.map(|s| s.0),"stock_entries":stock.map(|s| s.1)}));
    }
    let records = staged_files
        .keys()
        .any(|name| name.starts_with("w64_parhelion_investment_"));
    for entry in fs::read_dir(native)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if ignored.contains(name.as_str()) || staged_files.contains_key(&name) {
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
    for (name, path) in &staged_files {
        let destination = packages.join(name);
        ensure!(!destination.exists(), "Never overwrite a stock link");
        fs::copy(path, destination)?;
    }
    let reader = Reader::new(&packages, &folder.join("readback"), false)?;
    let named = reader
        .manager
        .lookup
        .named_tags
        .iter()
        .map(|e| (e.name.clone(), e.hash.0))
        .collect::<BTreeMap<_, _>>();
    let mut reader = reader;
    let globals = reader.tag(*named.get("investment_globals").context("Globals")?, None)?;
    let root = reader.tag(globals.u32(16)?, None)?;
    let items = reader.tag(root.u32(8 + 48 * 16)?, None)?;
    let rows = items
        .array(8, 24, Some(0x80807BE8))?
        .into_iter()
        .map(|row| Ok((items.u32(row)?, row)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let metadata = reader.tag(globals.u32(16 + 66 * 16)?, None)?;
    let strings = reader.tag(globals.u32(16 + 33 * 16)?, None)?;
    let icons = reader.tag(globals.u32(16 + 75 * 16)?, None)?;
    let assets = reader.tag(*named.get("investment_assets").context("Assets")?, None)?;
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
        strings,
        icons,
        parents,
    };
    // The stock definitions, read independently from the clean packages.
    let mut stock = Reader::new(native, &folder.join("stock-readback"), false)?;
    let stock_globals = stock
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|t| t.name == "investment_globals")
        .context("Stock globals")?
        .hash
        .0;
    let stock_globals = stock.tag(stock_globals, None)?;
    let stock_root = stock.tag(stock_globals.u32(16)?, None)?;
    let stock_items = stock.tag(stock_root.u32(8 + 48 * 16)?, None)?;
    let stock_definitions = stock_items
        .array(8, 24, Some(0x80807BE8))?
        .into_iter()
        .map(|row| Ok((stock_items.u32(row)?, stock_items.u32(row + 16)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut evidence = Vec::new();
    let mut watermarks = BTreeSet::new();
    for row in imported {
        let word = |key: &str| -> Result<u32> {
            Ok(u32::try_from(
                row[key]
                    .as_u64()
                    .with_context(|| format!("{key} missing"))?,
            )?)
        };
        let (item, weapon, template) = (word("item")?, word("weapon")?, word("template")?);
        let name = row["name"].as_str().unwrap_or_default();
        // The stock weapon keeps its own definition. The mod reaches it through the shared set.
        let weapon_tag = staged.definition_tag(weapon)?;
        ensure!(
            stock_definitions.get(&weapon) == Some(&weapon_tag),
            "{name}'s stock weapon definition changed"
        );
        let definition = staged.definition(item)?;
        let block = definition.pointer(0x40)?;
        ensure!(
            definition.u32(block - 4)? == 0x808077E3,
            "{name} has no plug block"
        );
        ensure!(
            u64::from(definition.u32(block)?) == row["category"].as_u64().unwrap_or_default(),
            "{name} left its weapon's ornament category"
        );
        ensure!(
            definition.u64(block + 0x40)? == 0 && definition.u64(block + 0x50)? == 0,
            "{name} keeps its template's unlock requirements"
        );
        let set = staged.ornament_set(weapon)?;
        let members = staged.set_members(set)?;
        let index = u16::try_from(staged.index(item)?)?;
        let joined = members
            .iter()
            .filter(|(member, _)| *member == index)
            .collect::<Vec<_>>();
        ensure!(
            matches!(joined.as_slice(), [(_, 0)]),
            "{name} is not one unconditional member of shared set {set}"
        );
        let template_index = u16::try_from(staged.index(template)?)?;
        ensure!(
            members.iter().any(|(member, _)| *member == template_index),
            "{name}'s template left shared set {set}"
        );
        let base_models = {
            let weapon_definition = staged.definition(weapon)?;
            staged.models(&weapon_definition)?
        };
        let models = staged.models(&definition)?;
        ensure!(
            !models.is_empty()
                && models
                    .iter()
                    .all(|model| !base_models.iter().any(|base| base.0 == model.0)),
            "{name} does not place a model of its own"
        );
        let palettes =
            |models: &[(u32, u32)]| models.iter().map(|model| model.1).collect::<BTreeSet<_>>();
        ensure!(
            palettes(&models).is_subset(&palettes(&base_models)),
            "{name} skins to palettes {:?}, its weapon to {:?}",
            palettes(&models),
            palettes(&base_models)
        );
        // The mod's tier is its source's, which can differ from its stock template's.
        let rarity = definition.u8(0xBA)?;
        ensure!(
            Some(u64::from(rarity)) == row["rarity"].as_u64(),
            "{name} has rarity {rarity}, its source {}",
            row["rarity"]
        );
        // One collectible of its own, on every page its stock ornament's is on, each listing it.
        let own = staged.collectibles(index)?;
        let [own] = own.as_slice() else {
            return Err(anyhow!("{name} has {} collectibles", own.len()));
        };
        ensure!(
            u64::from(own.hash) == row["collectible"].as_u64().unwrap_or_default(),
            "{name}'s collectible is not its own"
        );
        let stock_collectible = staged.collectibles(template_index)?;
        let [stock_collectible] = stock_collectible.as_slice() else {
            return Err(anyhow!(
                "{name}'s stock ornament has {} collectibles",
                stock_collectible.len()
            ));
        };
        ensure!(
            !stock_collectible.parents.is_empty()
                && stock_collectible
                    .parents
                    .iter()
                    .all(|page| own.parents.contains(page)),
            "{name} is on pages {:?}, its stock ornament on {:?}",
            own.parents,
            stock_collectible.parents
        );
        let mut pages = Vec::new();
        for &page in &stock_collectible.parents {
            let (_, _, children) = staged.node(page)?;
            ensure!(
                children.contains(&u16::try_from(own.index)?),
                "{name}'s Collections page {page} does not list it"
            );
            // The page and its ancestors, for the record.
            let mut path = Vec::new();
            let mut at = Some(page);
            while let Some(node) = at.filter(|_| path.len() < 8) {
                let (hash, parents, _) = staged.node(node)?;
                path.push(format!("{hash:08X}"));
                at = parents.first().copied();
            }
            pages.push(path);
        }
        // The icon draws the build's watermark rather than its stock ornament's.
        let watermark = staged.watermark(item)?;
        ensure!(
            ![0, u32::MAX].contains(&watermark) && watermark != staged.watermark(template)?,
            "{name}'s icon keeps its stock ornament's watermark"
        );
        watermarks.insert(watermark);
        evidence.push(json!({"ornament":row,"set":set,"set_members":members.len(),
            "models":models.iter().map(|m| format!("{:08X}", m.0)).collect::<Vec<_>>(),
            "palettes":palettes(&models),"weapon_models":base_models.iter().map(|m| format!("{:08X}", m.0)).collect::<Vec<_>>(),
            "rarity":rarity,"collectible":{"index":own.index,"hash":format!("{:08X}", own.hash),
                "parents":own.parents,"stock_parents":stock_collectible.parents,"pages":pages},
            "watermark":format!("{watermark:08X}")}));
    }
    // Every authored icon shares the one watermark the build authors for its runtime.
    let watermarks = watermarks.into_iter().collect::<Vec<_>>();
    let &[watermark] = watermarks.as_slice() else {
        return Err(anyhow!("The ornaments do not share one watermark"));
    };
    let pixels = watermark_pixels(&mut staged.reader, watermark, folder)?;
    staged.reader.finish()?;
    stock.finish()?;
    Ok(Readback {
        ornaments: evidence,
        watermark: pixels,
        records,
        packages: staged_packages,
    })
}

/// The pixel data of every texture the watermark layer `layer` draws, in lane order. Each
/// texture is also written to `folder` as a PNG when it is tightly packed RGBA8.
fn watermark_pixels(reader: &mut Reader, layer: u32, folder: &Path) -> Result<Vec<u8>> {
    let payload = reader.tag(layer, None)?;
    let mut pixels = Vec::new();
    for (lane, descriptor) in payload.array(0x20, 0x10, None)?.into_iter().enumerate() {
        for (position, at) in payload.array(descriptor, 4, None)?.into_iter().enumerate() {
            let header_tag = payload.u32(at)?;
            let header = reader.tag(header_tag, None)?;
            let data = reader.tag(reader.reference(header_tag)?, None)?;
            let (width, height) = (u32::from(header.u16(14)?), u32::from(header.u16(16)?));
            if header.u16(12)? == 0xCAFE && data.0.len() == (width * height * 4) as usize {
                image::save_buffer(
                    folder.join(format!("watermark-{lane}-{position}.png")),
                    &data.0,
                    width,
                    height,
                    image::ExtendedColorType::Rgba8,
                )?;
            }
            pixels.extend_from_slice(&data.0);
        }
    }
    ensure!(!pixels.is_empty(), "The watermark layer draws no texture");
    Ok(pixels)
}
