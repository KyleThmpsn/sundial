//! Independent package emission and merge readback for the runtime map's loading bitmap.
//! Failure inventory before implementation: several members consume the same stock slack,
//! appended words disappear from the sidecar, a moved descriptor names the stale bitmap,
//! an enabled subset or load order loses rows, stock mappings change, or the native bit writes
//! reach the following allocation. This uses real stock data through an explicitly configured
//! path and leaves packages, sidecars, merged payloads and a readback receipt.
use super::*;
use sundial::package_authoring::mod_content::{merge, plaintext};
use tiger_pkg::Package;

const MAP: TagHash = TagHash(0x80EC_3F60);

fn array(bytes: &[u8], descriptor: usize, stride: usize) -> (usize, usize) {
    let count = u64::from_le_bytes(bytes[descriptor..descriptor + 8].try_into().unwrap()) as usize;
    let pointer = descriptor + 8;
    let relative = i64::from_le_bytes(bytes[pointer..pointer + 8].try_into().unwrap());
    let header = (pointer as i64 + relative) as usize;
    assert_eq!(&bytes[header - 4..header], &0x8080_9FBDu32.to_le_bytes());
    assert_eq!(&bytes[header..header + 8], &(count as u64).to_le_bytes());
    let rows = header + 16;
    assert!(rows + count * stride <= bytes.len());
    (count, rows)
}

