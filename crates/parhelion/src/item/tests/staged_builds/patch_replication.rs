//! Persisted scalar edits keep the native replication allocation and privatize its whole graph.
use super::*;
use crate::recipe::{HexHash, WeaponSocketColumnRecipe, WeaponSocketPlugVariantRecipe};
use serde_json::json;
use sha2::{Digest, Sha256};
use sundial::package_authoring::{
    loading_index,
    sandbox_perk::{
        action,
        program::{
            Action, Asset, AttachmentTarget, NativeAssetPatch, NativeAssetResourceAppend,
            NativeAssetResourcePatch, Program, Trigger,
        },
    },
};

const HOST: u32 = 0x80B825C0;

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn recipe(stock: &PackageManager, amount: f32) -> (crate::WeaponRecipe, u32) {
    let graph = stock.read_tag(TagHash(HOST)).unwrap();
    let bindings = weapon_component_bindings(&graph, 0x7330E39F).unwrap();
    assert_eq!(bindings.len(), 1);
    let binding = &bindings[0];
    let owner = stock.read_tag(TagHash(binding.owner_tag)).unwrap();
    let instance = binding.resource_offset as usize;
    let definition = read_u64(&owner, instance + 8).unwrap() as usize;
    assert_eq!(read_u32(&owner, instance + 4).unwrap(), 0x80803B06);
    let settings = relative_target(&owner, 24).unwrap();
    let (count, _, rows, _) = array_at(&owner, settings + 88).unwrap();
    assert_eq!((count, rows), (1, definition));
    let (socket, plug, perk, sockets) = super::stock::runtime_plug(stock, 0x02222CBF);
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
            patches: vec![NativeAssetResourcePatch {
                binding_hash: binding.binding_hash,
                resource_index: 0,
                offset: (definition + 40 - instance).try_into().unwrap(),
                expected: owner[definition + 40..definition + 44].to_vec(),
                bytes: amount.to_le_bytes().to_vec(),
                imported_particle: None,
            }],
            appends: Vec::new(),
            remove_owners: Vec::new(),
            components: Vec::new(),
            programs: Vec::new(),
        }],
        ..Default::default()
    });
    let mut recipe = crate::WeaponRecipe::new_weapon_for_donor(
        format!("parhelion.patch-replication-{}", amount.to_bits()),
        0x02222CBF,
        "",
    )
    .unwrap();
    recipe.name = format!("Private Replication {amount}");
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
    (recipe, owner.len().try_into().unwrap())
}

/// Normalize only the recorded native references, leaving complete provider and event bytes intact.
fn normalize_graph(graph: &mut [u8], owners: &BTreeMap<u32, u32>, replication: u32) {
    for (descriptor, stride, fields) in [
        (16, 12, &[0][..]),
        (32, 72, &[8, 40][..]),
        (0x58, 40, &[16][..]),
        (0x68, 24, &[0][..]),
    ] {
        let (count, _, start, _) = array_at(graph, descriptor).unwrap();
        for index in 0..count {
            for field in fields {
                let at = start + index * stride + field;
                if let Some(original) = owners.get(&read_u32(graph, at).unwrap()) {
                    graph[at..at + 4].copy_from_slice(&original.to_le_bytes());
                }
            }
        }
    }
    graph[0x88..0x8C].copy_from_slice(&replication.to_le_bytes());
}

