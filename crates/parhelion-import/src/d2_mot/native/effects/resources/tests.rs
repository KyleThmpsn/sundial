//! Package-backed resource table verification. The native oracle is actual exported
//! Shadowkeep data. Source rows are also checked through a separate native reader.
//! Opt in with PARHELION_MOVEMENT_CORPUS and PARHELION_TABLE_OUTPUT.
use super::*;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn maps(corpus: &Path) -> (BTreeMap<u32, u32>, BTreeMap<u64, u32>) {
    let tags = read_json(&corpus.join("table-references.json"))
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| {
            (
                u32::from_str_radix(key, 16).unwrap(),
                u32::from_str_radix(value.as_str().unwrap(), 16).unwrap(),
            )
        })
        .collect();
    let hashes = read_json(&corpus.join("table-tag64.json"))["hashes"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| {
            (
                u64::from_str_radix(key, 16).unwrap(),
                u32::from_str_radix(value.as_str().unwrap(), 16).unwrap(),
            )
        })
        .collect();
    (tags, hashes)
}

/// Decode all branches independently of the writer, preserving order and flags.
fn contents(
    p: &Payload,
    modern: bool,
    tags: &BTreeMap<u32, u32>,
    hashes: &BTreeMap<u64, u32>,
) -> Value {
    let mut branches = Vec::new();
    for descriptor in [0x80, 0x98, 0xB0] {
        let mut groups = Vec::new();
        for group in p
            .array(
                descriptor,
                24,
                Some(if modern { 0x80808747 } else { 0x80808BD5 }),
            )
            .unwrap()
        {
            let mut rows = Vec::new();
            for row in p
                .array(
                    group + 8,
                    32,
                    Some(if modern { 0x80808749 } else { 0x80808BD7 }),
                )
                .unwrap()
            {
                let refs = p
                    .array(
                        row + 8,
                        if modern { 32 } else { 4 },
                        Some(if modern { 0x8080BC7B } else { 0x80800014 }),
                    )
                    .unwrap()
                    .into_iter()
                    .map(|at| {
                        let mut tag = p.u32(at).unwrap();
                        if modern {
                            let hash = p.u64(at + 8).unwrap();
                            if tag == u32::MAX && p.u32(at + 4).unwrap() == 0 && hash != 0 {
                                tag = hashes[&hash];
                            }
                            if tag != 0 && tag != u32::MAX {
                                tag = tags.get(&tag).copied().unwrap_or(u32::MAX);
                            }
                        }
                        tag
                    })
                    .collect::<Vec<_>>();
                rows.push(json!({"name": p.u64(row).unwrap(), "flags": p.u64(row + 24).unwrap(), "references": refs}));
            }
            groups.push(json!({"kind": p.u64(group).unwrap(), "rows": rows}));
        }
        branches.push(json!({"flags": p.u64(descriptor + 16).unwrap(), "groups": groups}));
    }
    json!(branches)
}

#[test]
#[ignore = "needs exported resource table corpus and PARHELION_TABLE_OUTPUT"]
fn resource_tables_preserve_source_rows_and_match_native_oracles() {
    let corpus = PathBuf::from(std::env::var_os("PARHELION_MOVEMENT_CORPUS").unwrap());
    let output = PathBuf::from(std::env::var_os("PARHELION_TABLE_OUTPUT").unwrap());
    fs::create_dir_all(&output).unwrap();
    let (tags, hashes) = maps(&corpus);
    let golden = read_json(&corpus.join("table-golden-pairs.json"));
    let pairs = read_json(&corpus.join("table-pairs.json"));
    assert!(!pairs.as_array().unwrap().is_empty());
    assert!(!golden.as_array().unwrap().is_empty());
    assert!(
        golden
            .as_array()
            .unwrap()
            .iter()
            .all(|pair| pairs.as_array().unwrap().contains(pair))
    );
    let mut report = Vec::new();
    for pair in pairs.as_array().unwrap() {
        let modern = pair[0].as_str().unwrap();
        let native = pair[1].as_str().unwrap();
        let source = Payload(fs::read(corpus.join(format!("tables/modern/{modern}.bin"))).unwrap());
        let expected = fs::read(corpus.join(format!("tables/shadowkeep/{native}.bin"))).unwrap();
        let result = emit(&source, &tags, &hashes).unwrap();
        assert_eq!(
            result.payload.u64(0).unwrap() as usize,
            result.payload.0.len()
        );
        assert_eq!(result.payload.0[8..0x80], source.0[8..0x80]);
        assert_eq!(
            contents(&result.payload, false, &tags, &hashes),
            contents(&source, true, &tags, &hashes),
            "{modern}"
        );
        for reference in &result.references {
            assert_eq!(
                result.payload.u32(reference.offset).unwrap(),
                reference.target.unwrap_or(u32::MAX)
            );
            assert_eq!(reference.target, tags.get(&reference.source).copied());
        }
        if golden.as_array().unwrap().contains(pair) {
            assert_eq!(
                result.payload.0, expected,
                "native resource table oracle {modern} -> {native}"
            );
        }
        fs::write(output.join(format!("{modern}.bin")), &result.payload.0).unwrap();
        report.push(
            json!({"modern": modern, "native": native, "identical": result.payload.0 == expected,
            "source_rows_preserved": true, "references": result.references,
            "installable": false, "gameplay_verified": false}),
        );
    }
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!(
        "Verified {} resource tables, {} byte identical. Artifacts: {}",
        report.len(),
        report.iter().filter(|r| r["identical"] == true).count(),
        output.display()
    );
}

#[test]
#[ignore = "needs exported resource table corpus"]
fn malformed_resource_tables_are_refused_and_missing_dependencies_are_recorded() {
    let corpus = PathBuf::from(std::env::var_os("PARHELION_MOVEMENT_CORPUS").unwrap());
    let (tags, hashes) = maps(&corpus);
    let source = Payload(fs::read(corpus.join("tables/modern/80A79648.bin")).unwrap());
    let group = source.array(0x80, 24, Some(0x80808747)).unwrap()[0];
    let row = source.array(group + 8, 32, Some(0x80808749)).unwrap()[0];
    let leaf = source.array(row + 8, 32, Some(0x8080BC7B)).unwrap()[0];
    let cases = [
        (0, 0u64.to_le_bytes().to_vec()),
        (0x88, i64::MIN.to_le_bytes().to_vec()),
        (0x80, u64::MAX.to_le_bytes().to_vec()),
        (
            source.pointer(0x88).unwrap() + 8,
            0x80808749u32.to_le_bytes().to_vec(),
        ),
        (
            source.pointer(0x88).unwrap() - 4,
            0u32.to_le_bytes().to_vec(),
        ),
        (leaf + 16, 1u32.to_le_bytes().to_vec()),
        (leaf + 4, 3u32.to_le_bytes().to_vec()),
    ];
    for (at, bytes) in cases {
        let mut damaged = source.clone();
        damaged.0[at..at + bytes.len()].copy_from_slice(&bytes);
        assert!(
            emit(&damaged, &tags, &hashes).is_err(),
            "malformed field +{at:X} accepted"
        );
    }
    assert!(emit(&source, &tags, &BTreeMap::new()).is_err());
    let unresolved = emit(&source, &BTreeMap::new(), &hashes).unwrap();
    assert!(!unresolved.references.is_empty());
    assert!(
        unresolved
            .references
            .iter()
            .all(|reference| reference.target.is_none()
                && unresolved.payload.u32(reference.offset).unwrap() == u32::MAX)
    );
}
