//! A perk column that rolls from its own choices, written the way the importers write one, on
//! Badlander's first rolled lane with two curated plugs. Those two stay swappable, and two more
//! choices, the second a private perk, are only in the roll pool. The staged packages must append
//! one set after the stock rows holding all four choices, the private perk as its own item, keep
//! every stock set as it shipped, and give the socket row that set, the two swappable choices and
//! a single-roll selection program. A fresh catalog scan must read the same socket.
use super::offered_mods::{investment_root, item_index, plug_sets};
use super::*;
use crate::perk::PerkRecipe;
use crate::recipe::{HexHash, WeaponSocketColumnRecipe};

const BADLANDER: u32 = 0x4F1C_712E;

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES pointing to Shadowkeep packages"]
#[allow(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent assertions together"
)]
fn real_rolled_column_appends_a_private_randomized_set() {
    let packages = crate::test_support::stock_packages();
    let ignored = crate::package_profile::CANONICAL_ARTIFACT_FILE_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    let view = crate::workflow::FilteredPackageView::create(&packages, &ignored).unwrap();
    let install = view.path().parent().unwrap();
    fs::write(install.join("destiny2.exe"), []).unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let cache_path = temporary.path().join("catalog.json");
    let baseline =
        InvestmentCatalog::load_with_cache_path(install, &cache_path, true, |_| {}).unwrap();
    let donor = baseline.weapon_donor(BADLANDER).unwrap();
    let socket = donor
        .sockets
        .iter()
        .find(|socket| {
            socket.randomized_plug_set_index.is_some() && socket.ordered_embedded_choices.len() >= 2
        })
        .unwrap();
    let curated = socket.ordered_embedded_choices[..2].to_vec();
    let rolled = baseline.supported_plug_sets(BADLANDER, &[]).unwrap()[socket.index]
        .plug_hashes
        .iter()
        .copied()
        .find(|plug| !curated.contains(plug))
        .unwrap();
    let mut perk = PerkRecipe::new();
    perk.name = "Rolled Perk".into();
    perk.effects = baseline
        .item_sandbox_perk_indices(rolled)
        .into_iter()
        .map(PerkRecipe::effect)
        .collect();
    let template = perk.template_plug.parse_u32().unwrap();
    let choices = [curated[0], curated[1], rolled, template];
    let source = open_manager(view.path()).unwrap();
    let stock_sets = plug_sets(&source);
    let stock_indices = choices[..3]
        .iter()
        .map(|&hash| item_index(&source, hash))
        .collect::<Vec<_>>();
    drop(source);

    let mut recipe = crate::WeaponRecipe::new_weapon_for_donor(
        "parhelion.rolled-column.integration",
        BADLANDER,
        "Badlander",
    )
    .unwrap();
    recipe.name = "Rolled Column".into();
    recipe.overrides.socket_columns = vec![None; donor.sockets.len()];
    recipe.overrides.socket_columns[socket.index] = Some(WeaponSocketColumnRecipe {
        choices: choices.iter().copied().map(HexHash::new).collect(),
        reusable_choices: Some(2),
        ..Default::default()
    });
    recipe.overrides.socket_plug_variants = vec![perk.at_socket(socket.index as u16, 3)];
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: view.path().to_path_buf(),
        staging_root: temporary.path().join("staging"),
        ignore_installed_authored_overlays: false,
        recipes: vec![recipe],
        maps: Vec::new(),
    })
    .unwrap();
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {}).unwrap();
    let [weapon] = build.weapons.as_slice() else {
        panic!("one weapon was built");
    };
    let [plug] = weapon.custom_plugs.as_slice() else {
        panic!("one private plug was built");
    };
    for artifact in &build.artifacts {
        view.add_overlay(&build.run_directory.join(&artifact.file_name))
            .unwrap();
    }

    // One set after the stock rows, holding every choice, each always available.
    let staged = open_manager(view.path()).unwrap();
    let staged_sets = plug_sets(&staged);
    assert_eq!(staged_sets.len(), stock_sets.len() + 1);
    for (set, (stock, staged)) in stock_sets.iter().zip(&staged_sets).enumerate() {
        assert_eq!(staged, stock, "plug set {set}");
    }
    let pool = stock_indices
        .iter()
        .copied()
        .chain([plug.item_index])
        .map(|index| (index, Vec::<u64>::new()))
        .collect::<Vec<_>>();
    assert_eq!(staged_sets[stock_sets.len()], pool);

    // The socket row rolls from that set, one plug per roll, and embeds the two swappable ones.
    let items = staged
        .read_tag(
            root_child_tag(&investment_root(&staged), ROOT_ITEM_DEFINITION_TABLE_SLOT).unwrap(),
        )
        .unwrap();
    let (count, _, rows, _) = array_at(&items, 8).unwrap();
    let row = find_u32_row_key(&items, rows, count, ITEM_ROW_SIZE, weapon.item_hash)
        .unwrap()
        .unwrap();
    let definition = staged
        .read_tag(TagHash(
            read_u32(&items, rows + row * ITEM_ROW_SIZE + 16).unwrap(),
        ))
        .unwrap();
    drop(staged);
    let sockets = relative_target(&definition, ITEM_ORDINARY_SOCKET_POINTER_OFFSET).unwrap();
    let (_, _, socket_rows, _) = array_at(&definition, sockets).unwrap();
    let socket_row = socket_rows + socket.index * ITEM_ORDINARY_SOCKET_ROW_SIZE;
    let set = u16::try_from(stock_sets.len()).unwrap();
    assert_eq!(
        read_u16(
            &definition,
            socket_row + ITEM_ORDINARY_SOCKET_RANDOMIZED_PLUG_SET_OFFSET
        )
        .unwrap(),
        set
    );
    assert_eq!(
        read_numeric_program(
            &definition,
            socket_row + ITEM_ORDINARY_SOCKET_RANDOMIZED_SELECTION_PROGRAM_OFFSET
        )
        .unwrap(),
        [WeaponNumericInstruction {
            opcode: 11,
            operand: 1
        }]
    );

    // The catalog reads the socket the way the app and Dawn do.
    let catalog =
        InvestmentCatalog::load_with_cache_path(install, &cache_path, true, |_| {}).unwrap();
    let authored = catalog
        .weapon_donor(weapon.item_hash)
        .unwrap()
        .sockets
        .swap_remove(socket.index);
    assert_eq!(authored.randomized_plug_set_index, Some(set));
    assert_eq!(authored.ordered_embedded_choices, curated);
    assert_eq!(authored.native_default, Some(curated[0]));
    crate::test_support::artifact(
        "rolled-column.json",
        &serde_json::json!({
            "weapon": format!("0x{:08X}", weapon.item_hash),
            "socket": socket.index,
            "swappable": curated.iter().map(|hash| format!("0x{hash:08X}")).collect::<Vec<_>>(),
            "randomized_plug_set": set,
            "pool_item_indices": pool.iter().map(|(index, _)| index).collect::<Vec<_>>(),
            "private_plug": format!("0x{:08X}", plug.item_hash),
            "staging": build.run_directory,
        }),
    );
}