/// Each changed identity must be a reciprocal typed native pair in the original payload.
fn normalize_owner(original: &[u8], private: &mut [u8], old: u32, new: u32) {
    assert_eq!(private.len(), original.len());
    let mut pairs = 0;
    for at in (0..original.len().saturating_sub(16)).step_by(8) {
        if read_u32(original, at).unwrap() != old {
            continue;
        }
        let target = read_u64(original, at + 8).unwrap() as usize;
        assert_eq!(read_u32(original, target).unwrap(), old);
        assert_eq!(read_u64(original, target + 8).unwrap(), at as u64);
        assert_eq!(read_u32(private, at).unwrap(), new);
        private[at..at + 4].copy_from_slice(&old.to_le_bytes());
        pairs += 1;
    }
    assert!(pairs >= 2);
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES, SUNDIAL_TEST_ARTIFACTS and SUNDIAL_TEST_REVISION"]
#[allow(clippy::cognitive_complexity)]
fn persisted_patch_only_graphs_keep_private_replication_and_native_layout() {
    let packages = crate::test_support::stock_packages();
    let output = crate::test_support::artifact_dir("patch-replication");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::create_dir(&output).expect("use a fresh artifact directory");
    let revision = std::env::var("SUNDIAL_TEST_REVISION").expect("source revision");
    let stock = open_manager(&packages).unwrap();
    let host = stock.read_tag(TagHash(HOST)).unwrap();
    let replica_tag = read_u32(&host, 0x88).unwrap();
    let replica = stock.read_tag(TagHash(replica_tag)).unwrap();
    let mut originals = BTreeMap::from([(HOST, host.clone()), (replica_tag, replica.clone())]);
    let (count, _, owner_rows, _) = array_at(&host, 16).unwrap();
    assert_eq!(count, 3);
    for index in 0..count {
        let tag = read_u32(&host, owner_rows + index * 12).unwrap();
        let owner = stock.read_tag(TagHash(tag)).unwrap();
        let allocation = read_u32(&owner, 0x44).unwrap();
        originals.insert(allocation, stock.read_tag(TagHash(allocation)).unwrap());
        originals.insert(tag, owner);
    }
    let mut recipes = Vec::new();
    for (index, amount) in [1.5, 2.5].into_iter().enumerate() {
        let path = output.join(format!("patch-{index}.parhelion.json"));
        recipe(&stock, amount).0.save_json(&path).unwrap();
        recipes.push(crate::WeaponRecipe::load_json(path).unwrap());
    }
    let project = WeaponProjectSpec {
        weapons: recipes
            .iter()
            .map(|recipe| recipe.to_spec().unwrap())
            .collect(),
    };
    let bundle = build_weapon_project_after_catalog_validation(&packages, &project).unwrap();
    let view = staged_view(&packages, ".parhelion-patch-replication-", &bundle);
    let staged = open_manager(&view.path().join("packages")).unwrap();
    let runtime_map = staged
        .read_tag(TagHash(SANDBOX_PERK_RUNTIME_MAP_TAG))
        .unwrap();
    let mut private = BTreeSet::new();
    let mut records = Vec::new();
    for (ordinal, amount) in [1.5_f32, 2.5].into_iter().enumerate() {
        let key = bundle.plan.weapons[ordinal].custom_plugs[0].perks[0].runtime_key;
        let assignment = sandbox_perk_runtime_assignment(&runtime_map, key)
            .unwrap()
            .unwrap();
        let action_bytes = staged.read_tag(TagHash(assignment.runtime_tag)).unwrap();
        let decoded = action::decode(&action_bytes).unwrap();
        let attachment = decoded.effects().find(|effect| effect.kind == 1).unwrap();
        let root = read_u32(&action_bytes, attachment.offset + 16).unwrap();
        let graph = staged.read_tag(TagHash(root)).unwrap();
        let replication_tag = read_u32(&graph, 0x88).unwrap();
        let replication = staged.read_tag(TagHash(replication_tag)).unwrap();
        assert_eq!(read_u32(&replication, 8).unwrap(), root);
        let mut normalized = replication.clone();
        normalized[8..12].copy_from_slice(&HOST.to_le_bytes());
        assert_eq!(normalized, replica);
        let (owners, _, start, _) = array_at(&graph, 16).unwrap();
        assert_eq!(owners, count);
        let mut mapping = BTreeMap::new();
        let mut required = vec![root, replication_tag];
        let mut owner_records = Vec::new();
        for index in 0..owners {
            let tag = read_u32(&graph, start + index * 12).unwrap();
            let old = read_u32(&host, owner_rows + index * 12).unwrap();
            let owner = staged.read_tag(TagHash(tag)).unwrap();
            let allocation = read_u32(&owner, 0x44).unwrap();
            required.push(tag);
            assert!(stock.get_entry(TagHash(tag)).is_none() && private.insert(tag));
            mapping.insert(tag, old);
            let mut normalized = owner.clone();
            normalize_owner(&originals[&old], &mut normalized, old, tag);
            if index == 2 {
                let settings = relative_target(&owner, 24).unwrap();
                let (rows, _, definitions, _) = array_at(&owner, settings + 88).unwrap();
                assert_eq!(rows, 1);
                assert_eq!(
                    read_u32(&owner, definitions + 40).unwrap(),
                    amount.to_bits()
                );
                normalized[definitions + 40..definitions + 44]
                    .copy_from_slice(&originals[&old][definitions + 40..definitions + 44]);
            }
            assert_eq!(normalized, originals[&old]);
            fs::write(
                output.join(format!("patch-{ordinal}-owner-{index}.bin")),
                &owner,
            )
            .unwrap();
            owner_records
                .push(json!({"tag":tag,"source":old,"allocation":allocation,"sha256":sha(&owner)}));
        }
        let mut normalized = graph.clone();
        normalize_graph(&mut normalized, &mapping, replica_tag);
        assert_eq!(normalized, host);
        assert!(stock.get_entry(TagHash(root)).is_none() && private.insert(root));
        assert!(
            stock.get_entry(TagHash(replication_tag)).is_none() && private.insert(replication_tag)
        );
        let mut cache = crate::shared_tag_dependency_index::native::Companions::new();
        let (companion, loading) = crate::shared_tag_dependency_index::native::native_companion(
            &staged.package_dir,
            &staged,
            replication_tag,
            &mut cache,
        )
        .unwrap();
        assert!(stock.get_entry(*companion).is_none() && private.insert(companion.0));
        let entries = loading_index::entries(
            &loading_index::decode(loading, companion.0, replication_tag).unwrap(),
        );
        required.push(companion.0);
        required.extend(
            owner_records
                .iter()
                .map(|record| record["allocation"].as_u64().unwrap() as u32),
        );
        for tag in required {
            let tag = TagHash(tag);
            assert!(entries.contains(&(tag.pkg_id(), tag.entry_index())));
        }
        for (name, bytes) in [
            ("action", &action_bytes),
            ("graph", &graph),
            ("replication", &replication),
            ("loading", loading),
        ] {
            fs::write(output.join(format!("patch-{ordinal}-{name}.bin")), bytes).unwrap();
        }
        records.push(json!({"root":root,"replication":replication_tag,"companion":companion.0,
            "owners":owner_records,"graph_sha256":sha(&graph),"replication_sha256":sha(&replication),"loading_sha256":sha(loading)}));
    }
    let mut grown = recipes[0].clone();
    let edit = &mut grown.overrides.socket_plug_variants[0].sandbox_perks[0]
        .program
        .as_mut()
        .unwrap()
        .native_asset_patches[0];
    edit.appends.push(NativeAssetResourceAppend {
        binding_hash: 0x7330E39F,
        resource_index: 0,
        expected_owner_size: recipe(&stock, 1.5).1,
        bytes: vec![0; 16],
    });
    let path = output.join("growth-rejected.parhelion.json");
    grown.save_json(&path).unwrap();
    let grown = crate::WeaponRecipe::load_json(path).unwrap();
    let rejected = build_weapon_project_after_catalog_validation(
        &packages,
        &WeaponProjectSpec {
            weapons: vec![grown.to_spec().unwrap()],
        },
    )
    .unwrap_err();
    assert!(rejected.to_string().contains("native allocation layout"));
    let rebuilt = build_weapon_project_after_catalog_validation(&packages, &project).unwrap();
    assert_eq!(
        bundle
            .artifacts
            .iter()
            .map(|artifact| sha(artifact.bytes()))
            .collect::<Vec<_>>(),
        rebuilt
            .artifacts
            .iter()
            .map(|artifact| sha(artifact.bytes()))
            .collect::<Vec<_>>()
    );
    for (&tag, original) in &originals {
        assert_eq!(&stock.read_tag(TagHash(tag)).unwrap(), original);
        assert_eq!(&staged.read_tag(TagHash(tag)).unwrap(), original);
    }
    let emitted = output.join("packages");
    fs::create_dir(&emitted).unwrap();
    let files = bundle
        .write_new(&emitted)
        .unwrap()
        .into_iter()
        .map(|path| {
            json!({
        "file":path.file_name().unwrap().to_string_lossy(),"sha256":sha(&fs::read(&path).unwrap())})
        })
        .collect::<Vec<_>>();
    fs::write(output.join("readback.json"), serde_json::to_vec_pretty(&json!({
        "native_build":"86657.20.08.23.1800.d2_rc___release","revision":revision,
        "executable_sha256":sha(&fs::read(std::env::current_exe().unwrap()).unwrap()),
        "source_packages":packages,"source_payloads":originals.iter().map(|(tag,bytes)|json!({"tag":tag,"sha256":sha(bytes)})).collect::<Vec<_>>(),
        "recipes":recipes,"cases":records,"packages":files,"stock_preserved":true,"growth_rejected":true,
        "limits":"Persisted recipes and staged native package readback. No live activation or network execution."
    })).unwrap()).unwrap();
}
