//! Configured hand toggling of the family on Sunrise 0.6. Sunrise has no switch for a mod, so a
//! user turns one off by moving its folder out of `mods` and on by moving it back. Here a folder
//! moves to a dot-prefixed name beside it, which the loader skips. Every state is read through
//! Sundial's loader port and its catalog, and the loading index is decoded independently.
//!
//! Before implementation, the failure inventory covered: a member that needs another member to
//! load, a registration or loading index entry naming a package of a removed member, a later
//! member's HUD, animation, ability or Collections entry replacing an earlier member's, a moved
//! row selecting the wrong perk, plug, item, icon or Collections node, a restored member changing
//! persisted identities or registering its items twice, an unchanged member rewritten when another
//! changes or is restored, account data changed by a toggle or by the next install, and map
//! worlds, activities and scripts outside their source's folder. Foreign mods, duplicate IDs,
//! changed receipts, interrupted updates and rollback are covered by
//! `install::tests::loader::family` and `coexistence`, and the legacy layouts by
//! `importer_mods_reuse_changed_content_and_retire_as_one_generation`, and the shared rows
//! Parhelion holds for a source by `coupled`.
use super::*;

mod coupled;

/// The member folders that hold the family, by mod ID.
fn members(root: &Path) -> BTreeMap<String, PathBuf> {
    mod_content::family_directories(root).unwrap()
}

/// Moves member `id` out of the loader's sight, as a user turns it off.
fn turn_off(root: &Path, id: &str) {
    let mods = mod_content::runtime_directory(root).join("mods");
    fs::rename(mods.join(id), mods.join(format!(".{id}"))).unwrap();
}

/// Moves member `id` back where the loader reads it.
fn turn_on(root: &Path, id: &str) {
    let mods = mod_content::runtime_directory(root).join("mods");
    fs::rename(mods.join(format!(".{id}")), mods.join(id)).unwrap();
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}

/// The first row of the typed array whose `{count, relative}` descriptor is at `descriptor`, and
/// its count. The relative offset counts from its own field to the array header, whose 16 bytes
/// repeat the count before the rows.
fn array(bytes: &[u8], descriptor: usize) -> (usize, usize) {
    let count = usize::try_from(u64_at(bytes, descriptor)).unwrap();
    if count == 0 {
        return (0, 0);
    }
    let relative = i64::from_le_bytes(bytes[descriptor + 8..descriptor + 16].try_into().unwrap());
    let header = usize::try_from(i64::try_from(descriptor + 8).unwrap() + relative).unwrap();
    assert_eq!(usize::try_from(u64_at(bytes, header)).unwrap(), count);
    (header + 16, count)
}

/// Every package entry the loading index names. Its 40-byte group rows sit in the array at
/// `+0x10`. Each holds its package ID at `+0`, a bitmap of entries whose u32 word count is at `+8`
/// and offset at `+16`, and a u16 entry list at `+24`. Entry numbers keep 13 bits.
fn loading_index(bytes: &[u8]) -> BTreeSet<(u16, u16)> {
    let (rows, groups) = array(bytes, 0x10);
    let mut entries = BTreeSet::new();
    for group in 0..groups {
        let row = rows + group * 40;
        let package = u16::from_le_bytes(bytes[row..row + 2].try_into().unwrap());
        let words = u32::from_le_bytes(bytes[row + 8..row + 12].try_into().unwrap()) as usize;
        let relative = i64::from_le_bytes(bytes[row + 16..row + 24].try_into().unwrap());
        if relative != 0 {
            let header = usize::try_from(i64::try_from(row + 16).unwrap() + relative).unwrap();
            for word in 0..words {
                let at = header + 16 + word * 4;
                let bits = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
                for bit in (0..32).filter(|bit| bits & (1 << bit) != 0) {
                    entries.insert((package, u16::try_from(word * 32 + bit).unwrap()));
                }
            }
        }
        let (list, count) = array(bytes, row + 24);
        for index in 0..count {
            let at = list + index * 2;
            let entry = u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) & 0x1FFF;
            entries.insert((package, entry));
        }
    }
    entries
}

