//! Opt-in package-backed owner and allocation oracle, specified before emission.
use super::*;
use serde_json::json;
use std::{fs, path::PathBuf};

fn read(path: &std::path::Path) -> Payload {
    Payload(fs::read(path).unwrap())
}

fn records(p: &Payload) -> Vec<(usize, u32)> {
    let owner = p.u32(p.pointer(16).unwrap()).unwrap();
    (0..p.0.len().saturating_sub(15))
        .step_by(8)
        .filter_map(|at| {
            if p.u32(at).ok() != Some(owner) {
                return None;
            }
            let twin = usize::try_from(p.u64(at + 8).ok()?).ok()?;
            (twin != at
                && p.u32(twin).ok() == Some(owner)
                && p.u64(twin + 8).ok() == Some(at as u64))
            .then(|| (at, p.u32(at + 4).unwrap()))
        })
        .collect()
}

#[test]
#[ignore = "requires an explicitly configured exported damage corpus"]
#[expect(
    clippy::cognitive_complexity,
    reason = "The opt-in corpus oracle keeps conversion, independent comparisons and artifact emission in one repeatable sweep"
)]
fn native_owners_and_allocations() {
    let Some(directory) = std::env::var_os("PARHELION_DAMAGE_CORPUS") else {
        return;
    };
    let directory = PathBuf::from(directory);
    let pairs: Vec<[String; 2]> =
        serde_json::from_slice(&fs::read(directory.join("pairs.json")).unwrap()).unwrap();
    let goldens: Vec<[String; 2]> =
        serde_json::from_slice(&fs::read(directory.join("golden-pairs.json")).unwrap()).unwrap();
    assert!(!pairs.is_empty(), "damage corpus has no pairs");
    assert!(!goldens.is_empty(), "damage corpus has no native goldens");
    let mut metadata = BTreeMap::new();
    for side in ["modern", "shadowkeep"] {
        for row in fs::read_dir(directory.join("all-providers").join(side)).unwrap() {
            let path = row.unwrap().path();
            let name = path.file_stem().unwrap().to_str().unwrap();
            metadata.insert(u32::from_str_radix(name, 16).unwrap(), read(&path));
        }
    }
    let envelope = read(&PathBuf::from(
        std::env::var_os("PARHELION_DAMAGE_INPUT").expect("input envelope"),
    ));
    let output = std::env::var_os("PARHELION_DAMAGE_OUTPUT").map(PathBuf::from);
    if let Some(output) = &output {
        fs::create_dir_all(output).unwrap();
    }
    let mut report = Vec::new();
    let mut converted = 0;
    let mut identical = 0;
    for pair in &pairs {
        let source = read(&directory.join("modern").join(format!("{}.bin", pair[0])));
        let native = read(
            &directory
                .join("shadowkeep")
                .join(format!("{}.bin", pair[1])),
        );
        let allocation_tag = native.u32(0x44).unwrap();
        let allocation = read(
            &directory
                .join("allocations")
                .join(format!("{allocation_tag:08X}.bin")),
        );
        let native_tag = native.u32(native.pointer(16).unwrap()).unwrap();
        let result = emit(
            &source,
            &native,
            &allocation,
            &envelope,
            &metadata,
            native_tag,
            allocation_tag,
        );
        if let Err(error) = result {
            assert!(!goldens.contains(pair), "native golden failed: {error:#}");
            assert!(
                format!("{error:#}").contains("impact"),
                "{}: {error:#}",
                pair[0]
            );
            report.push(
                json!({"source": pair[0], "native": pair[1], "converted": false,
                               "error": format!("{error:#}")}),
            );
            continue;
        }
        let result = result.unwrap();
        converted += 1;
        let same = result.owner.0 == native.0;
        identical += usize::from(same);
        if goldens.contains(pair) {
            assert!(same, "{} stable native payload differs", pair[0]);
        }
        // Allocation metadata is an independent package artifact. The known
        // paired runtime input shape must reproduce it without using row votes.
        assert_eq!(result.allocation.0, allocation.0, "{} allocation", pair[0]);
        if let Some(output) = &output {
            fs::write(
                output.join(format!("{}-owner.bin", pair[0])),
                &result.owner.0,
            )
            .unwrap();
            fs::write(
                output.join(format!("{}-allocation.bin", pair[0])),
                &result.allocation.0,
            )
            .unwrap();
        }
        let keys: std::collections::BTreeSet<_> =
            result.objects.iter().map(|row| row.source).collect();
        assert_eq!(keys.len(), result.objects.len(), "duplicate object key");
        for &(at, _) in &records(&source) {
            let declared = source
                .u32(usize::try_from(source.u64(at + 8).unwrap()).unwrap() + 4)
                .unwrap();
            let row = result
                .objects
                .iter()
                .find(|row| row.source.offset == at as u64 && row.source.class == declared)
                .unwrap();
            let target = usize::try_from(row.target.offset).unwrap();
            let declared_target = result
                .owner
                .u32(usize::try_from(result.owner.u64(target + 8).unwrap()).unwrap() + 4)
                .unwrap();
            assert_eq!(row.target.class, declared_target);
        }
        report.push(
            json!({"source": pair[0], "native": pair[1], "converted": true,
                           "byte_identical": same, "objects": result.objects,
                           "omitted_methods": result.omitted_methods}),
        );
    }
    assert!(converted > 0, "no damage owners converted");
    assert!(identical >= goldens.len());
    // Structural and state failures must refuse rather than make a partial owner.
    let pair = &goldens[0];
    let source = read(&directory.join("modern").join(format!("{}.bin", pair[0])));
    let native = read(
        &directory
            .join("shadowkeep")
            .join(format!("{}.bin", pair[1])),
    );
    let allocation_tag = native.u32(0x44).unwrap();
    let allocation = read(
        &directory
            .join("allocations")
            .join(format!("{allocation_tag:08X}.bin")),
    );
    let owner = native.u32(native.pointer(16).unwrap()).unwrap();
    for bad in [
        {
            let mut p = source.clone();
            p.0.truncate(64);
            p
        },
        {
            let mut p = source.clone();
            p.0[0] ^= 1;
            p
        },
        {
            let mut p = source.clone();
            let i = p.pointer(16).unwrap();
            p.0[i + 24] = 1;
            p
        },
        {
            let mut p = source.clone();
            let d = p.pointer(24).unwrap();
            p.0[d + 0x58 + 48] = 63;
            p
        },
        {
            let mut p = source.clone();
            let d = p.pointer(24).unwrap();
            p.0[d + 0x1F0] = 1;
            p
        },
    ] {
        assert!(
            emit(
                &bad,
                &native,
                &allocation,
                &envelope,
                &metadata,
                owner,
                allocation_tag
            )
            .is_err()
        );
    }
    let empty: BTreeMap<u32, Payload> = BTreeMap::new();
    assert!(
        emit(
            &source,
            &native,
            &allocation,
            &envelope,
            &empty,
            owner,
            allocation_tag
        )
        .is_err()
    );
    let artifact = json!({"converted": converted, "byte_identical": identical,
                         "rejection_checks": 6,
                         "installable": false, "gameplay_verified": false,
                         "rows": report});
    if let Some(path) = output {
        fs::write(
            path.join("report.json"),
            serde_json::to_vec_pretty(&artifact).unwrap(),
        )
        .unwrap();
    }
}
