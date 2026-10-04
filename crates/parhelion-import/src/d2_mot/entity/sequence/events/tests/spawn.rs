//! External package contract specified before the spawn adapter.
use super::super::spawn;
use super::{Bindings, Payload};
use crate::d2_mot::entity::sequence::Resource;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::PathBuf};

#[test]
#[ignore = "requires explicitly configured source and native sequence exports"]
fn spawn_package_oracle() -> anyhow::Result<()> {
    let corpus = PathBuf::from(std::env::var("PARHELION_SEQUENCE_CORPUS")?);
    let output = PathBuf::from(std::env::var("PARHELION_SPAWN_OUTPUT")?);
    anyhow::ensure!(!output.exists(), "spawn oracle output already exists");
    let pairs: serde_json::Value =
        serde_json::from_slice(&fs::read(corpus.join("spawn-event-golden-pairs.json"))?)?;
    let mut artifacts = Vec::new();
    let mut refusals = 0;
    for pair in pairs
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("spawn pairs"))?
    {
        let source_tag = pair["source"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("source tag"))?;
        let native_tag = pair["native"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("native tag"))?;
        let from = usize::try_from(
            pair["source_at"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("source offset"))?,
        )?;
        let to = usize::try_from(
            pair["native_at"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("native offset"))?,
        )?;
        let source = Payload(fs::read(
            corpus.join("modern").join(format!("{source_tag}.bin")),
        )?);
        let native = Payload(fs::read(
            corpus.join("native").join(format!("{native_tag}.bin")),
        )?);
        let mut bindings = Bindings::default();
        bindings.resources.insert(
            from + 0x70,
            Resource {
                source_class: 0x80809AD8,
                native_class: 0x80809C0F,
                tag: native.u32(to + 0x74)?,
            },
        );
        let names = source.array(source.pointer(24)? + 0x250, 4, Some(0x80809538))?;
        let locators = (0..u32::try_from(names.len())?)
            .map(|i| (i, i))
            .collect::<BTreeMap<_, _>>();
        let mut emitted = native.clone();
        emitted.0[to + 16..to + 224].fill(0xCC);
        spawn(
            &source,
            from,
            &mut emitted,
            to,
            &bindings,
            &locators,
            names.len(),
        )?;
        anyhow::ensure!(
            emitted.0[to..to + 224] == native.0[to..to + 224],
            "complete spawn definition differs from native fixture"
        );
        for (delta, word) in [
            (-4isize, 0x808091D7u32),
            (28, 15),
            (8, 0x40),
            (16, 0x7FC00000),
            (0x20, 0),
            (0x24, 1),
            (0x28, 1),
            (0x38, 0x7FC00000),
            (0x60, 0x10000),
            (0x64, 1),
            (0x64, 0x03000000),
            (0x68, 256),
            (0x6C, 1),
            (0x70, 0),
            (0x74, 2),
            (0x80, 1),
            (0x90, 1),
            (0xA0, 1),
            (0xB4, 0x7FC00000),
            (0xC8, 1),
            (0xCC, 1),
            (0xD0, 2),
            (0xD4, 1),
            (0xF8, 1),
            (0x100, 0),
            (0x104, 0),
            (0x108, 0),
            (0x10C, 1),
        ] {
            let field = from
                .checked_add_signed(delta)
                .ok_or_else(|| anyhow::anyhow!("spawn mutation offset"))?;
            let mut invalid = source.clone();
            invalid.0[field..field + 4].copy_from_slice(&word.to_le_bytes());
            anyhow::ensure!(
                spawn(
                    &invalid,
                    from,
                    &mut native.clone(),
                    to,
                    &bindings,
                    &locators,
                    names.len()
                )
                .is_err(),
                "malformed spawn definition accepted at {delta:X}"
            );
            refusals += 1;
        }
        for dependency in [
            None,
            Some((0x80809AD8, 0x80806E53, native.u32(to + 0x74)?)),
            Some((0x80806920, 0x80809C0F, native.u32(to + 0x74)?)),
            Some((0x80809AD8, 0x80809C0F, 0)),
        ] {
            let mut invalid = Bindings::default();
            if let Some((source_class, native_class, tag)) = dependency {
                invalid.resources.insert(
                    from + 0x70,
                    Resource {
                        source_class,
                        native_class,
                        tag,
                    },
                );
            }
            anyhow::ensure!(
                spawn(
                    &source,
                    from,
                    &mut native.clone(),
                    to,
                    &invalid,
                    &locators,
                    names.len()
                )
                .is_err(),
                "invalid spawn entity binding accepted"
            );
            refusals += 1;
        }
        let mut attached = source.clone();
        attached.0[from + 0x68..from + 0x6C].copy_from_slice(&0u32.to_le_bytes());
        for (mapping, count) in [
            (BTreeMap::new(), names.len()),
            (
                BTreeMap::from([(0, u32::try_from(names.len())?)]),
                names.len(),
            ),
            (locators.clone(), 0),
        ] {
            anyhow::ensure!(
                spawn(
                    &attached,
                    from,
                    &mut native.clone(),
                    to,
                    &bindings,
                    &mapping,
                    count
                )
                .is_err(),
                "unmapped or invalid spawn attachment accepted"
            );
            refusals += 1;
        }
        let mut short = source.clone();
        short.0.truncate(from + 271);
        anyhow::ensure!(
            spawn(
                &short,
                from,
                &mut native.clone(),
                to,
                &bindings,
                &locators,
                names.len()
            )
            .is_err(),
            "truncated spawn source accepted"
        );
        refusals += 1;
        let mut short = native.clone();
        short.0.truncate(to + 223);
        anyhow::ensure!(
            spawn(
                &source,
                from,
                &mut short,
                to,
                &bindings,
                &locators,
                names.len()
            )
            .is_err(),
            "truncated spawn target accepted"
        );
        refusals += 1;
        let mut wrong = native.clone();
        wrong.0[to - 4..to].copy_from_slice(&0x808093BEu32.to_le_bytes());
        anyhow::ensure!(
            spawn(
                &source,
                from,
                &mut wrong,
                to,
                &bindings,
                &locators,
                names.len()
            )
            .is_err(),
            "wrong native spawn class accepted"
        );
        refusals += 1;
        artifacts.push((
            format!("{source_tag}-{from:X}-{native_tag}"),
            emitted.0[to..to + 224].to_vec(),
            format!("{:x}", Sha256::digest(&source.0)),
            format!("{:x}", Sha256::digest(&native.0)),
        ));
    }
    let catalog: serde_json::Value =
        serde_json::from_slice(&fs::read(corpus.join("native-catalog.json"))?)?;
    let mut runtimes = 0;
    for owner in catalog["owners"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("native owners"))?
    {
        let tag = owner["tag"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("native owner tag"))?;
        for event in owner["events"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("native events"))?
        {
            if event["definition_class"] != "80808881" {
                continue;
            }
            let native = Payload(fs::read(corpus.join("native").join(format!("{tag}.bin")))?);
            let at = usize::try_from(
                event["instance"]
                    .as_u64()
                    .ok_or_else(|| anyhow::anyhow!("runtime offset"))?,
            )?;
            anyhow::ensure!(
                native.u32(at - 4)? == 0x80808880
                    && native.0.get(at + 24..at + 80) == Some(&[0u8; 56][..])
                    && native.0.get(at + 80..at + 96) == Some(&[0xFFu8; 16][..])
                    && native.0.get(at + 96..at + 112) == Some(&[0u8; 16][..]),
                "spawn runtime envelope differs"
            );
            runtimes += 1;
        }
    }
    anyhow::ensure!(
        !artifacts.is_empty() && runtimes > 0 && refusals > 0,
        "spawn oracle coverage differs"
    );
    fs::create_dir_all(&output)?;
    let mut rows = Vec::new();
    for (key, bytes, source_sha256, native_sha256) in artifacts {
        fs::write(output.join(format!("{key}.bin")), &bytes)?;
        rows.push(serde_json::json!({"key":key,"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes)),"source_owner_sha256":source_sha256,"native_owner_sha256":native_sha256}));
    }
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"definitions":rows,"refusals":refusals,"native_runtime_envelopes":runtimes,"scope":"Complete definitions against exported native counterparts with fixture object prefixes and typed entity bindings. Whole owner and entity closure are unverified.","installable":false,"gameplay_verified":false}),
        )?,
    )?;
    Ok(())
}