/// What Sundial's catalog reads of an item through the loader port. Rows the merge renumbers
/// resolve here to the hashes and tags they name, so a row that moves wrong changes the identity.
fn identity(
    catalog: &sundial::investment::InvestmentCatalog,
    perks: &[u8],
    hash: u32,
) -> serde_json::Value {
    json!({
        "definition": catalog.item_definition_tag(hash),
        "strings": catalog.item_string_tag(hash),
        "name": catalog.item_display_name(hash),
        "collections": catalog.item_collection_paths(hash),
        "perks": catalog
            .item_sandbox_perk_indices(hash)
            .into_iter()
            .map(|index| sandbox_perk_index_hash_at(perks, usize::from(index)).unwrap())
            .collect::<Vec<_>>(),
        // A plug set's row number moves with the rows an earlier member adds, so a socket is
        // compared by what it offers: the plugs its sets resolve to.
        "sockets": catalog.weapon_donor(hash).map(|donor| {
            donor
                .sockets
                .iter()
                .map(|socket| {
                    json!({
                        "index": socket.index,
                        "label": socket.label,
                        "default": socket.native_default,
                        "embedded": socket.ordered_embedded_choices,
                        "reusable": socket.reusable_plug_set_index.is_some(),
                        "randomized": socket
                            .randomized_plug_set_index
                            .map(|_| &socket.ordered_randomized_choices),
                    })
                })
                .collect::<Vec<_>>()
        }),
    })
}

/// The items and private plugs each member registers, by mod ID.
fn items(
    recipes: &[crate::WeaponRecipe],
    build: &crate::BuildReport,
) -> BTreeMap<String, BTreeSet<u32>> {
    let mut owned = BTreeMap::<String, BTreeSet<u32>>::new();
    assert_eq!(recipes.len(), build.weapons.len());
    // The report lists the project's weapons in the project's own order.
    for recipe in recipes {
        let hash = recipe.identity.item_hash.parse_u32().unwrap();
        let weapon = build
            .weapons
            .iter()
            .find(|weapon| weapon.item_hash == hash)
            .unwrap_or_else(|| panic!("the build has no item {hash:08X}"));
        owned
            .entry(member_id(origin(recipe).as_deref()))
            .or_default()
            .extend(
                std::iter::once(weapon.item_hash)
                    .chain(weapon.custom_plugs.iter().map(|plug| plug.item_hash)),
            );
    }
    owned
}

/// Every member of `build` except `rebuilt` built the bytes its installed folder holds, the main
/// mod included, so installation keeps those folders and writes `rebuilt` alone. A source that
/// builds into Parhelion has no folder of its own. Each caller also checks the installed files.
pub(super) fn assert_only_rebuilt(build: &crate::BuildReport, rebuilt: &str) {
    assert!(!build.mods.is_empty());
    for report in build
        .mods
        .iter()
        .filter(|report| report.status != crate::ModStatus::InParhelion)
    {
        assert_eq!(
            report.status == crate::ModStatus::Kept,
            report.id != rebuilt,
            "{} is {}",
            report.id,
            report.status.label()
        );
    }
}

/// The package IDs each installed member's receipt names.
fn packages_of(root: &Path) -> BTreeMap<String, BTreeSet<u16>> {
    members(root)
        .into_iter()
        .map(|(id, folder)| {
            let receipt: mod_content::Receipt =
                mod_content::read_json(&folder.join(mod_content::RECEIPT)).unwrap();
            let ids = receipt
                .artifacts
                .iter()
                .filter(|artifact| artifact.path.starts_with("packages/"))
                .filter(|artifact| artifact.path.ends_with(".pkg"))
                .map(|artifact| {
                    let parsed = tiger_pkg::manager::PackagePath::parse(&artifact.path).unwrap();
                    u16::from_str_radix(&parsed.id, 16).unwrap()
                })
                .collect();
            (id, ids)
        })
        .collect()
}

/// Every file of each member, by its path under the mods folder, with its times and digest.
fn written(root: &Path) -> BTreeMap<String, String> {
    let mods = mod_content::runtime_directory(root).join("mods");
    mod_content::files(root)
        .unwrap()
        .into_iter()
        .map(|path| {
            let key = path
                .strip_prefix(&mods)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            // A file written again gets new times even when its bytes are equal. Times stand in
            // for file identity, since an open handle would keep the install from replacing it.
            let metadata = fs::metadata(&path).unwrap();
            let written = (metadata.created().unwrap(), metadata.modified().unwrap());
            let digest = crate::artifact::digest_file(&path).unwrap().sha256;
            (key, format!("{written:?} {digest}"))
        })
        .collect()
}

