//! Configured external-package oracle, specified before the delay adapter.
//! This isolates event definition serialization while effect closure is open.
use super::super::Resource;
use super::{Bindings, Payload, delay, render, table};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::{fs, path::PathBuf};

mod spawn;

#[test]
#[ignore = "requires explicitly configured source and native sequence exports"]
fn delay_package_oracle() -> anyhow::Result<()> {
    let corpus = PathBuf::from(std::env::var("PARHELION_SEQUENCE_CORPUS")?);
    let output = PathBuf::from(std::env::var("PARHELION_SEQUENCE_OUTPUT")?);
    anyhow::ensure!(!output.exists(), "sequence oracle output already exists");
    let pairs: serde_json::Value = serde_json::from_slice(&fs::read(corpus.join("pairs.json"))?)?;
    let mut cases = Vec::new();
    let mut rejections = 0;
    for pair in pairs["pairs"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("sequence pair list"))?
    {
        let source_tag = pair["source"]["tag"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("source tag"))?;
        let native_tag = pair["native"]["tag"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("native tag"))?;
        let source = Payload(fs::read(
            corpus.join("modern").join(format!("{source_tag}.bin")),
        )?);
        let native = Payload(fs::read(
            corpus.join("native").join(format!("{native_tag}.bin")),
        )?);
        let source_nodes = pair["source"]["nodes"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("source nodes"))?;
        let native_nodes = pair["native"]["nodes"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("native nodes"))?;
        anyhow::ensure!(
            source_nodes.len() == native_nodes.len(),
            "paired node count differs"
        );
        for (s, n) in source_nodes.iter().zip(native_nodes) {
            if s["cls"] != "808091D7" {
                continue;
            }
            anyhow::ensure!(
                n["cls"] == "808093CB" && s["group"] == "event",
                "delay counterpart differs"
            );
            let from = usize::try_from(
                s["offset"]
                    .as_u64()
                    .ok_or_else(|| anyhow::anyhow!("source offset"))?,
            )?;
            let to = usize::try_from(
                n["offset"]
                    .as_u64()
                    .ok_or_else(|| anyhow::anyhow!("native offset"))?,
            )?;
            let mut emitted = native.clone();
            emitted
                .0
                .get_mut(to + 16..to + 56)
                .ok_or_else(|| anyhow::anyhow!("native delay extent"))?
                .fill(0xCC);
            delay(&source, from, &mut emitted, to)?;
            anyhow::ensure!(
                emitted.0[to..to + 56] == native.0[to..to + 56],
                "delay definition differs for {source_tag} to {native_tag}"
            );
            for (at, bytes) in [
                (from - 4, 0x808091D9u32.to_le_bytes()),
                (from + 28, 1u32.to_le_bytes()),
                (from + 8, 0x40u32.to_le_bytes()),
                (from + 16, f32::NAN.to_le_bytes()),
            ] {
                let mut invalid = source.clone();
                invalid.0[at..at + 4].copy_from_slice(&bytes);
                let mut target = native.clone();
                anyhow::ensure!(
                    delay(&invalid, from, &mut target, to).is_err(),
                    "malformed delay accepted"
                );
                rejections += 1;
            }
            let key = format!("{source_tag}-{native_tag}-{}", s["index"]);
            let payload = emitted.0[to..to + 56].to_vec();
            cases.push((key, payload));
        }
    }
    anyhow::ensure!(
        !cases.is_empty() && rejections > 0,
        "configured delay fixture coverage differs"
    );
    fs::create_dir_all(&output)?;
    let mut rows = Vec::new();
    for (key, payload) in cases {
        fs::write(output.join(format!("{key}.bin")), &payload)?;
        rows.push(serde_json::json!({"key": key, "bytes": payload.len(), "sha256": hex::encode(Sha256::digest(&payload))}));
    }
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "definitions": rows, "rejections": rejections,
            "scope": "Delay definition serialization with externally exported native object prefixes. Event scheduling, dependency closure and gameplay are unverified.",
            "installable": false, "gameplay_verified": false
        }))?,
    )?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly configured source and native render-event exports"]
