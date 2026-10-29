//! Persisted bank replacement follows private allocation, providers and loading after staging.
//! The preimplementation failure inventory covers stale replication, owner-order changes,
//! missing callbacks, shared identities, wrong allocation witnesses and donor mutation.
use super::*;
use crate::recipe::{HexHash, WeaponSocketColumnRecipe, WeaponSocketPlugVariantRecipe};
use serde_json::json;
use sha2::{Digest, Sha256};
use sundial::package_authoring::{
    loading_index,
    sandbox_perk::{
        action,
        program::{
            Action, Asset, AttachmentTarget, NativeAssetComponent, NativeAssetPatch,
            NativeAssetResourcePatch, Program, Trigger,
        },
    },
};

const HOST: u32 = 0x80F9F7CF;
const CASES: [(u32, u32, usize); 2] = [(0x80BB760E, 0x80C3DDE2, 1), (0x80B7CB8B, 0x80C0020A, 2)];

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn patches(manager: &PackageManager, graph: u32) -> Vec<NativeAssetResourcePatch> {
    let graph = manager.read_tag(TagHash(graph)).unwrap();
    weapon_component_bindings(&graph, 0x7330E39F)
        .unwrap()
        .into_iter()
        .map(|binding| {
            let owner = manager.read_tag(TagHash(binding.owner_tag)).unwrap();
            let instance = binding.resource_offset as usize;
            let definition = read_u64(&owner, instance + 8).unwrap() as usize;
            assert_eq!(read_u32(&owner, instance + 4).unwrap(), 0x80803B06);
            let settings = relative_target(&owner, 24).unwrap();
            let (count, _, rows, _) = array_at(&owner, settings + 88).unwrap();
            let index = (0..count)
                .find(|index| rows + index * 88 == definition)
                .expect("provider reaches an actual modifier row");
            NativeAssetResourcePatch {
                binding_hash: binding.binding_hash,
                resource_index: binding.resource_index.try_into().unwrap(),
                offset: (definition + 40 - instance).try_into().unwrap(),
                expected: owner[definition + 40..definition + 44].to_vec(),
                bytes: (1.5 + index as f32).to_le_bytes().to_vec(),
                imported_particle: None,
            }
        })
        .collect()
}

/// Compare each complete exported descriptor, retaining exclusions and method identities.
/// Only private owner identities and their validated component ordinals are normalized.
fn providers(graph: &[u8], owner: u32, ordinal: usize) -> Vec<([u8; 40], [u8; 24])> {
    let (count, _, maps, _) = array_at(graph, 0x58).unwrap();
    let (references, _, refs, _) = array_at(graph, 0x68).unwrap();
    let (owners, _, start, _) = array_at(graph, 16).unwrap();
    assert_eq!(count, references);
    assert!(ordinal < owners);
    assert_eq!(read_u32(graph, start + ordinal * 12).unwrap(), owner);
    let mut result = Vec::new();
    for index in 0..count {
        let at = maps + index * 40;
        if read_u32(graph, at + 16).unwrap() != owner {
            continue;
        }
        let mut map: [u8; 40] = graph[at..at + 40].try_into().unwrap();
        let at = refs + index * 24;
        let mut reference: [u8; 24] = graph[at..at + 24].try_into().unwrap();
        assert_eq!(read_u32(&reference, 0).unwrap(), owner);
        assert_eq!(read_u64(&reference, 16).unwrap(), ordinal as u64);
        map[16..20].fill(0);
        reference[..4].fill(0);
        reference[16..24].fill(0);
        result.push((map, reference));
    }
    assert!(!result.is_empty());
    result.sort_unstable();
    result
}