/// The persisted account's SQLite tables, compared by content.
fn account(path: &Path) -> serde_json::Value {
    json!(account_tables(path))
}

/// Reads one toggle state and checks it against the all-on identities `expected`, which every
/// loaded member's items keep. `owned` lists each member's items and `packages` the package IDs of
/// every member, loaded or not.
fn observe(
    installation: &Installation,
    cache: &Path,
    state: &str,
    owned: &BTreeMap<String, BTreeSet<u32>>,
    packages: &BTreeMap<String, BTreeSet<u16>>,
    expected: Option<&BTreeMap<u32, serde_json::Value>>,
) -> (BTreeMap<u32, serde_json::Value>, serde_json::Value) {
    let root = installation.packages.parent().unwrap();
    let loaded = members(root).into_keys().collect::<BTreeSet<_>>();
    let notices = mod_content::notices(root).unwrap().unwrap_or_default();
    // Nothing a member adds is dropped, refused or served from a later member.
    assert!(notices.is_empty(), "{state}: {notices:?}");
    let catalog = sundial::investment::InvestmentCatalog::load_with_cache_path(
        root,
        &cache.join(format!("{state}.json")),
        true,
        |_| {},
    )
    .unwrap();
    let manager =
        sundial::package_authoring::open_shadowkeep_package_manager(&installation.packages)
            .unwrap();
    let perks = manager
        .read_tag(TagHash(mod_content::merge::recipes::PERK_DEFINITIONS))
        .unwrap();
    let mut identities = BTreeMap::new();
    for (id, hashes) in owned {
        for &hash in hashes {
            let read = identity(&catalog, &perks, hash);
            if loaded.contains(id) {
                assert!(
                    !read["definition"].is_null(),
                    "{state}: {id} item {hash:08X} is missing"
                );
                if let Some(expected) = expected {
                    assert_eq!(
                        &read, &expected[&hash],
                        "{state}: {id} item {hash:08X} changed"
                    );
                }
            } else {
                assert!(
                    read["definition"].is_null(),
                    "{state}: {id} is off, but its item {hash:08X} is registered"
                );
            }
            identities.insert(hash, read);
        }
    }
    let index = loading_index(
        &manager
            .read_tag(crate::item::RUNTIME_DEPENDENCY_COMPANION)
            .unwrap(),
    );
    let mut resolved = 0;
    for (id, ids) in packages {
        for &(package, entry) in index.iter().filter(|(package, _)| ids.contains(package)) {
            assert!(
                loaded.contains(id),
                "{state}: the loading index names {package:04X} of {id}, which is off"
            );
            assert!(
                manager.read_tag(TagHash::new(package, entry)).is_ok(),
                "{state}: the loading index names {package:04X}:{entry} that no package holds"
            );
            resolved += 1;
        }
    }
    let evidence = json!({
        "state": state,
        "loaded": loaded,
        "loading_index_entries_of_members": resolved,
        "account": account(&installation.account),
    });
    (identities, evidence)
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES, SUNRISE_MOD_LOADER_DLL, SUNDIAL_IMPORTER_MOD_SCENARIO and SUNDIAL_TEST_ARTIFACTS"]
#[allow(
    clippy::cognitive_complexity,
    reason = "One ordered toggle lifecycle with independent readback"
)]
fn each_member_folder_can_be_removed_and_restored_by_hand() {
    let scenario_path = PathBuf::from(
        std::env::var_os("SUNDIAL_IMPORTER_MOD_SCENARIO")
            .expect("SUNDIAL_IMPORTER_MOD_SCENARIO names the original library recipe selection"),
    );
    let scenario: Scenario = serde_json::from_slice(&fs::read(&scenario_path).unwrap()).unwrap();
    let recipes = scenario
        .recipes
        .iter()
        .map(|path| crate::WeaponRecipe::load_json(path).unwrap())
        .collect::<Vec<_>>();
    let origins = recipes.iter().filter_map(origin).collect::<BTreeSet<_>>();
    assert!(
        recipes.iter().any(|recipe| origin(recipe).is_none()),
        "native content is required"
    );
    assert!(
        origins.len() >= 2,
        "at least two importer origins are required"
    );
    let maps = scenario
        .maps
        .iter()
        .map(|path| crate::map::load_recipe(path).unwrap())
        .collect::<Vec<_>>();
    let output = crate::test_support::artifact_dir("importer-mod-toggle");
    fs::create_dir_all(&output).unwrap();
    let mut evidence = Vec::new();
    // The mod loader came with Sunrise 0.6, which keeps the account in SQLite under settings v18,
    // so JSON accounts never meet mod folders.
    let schema = 18;
    {
        let installation = installation("importer-mod-toggle-");
        let root = installation.packages.parent().unwrap().to_path_buf();
        let cache = output.join(format!("schema-{schema}-catalogs"));
        fs::create_dir_all(&cache).unwrap();
        let build = |recipes: &[crate::WeaponRecipe], name: &str| {
            let request = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
                package_directory: installation.packages.clone(),
                staging_root: output.join(format!("schema-{schema}-{name}")),
                ignore_installed_authored_overlays: true,
                recipes: recipes.to_vec(),
                maps: maps.clone(),
            })
            .unwrap();
            crate::build_and_stage_snapshot_with_progress(&request, |_| {}).unwrap()
        };
        let backups = output.join(format!("schema-{schema}-backups"));

        let first = build(&recipes, "first");
        let staged = manifests(&first);
        assert_independent(&staged, &origins, &maps);
        install(&first, &installation.packages, &backups);
        let owned = items(&recipes, &first);
        let packages = packages_of(&root);
        let ids = packages.keys().cloned().collect::<Vec<_>>();
        assert_eq!(ids.len(), origins.len() + 1);
        let account_installed = account(&installation.account);
        let (expected, state) = observe(&installation, &cache, "on", &owned, &packages, None);
        let mut states = vec![state];

        // Each member off in turn, then each member alone.
        for id in &ids {
            turn_off(&root, id);
            states.push(
                observe(
                    &installation,
                    &cache,
                    &format!("without-{id}"),
                    &owned,
                    &packages,
                    Some(&expected),
                )
                .1,
            );
            turn_on(&root, id);
        }
        for id in &ids {
            for other in ids.iter().filter(|other| *other != id) {
                turn_off(&root, other);
            }
            states.push(
                observe(
                    &installation,
                    &cache,
                    &format!("only-{id}"),
                    &owned,
                    &packages,
                    Some(&expected),
                )
                .1,
            );
            for other in ids.iter().filter(|other| *other != id) {
                turn_on(&root, other);
            }
        }
        states.push(
            observe(
                &installation,
                &cache,
                "restored",
                &owned,
                &packages,
                Some(&expected),
            )
            .1,
        );
        // Moving folders changes no account data.
        assert_eq!(account(&installation.account), account_installed);

        // A build of the same recipes while one member is off rebuilds that member alone, and the
        // install changes nothing else and no account data.
        let off = member_id(origins.iter().next().map(String::as_str));
        let before = written(&root);
        turn_off(&root, &off);
        let rebuilt = build(&recipes, "rebuilt");
        assert_only_rebuilt(&rebuilt, &off);
        install(&rebuilt, &installation.packages, &backups);
        // The folder moved aside is the user's, and stays where they put it.
        assert!(
            mod_content::runtime_directory(&root)
                .join("mods")
                .join(format!(".{off}"))
                .is_dir()
        );
        let after = written(&root);
        for (path, digest) in &before {
            if !path.starts_with(&format!("{off}/")) {
                assert_eq!(after.get(path), Some(digest), "{path} was rewritten");
            }
        }
        assert_eq!(account(&installation.account), account_installed);
        states.push(
            observe(
                &installation,
                &cache,
                "reinstalled",
                &owned,
                &packages_of(&root),
                Some(&expected),
            )
            .1,
        );
        fs::remove_dir_all(
            mod_content::runtime_directory(&root)
                .join("mods")
                .join(format!(".{off}")),
        )
        .unwrap();

        // An item added to one source writes that source's folder alone.
        let added = crate::WeaponRecipe::load_json(&scenario.added_recipe).unwrap();
        let changed = member_id(Some(
            origin(&added)
                .expect("the added recipe is imported")
                .as_str(),
        ));
        let mut grown = recipes.clone();
        grown.push(added);
        let before = written(&root);
        let addition = build(&grown, "added");
        assert_only_rebuilt(&addition, &changed);
        install(&addition, &installation.packages, &backups);
        let after = written(&root);
        for (path, digest) in &before {
            if !path.starts_with(&format!("{changed}/")) {
                assert_eq!(after.get(path), Some(digest), "{path} was rewritten");
            }
        }
        let mut grown_items = items(&grown, &addition);
        let new_items = grown_items[&changed]
            .difference(&owned[&changed])
            .copied()
            .collect::<Vec<_>>();
        assert!(!new_items.is_empty());
        let (with_added, state) = observe(
            &installation,
            &cache,
            "added",
            &grown_items,
            &packages_of(&root),
            None,
        );
        // The changed member's records take new places around its added item's, so its other
        // items keep everything but their record tags. Every other member's items are as they were.
        for (hash, identity) in &expected {
            let (mut seen, mut identity) = (with_added[hash].clone(), identity.clone());
            if owned[&changed].contains(hash) {
                for field in ["definition", "strings"] {
                    seen.as_object_mut().unwrap().remove(field);
                    identity.as_object_mut().unwrap().remove(field);
                }
            }
            assert_eq!(seen, identity, "added: item {hash:08X} changed");
        }
        states.push(state);
        // Back to the scenario's own recipes, which removes the added item from that source alone.
        let before = written(&root);
        let restored = build(&recipes, "without-added");
        assert_only_rebuilt(&restored, &changed);
        install(&restored, &installation.packages, &backups);
        let after = written(&root);
        for (path, digest) in &before {
            if !path.starts_with(&format!("{changed}/")) {
                assert_eq!(after.get(path), Some(digest), "{path} was rewritten");
            }
        }
        grown_items
            .get_mut(&changed)
            .unwrap()
            .retain(|hash| !new_items.contains(hash));
        assert_eq!(grown_items, owned);

        // Retiring a source, the Halo: Reach maps' source when maps are selected, removes its
        // folder with a backup and leaves the other sources' files as they were. The main mod
        // counts every member's badges in the Badges total, so it may be written again. Selecting
        // the source again restores every identity.
        let main = member_id(None);
        let retired = member_id(Some(if maps.is_empty() {
            origins.iter().next().unwrap().as_str()
        } else {
            "halo_reach"
        }));
        let before = written(&root);
        let kept = recipes
            .iter()
            .filter(|recipe| member_id(origin(recipe).as_deref()) != retired)
            .cloned()
            .collect::<Vec<_>>();
        let selected_maps = maps.clone();
        let request = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
            package_directory: installation.packages.clone(),
            staging_root: output.join(format!("schema-{schema}-retired")),
            ignore_installed_authored_overlays: true,
            recipes: kept,
            maps: if retired == member_id(Some("halo_reach")) {
                Vec::new()
            } else {
                selected_maps
            },
        })
        .unwrap();
        let without = crate::build_and_stage_snapshot_with_progress(&request, |_| {}).unwrap();
        let (report, _) = install(&without, &installation.packages, &backups);
        assert!(!members(&root).contains_key(&retired));
        assert!(report.backup_directory.join("mods").join(&retired).is_dir());
        let after = written(&root);
        for (path, digest) in &before {
            if !path.starts_with(&format!("{retired}/")) && !path.starts_with(&format!("{main}/")) {
                assert_eq!(after.get(path), Some(digest), "{path} was rewritten");
            }
        }
        states.push(
            observe(
                &installation,
                &cache,
                "retired",
                &owned,
                &packages,
                Some(&expected),
            )
            .1,
        );
        let back = build(&recipes, "selected-again");
        install(&back, &installation.packages, &backups);
        assert_eq!(items(&recipes, &back), owned);
        states.push(
            observe(
                &installation,
                &cache,
                "selected-again",
                &owned,
                &packages_of(&root),
                Some(&expected),
            )
            .1,
        );
        evidence.push(json!({
            "schema": schema,
            "members": ids,
            "first": staged,
            "states": states,
        }));
    }
    let source_revision = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .map(|result| String::from_utf8_lossy(&result.stdout).trim().to_owned())
        .unwrap();
    fs::write(
        output.join("receipt.json"),
        serde_json::to_vec_pretty(&json!({
            "source_revision": source_revision,
            "executable": crate::artifact::digest_file(&std::env::current_exe().unwrap())
                .unwrap()
                .sha256,
            "inputs": scenario_path,
            "toggle": "folder moved to a dot-prefixed name beside it",
            "runs": evidence,
            "limits": [
                "Offline loader port and catalog only",
                "No Sunrise loader log or Mods page capture",
                "No weapon, ability, HUD or map gameplay",
                "Account items of a member that is off are not exercised in game",
            ],
        }))
        .unwrap(),
    )
    .unwrap();
}