fn render_body_package_oracle() -> anyhow::Result<()> {
    let corpus = PathBuf::from(std::env::var("PARHELION_SEQUENCE_CORPUS")?);
    let output = PathBuf::from(std::env::var("PARHELION_RENDER_OUTPUT")?);
    anyhow::ensure!(!output.exists(), "render oracle output already exists");
    let native_catalog: serde_json::Value =
        serde_json::from_slice(&fs::read(corpus.join("native-catalog.json"))?)?;
    let source_catalog: serde_json::Value = serde_json::from_slice(&fs::read(
        corpus
            .parent()
            .ok_or_else(|| anyhow::anyhow!("sequence corpus parent"))?
            .join("enigma-resource-entities/catalog.json"),
    )?)?;
    let mut fixtures = Vec::new();
    for owner in native_catalog["owners"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("native owner census"))?
    {
        let tag = owner["tag"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("native owner tag"))?;
        let native = Payload(fs::read(corpus.join("native").join(format!("{tag}.bin")))?);
        for event in owner["events"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("native event list"))?
        {
            if event["definition_class"] != "80806E51" {
                continue;
            }
            let at = usize::try_from(
                event["offset"]
                    .as_u64()
                    .ok_or_else(|| anyhow::anyhow!("native render offset"))?,
            )?;
            let instance = usize::try_from(native.u64(at + 8)?)?;
            anyhow::ensure!(
                native.u32(at - 4)? == 0x80806E51
                    && native.u32(instance - 4)? == 0x80806E50
                    && native.u64(instance + 8)? == at as u64,
                "native render reciprocal pair differs"
            );
            let mut state = [0u8; 56];
            state[40..48].fill(0xFF);
            anyhow::ensure!(
                native.bytes::<56>(instance + 24)? == state,
                "native render runtime state differs"
            );
            anyhow::ensure!(
                native.0.get(at + 64..at + 280) == Some(&[0u8; 216][..]),
                "native render expression body is not empty"
            );
            fixtures.push((native.clone(), at));
        }
    }
    anyhow::ensure!(
        !fixtures.is_empty(),
        "render runtime census coverage differs"
    );
    let mut artifacts = Vec::new();
    let mut refusals = 0usize;
    for reference in source_catalog["dependencies"]["renders"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("source render dependencies"))?
    {
        let tag = reference["owner"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("source render owner"))?;
        let offset = usize::try_from(
            reference["offset"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("source render resource offset"))?,
        )?;
        let from = offset
            .checked_sub(0x20)
            .ok_or_else(|| anyhow::anyhow!("source render definition offset"))?;
        let source = Payload(fs::read(
            corpus
                .parent()
                .unwrap()
                .join("enigma-resource-owners")
                .join(format!("{tag}.bin")),
        )?);
        let resource = source.u32(offset)?;
        let flags = source.u32(from + 0x24)?;
        let (native, to) = fixtures
            .iter()
            .find(|(native, at)| {
                native.u32(at + 0x3C).ok() == Some(flags)
                    && native.u32(at + 0x38).ok().map(|tag| tag == u32::MAX)
                        == Some(resource == u32::MAX)
            })
            .ok_or_else(|| anyhow::anyhow!("native render body fixture absent"))?;
        let mut bindings = Bindings::default();
        if resource != u32::MAX {
            bindings.resources.insert(
                offset,
                Resource {
                    source_class: 0x8080694E,
                    native_class: 0x80806E53,
                    tag: native.u32(to + 0x38)?,
                },
            );
        }
        let mut emitted = native.clone();
        emitted.0[*to + 16..*to + 280].fill(0xCC);
        render(&source, from, &mut emitted, *to, &bindings)?;
        anyhow::ensure!(
            emitted.0[*to + 0x38..*to + 280] == native.0[*to + 0x38..*to + 280],
            "render resource, flags or expression body differs from native fixture"
        );
        let mut common = source.bytes::<28>(from)?;
        common[8..12].copy_from_slice(&(source.u32(from + 8)? & !0x20).to_le_bytes());
        anyhow::ensure!(
            emitted.bytes::<28>(*to + 16)? == common
                && emitted.u32(*to + 0x30)? == 34
                && emitted.u32(*to + 0x2C)? == 0
                && emitted.u32(*to + 0x34)? == 0,
            "authored render common fields or native padding differ"
        );
        for (field, bytes) in [
            (from - 4, 0x808067B9u32.to_le_bytes()),
            (from + 0x1C, 0u32.to_le_bytes()),
            (from + 8, 0x40u32.to_le_bytes()),
            (from + 16, f32::NAN.to_le_bytes()),
            (from + 0x24, 8u32.to_le_bytes()),
            (from + 0x28, 1u32.to_le_bytes()),
            (offset, 0u32.to_le_bytes()),
            (offset, 0xFFFF0000u32.to_le_bytes()),
        ] {
            let mut invalid = source.clone();
            invalid.0[field..field + 4].copy_from_slice(&bytes);
            anyhow::ensure!(
                render(&invalid, from, &mut native.clone(), *to, &bindings).is_err(),
                "malformed render event accepted"
            );
            refusals += 1;
        }
        let mut short_source = source.clone();
        short_source.0.truncate(from + 0xFF);
        anyhow::ensure!(
            render(&short_source, from, &mut native.clone(), *to, &bindings).is_err(),
            "truncated render source accepted"
        );
        refusals += 1;
        let mut short_native = native.clone();
        short_native.0.truncate(*to + 279);
        anyhow::ensure!(
            render(&source, from, &mut short_native, *to, &bindings).is_err(),
            "truncated render target accepted"
        );
        refusals += 1;
        let mut wrong_native = native.clone();
        wrong_native.0[*to - 4..*to].copy_from_slice(&0x80806CC6u32.to_le_bytes());
        anyhow::ensure!(
            render(&source, from, &mut wrong_native, *to, &bindings).is_err(),
            "wrong native render class accepted"
        );
        refusals += 1;
        if resource != u32::MAX {
            anyhow::ensure!(
                render(
                    &source,
                    from,
                    &mut native.clone(),
                    *to,
                    &Bindings::default()
                )
                .is_err(),
                "untranslated render resource accepted"
            );
            refusals += 1;
            for (class, tag) in [(0x80806E28, native.u32(to + 0x38)?), (0x80806E53, 0)] {
                let mut invalid = Bindings::default();
                invalid.resources.insert(
                    offset,
                    Resource {
                        source_class: 0x8080694E,
                        native_class: class,
                        tag,
                    },
                );
                anyhow::ensure!(
                    render(&source, from, &mut native.clone(), *to, &invalid).is_err(),
                    "invalid native render resource accepted"
                );
                refusals += 1;
            }
        }
        let key = format!("{tag}-{from:X}");
        artifacts.push((key, emitted.0[*to..*to + 280].to_vec()));
    }
    anyhow::ensure!(
        !artifacts.is_empty() && refusals > 0,
        "source render oracle coverage differs"
    );
    fs::create_dir_all(&output)?;
    let mut rows = Vec::new();
    for (key, bytes) in artifacts {
        fs::write(output.join(format!("{key}.bin")), &bytes)?;
        rows.push(serde_json::json!({"key": key, "bytes": bytes.len(),
            "sha256": hex::encode(Sha256::digest(&bytes))}));
    }
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "definitions": rows, "runtime_fixtures": fixtures.len(), "refusals": refusals,
            "scope": "Empty render-event resource, flags and expression bodies compared with exported native fixtures. Authored common fields checked separately. Object prefixes and resource bindings are external fixture data. Complete render-owner twins, shader conversion, sequence execution and gameplay are unverified.",
            "installable": false, "gameplay_verified": false
        }))?,
    )?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly configured source and native table-event exports"]