fn mappings(bytes: &[u8]) -> BTreeMap<u32, u32> {
    let (count, rows) = array(bytes, 8, 8);
    let entries = bytes[rows..rows + count * 8]
        .chunks_exact(8)
        .map(|row| {
            (
                u32::from_le_bytes(row[..4].try_into().unwrap()),
                u32::from_le_bytes(row[4..].try_into().unwrap()),
            )
        })
        .collect::<Vec<_>>();
    assert!(entries.windows(2).all(|pair| pair[0].0 < pair[1].0));
    entries.into_iter().collect()
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES and a fresh SUNDIAL_TEST_ARTIFACTS directory"]
#[allow(
    clippy::cognitive_complexity,
    reason = "Keeps the package lifecycle and independent merged readback together"
)]
fn independent_mods_keep_loading_bits_for_every_merged_runtime_name() {
    let source = crate::test_support::stock_packages();
    let output = crate::test_support::artifact_dir("runtime-name-capacity");
    fs::create_dir(&output).expect("choose a fresh artifact directory");
    let manager = open_manager(&source).unwrap();
    let stock = manager.read_tag(MAP).unwrap();
    let original = mappings(&stock);
    let (stock_words, _) = array(&stock, 24, 4);
    let slack = stock_words * 32 - original.len();
    assert!(
        slack > 0 && slack < 32,
        "configured corpus must exercise shared partial-word slack"
    );
    let entity = *original
        .values()
        .next()
        .expect("the native runtime map must not be empty");
    let _profile = crate::package::profile::hold(
        crate::package::profile::Window::FULL,
        crate::package::profile::Host::standalone(0x0CFE),
    );
    let mut next_key = 0xF100_0000u32;
    let mut members = Vec::new();
    let mut receipts = Vec::new();
    for (member, added) in [slack, slack, slack + 32].into_iter().enumerate() {
        let mut built = stock.clone();
        let mut expected = BTreeMap::new();
        while expected.len() < added {
            next_key = next_key.checked_add(1).unwrap();
            if original.contains_key(&next_key) {
                continue;
            }
            expected.insert(next_key, entity);
            built = append_weapon_entity_assignment(built, next_key, entity).unwrap();
        }
        let artifact = crate::package::extend::build_extended_overlay(
            &source,
            MAP.pkg_id(),
            &[crate::ReplacementSpec {
                tag: MAP,
                payload: built,
            }],
            &[],
        )
        .unwrap();
        let directory = output.join(format!("member-{member}"));
        let path = artifact.write_new(&directory).unwrap();
        let name = path.file_name().unwrap().to_str().unwrap();
        let data = crate::package::merge_data::describe(&source, &[(name, artifact.bytes())]);
        assert_eq!(
            data.sidecars.len(),
            1,
            "{}",
            String::from_utf8_lossy(&data.report)
        );
        fs::write(directory.join("merge-data.json"), &data.report).unwrap();
        let (side_name, side_bytes) = &data.sidecars[0];
        fs::write(directory.join(side_name), side_bytes).unwrap();
        let records = plaintext::records_in(artifact.bytes()).unwrap();
        let sidecar = merge::sidecar::Sidecar::parse(
            side_bytes,
            records.package,
            records.patch,
            &records.digest,
        )
        .unwrap();
        let package = tiger_pkg::PackageD2PreBL::open(path.to_str().unwrap()).unwrap();
        let payload = package.read_entry(MAP.entry_index() as usize).unwrap();
        let mut all = original.clone();
        all.extend(&expected);
        assert_eq!(mappings(&payload), all);
        receipts.push(serde_json::json!({"package": path, "digest": crate::artifact::digest_file(&path).unwrap(),
            "sidecar": side_name, "sidecar_digest": crate::artifact::digest_bytes(side_bytes), "added": added}));
        members.push((payload, sidecar, expected));
    }
    let mut combinations = Vec::new();
    for mask in 1u8..8 {
        for reverse in [false, true] {
            let mut selected = (0..3)
                .filter(|index| mask & (1 << index) != 0)
                .collect::<Vec<_>>();
            if reverse {
                selected.reverse();
            }
            let entry = merge::EntryInputs {
                package: MAP.pkg_id(),
                entry: u32::from(MAP.entry_index()),
                inputs: selected
                    .iter()
                    .map(|&owner| merge::Input {
                        owner,
                        bytes: &members[owner].0,
                        sidecar: Some(&members[owner].1),
                    })
                    .collect(),
            };
            let merged = merge::merge(&[entry]);
            assert!(
                merged.fallbacks.is_empty()
                    && merged.ignored.is_empty()
                    && merged.dropped.is_empty()
            );
            assert!(merged.rows_off.is_none());
            let bytes = merged
                .replacements
                .iter()
                .find(|r| r.package == MAP.pkg_id() && r.entry == u32::from(MAP.entry_index()))
                .map_or(members[selected[0]].0.as_slice(), |r| r.bytes.as_slice());
            let mut expected = original.clone();
            for &member in &selected {
                expected.extend(&members[member].2);
            }
            assert_eq!(mappings(bytes), expected);
            let (words, start) = array(bytes, 24, 4);
            assert!(
                expected.len() <= words * 32,
                "{selected:?}: {} rows exceed {words} words",
                expected.len()
            );
            assert!(
                bytes[start..start + words * 4]
                    .iter()
                    .all(|byte| *byte == 0)
            );
            // Apply the native consumer's one-bit-per-row writes beside a canary. Capacity
            // comes from the emitted descriptor, never from the producer's sizing helper.
            let mut loaded = vec![0u8; words * 4];
            loaded.extend_from_slice(&[0xA5; 16]);
            for index in 0..expected.len() {
                loaded[index / 8] |= 1 << (index % 8);
            }
            assert_eq!(&loaded[words * 4..], &[0xA5; 16]);
            let file = format!("merged-{mask}-{reverse}.bin");
            fs::write(output.join(&file), bytes).unwrap();
            combinations.push(serde_json::json!({"members": selected, "file": file,
                "rows": expected.len(), "words": words, "digest": crate::artifact::digest_bytes(bytes)}));
        }
    }
    assert_eq!(manager.read_tag(MAP).unwrap(), stock);
    fs::write(output.join("readback.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "revision": std::env::var("SUNDIAL_TEST_REVISION").ok(),
        "executable": crate::artifact::digest_file(&std::env::current_exe().unwrap()).unwrap(),
        "stock_map": crate::artifact::digest_bytes(&stock), "stock_rows": original.len(), "stock_words": stock_words,
        "packages": receipts, "combinations": combinations,
        "limits": ["Native package emission with the compatible loader merge reader", "No live game execution"]
    })).unwrap()).unwrap();
}