fn recipes(stock: &PackageManager, output: &Path, host: &[u8]) -> Vec<crate::WeaponRecipe> {
    let (count, _, owners, _) = array_at(host, 16).unwrap();
    assert_eq!(count, 3);
    let removed = read_u32(host, owners + 24).unwrap();
    let (socket, plug, perk, sockets) = super::stock::runtime_plug(stock, 0x02222CBF);
    CASES
        .iter()
        .enumerate()
        .map(|(ordinal, &(bank, replication, count))| {
            let patches = patches(stock, bank);
            assert_eq!(patches.len(), count);
            let mut effect = crate::perk::PerkRecipe::effect(perk);
            effect.program = Some(Program {
                trigger: Trigger::WeaponKill,
                actions: vec![Action::Attach {
                    asset: Asset {
                        graph: HOST,
                        ..Default::default()
                    },
                    mode: AttachmentTarget::Player,
                    keys: [0x811C9DC5; 2],
                    float_bits: [0; 4],
                }],
                native_asset_patches: vec![NativeAssetPatch {
                    action_index: 0,
                    source_graph: HOST,
                    patches: Vec::new(),
                    appends: Vec::new(),
                    remove_owners: vec![removed],
                    components: vec![NativeAssetComponent {
                        source_graph: bank,
                        patches,
                        replication: Some(replication),
                    }],
                    programs: Vec::new(),
                }],
                ..Default::default()
            });
            let mut recipe = crate::WeaponRecipe::new_weapon_for_donor(
                format!("parhelion.replicated-bank-{ordinal}"),
                0x02222CBF,
                "",
            )
            .unwrap();
            recipe.name = format!("Replicated Bank {count}");
            recipe
                .overrides
                .socket_columns
                .resize_with(sockets, || None);
            recipe.overrides.socket_columns[socket] = Some(WeaponSocketColumnRecipe {
                choices: vec![HexHash::new(plug)],
                ..Default::default()
            });
            recipe
                .overrides
                .socket_plug_variants
                .push(WeaponSocketPlugVariantRecipe {
                    projectile: None,
                    socket_index: socket.try_into().unwrap(),
                    choice_index: 0,
                    source_plug_hash: HexHash::new(plug),
                    name: Some(recipe.name.clone()),
                    replace_effects: true,
                    sandbox_perks: vec![effect],
                    investment_stats: Vec::new(),
                    classification_donor_hash: None,
                    icon: None,
                    description: None,
                    offer_everywhere: false,
                    additional_sandbox_perks: Vec::new(),
                });
            let path = output.join(format!("bank-{count}.parhelion.json"));
            recipe.save_json(&path).unwrap();
            crate::WeaponRecipe::load_json(path).unwrap()
        })
        .collect()
}

