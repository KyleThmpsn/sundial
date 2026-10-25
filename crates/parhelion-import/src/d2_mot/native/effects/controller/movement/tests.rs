//! The corpus oracle: every projectile that exists in both game versions with identical movement
//! identities is rewritten from its modern owner and compared, record by record, with the real
//! native owner. Opt in with `PARHELION_MOVEMENT_CORPUS`, a directory holding
//! `modern-export/raw`, `shadowkeep-export/raw` (the exported owners and allocations),
//! `field-map.json` (the exact pairs), the two `projectiles.json` surveys, `references.json`
//! and `sub-references.json` (modern tags and 64-bit hashes to native tags), `tag64.json` (the
//! hashes resolved to modern tags) and `category-correspondence.json` (the dictionary index map).
use super::*;
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::PathBuf};

fn corpus() -> Option<PathBuf> {
    std::env::var_os("PARHELION_MOVEMENT_CORPUS").map(PathBuf::from)
}

fn read_json(path: PathBuf) -> Value {
    serde_json::from_slice(&fs::read(&path).unwrap_or_else(|_| panic!("read {}", path.display())))
        .unwrap()
}

fn hex_map(value: &Value) -> impl Iterator<Item = (u64, u32)> + '_ {
    value.as_object().unwrap().iter().map(|(key, value)| {
        (
            u64::from_str_radix(key, 16).unwrap(),
            u32::from_str_radix(value.as_str().unwrap(), 16).unwrap(),
        )
    })
}

/// The resolver over the corpus maps: modern tags and 64-bit hashes to native tags, the hashes
/// to their modern tags, and the category dictionary correspondence.
fn resources(corpus: &std::path::Path) -> Resources {
    let mut tags: BTreeMap<u32, u32> = hex_map(&read_json(corpus.join("references.json")))
        .map(|(k, v)| (k as u32, v))
        .collect();
    let hashes: BTreeMap<u64, u32> =
        hex_map(&read_json(corpus.join("tag64.json"))["hashes"]).collect();
    for file in ["sub-references.json", "head-references.json"] {
        for (key, native) in hex_map(&read_json(corpus.join(file))) {
            let modern = if key > u64::from(u32::MAX) {
                hashes[&key]
            } else {
                key as u32
            };
            tags.insert(modern, native);
        }
    }
    let correspondence = read_json(corpus.join("category-correspondence.json"));
    let mut categories = vec![None; correspondence["source"]["names"].as_array().unwrap().len()];
    for (modern, native) in correspondence["mapping"].as_object().unwrap() {
        categories[modern.parse::<usize>().unwrap()] =
            Some(u16::try_from(native.as_u64().unwrap()).unwrap());
    }
    Resources {
        tags,
        hashes,
        categories,
    }
}

