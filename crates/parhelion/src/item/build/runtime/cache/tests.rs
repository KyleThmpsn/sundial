//! Persist, relocate, emit, then independently follow sound and clip references.
//! The isolation failure model is retained in the build follow-up investigation.
use super::*;
use serde_json::{Value, json};

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

// Wwise v113 chunk and object traversal, independent of the cache's reference discovery.
fn media_fields(bank: &[u8]) -> Vec<usize> {
    assert_eq!(&bank[..4], b"BKHD");
    assert_eq!(word(bank, 8), 113);
    let mut result = Vec::new();
    let mut chunk = 0;
    while chunk < bank.len() {
        let end = chunk + 8 + word(bank, chunk + 4) as usize;
        assert!(end <= bank.len());
        if &bank[chunk..chunk + 4] == b"HIRC" {
            let mut object = chunk + 12;
            for _ in 0..word(bank, chunk + 8) {
                if bank[object] == 2 {
                    assert_eq!(bank[object + 13], 2);
                    result.push(object + 14);
                }
                object += 5 + word(bank, object + 1) as usize;
            }
            assert_eq!(object, end);
        }
        chunk = end;
    }
    assert!(
        !result.is_empty(),
        "The fixture needs streamed sound sources"
    );
    result
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES, PARHELION_CACHE_IMPORT_GRAPH and fresh SUNDIAL_TEST_ARTIFACTS"]
#[allow(clippy::cognitive_complexity)]
fn imported_fragment_reopens_across_packages_and_rejects_changed_inputs() {
    let packages = crate::test_support::stock_packages();
    let graph = PathBuf::from(std::env::var_os("PARHELION_CACHE_IMPORT_GRAPH").unwrap());
    let output = crate::test_support::artifact_dir("imported-cache-relocation");
    assert!(!output.exists(), "Use a fresh output directory");
    fs::create_dir_all(output.join("inputs")).unwrap();
    let manifest = fs::read(graph.join("asset-graph.json")).unwrap();
    let graph_value: Value = serde_json::from_slice(&manifest).unwrap();
    let event = &graph_value["audio"]["matched_events"][0];
    let native = &event["native_sounds"][0];
    let source = &event["sounds"][0];
    let hex = |v: &Value| u32::from_str_radix(v.as_str().unwrap(), 16).unwrap();
    let manager = open_manager(&packages).unwrap();
    let id = crate::package_profile::PARHELION_ASSET_PACKAGE_ID;
    let allocated = |package, ordinal| {
        AppendedTagAllocator::new(package, 0)
            .assigned_tag(ordinal, "Fixture", "asset")
            .unwrap()
            .0
    };
    let mut tags = Vec::new();
    let mut native_inputs = BTreeMap::new();
    let mut ids = BTreeMap::new();
    let mut inputs = BTreeMap::new();
    fs::write(output.join("inputs/asset-graph.json"), &manifest).unwrap();
    inputs.insert(PathBuf::from("asset-graph.json"), hash(&manifest));
    let native_media = native["media"].as_array().expect("Nonempty native media");
    let imported_media = source["media"].as_array().expect("Nonempty imported media");
    assert_eq!(
        native_media.len(),
        imported_media.len(),
        "Choose matching variation counts"
    );
    assert!(!native_media.is_empty());
    let mut changed_file = None;
    for (index, media) in imported_media.iter().enumerate() {
        let row = graph_value["audio"]["transcoded_media"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["source_tag"] == *media)
            .expect("Converted PCM must be present");
        let payload = fs::read(graph.join(row["file"].as_str().unwrap())).unwrap();
        assert!(payload.len() > 44 && &payload[..4] == b"RIFF");
        let name = PathBuf::from(format!("media-{index}.wem"));
        fs::write(output.join("inputs").join(&name), &payload).unwrap();
        inputs.insert(name.clone(), hash(&payload));
        changed_file.get_or_insert(name);
        ids.insert(hex(&native["media_ids"][index]), allocated(id, index));
        tags.push(NewTagSpec {
            template_tag: TagHash(hex(&native_media[index])),
            payload,
            storage: crate::NewTagStorageMode::AudioMedia,
        });
    }
    let mut bank = manager.read_tag(TagHash(hex(&native["bank"]))).unwrap();
    native_inputs.insert(hex(&native["bank"]), hash(&bank));
    for offset in media_fields(&bank) {
        let replacement = ids[&word(&bank, offset)];
        bank[offset..offset + 4].copy_from_slice(&replacement.to_le_bytes());
    }
    let bank_index = tags.len();
    tags.push(NewTagSpec {
        template_tag: TagHash(hex(&native["bank"])),
        payload: bank,
        storage: crate::NewTagStorageMode::AudioBank,
    });
    let mut sound = manager.read_tag(TagHash(hex(&native["tag"]))).unwrap();
    native_inputs.insert(hex(&native["tag"]), hash(&sound));
    assert_eq!(word(&sound, 0x40) as usize, native_media.len());
    sound[0x14..0x18].copy_from_slice(&allocated(id, bank_index).to_le_bytes());
    for index in 0..native_media.len() {
        sound[0x50 + index * 4..0x54 + index * 4]
            .copy_from_slice(&allocated(id, index).to_le_bytes());
    }
    let sound_index = tags.len();
    tags.push(NewTagSpec {
        template_tag: TagHash(hex(&native["tag"])),
        payload: sound,
        storage: crate::NewTagStorageMode::InheritTemplate,
    });
    let clip = &graph_value["animation"]["first_person"]["clips"][0];
    let clip_payload = fs::read(graph.join(clip["file"].as_str().unwrap())).unwrap();
    fs::write(output.join("inputs/clip.bin"), &clip_payload).unwrap();
    inputs.insert(PathBuf::from("clip.bin"), hash(&clip_payload));
    let clip_index = tags.len();
    tags.push(NewTagSpec {
        template_tag: TagHash(clip["native"].as_u64().unwrap() as u32),
        payload: clip_payload.clone(),
        storage: crate::NewTagStorageMode::InheritTemplate,
    });
    let expected = tags
        .iter()
        .map(|tag| hash(&tag.payload))
        .collect::<Vec<_>>();
    let verified = RefCell::new(BTreeMap::new());
    let make_pending = |moved: bool| Pending {
        key: hash(b"persisted imported graph"),
        state: hash(if moved { b"moved" } else { b"seed" }),
        path: output.join("fragment.runtime"),
        allocator: AppendedTagAllocator::new(id + 2, 0),
        host_start: 0,
        asset_starts: if moved { vec![8191] } else { vec![] },
        group_start: 0,
        placed_start: 0,
        impact_start: 0,
        animation: BTreeMap::new(),
        assignments: vec![],
        verified: &verified,
        import_root: Some(output.join("inputs")),
        identity: None,
        timings: RefCell::new(Timings::default()),
        returns: false,
    };
    let mut seed = crate::asset_packages::AssetPackages::default();
    let group = seed
        .reserve_group(tags.iter().map(|tag| tag.payload.len()))
        .unwrap();
    seed.packages[group].tags = tags;
    let mut placed = vec![
        TagHash(allocated(id, sound_index)),
        TagHash(allocated(id, clip_index)),
    ];
    let mut impacts = vec![];
    let mut symbols = BTreeMap::new();
    let mut animation = BTreeMap::from([(
        (0x8080_8F49, hash(&clip_payload)),
        allocated(id, clip_index),
    )]);
    make_pending(false)
        .save(
            &manager,
            native_inputs.clone(),
            (None, None),
            &[],
            &RuntimeAssets {
                packages: &mut seed,
                placed: &mut placed,
                impacts: &mut impacts,
                particle_symbols: &mut symbols,
            },
            &[],
            &animation,
            Some(inputs),
        )
        .expect("The fragment must persist");
    // Occupy the first package with unreferenced raw slots. The imported group must move whole.
    let prefix = || {
        crate::asset_packages::AssetPackages::primary(
            (0..8191)
                .map(|_| NewTagSpec {
                    template_tag: TagHash(hex(&native_media[0])),
                    payload: vec![0],
                    storage: crate::NewTagStorageMode::AudioMedia,
                })
                .collect(),
            vec![],
        )
        .unwrap()
    };
    let mut assets = prefix();
    placed.clear();
    animation.clear();
    let mut host = vec![];
    let mut assignments = vec![];
    let pending = make_pending(true);
    pending
        .restore(
            &manager,
            &mut host,
            &mut RuntimeAssets {
                packages: &mut assets,
                placed: &mut placed,
                impacts: &mut impacts,
                particle_symbols: &mut symbols,
            },
            &mut assignments,
            &mut animation,
        )
        .expect("Supported imports must relocate");
    assert_eq!(assets.packages.len(), 2);
    assert_eq!(
        placed,
        [
            TagHash(allocated(id + 1, sound_index)),
            TagHash(allocated(id + 1, clip_index))
        ]
    );
    let destination = output.join("packages");
    fs::create_dir(&destination).unwrap();
    let mut emitted = BTreeMap::new();
    for package in &assets.packages {
        let profile = crate::package_profile::authored_package(package.id).unwrap();
        let artifact = crate::extend::build_standalone_package_with_references(
            &packages,
            package.id,
            profile.file_name,
            &package.tags,
            &package.references,
        )
        .unwrap();
        fs::write(destination.join(profile.file_name), artifact.bytes()).unwrap();
        emitted.insert(
            profile.file_name,
            hex::encode(Sha256::digest(artifact.bytes())),
        );
    }
    let reopened = open_manager(&destination).unwrap();
    let sound = reopened.read_tag(placed[0]).unwrap();
    let bank_tag = TagHash(word(&sound, 0x14));
    assert_eq!(bank_tag.pkg_id(), id + 1);
    let bank = reopened.read_tag(bank_tag).unwrap();
    let media_tags = (0..word(&sound, 0x40) as usize)
        .map(|i| word(&sound, 0x50 + i * 4))
        .collect::<BTreeSet<_>>();
    let bank_media = media_fields(&bank)
        .into_iter()
        .map(|at| word(&bank, at))
        .collect::<BTreeSet<_>>();
    assert_eq!(media_tags, bank_media);
    for (index, tag) in media_tags.iter().enumerate() {
        assert_eq!(TagHash(*tag).pkg_id(), id + 1);
        assert_eq!(
            hash(&reopened.read_tag(TagHash(*tag)).unwrap()),
            expected[index]
        );
    }
    assert_eq!(reopened.read_tag(placed[1]).unwrap(), clip_payload);
    // A changed sharing decision must leave all caller state untouched before recompilation.
    let mut rejected = prefix();
    let mut rejected_placed = vec![];
    let mut shared = BTreeMap::from([((0x8080_8F49, hash(&clip_payload)), allocated(id, 0))]);
    let shared_before = shared.clone();
    assert!(
        pending
            .restore(
                &manager,
                &mut host,
                &mut RuntimeAssets {
                    packages: &mut rejected,
                    placed: &mut rejected_placed,
                    impacts: &mut impacts,
                    particle_symbols: &mut symbols,
                },
                &mut assignments,
                &mut shared
            )
            .is_err()
    );
    assert_eq!(rejected.packages.len(), 1);
    assert_eq!(rejected.packages[0].tags.len(), 8191);
    assert!(host.is_empty() && assignments.is_empty() && rejected_placed.is_empty());
    assert_eq!(shared, shared_before);
    let changed = output.join("inputs").join(changed_file.unwrap());
    let mut bytes = fs::read(&changed).unwrap();
    let before = hash(&bytes);
    *bytes.last_mut().unwrap() ^= 1;
    fs::write(&changed, &bytes).unwrap();
    shared.clear();
    assert!(
        pending
            .restore(
                &manager,
                &mut host,
                &mut RuntimeAssets {
                    packages: &mut rejected,
                    placed: &mut rejected_placed,
                    impacts: &mut impacts,
                    particle_symbols: &mut symbols,
                },
                &mut assignments,
                &mut shared
            )
            .is_err()
    );
    assert_eq!(rejected.packages.len(), 1);
    assert!(
        host.is_empty()
            && assignments.is_empty()
            && rejected_placed.is_empty()
            && shared.is_empty()
    );
    fs::write(output.join("verification.json"), serde_json::to_vec_pretty(&json!({
        "native_build":"86657.20.08.23.1800.d2_rc", "source_graph":graph,
        "source_packages":packages,"executable_sha256":crate::artifact::digest_file(&std::env::current_exe().unwrap()).unwrap(),
        "packages":emitted,"sound":placed[0].0,"clip":placed[1].0,"bank":bank_tag.0,"native_inputs":native_inputs,
        "media":media_tags,"pcm_size":bytes.len(),"before":before,"after":hash(&bytes),
        "shared_clip_rejection_preserved_state":true,"same_size_edit_rejected":true,
        "repeat_filter":"item::build::runtime::cache::tests::imported_fragment_reopens_across_packages_and_rejects_changed_inputs",
        "limits":"Persisted fragment and package readback with configured imported PCM and clip payloads. No whole importer, runtime playback or gameplay claim."
    })).unwrap()).unwrap();
}