#[allow(clippy::cognitive_complexity)]
fn bank(owner: &[u8], count: usize, allocation: &[u8]) {
    let live = relative_target(owner, 16).unwrap();
    let settings = relative_target(owner, 24).unwrap();
    let (rows, _, definitions, _) = array_at(owner, settings + 88).unwrap();
    let (instances, _, values, _) = array_at(owner, live + 80).unwrap();
    let (calls, _, lifecycle, _) = array_at(owner, 48).unwrap();
    assert_eq!((rows, instances, calls), (count, count, count + 1));
    assert_eq!(read_u64(owner, 0x48).unwrap(), (settings - live) as u64);
    for row in 0..rows {
        let definition = definitions + row * 88;
        let instance = values + row * 128;
        assert_eq!(
            read_u32(owner, definition + 40).unwrap(),
            (1.5 + row as f32).to_bits()
        );
        assert_eq!(read_u64(owner, definition + 8).unwrap(), instance as u64);
        assert_eq!(read_u64(owner, instance + 8).unwrap(), definition as u64);
        assert_eq!(read_u32(owner, definition + 4).unwrap(), 0x80803B05);
        assert_eq!(read_u32(owner, instance + 4).unwrap(), 0x80803B06);
        assert_eq!(
            relative_target(owner, lifecycle + row * 16).unwrap(),
            instance
        );
        assert_eq!(
            read_u32(owner, lifecycle + row * 16 + 8).unwrap(),
            0x808045BE
        );
        assert_eq!(
            read_u32(owner, lifecycle + row * 16 + 12).unwrap(),
            0x00010000
        );
    }
    assert_eq!(
        relative_target(owner, lifecycle + rows * 16).unwrap(),
        live + 48
    );
    assert_eq!(
        read_u32(owner, lifecycle + rows * 16 + 8).unwrap(),
        0x80803B00
    );
    assert_eq!(
        read_u32(owner, lifecycle + rows * 16 + 12).unwrap(),
        0x0001FFFF
    );
    let (members, _, member, _) = array_at(allocation, 32).unwrap();
    assert_eq!(members, 1);
    assert_eq!(read_u32(allocation, member).unwrap(), 0xFB05ED04);
    let (elements, _, counted, _) = array_at(allocation, member + 24).unwrap();
    assert_eq!(elements, 1);
    assert_eq!(read_u32(allocation, counted).unwrap(), 0x587BC3F9);
    assert_eq!(read_u32(allocation, counted + 16).unwrap(), 0x80803B05);
    assert_eq!(read_u32(allocation, counted + 20).unwrap() as usize, rows);
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES, SUNDIAL_TEST_ARTIFACTS and SUNDIAL_TEST_REVISION"]
#[allow(clippy::cognitive_complexity)]
fn persisted_replicated_banks_keep_allocations_providers_and_loading() {
    let packages = crate::test_support::stock_packages();
    let output = crate::test_support::artifact_dir("replicated-bank");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::create_dir(&output).expect("use a fresh artifact directory");
    let revision = std::env::var("SUNDIAL_TEST_REVISION").expect("source revision");
    let stock = open_manager(&packages).unwrap();
    let host = stock.read_tag(TagHash(HOST)).unwrap();
    let (_, _, original_owners, _) = array_at(&host, 16).unwrap();
    let mut originals = BTreeMap::new();
    for graph_tag in std::iter::once(HOST).chain(CASES.iter().map(|case| case.0)) {
        let graph = stock.read_tag(TagHash(graph_tag)).unwrap();
        let (count, _, owners, _) = array_at(&graph, 16).unwrap();
        for index in 0..count {
            let tag = read_u32(&graph, owners + index * 12).unwrap();
            let owner = stock.read_tag(TagHash(tag)).unwrap();
            let allocation = read_u32(&owner, 0x44).unwrap();
            originals.insert(allocation, stock.read_tag(TagHash(allocation)).unwrap());
            originals.insert(tag, owner);
        }
        let replication = read_u32(&graph, 0x88).unwrap();
        if replication != u32::MAX {
            originals.insert(replication, stock.read_tag(TagHash(replication)).unwrap());
        }
        originals.insert(graph_tag, graph);
    }
    for (_, witness, _) in CASES {
        originals.insert(witness, stock.read_tag(TagHash(witness)).unwrap());
    }
    let recipes = recipes(&stock, &output, &host);
    let bundle = build_weapon_project_after_catalog_validation(
        &packages,
        &WeaponProjectSpec {
            weapons: recipes
                .iter()
                .map(|recipe| recipe.to_spec().unwrap())
                .collect(),
        },
    )
    .unwrap();
    let view = staged_view(&packages, ".parhelion-replicated-bank-", &bundle);
    let staged = open_manager(&view.path().join("packages")).unwrap();
    let runtime_map = staged
        .read_tag(TagHash(SANDBOX_PERK_RUNTIME_MAP_TAG))
        .unwrap();
    let mut private = BTreeSet::new();
    let mut records = Vec::new();
    for (ordinal, &(source_bank, witness, row_count)) in CASES.iter().enumerate() {
        let key = bundle.plan.weapons[ordinal].custom_plugs[0].perks[0].runtime_key;
        let assignment = sandbox_perk_runtime_assignment(&runtime_map, key)
            .unwrap()
            .unwrap();
        let action_bytes = staged.read_tag(TagHash(assignment.runtime_tag)).unwrap();
        let decoded = action::decode(&action_bytes).unwrap();
        let attachment = decoded.effects().find(|effect| effect.kind == 1).unwrap();
        let root_tag = read_u32(&action_bytes, attachment.offset + 16).unwrap();
        assert!(stock.get_entry(TagHash(root_tag)).is_none() && private.insert(root_tag));
        let graph = staged.read_tag(TagHash(root_tag)).unwrap();
        let (count, _, owners, _) = array_at(&graph, 16).unwrap();
        assert_eq!(count, 3);
        let replication_tag = read_u32(&graph, 0x88).unwrap();
        assert!(
            stock.get_entry(TagHash(replication_tag)).is_none() && private.insert(replication_tag)
        );
        let replication = staged.read_tag(TagHash(replication_tag)).unwrap();
        assert_eq!(
            staged
                .get_entry(TagHash(replication_tag))
                .unwrap()
                .reference,
            0x80809BB6
        );
        assert_eq!(read_u32(&replication, 8).unwrap(), root_tag);
        assert_eq!(&replication[12..48], &[0; 36]);
        let (count, _, rep_rows, cls) = array_at(&replication, 48).unwrap();
        assert_eq!((count, cls), (3, 0x80809BB8));
        let original_bank = stock.read_tag(TagHash(source_bank)).unwrap();
        let (_, _, bank_owners, _) = array_at(&original_bank, 16).unwrap();
        let bank_owner = read_u32(&original_bank, bank_owners).unwrap();
        let mut owner_records = Vec::new();
        let mut required = vec![root_tag, replication_tag];
        for index in 0..3 {
            let tag = read_u32(&graph, owners + index * 12).unwrap();
            assert!(stock.get_entry(TagHash(tag)).is_none() && private.insert(tag));
            let owner = staged.read_tag(TagHash(tag)).unwrap();
            let live = relative_target(&owner, 16).unwrap();
            let definition = relative_target(&owner, 24).unwrap();
            assert_eq!(read_u32(&owner, live).unwrap(), tag);
            assert_eq!(read_u32(&owner, definition).unwrap(), tag);
            assert_eq!(read_u64(&owner, live + 8).unwrap(), definition as u64);
            assert_eq!(read_u64(&owner, definition + 8).unwrap(), live as u64);
            let allocation = read_u32(&owner, 0x44).unwrap();
            let row = rep_rows + index * 32;
            assert_eq!(
                read_u32(&replication, row).unwrap(),
                read_u32(&owner, live - 4).unwrap()
            );
            assert_eq!(read_u32(&replication, row + 4).unwrap(), allocation);
            assert_eq!(&replication[row + 8..row + 24], &[0; 16]);
            let (before, original, source_index) = if index == 2 {
                (&original_bank, bank_owner, 0)
            } else {
                (
                    &host,
                    read_u32(&host, original_owners + index * 12).unwrap(),
                    index,
                )
            };
            assert_eq!(
                providers(&graph, tag, index),
                providers(before, original, source_index)
            );
            let allocation_bytes = staged.read_tag(TagHash(allocation)).unwrap();
            if index == 2 {
                bank(&owner, row_count, &allocation_bytes);
                let witness = stock.read_tag(TagHash(witness)).unwrap();
                let (count, _, start, _) = array_at(&witness, 48).unwrap();
                let matching = (0..count)
                    .filter(|&i| read_u32(&witness, start + i * 32 + 4).unwrap() == allocation)
                    .collect::<Vec<_>>();
                assert_eq!(matching.len(), 1);
                assert_eq!(
                    &replication[row..row + 32],
                    &witness[start + matching[0] * 32..start + (matching[0] + 1) * 32]
                );
            }
            required.extend([tag, allocation]);
            fs::write(
                output.join(format!("bank-{row_count}-owner-{index}.bin")),
                &owner,
            )
            .unwrap();
            owner_records.push(json!({"tag":tag,"allocation":allocation,"sha256":sha(&owner),"allocation_sha256":sha(&allocation_bytes)}));
            assert_eq!(
                staged.read_tag(TagHash(original)).unwrap(),
                stock.read_tag(TagHash(original)).unwrap()
            );
        }
        let (outputs, _, output_rows, _) = array_at(&graph, 32).unwrap();
        assert_eq!(outputs, 2);
        for index in 0..outputs {
            assert_eq!(
                read_u32(&graph, output_rows + index * 72 + 40).unwrap(),
                u32::MAX
            );
            assert_eq!(read_u64(&graph, output_rows + index * 72 + 48).unwrap(), 0);
        }
        let mut cache = crate::shared_tag_dependency_index::native::Companions::new();
        let (companion, loading) = crate::shared_tag_dependency_index::native::native_companion(
            &staged.package_dir,
            &staged,
            replication_tag,
            &mut cache,
        )
        .unwrap();
        assert!(stock.get_entry(*companion).is_none());
        let entries = loading_index::entries(
            &loading_index::decode(loading, companion.0, replication_tag).unwrap(),
        );
        required.push(companion.0);
        for tag in required {
            let tag = TagHash(tag);
            assert!(entries.contains(&(tag.pkg_id(), tag.entry_index())));
        }
        fs::write(
            output.join(format!("bank-{row_count}-action.bin")),
            &action_bytes,
        )
        .unwrap();
        fs::write(output.join(format!("bank-{row_count}-graph.bin")), &graph).unwrap();
        fs::write(
            output.join(format!("bank-{row_count}-replication.bin")),
            &replication,
        )
        .unwrap();
        fs::write(
            output.join(format!("bank-{row_count}-loading.bin")),
            loading,
        )
        .unwrap();
        records.push(
            json!({"rows":row_count,"action":assignment.runtime_tag,"root":root_tag,
            "replication":replication_tag,"companion":companion.0,"owners":owner_records,
            "action_sha256":sha(&action_bytes),"graph_sha256":sha(&graph),
            "replication_sha256":sha(&replication),"loading_sha256":sha(loading)}),
        );
    }
    let mut wrong = recipes[1].clone();
    wrong.overrides.socket_plug_variants[0].sandbox_perks[0]
        .program
        .as_mut()
        .unwrap()
        .native_asset_patches[0]
        .components[0]
        .replication = Some(CASES[0].1);
    let wrong_path = output.join("wrong-allocation.parhelion.json");
    wrong.save_json(&wrong_path).unwrap();
    let wrong = crate::WeaponRecipe::load_json(wrong_path).unwrap();
    assert!(
        build_weapon_project_after_catalog_validation(
            &packages,
            &WeaponProjectSpec {
                weapons: vec![wrong.to_spec().unwrap()],
            }
        )
        .is_err()
    );
    for (&tag, original) in &originals {
        assert_eq!(&staged.read_tag(TagHash(tag)).unwrap(), original);
        assert_eq!(&stock.read_tag(TagHash(tag)).unwrap(), original);
    }
    let emitted = output.join("packages");
    fs::create_dir(&emitted).unwrap();
    let files = bundle.write_new(&emitted).unwrap().into_iter().map(|path| json!({
        "file":path.file_name().unwrap().to_string_lossy(),"sha256":sha(&fs::read(&path).unwrap())
    })).collect::<Vec<_>>();
    fs::write(output.join("readback.json"), serde_json::to_vec_pretty(&json!({
        "native_build":"86657.20.08.23.1800.d2_rc___release","revision":revision,
        "executable_sha256":sha(&fs::read(std::env::current_exe().unwrap()).unwrap()),
        "source_packages":packages,"source_graph_sha256":sha(&host),
        "source_payloads":originals.iter().map(|(tag,bytes)|json!({"tag":tag,"sha256":sha(bytes)})).collect::<Vec<_>>(),
        "recipes":recipes,"cases":records,"packages":files,"stock_preserved":true,
        "limits":"Persisted recipe and native package readback. No network initialization or gameplay execution."
    })).unwrap()).unwrap();
}