fn movement_tags(survey: &Value) -> BTreeMap<String, String> {
    survey["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| !row["movement"].is_null())
        .map(|row| {
            (
                row["entity"].as_str().unwrap().to_owned(),
                row["movement"]["tag"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[test]
#[ignore = "needs PARHELION_MOVEMENT_CORPUS with the exported projectile corpus"]
#[expect(
    clippy::cognitive_complexity,
    reason = "The opt-in corpus oracle keeps conversion, independent comparisons and artifact emission in one repeatable sweep"
)]
fn exact_pairs_convert_and_report_native_differences() {
    let Some(corpus) = corpus() else {
        return;
    };
    let pairs = read_json(corpus.join("field-map.json"));
    let modern = movement_tags(&read_json(corpus.join("modern/projectiles.json")));
    let native = movement_tags(&read_json(corpus.join("shadowkeep/projectiles.json")));
    let resources = resources(&corpus);
    let template = Payload(fs::read(corpus.join("shadowkeep-export/raw/815282E7.bin")).unwrap());
    let template_allocation =
        Payload(fs::read(corpus.join("shadowkeep-export/raw/815282E9.bin")).unwrap());
    let mut seen = std::collections::BTreeSet::new();
    let mut converted = 0;
    let mut failures = Vec::new();
    let mut identical = 0;
    let mut diffs: BTreeMap<u32, BTreeMap<usize, usize>> = BTreeMap::new();
    let mut words: BTreeMap<u32, usize> = BTreeMap::new();
    let mut size_pairs = Vec::new();
    for pair in pairs["exact_pairs"].as_array().unwrap() {
        let (me, ne) = (pair[0].as_str().unwrap(), pair[1].as_str().unwrap());
        if !seen.insert(ne.to_owned()) {
            continue;
        }
        let source = Payload(
            fs::read(corpus.join(format!("modern-export/raw/{}.bin", modern[me]))).unwrap(),
        );
        let expected = Payload(
            fs::read(corpus.join(format!("shadowkeep-export/raw/{}.bin", native[ne]))).unwrap(),
        );
        let owner_tag = expected.u32(expected.pointer(16).unwrap()).unwrap();
        let allocation_tag = expected.u32(0x44).unwrap();
        let result = match emit(
            &source,
            &template,
            &template_allocation,
            &resources,
            owner_tag,
            allocation_tag,
        ) {
            Ok(result) => result,
            Err(error) => {
                failures.push(format!("{me} -> {ne}: {error:#}"));
                continue;
            }
        };
        converted += 1;
        assert!(!result.objects.is_empty(), "movement owner has no objects");
        // Entity references name the declared type of each reciprocal object.
        // The class stored at +4 instead names its paired object.
        for relocation in &result.objects {
            let from = relocation.source.offset as usize;
            let to = relocation.target.offset as usize;
            let source_twin = source.u64(from + 8).unwrap() as usize;
            let native_twin = result.owner.u64(to + 8).unwrap() as usize;
            assert_eq!(
                relocation.source.class,
                source.u32(source_twin + 4).unwrap()
            );
            assert_eq!(
                relocation.target.class,
                result.owner.u32(native_twin + 4).unwrap()
            );
        }
        size_pairs.push((result.owner.0.len(), expected.0.len()));
        if let Some(dump) = std::env::var_os("PARHELION_MOVEMENT_DUMP") {
            let dump = PathBuf::from(dump);
            fs::create_dir_all(&dump).unwrap();
            fs::write(dump.join(format!("{}.bin", native[ne])), &result.owner.0).unwrap();
            fs::write(
                dump.join(format!("{}-allocation.bin", native[ne])),
                &result.allocation.0,
            )
            .unwrap();
        }
        if result.owner.0 == expected.0 {
            identical += 1;
            continue;
        }
        // Compare record by record: the rewritten records in their relocation order against the
        // native records in file order. Unresolved head references are the assembler's.
        let unresolved: std::collections::BTreeSet<usize> =
            result.siblings.iter().map(|(at, _)| *at).collect();
        let native_records = records_of(&expected);
        let mut targets = records_of(&result.owner);
        targets.sort_unstable();
        for ((at, class), (nat, _)) in targets.iter().zip(native_records.iter()) {
            let end = native_records
                .iter()
                .map(|(o, _)| *o)
                .find(|o| *o > *nat)
                .unwrap_or(expected.0.len());
            let len = (end - nat).min(result.owner.0.len().saturating_sub(*at));
            for k in (0..len.saturating_sub(3)).step_by(4) {
                if unresolved.contains(&(at + k)) {
                    continue;
                }
                *words.entry(*class).or_default() += 1;
                if result.owner.0[at + k..at + k + 4] != expected.0[nat + k..nat + k + 4] {
                    *diffs.entry(*class).or_default().entry(k).or_default() += 1;
                }
            }
        }
    }
    println!(
        "converted {converted}, identical {identical}, failed {}",
        failures.len()
    );
    for failure in &failures {
        println!("  {failure}");
    }
    println!(
        "sizes (rewritten, native): {:?}",
        &size_pairs[..size_pairs.len().min(8)]
    );
    for (class, by_offset) in &diffs {
        let mut worst: Vec<_> = by_offset.iter().collect();
        worst.sort_by(|a, b| b.1.cmp(a.1));
        let total: usize = by_offset.values().sum();
        println!(
            "  {class:08X}: {total}/{} word diffs; worst {}",
            words[class],
            worst
                .iter()
                .take(10)
                .map(|(k, v)| format!("+{k:X}:{v}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    assert!(converted > 0, "no movement owners converted");
    assert!(
        failures.is_empty(),
        "{} pairs did not convert",
        failures.len()
    );
}

fn records_of(p: &Payload) -> Vec<(usize, u32)> {
    let owner = p.u32(p.pointer(16).unwrap()).unwrap();
    let mut out = Vec::new();
    let mut at = 0;
    while at + 24 <= p.0.len() {
        if p.u32(at).unwrap() == owner {
            let class = p.u32(at + 4).unwrap();
            if (0x8080_0000..0x8081_0000).contains(&class) {
                let twin = p.u64(at + 8).unwrap() as usize;
                if twin.is_multiple_of(8)
                    && twin + 16 <= p.0.len()
                    && twin != at
                    && p.u32(twin).unwrap() == owner
                    && p.u64(twin + 8).unwrap() as usize == at
                {
                    out.push((at, class));
                }
            }
        }
        at += 8;
    }
    out
}

/// Rewrite one exported modern movement owner. Opt in with `PARHELION_MOVEMENT_SOURCE` (the
/// modern owner), `PARHELION_MOVEMENT_TEMPLATE` (a native owner beside its allocation, named
/// `<tag>-allocation.bin` or by `PARHELION_MOVEMENT_TEMPLATE_ALLOCATION`),
/// `PARHELION_MOVEMENT_CORPUS` (the corpus directory with the resource maps) and
/// `PARHELION_MOVEMENT_OUTPUT` (a directory).
#[test]
#[ignore = "needs the PARHELION_MOVEMENT_SOURCE family of paths"]
fn rewrite_one_owner() {
    let Some(source) = std::env::var_os("PARHELION_MOVEMENT_SOURCE") else {
        return;
    };
    let template = PathBuf::from(std::env::var_os("PARHELION_MOVEMENT_TEMPLATE").unwrap());
    let allocation = std::env::var_os("PARHELION_MOVEMENT_TEMPLATE_ALLOCATION")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            template.with_file_name(format!(
                "{}-allocation.bin",
                template.file_stem().unwrap().to_string_lossy()
            ))
        });
    let resources = resources(&corpus().expect("PARHELION_MOVEMENT_CORPUS"));
    let output = PathBuf::from(std::env::var_os("PARHELION_MOVEMENT_OUTPUT").unwrap());
    fs::create_dir_all(&output).unwrap();
    let source = Payload(fs::read(source).unwrap());
    let template = Payload(fs::read(&template).unwrap());
    let template_allocation = Payload(fs::read(allocation).unwrap());
    let mut result = emit(
        &source,
        &template,
        &template_allocation,
        &resources,
        0x81FE0100,
        0x81FE0101,
    )
    .unwrap();
    let mut unmapped = Vec::new();
    if let Some(entity) = std::env::var_os("PARHELION_MOVEMENT_ENTITY") {
        let mut metadata = BTreeMap::new();
        for dialect in ["modern", "shadowkeep"] {
            let directory = corpus().unwrap().join("interfaces").join(dialect);
            for entry in fs::read_dir(directory).unwrap() {
                let path = entry.unwrap().path();
                if path.extension().is_some_and(|ext| ext == "bin") {
                    let tag = u32::from_str_radix(path.file_stem().unwrap().to_str().unwrap(), 16)
                        .unwrap();
                    metadata.insert(tag, Payload(fs::read(path).unwrap()));
                }
            }
        }
        let interfaces = result.interfaces(&source, &metadata).unwrap();
        result.objects.extend(interfaces.objects);
        unmapped = interfaces.unmapped;
        let graph =
            crate::d2_mot::entity::links::Graph::read(&Payload(fs::read(entity).unwrap()), true)
                .unwrap();
        let source_owner = source.u32(source.pointer(16).unwrap()).unwrap();
        let mapped: BTreeMap<_, _> = result
            .objects
            .iter()
            .map(|row| (row.source, row.target))
            .collect();
        assert_eq!(
            mapped.len(),
            result.objects.len(),
            "ambiguous movement objects"
        );
        for object in graph
            .objects()
            .filter(|object| object.owner == source_owner)
        {
            assert!(
                mapped.contains_key(&object),
                "unmapped entity endpoint {object:?}"
            );
        }
    }
    fs::write(output.join("owner.bin"), &result.owner.0).unwrap();
    fs::write(output.join("allocation.bin"), &result.allocation.0).unwrap();
    let report = serde_json::json!({
        "objects": result.objects,
        "unmapped_interfaces": unmapped,
        "siblings": result.siblings.iter().map(|(at, tag)| serde_json::json!({"offset": at, "tag": format!("{tag:08X}")})).collect::<Vec<_>>(),
        "installable": false,
        "gameplay_verified": false,
    });
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!(
        "rewrote {} records into {} bytes",
        result.objects.len(),
        result.owner.0.len()
    );
}