fn table_event_package_oracle() -> anyhow::Result<()> {
    let corpus = PathBuf::from(std::env::var("PARHELION_SEQUENCE_CORPUS")?);
    let output = PathBuf::from(std::env::var("PARHELION_TABLE_EVENT_OUTPUT")?);
    anyhow::ensure!(!output.exists(), "table event oracle output already exists");
    let pairs: serde_json::Value =
        serde_json::from_slice(&fs::read(corpus.join("table-event-pairs.json"))?)?;
    let mut artifacts = Vec::new();
    let mut refusals = 0;
    for pair in pairs
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("table event pair list"))?
    {
        let source_tag = pair["source"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("source table event owner"))?;
        let native_tag = pair["native"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("native table event owner"))?;
        let source = Payload(fs::read(
            corpus.join("modern").join(format!("{source_tag}.bin")),
        )?);
        let native = Payload(fs::read(
            corpus.join("native").join(format!("{native_tag}.bin")),
        )?);
        let from = usize::try_from(
            pair["source_offset"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("source table event offset"))?,
        )?;
        let to = usize::try_from(
            pair["native_offset"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("native table event offset"))?,
        )?;
        let source_names = source.array(source.pointer(24)? + 0x250, 4, Some(0x80809538))?;
        let native_names = native.array(native.pointer(24)? + 0x1D0, 4, Some(0x80808614))?;
        let source_locator = source.u32(from + 0x68)?;
        let native_locator = native.u32(to + 0x78)?;
        let source_name = source.u32(
            *source_names
                .get(source_locator as usize)
                .ok_or_else(|| anyhow::anyhow!("source locator outside names"))?,
        )?;
        let native_name = native.u32(
            *native_names
                .get(native_locator as usize)
                .ok_or_else(|| anyhow::anyhow!("native locator outside names"))?,
        )?;
        anyhow::ensure!(
            source_name == native_name,
            "table event attachment name differs"
        );
        let locators = BTreeMap::from([(source_locator, native_locator)]);
        let mut bindings = Bindings::default();
        bindings.resources.insert(
            from + 0x30,
            Resource {
                source_class: 0x8080873F,
                native_class: 0x80808BCD,
                tag: native.u32(to + 0x50)?,
            },
        );
        let mut emitted = native.clone();
        emitted.0[to + 16..to + 128].fill(0xCC);
        table(
            &source,
            from,
            &mut emitted,
            to,
            &bindings,
            &locators,
            native_names.len(),
        )?;
        anyhow::ensure!(
            emitted.0[to..to + 128] == native.0[to..to + 128],
            "complete table event definition differs from native fixture"
        );
        for (field, word) in [
            (from - 4, 0x808091D7),
            (from + 28, 0),
            (from + 8, 0x40),
            (from + 16, 0x7FC00000),
            (from + 0x20, 0),
            (from + 0x28, 1),
            (from + 0x30, 0),
            (from + 0x34, 0),
            (from + 0x38, 1),
            (from + 0x40, 1),
            (from + 0x50, 1),
            (from + 0x60, 0x7FC00000),
            (from + 0x60, 0x3F800000),
            (from + 0x6C, 2),
            (from + 0x6C, 0x01010100),
            (from + 0x70, 1),
            (from + 0x78, 0),
            (from + 0x68, u32::MAX),
        ] {
            let mut invalid = source.clone();
            invalid.0[field..field + 4].copy_from_slice(&word.to_le_bytes());
            anyhow::ensure!(
                table(
                    &invalid,
                    from,
                    &mut native.clone(),
                    to,
                    &bindings,
                    &locators,
                    native_names.len()
                )
                .is_err(),
                "malformed table event accepted"
            );
            refusals += 1;
        }
        anyhow::ensure!(
            table(
                &source,
                from,
                &mut native.clone(),
                to,
                &Bindings::default(),
                &locators,
                native_names.len()
            )
            .is_err(),
            "untranslated table event resource accepted"
        );
        refusals += 1;
        for (class, tag) in [(0x80806E53, native.u32(to + 0x50)?), (0x80808BCD, 0)] {
            let mut invalid = Bindings::default();
            invalid.resources.insert(
                from + 0x30,
                Resource {
                    source_class: 0x8080873F,
                    native_class: class,
                    tag,
                },
            );
            anyhow::ensure!(
                table(
                    &source,
                    from,
                    &mut native.clone(),
                    to,
                    &invalid,
                    &locators,
                    native_names.len()
                )
                .is_err(),
                "invalid native table dependency accepted"
            );
            refusals += 1;
        }
        for invalid in [
            BTreeMap::new(),
            BTreeMap::from([(source_locator, u32::try_from(native_names.len())?)]),
        ] {
            anyhow::ensure!(
                table(
                    &source,
                    from,
                    &mut native.clone(),
                    to,
                    &bindings,
                    &invalid,
                    native_names.len()
                )
                .is_err(),
                "invalid table attachment mapping accepted"
            );
            refusals += 1;
        }
        let mut short_source = source.clone();
        short_source.0.truncate(from + 0x7F);
        anyhow::ensure!(
            table(
                &short_source,
                from,
                &mut native.clone(),
                to,
                &bindings,
                &locators,
                native_names.len()
            )
            .is_err(),
            "truncated table event source accepted"
        );
        refusals += 1;
        let mut short_native = native.clone();
        short_native.0.truncate(to + 127);
        anyhow::ensure!(
            table(
                &source,
                from,
                &mut short_native,
                to,
                &bindings,
                &locators,
                native_names.len()
            )
            .is_err(),
            "truncated table event target accepted"
        );
        refusals += 1;
        artifacts.push((
            format!("{source_tag}-{native_tag}"),
            emitted.0[to..to + 128].to_vec(),
        ));
    }
    anyhow::ensure!(
        !artifacts.is_empty() && refusals > 0,
        "table event oracle coverage differs"
    );
    fs::create_dir_all(&output)?;
    let mut rows = Vec::new();
    for (key, bytes) in artifacts {
        fs::write(output.join(format!("{key}.bin")), &bytes)?;
        rows.push(serde_json::json!({"key": key, "bytes": bytes.len(),
            "sha256": hex::encode(Sha256::digest(&bytes))}));
    }
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "definitions": rows, "refusals": refusals,
            "scope": "Complete table-event definition compared with an exported native counterpart. Native object prefix and table binding are external fixtures. Attachment names checked independently. Whole owner, table leaf closure, scheduling and gameplay are unverified.",
            "installable": false, "gameplay_verified": false
        }))?,
    )?;
    Ok(())
}
