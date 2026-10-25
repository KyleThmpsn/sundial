//! Configured authoring, persistence and package readback for interacting Void perks.
#![cfg(feature = "d2-model-importer")]

use anyhow::{Context, Result, ensure};
use parhelion::{BatchBuildRequest, BatchBuildSnapshot, WeaponRecipe, perk::import};
use parhelion_import::d2_mot::{payload::Payload, reader::Reader};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
};
use sundial::{
    investment::InvestmentCatalog,
    package_authoring::{
        PackageManager, declared_payload_references, resolve_live_named_tag,
        sandbox_perk::{action, load_sandbox_perk_runtime_action},
    },
};

fn configured(name: &str) -> Result<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("Set {name}"))
}

fn manager(reader: &Reader) -> Result<PackageManager> {
    PackageManager::new(
        &reader.manager.package_dir,
        reader.manager.version,
        Some(reader.manager.platform),
    )
    .map_err(anyhow::Error::msg)
}

fn stock_view(native: &Path, root: &Path) -> Result<PathBuf> {
    let packages = root.join("packages");
    let installation = native.parent().context("Native root")?;
    fs::create_dir_all(&packages)?;
    fs::create_dir_all(root.join("bin/x64"))?;
    fs::hard_link(installation.join("destiny2.exe"), root.join("destiny2.exe"))?;
    for entry in fs::read_dir(native)? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|ext| ext == "pkg") {
            fs::hard_link(entry.path(), packages.join(entry.file_name()))?;
        }
    }
    fs::copy(
        installation.join("bin/x64/oo2core_3_win64.dll"),
        root.join("bin/x64/oo2core_3_win64.dll"),
    )?;
    Ok(packages)
}

// Read the emitted graph closure through declared native fields. Names and
// arbitrary aligned words are never interpreted as attachment references.
fn closure(
    reader: &mut Reader,
    native: &PackageManager,
    stock: &PackageManager,
    roots: &BTreeSet<u32>,
) -> Result<BTreeMap<u32, Payload>> {
    let mut pending = roots.clone();
    let mut graphs = BTreeSet::new();
    let mut owners = BTreeMap::new();
    while let Some(tag) = pending.pop_first() {
        if !graphs.insert(tag) {
            continue;
        }
        ensure!(
            graphs.len() <= 512,
            "Attachment graph closure exceeds its bound"
        );
        let graph = reader.tag(tag, Some(0x80809C0F))?;
        let parts = graph.array(16, 12, Some(0x80809C04))?;
        ensure!(!parts.is_empty(), "Reachable attachment graph is empty");
        for row in parts {
            let tag = graph.u32(row)?;
            if owners.contains_key(&tag) {
                continue;
            }
            let owner = reader.tag(tag, Some(0x80809C36))?;
            ensure!(
                stock.get_entry(tiger_pkg::TagHash(tag)).is_none(),
                "Mutable attachment owner still uses a stock donor"
            );
            for (_, child) in
                declared_payload_references(native, tag, &owner.0).map_err(anyhow::Error::msg)?
            {
                let entry = native
                    .get_entry(tiger_pkg::TagHash(child))
                    .context("Declared attachment resource missing")?;
                if entry.reference == 0x80809C0F {
                    pending.insert(child);
                }
            }
            owners.insert(tag, (*owner).clone());
        }
    }
    ensure!(!owners.is_empty(), "No reachable attachment owners");
    Ok(owners)
}

fn health(source: &Payload, native: &Payload) -> Result<serde_json::Value> {
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let ni = native.pointer(16)?;
    let nd = native.pointer(24)?;
    ensure!(
        source.u32(sd - 4)? == 0x80803FFC && native.u32(nd - 4)? == 0x80804B8A,
        "Health root type differs"
    );
    // These are observable provider selectors established by the two original
    // getter dispatches, independent of the importer implementation.
    let read_outputs = |p: &Payload, modern: bool| -> Result<BTreeMap<u32, (u32, u8, u8)>> {
        let d = p.pointer(24)?;
        let field = if modern { 0x440 } else { 0x398 };
        let stride = if modern { 64 } else { 56 };
        let class = if modern { 0x80804004 } else { 0x80804B92 };
        let name = if modern { 48 } else { 40 };
        p.array(d + field, stride, Some(class))?
            .into_iter()
            .map(|r| {
                let kind = p.u8(r + name + 12)?;
                let kind = if modern && kind == 5 { 3 } else { kind };
                Ok((
                    p.u32(r + name)?,
                    (p.u32(r + name + 8)?, kind, p.u8(r + name + 13)?),
                ))
            })
            .collect()
    };
    let expected = read_outputs(source, true)?;
    let actual = read_outputs(native, false)?;
    ensure!(
        !expected.is_empty() && expected == actual,
        "Health outputs lost a region, selector, name or numeric transform"
    );
    let sr = source.array(sd + 0x410, 248, Some(0x808040E1))?;
    let nr = native.array(nd + 0x368, 80, Some(0x80804C5F))?;
    ensure!(
        !sr.is_empty() && sr.len() == nr.len(),
        "Health region count differs"
    );
    let mut regions = Vec::new();
    for (&s, &n) in sr.iter().zip(&nr) {
        for (a, b) in [
            (0x10, 0x10),
            (0x14, 0x14),
            (0x44, 0x18),
            (0x48, 0x1C),
            (0x18, 0x20),
            (0x1C, 0x24),
            (0x20, 0x28),
            (0x24, 0x2C),
            (0x38, 0x48),
            (0xD0, 0x40),
        ] {
            ensure!(
                source.u32(s + a)? == native.u32(n + b)?,
                "Health region value differs at source {a:X}, native {b:X}"
            );
        }
        regions.push(json!({"offset":n,"capacity_scale":native.f32(n+0x14)?,
            "initial_fraction":native.f32(n+0x18)?,"history":[native.f32(n+0x20)?,native.f32(n+0x24)?,native.f32(n+0x28)?,native.f32(n+0x2C)?]}));
    }
    ensure!(
        source.u32(si + 0x470)? == native.u32(ni + 0x324)?,
        "Initial health capacity differs"
    );
    ensure!(
        source.u32(sd + 0x208)? == native.u32(nd + 0x1A8)?,
        "Health initialization fallback differs"
    );
    let source_expressions = source.array(sd + 0x400, 80, Some(0x80808632))?;
    let native_expressions = native.array(nd + 0x358, 80, Some(0x808089F8))?;
    ensure!(
        source_expressions.len() == native_expressions.len(),
        "Health section expressions were omitted"
    );
    let mut input_count = 0usize;
    for (&s, &n) in source_expressions.iter().zip(&native_expressions) {
        ensure!(
            source.u64(s + 48)? == native.u64(n + 48)?,
            "Health expression input count differs"
        );
        let a = source.array(s + 64, 40, Some(0x80809591))?;
        let b = native.array(n + 64, 40, Some(0x80809789))?;
        ensure!(
            a.len() == b.len(),
            "Health expression external inputs were dropped"
        );
        input_count += b.len();
    }
    Ok(
        json!({"outputs":actual,"regions":regions,"initial_capacity":native.f32(ni+0x324)?,
        "fallback_capacity":native.f32(nd+0x1A8)?,
        "expressions":native_expressions.len(),"external_inputs":input_count}),
    )
}

fn locator_pool(source: &Payload, owners: &BTreeMap<u32, Payload>) -> Result<serde_json::Value> {
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let names = source
        .array(sd + 0x250, 4, Some(0x80809538))?
        .into_iter()
        .map(|at| source.u32(at))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        !names.is_empty(),
        "Required source locator declarations are empty"
    );
    let ranges = source
        .array(sd + 0x230, 6, Some(0x8080953B))?
        .into_iter()
        .map(|at| source.bytes::<6>(at))
        .collect::<Result<Vec<_>>>()?;
    let mut evidence = Vec::new();
    for (tag, native) in owners {
        let nd = native.pointer(24)?;
        if native.u32(nd - 4)? != 0x808084E9 {
            continue;
        }
        let actual_names = native
            .array(nd + 0x1D0, 4, Some(0x80808614))?
            .into_iter()
            .map(|at| native.u32(at))
            .collect::<Result<Vec<_>>>()?;
        if actual_names != names {
            continue;
        }
        let actual_ranges = native
            .array(nd + 0x1B0, 6, Some(0x80808617))?
            .into_iter()
            .map(|at| native.bytes::<6>(at))
            .collect::<Result<Vec<_>>>()?;
        ensure!(ranges == actual_ranges, "Locator start and count changed");
        let ni = native.pointer(16)?;
        let mut capacities = Vec::new();
        for (index, (s, n, stride, sc, nc)) in [
            (0x280, 0x170, 32, 0x80809F4F, 0x80809F75),
            (0x290, 0x180, 32, 0x80809F4F, 0x80809F75),
            (0x2A0, 0x190, 2, 0x8080953C, 0x80808618),
            (0x2B0, 0x1A0, 24, 0x80809AC8, 0x80809BFF),
        ]
        .into_iter()
        .enumerate()
        {
            let declared = source.u32(sd + 0x260 + index * 4)?;
            let expected = source.array(si + s, stride, Some(sc))?.len();
            let actual = native.array(ni + n, stride, Some(nc))?.len();
            ensure!(
                expected == declared as usize
                    && actual == expected
                    && native.u32(nd + 0x1E0 + index * 4)? == declared,
                "Private locator storage capacity differs"
            );
            capacities.push(actual);
        }
        ensure!(
            capacities[1] > names.len(),
            "Required multi-slot locator witness is absent"
        );
        evidence.push(
            json!({"owner":tag,"names":names,"ranges":actual_ranges,"capacities":capacities,
            "sha256":hex::encode(Sha256::digest(&native.0))}),
        );
    }
    ensure!(
        !evidence.is_empty(),
        "Private locator sequence was not reachable"
    );
    Ok(json!(evidence))
}

fn named_write(source: &Payload, owners: &BTreeMap<u32, Payload>) -> Result<serde_json::Value> {
    let sd = source.pointer(24)?;
    let rows = source.array(sd + 0x1D8, 24, Some(0x808091F1))?;
    ensure!(
        rows.len() == 1 && source.u64(sd + 0x208)? == 0 && source.u64(sd + 0x380)? == 0,
        "Required input-free source write sequence differs"
    );
    let event = source.pointer(rows[0] + 16)?;
    ensure!(
        source.u32(event - 4)? == 0x8080847E && source.u32(event + 28)? == 3,
        "Required source event is not a channel write"
    );
    let name = source.u32(event)?;
    let target = source.u32(event + 0x68)?;
    let program = |p: &Payload, at: usize| -> Result<(Vec<u8>, Vec<[u8; 16]>)> {
        let code = p
            .array(at, 1, Some(0x80800009))?
            .into_iter()
            .map(|a| p.u8(a))
            .collect::<Result<Vec<_>>>()?;
        let constants = p
            .array(at + 16, 16, Some(0x80800090))?
            .into_iter()
            .map(|a| p.bytes::<16>(a))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            p.u64(at + 32)? == 1 && p.u64(at + 40)? == 1 && p.u64(at + 48)? == 0,
            "Named write unexpectedly reads an external scalar input"
        );
        Ok((code, constants))
    };
    let (source_code, constants) = program(source, event + 0x28)?;
    ensure!(
        source_code == [0x42, 0, 0x4C, 0] && constants.len() == 1,
        "Required literal write witness differs"
    );
    let mut evidence = Vec::new();
    for (tag, native) in owners {
        let nd = native.pointer(24)?;
        if native.u32(nd - 4)? != 0x808084E9 {
            continue;
        }
        for row in native.array(nd + 0x168, 24, Some(0x808093E6))? {
            let at = native.pointer(row + 16)?;
            if native.u32(at - 4)? != 0x80808879 || native.u32(at + 16)? != name {
                continue;
            }
            ensure!(
                native.u32(at + 0x30)? == 3 && native.u32(at + 0x78)? == target,
                "Private channel write lost its target or event kind"
            );
            let (code, actual) = program(native, at + 0x38)?;
            // Native VM 34 loads a Float4 literal and 3E writes output zero.
            ensure!(
                code == [0x34, 0, 0x3E, 0] && actual == constants,
                "Private channel write changed its value"
            );
            evidence.push(json!({"owner":tag,"event":name,"target":target,
                "native_code":code,"constants":actual,
                "sha256":hex::encode(Sha256::digest(&native.0))}));
        }
    }
    ensure!(!evidence.is_empty(), "Named channel write is not reachable");
    Ok(json!(evidence))
}

fn sequence_evidence(
    case: usize,
    source: &mut Reader,
    owners: &BTreeMap<u32, Payload>,
) -> Result<serde_json::Value> {
    let owner = source.tag([0x80C33FD8, 0x80A5AE93][case], Some(0x80809B06))?;
    if case == 0 {
        locator_pool(&owner, owners)
    } else {
        named_write(&owner, owners)
    }
}

fn omitted_indexed_particles(
    source: &Payload,
    staged: &mut Reader,
    stock: &PackageManager,
) -> Result<serde_json::Value> {
    let sd = source.pointer(24)?;
    let expected = source
        .array(sd + 0x1D8, 24, Some(0x808091F1))?
        .into_iter()
        .map(|row| source.u32(source.pointer(row + 16)?))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        expected.len() == 4 && source.u64(sd + 0x280)? == 1,
        "Required direct and indexed particle source differs"
    );
    let mut evidence = Vec::new();
    for tag in staged.classes(0x80809C36) {
        if stock.get_entry(tiger_pkg::TagHash(tag)).is_some() {
            continue;
        }
        let owner = staged.tag(tag, Some(0x80809C36))?;
        let nd = owner.pointer(24)?;
        if owner.u32(nd - 4)? != 0x808084E9 {
            continue;
        }
        let events = owner
            .array(nd + 0x168, 24, Some(0x808093E6))?
            .into_iter()
            .map(|row| owner.pointer(row + 16))
            .collect::<Result<Vec<_>>>()?;
        let names = events
            .iter()
            .map(|&at| owner.u32(at + 16))
            .collect::<Result<Vec<_>>>()?;
        if names != expected {
            continue;
        }
        // Other source variants may share event names and retain all four effects.
        if owner.u32(events[3] - 4)? != 0x808093CB {
            continue;
        }
        let mut systems = Vec::new();
        for &event in &events[..3] {
            ensure!(
                owner.u32(event - 4)? == 0x80806CC6 && owner.u32(event + 0x30)? == 4,
                "A direct particle event was removed with its unused group"
            );
            let rows = owner.array(event + 0x40, 24, Some(0x80806CC8))?;
            ensure!(
                !rows.is_empty(),
                "Retained direct particle event has no system"
            );
            for row in rows {
                let system = owner.u32(row + 16)?;
                ensure!(
                    owner.u32(row + 20)? == u32::MAX
                        && stock.get_entry(tiger_pkg::TagHash(system)).is_none(),
                    "Retained event no longer selects a private direct system"
                );
                staged.tag(system, Some(0x80806E28))?;
                systems.push(system);
            }
        }
        for field in [0x200, 0x230, 0x240] {
            ensure!(
                owner.u64(nd + field)? == 0,
                "Unused particle table was retained"
            );
        }
        let instance = usize::try_from(owner.u64(nd + 0x1F8)?)?;
        ensure!(
            owner.u64(instance + 0x20)? == 0 && owner.u32(nd + 0x268)? == 0,
            "Unused particle runtime capacity was retained"
        );
        evidence.push(json!({"owner":tag,"events":names,"direct_systems":systems,
            "sha256":hex::encode(Sha256::digest(&owner.0))}));
    }
    ensure!(
        !evidence.is_empty(),
        "Required private particle omission case was not emitted"
    );
    Ok(json!(evidence))
}

fn bank_dependencies(
    source: &Payload,
    owners: &BTreeMap<u32, Payload>,
) -> Result<serde_json::Value> {
    let sd = source.pointer(24)?;
    let rows = source.array(sd + 0x148, 112, Some(0x808095A9))?;
    ensure!(!rows.is_empty(), "Source channel bank is empty");
    let names = rows
        .iter()
        .map(|row| source.u32(*row))
        .collect::<Result<Vec<_>>>()?;
    let mut candidates = Vec::new();
    for (tag, owner) in owners {
        let d = owner.pointer(24)?;
        if owner.u32(d - 4)? != 0x80809790 {
            continue;
        }
        let native = owner.array(d + 0xD8, 112, Some(0x808097A1))?;
        let actual = native
            .iter()
            .map(|row| owner.u32(*row))
            .collect::<Result<Vec<_>>>()?;
        if actual == names {
            candidates.push((*tag, owner, native));
        }
    }
    ensure!(
        candidates.len() == 1,
        "Private channel bank is absent or ambiguous"
    );
    let (tag, native, native_rows) = &candidates[0];
    let mut evidence = Vec::new();
    let mut computed_edges = 0;
    for (ordinal, (&s, &n)) in rows.iter().zip(native_rows).enumerate() {
        let inputs = |p: &Payload, row: usize| -> Result<Vec<u32>> {
            p.array(row + 56, 4, Some(0x80800007))?
                .iter()
                .map(|at| {
                    names
                        .get(p.u32(*at)? as usize)
                        .copied()
                        .context("Bank input declaration is outside its table")
                })
                .collect()
        };
        let expected = inputs(source, s)?;
        ensure!(
            expected == inputs(native, n)?,
            "Bank input identity changed"
        );
        let fanout = |p: &Payload, row: usize| -> Result<BTreeSet<u32>> {
            let mut selected = BTreeSet::new();
            for (word, at) in p.array(row + 80, 4, Some(0x8080000B))?.iter().enumerate() {
                let mask = p.u32(*at)?;
                for bit in 0..32 {
                    if mask & (1 << bit) != 0 {
                        selected.insert(
                            *names
                                .get(word * 32 + bit)
                                .context("Bank fanout exceeds its table")?,
                        );
                    }
                }
            }
            Ok(selected)
        };
        let scheduled = fanout(source, s)?;
        ensure!(
            scheduled == fanout(native, n)?,
            "Bank refresh fanout changed"
        );
        for input in source.array(s + 56, 4, Some(0x80800007))? {
            let index = source.u32(input)? as usize;
            if source.u64(rows[index] + 8)? != 0 {
                ensure!(
                    index < ordinal && native.u64(native_rows[index] + 8)? != 0,
                    "Computed input was dropped or moved after its consumer"
                );
                computed_edges += 1;
            }
        }
        evidence.push(json!({"name":names[ordinal],"inputs":expected,"refreshes":scheduled}));
    }
    ensure!(
        computed_edges > 0,
        "Required computed bank dependencies were not inspected"
    );
    Ok(
        json!({"tag":tag,"computed_edges":computed_edges,"declarations":evidence,
        "sha256":hex::encode(Sha256::digest(&native.0))}),
    )
}

fn shield_response(
    source: &mut Reader,
    staged: &mut Reader,
    native: &PackageManager,
    owners: &BTreeMap<u32, Payload>,
) -> Result<serde_json::Value> {
    let original = source.tag(0x80C3C10C, Some(0x80809B06))?;
    let source_rows = original.array(original.pointer(24)? + 0xB0, 8, Some(0x80803FAD))?;
    ensure!(source_rows.len() == 1, "Shield response source changed");
    let expected = original.pointer(source_rows[0])?;
    ensure!(
        original.u32(expected - 4)? == 0x80803FCD,
        "Shield response source type differs"
    );
    let mut resources = BTreeSet::new();
    for (tag, owner) in owners {
        for (_, child) in
            declared_payload_references(native, *tag, &owner.0).map_err(anyhow::Error::msg)?
        {
            if native
                .get_entry(tiger_pkg::TagHash(child))
                .is_some_and(|entry| entry.reference == 0x80809C36)
            {
                resources.insert(child);
            }
        }
    }
    let mut matches = Vec::new();
    for tag in resources {
        let payload = staged.tag(tag, Some(0x80809C36))?;
        let definition = payload.pointer(24)?;
        if payload.u32(definition - 4)? != 0x80804B02 {
            continue;
        }
        for row in payload.array(definition + 0x78, 8, Some(0x80804B3D))? {
            let record = payload.pointer(row)?;
            if payload.u32(record - 4)? == 0x80804B5D {
                ensure!(
                    original.bytes::<16>(expected)? == payload.bytes::<16>(record)?,
                    "Shield health-region response differs from source"
                );
                matches.push(json!({"tag":tag,"offset":record,
                    "sha256":hex::encode(Sha256::digest(&payload.0))}));
            }
        }
    }
    ensure!(
        matches.len() == 1,
        "Shield response is absent or duplicated in the reachable resources"
    );
    Ok(json!(matches))
}

fn timed_reports(
    source: &mut Reader,
    owners: &BTreeMap<u32, Payload>,
) -> Result<Vec<serde_json::Value>> {
    let mut reports = Vec::new();
    for tag in [0x80A5AE6E, 0x80A5AE7C] {
        let original = source.tag(tag, Some(0x80809B06))?;
        let sd = original.pointer(24)?;
        let key = original.u32(sd + 0x2A8)?;
        let candidates = owners
            .iter()
            .filter(|(_, owner)| {
                owner.pointer(24).ok().is_some_and(|d| {
                    owner.u32(d - 4).ok() == Some(0x80804205)
                        && owner.u32(d + 0x2C4).ok() == Some(key)
                })
            })
            .collect::<Vec<_>>();
        ensure!(
            candidates.len() == 1,
            "Timed report owner is absent or ambiguous"
        );
        let (emitted, owner) = candidates[0];
        let nd = owner.pointer(24)?;
        ensure!(
            original.u64(sd + 0x230)? == 0
                && original.u32(sd + 0x1C8)? == owner.u32(nd + 0x160)?
                && original.u32(sd + 0x238)? == owner.u32(nd + 0x1CC)?
                && owner.u32(nd + 0x1C8)? == 0x811C9DC5,
            "Timed status lost its name, remaining-time mode or empty grouping key"
        );
        reports.push(json!({"source":tag,"emitted":emitted,"duration_key":key,
            "status_name":owner.u32(nd+0x160)?,"remaining_time_mode":owner.u32(nd+0x1CC)?,
            "grouping_key":owner.u32(nd+0x1C8)?}));
    }
    Ok(reports)
}

fn category_dictionary(
    reader: &mut Reader,
    manager: &PackageManager,
    stock: &PackageManager,
    owners: &BTreeMap<u32, Payload>,
) -> Result<Payload> {
    let mut dictionaries = BTreeSet::new();
    for (&tag, owner) in owners {
        for (_, target) in
            declared_payload_references(manager, tag, &owner.0).map_err(anyhow::Error::msg)?
        {
            if manager
                .get_entry(tiger_pkg::TagHash(target))
                .is_some_and(|entry| entry.reference == 0x808094B4)
            {
                ensure!(
                    stock.get_entry(tiger_pkg::TagHash(target)).is_none(),
                    "Attachment query still uses the stock dictionary"
                );
                dictionaries.insert(target);
            }
        }
    }
    ensure!(
        dictionaries.len() == 1,
        "Attachment does not reference one private category dictionary"
    );
    Ok((*reader.tag(*dictionaries.first().unwrap(), Some(0x808094B4))?).clone())
}

// The dictionary is decoded from the staged package, independently of the
// translator's correspondence. A singleton basis proves each union bank for
// arbitrary category sets because intersection distributes over their union.
fn category_masks(
    source: (&Payload, usize, &Payload),
    native: (&Payload, usize, &Payload),
    stock: &Payload,
) -> Result<serde_json::Value> {
    let (sp, sm, sd) = source;
    let (np, nm, nd) = native;
    let names = |p: &Payload| -> Result<Vec<u32>> {
        p.array(8, 4, Some(0x80800070))?
            .into_iter()
            .map(|at| p.u32(at))
            .collect()
    };
    let source_names = names(sd)?;
    let native_names = names(nd)?;
    let stock_names = names(stock)?;
    ensure!(
        native_names.len() <= 320 && native_names.starts_with(&stock_names),
        "Private category dictionary moved stock indices or exceeds capacity"
    );
    let mut labels = BTreeMap::new();
    for (index, &name) in native_names.iter().enumerate() {
        labels.insert(name, BTreeSet::from([index]));
    }
    for row in nd.array(24, 44, Some(0x808094BE))? {
        let name = nd.u32(row)?;
        let bits = (0..320)
            .filter(|&bit| nd.0[row + 4 + bit / 8] & (1 << (bit % 8)) != 0)
            .collect::<BTreeSet<_>>();
        ensure!(
            labels.insert(name, bits).is_none(),
            "Category group shadows an atom"
        );
    }
    let source_ops: &[usize] = match sp.u32(sm - 4)? {
        0x8080976E => &[0, 2],
        0x8080976D => &[0, 1, 2, 3],
        _ => anyhow::bail!("Source category predicate has an unknown form"),
    };
    let native_ops: &[usize] = match np.u32(nm - 4)? {
        0x808093F6 => &[0, 2],
        0x808093F5 => &[0, 1, 2, 3],
        _ => anyhow::bail!("Native category predicate has an unknown form"),
    };
    ensure!(
        source_ops == native_ops,
        "Category predicate operations changed"
    );
    let mut merged = BTreeMap::<usize, Vec<u32>>::new();
    for &name in &source_names {
        if let Some(bits) = labels.get(&name) {
            ensure!(
                bits.len() == 1,
                "Source atom does not resolve to one native query bit"
            );
            merged.entry(*bits.first().unwrap()).or_default().push(name);
        }
    }
    for (bank, &operation) in source_ops.iter().enumerate() {
        ensure!(
            sp.u8(sm + source_ops.len() * 56 + bank)?
                == np.u8(nm + native_ops.len() * 40 + bank)?,
            "Category predicate empty flag changed"
        );
        let mut reachable = BTreeSet::new();
        for (index, &name) in source_names.iter().enumerate() {
            let expected = sp.u8(sm + bank * 56 + index / 8)? & (1 << (index % 8)) != 0;
            let bits = labels.get(&name);
            let actual = bits.is_some_and(|bits| {
                bits.iter()
                    .any(|&bit| np.0[nm + bank * 40 + bit / 8] & (1 << (bit % 8)) != 0)
            });
            ensure!(
                actual == expected,
                "Category predicate changed for atom {name:08X} in bank {bank}"
            );
            if expected {
                if operation == 1 || operation == 3 {
                    ensure!(
                        bits.is_some_and(|bits| bits.iter().all(|bit| merged[bit].len() == 1)),
                        "An all-members predicate uses a merged category"
                    );
                }
                reachable.extend(bits.into_iter().flatten().copied());
            }
        }
        let actual = (0..320)
            .filter(|&bit| np.0[nm + bank * 40 + bit / 8] & (1 << (bit % 8)) != 0)
            .collect::<BTreeSet<_>>();
        ensure!(
            actual == reachable,
            "Native category predicate contains an unrelated bit"
        );
    }
    merged.retain(|_, names| names.len() > 1);
    Ok(
        json!({"source_atoms":source_names.len(),"native_atoms":native_names.len(),
        "stock_atoms":stock_names.len(),"merged_union_atoms":merged,
        "checked_operations":source_ops,"native_dictionary_sha256":hex::encode(Sha256::digest(&nd.0))}),
    )
}

fn event_filters(
    source: (&Payload, usize),
    native: (&Payload, usize),
    fields: &[(usize, usize)],
    dictionaries: (&Payload, &Payload, &Payload),
) -> Result<Vec<serde_json::Value>> {
    let ((sp, sa), (np, na), (sd, nd, stock)) = (source, native, dictionaries);
    let mut output = Vec::new();
    for &(sf, nf) in fields {
        if sp.u8(sa + sf)? == 255 {
            if np.u8(na + nf)? != 255 {
                let mask = np.pointer(na + nf + 8)?;
                let count = match np.u32(mask - 4)? {
                    0x808093F6 => 2,
                    0x808093F5 => 4,
                    _ => anyhow::bail!("Empty event filter has an unknown mask"),
                };
                for bank in 0..count {
                    ensure!(
                        np.u8(mask + count * 40 + bank)? == 1,
                        "An empty source event filter became restrictive"
                    );
                }
            }
            output.push(json!({"source_predicate":sa+sf,"native_predicate":na+nf,"empty":true}));
            continue;
        }
        ensure!(
            sp.u8(sa + sf)? <= 1 && np.u8(na + nf)? <= 1,
            "Required event category filter became unconditional"
        );
        output.push(category_masks(
            (sp, sp.pointer(sa + sf + 8)?, sd),
            (np, np.pointer(na + nf + 8)?, nd),
            stock,
        )?);
    }
    Ok(output)
}

fn hosted_filters(
    source_reader: &mut Reader,
    staged: &mut Reader,
    manager: &PackageManager,
    owners: &BTreeMap<u32, Payload>,
    dictionaries: (&Payload, &Payload, &Payload),
) -> Result<serde_json::Value> {
    let mut actions = BTreeSet::new();
    for (&tag, owner) in owners {
        for (_, target) in
            declared_payload_references(manager, tag, &owner.0).map_err(anyhow::Error::msg)?
        {
            if manager
                .get_entry(tiger_pkg::TagHash(target))
                .is_some_and(|entry| entry.reference == 0x808040B5)
            {
                actions.insert(target);
            }
        }
    }
    ensure!(
        actions.len() == 1,
        "Volatile Rounds lost its private hosted action"
    );
    let np = staged.tag(*actions.first().unwrap(), Some(0x808040B5))?;
    let native = action::decode(&np.0).map_err(anyhow::Error::msg)?;
    let sp = source_reader.tag(0x80C33C21, Some(0x8080B835))?;
    let source = parhelion_import::d2_mot::gameplay::perks::controller::read(&sp)?;
    let hit = native
        .conditions()
        .into_iter()
        .filter(|condition| condition.kind == 4)
        .collect::<Vec<_>>();
    ensure!(hit.len() == 1, "Hosted action lost its damage trigger");
    let hit = event_filters(
        (&sp, source.states[0].transitions[0].conditions[0].offset),
        (&np, hit[0].offset),
        &[(0x88, 0x80)],
        dictionaries,
    )?;
    let source_effects = sp
        .array(source.states[0].offset + 8, 24, Some(0x808037AB))?
        .into_iter()
        .map(|at| sp.pointer(at))
        .collect::<Result<Vec<_>>>()?;
    let native_effects = native
        .effects()
        .filter(|effect| effect.kind == 40)
        .collect::<Vec<_>>();
    ensure!(
        source_effects.len() == native_effects.len() && !source_effects.is_empty(),
        "Hosted action lost a persistent event modifier"
    );
    let mut modifiers = Vec::new();
    for (&s, n) in source_effects.iter().zip(native_effects) {
        ensure!(
            sp.u32(s - 4)? == 0x8080B4E1,
            "Unexpected source persistent effect"
        );
        modifiers.push(event_filters(
            (&sp, s),
            (&np, n.offset),
            &[(0xA8, 0xA0), (0x138, 0x110)],
            dictionaries,
        )?);
    }
    Ok(json!({"action":actions,"hit":hit,"modifiers":modifiers}))
}

fn private_roots(
    decoded: &action::DecodedAction,
    payload: &Payload,
    staged: &mut Reader,
    stock: &PackageManager,
    output: &Path,
) -> Result<BTreeSet<u32>> {
    let mut roots = BTreeSet::new();
    for effect in decoded.groups.iter().flat_map(|group| &group.effects) {
        let at = match effect.kind {
            1 | 3 => effect.offset + 16,
            _ => continue,
        };
        let tag = payload.u32(at)?;
        let graph = staged.tag(tag, Some(0x80809C0F))?;
        ensure!(graph.u64(16)? > 0, "Effect graph is empty");
        ensure!(
            stock.get_entry(tiger_pkg::TagHash(tag)).is_none(),
            "Effect graph is still a stock mutable donor"
        );
        roots.insert(tag);
        fs::write(output.join(format!("effect-{tag:08X}.bin")), &graph.0)?;
    }
    ensure!(
        !roots.is_empty(),
        "Imported perk has no reachable attachment"
    );
    Ok(roots)
}

#[test]
#[ignore = "Requires clean SUNDIAL_STOCK_PACKAGES, PARHELION_IMPORT_MODERN_PACKAGES and fresh PARHELION_VOID_OUTPUT"]
fn paired_perks_survive_reload_and_have_private_reachable_runtime() -> Result<()> {
    let native = configured("SUNDIAL_STOCK_PACKAGES")?;
    let modern = configured("PARHELION_IMPORT_MODERN_PACKAGES")?;
    let output = configured("PARHELION_VOID_OUTPUT")?;
    ensure!(!output.exists(), "Use a fresh artifact directory");
    fs::create_dir_all(&output)?;
    let stock = InvestmentCatalog::load_with_cache_path(
        native.parent().context("Native root")?,
        &output.join("stock-catalog.json"),
        true,
        |_| {},
    )
    .map_err(anyhow::Error::msg)?;
    let mut stock_reader = Reader::new(&native, &output.join("stock"), false)?;
    let mut source_reader = Reader::new(&modern, &output.join("source-readback"), true)?;
    let stock_manager = manager(&stock_reader)?;
    let globals_tag = resolve_live_named_tag(&stock_manager, "investment_globals", None)
        .map_err(anyhow::Error::msg)?;
    let globals = stock_reader.tag(globals_tag.0, None)?;
    let donor = stock
        .weapon_donors()
        .into_iter()
        .find(|d| d.name == "Gnawing Hunger")
        .and_then(|d| stock.weapon_donor(d.hash))
        .context("Native Void weapon")?;
    let slots = donor
        .sockets
        .iter()
        .filter(|s| s.index >= 3 && s.max_authored_choices > 0)
        .filter_map(|s| {
            let plug = s.native_default?;
            stock
                .item_sandbox_perk_indices(plug)
                .into_iter()
                .find(|&index| {
                    load_sandbox_perk_runtime_action(&stock_manager, &globals.0, usize::from(index))
                        .is_ok()
                })
                .map(|index| (s.index as u16, plug, index))
        })
        .take(2)
        .collect::<Vec<_>>();
    ensure!(
        slots.len() == 2,
        "Required two native perk sockets are absent"
    );
    let saved: serde_json::Value =
        serde_json::from_str(include_str!("../recipes/every-end.parhelion.json"))?;
    let mut recipe = WeaponRecipe::from_json_str(&serde_json::to_string(&json!({
        "schema":1,"namespace":saved["namespace"],"collection_placement":"sunrise_badge",
        "donor":{"item_hash":format!("0x{:08X}",donor.summary.hash),"expected_name":null},
        "identity":saved["identity"],"name":saved["name"],"flavor":"Private Void import verification.","source":"Parhelion"
    }))?)?;
    recipe.rename_authored_item("Void Pair Verification")?;
    let mut originals = Vec::new();
    let mut preparation_failures = Vec::new();
    for ((hash, name), &(socket, plug, perk)) in [
        (2048641572u32, "Destabilizing Rounds"),
        (776531651, "Repulsor Brace"),
    ]
    .into_iter()
    .zip(&slots)
    {
        originals.push(
            load_sandbox_perk_runtime_action(&stock_manager, &globals.0, usize::from(perk))
                .map_err(anyhow::Error::msg)?,
        );
        let prepared = import::prepare(&import::Request {
            modern_packages: &modern,
            native_packages: &native,
            output: &output.join(format!("prepare-{hash:08X}")),
            recipe: &recipe,
            plug_hash: hash,
            socket_index: socket,
            choice_index: 0,
            source_plug_hash: plug,
            source_perk_index: perk,
            profile_key: 0,
            name,
        });
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                println!("{name} preparation failed: {error}");
                preparation_failures.push(json!({"source":hash,"name":name,"error":error}));
                continue;
            }
        };
        let mut concurrent = recipe.clone();
        concurrent.name.push_str(" Changed");
        let before = concurrent.clone();
        ensure!(
            prepared.apply(&mut concurrent).is_err() && concurrent == before,
            "Prepared import replaced concurrent work"
        );
        prepared.apply(&mut recipe).map_err(anyhow::Error::msg)?;
        fs::write(
            output.join(format!("source-{hash:08X}.json")),
            serde_json::to_vec_pretty(&prepared.provenance)?,
        )?;
    }
    if !preparation_failures.is_empty() {
        fs::write(
            output.join("preparation-failures.json"),
            serde_json::to_vec_pretty(&preparation_failures)?,
        )?;
        anyhow::bail!(
            "{} imported perks failed preparation. See preparation-failures.json.",
            preparation_failures.len()
        );
    }
    let recipe_path = output.join("void-pair.parhelion.json");
    fs::write(&recipe_path, recipe.to_json_pretty()?)?;
    let recipe = WeaponRecipe::load_json(&recipe_path)?;
    ensure!(
        recipe.overrides.socket_plug_variants.len() == 2,
        "Second import lost the first private perk"
    );
    let snapshot = BatchBuildSnapshot::new(BatchBuildRequest {
        package_directory: native.clone(),
        staging_root: output.join("build"),
        ignore_installed_authored_overlays: true,
        maps: Vec::new(),
        recipes: vec![recipe.clone()],
    })
    .map_err(anyhow::Error::msg)?;
    let built =
        parhelion::build_and_stage_snapshot_with_progress(&snapshot, |p| println!("{:?}", p.phase))
            .map_err(anyhow::Error::msg)?;
    let root = output.join("view");
    let packages = stock_view(&native, &root)?;
    for artifact in &built.artifacts {
        let destination = packages.join(&artifact.file_name);
        ensure!(!destination.exists(), "Never overwrite a stock hard link");
        fs::copy(built.run_directory.join(&artifact.file_name), destination)?;
    }
    let catalog = InvestmentCatalog::load_with_cache_path(
        &root,
        &output.join("staged-catalog.json"),
        true,
        |_| {},
    )
    .map_err(anyhow::Error::msg)?;
    let mut staged = Reader::new(&packages, &output.join("readback"), false)?;
    let staged_manager = manager(&staged)?;
    let particle_source = source_reader.tag(0x80D8EABC, Some(0x80809B06))?;
    let particle_omissions =
        omitted_indexed_particles(&particle_source, &mut staged, &stock_manager)?;
    let globals_tag = resolve_live_named_tag(&staged_manager, "investment_globals", None)
        .map_err(anyhow::Error::msg)?;
    let globals = staged.tag(globals_tag.0, None)?;
    let item = catalog
        .weapon_donor(recipe.identity.item_hash.parse_u32()?)
        .context("Staged weapon")?;
    let mut identities = BTreeSet::new();
    let mut evidence = Vec::new();
    for (case, &(socket, stock_plug, stock_perk)) in slots.iter().enumerate() {
        let plug = item.sockets[usize::from(socket)]
            .native_default
            .context("Private default plug")?;
        ensure!(
            plug != stock_plug && identities.insert(plug),
            "Private plug identity was lost"
        );
        let indices = catalog.item_sandbox_perk_indices(plug);
        ensure!(
            indices.len() == 1 && indices[0] != stock_perk,
            "Private runtime assignment was lost"
        );
        let runtime =
            load_sandbox_perk_runtime_action(&staged_manager, &globals.0, usize::from(indices[0]))
                .map_err(anyhow::Error::msg)?;
        ensure!(
            runtime.action_tag != originals[case].action_tag
                && identities.insert(runtime.action_tag.0),
            "Runtime action is shared"
        );
        let decoded = action::decode(&runtime.action_payload).map_err(anyhow::Error::msg)?;
        let p = Payload(runtime.action_payload.clone());
        let kills = decoded
            .conditions()
            .into_iter()
            .filter(|c| c.kind == 2)
            .collect::<Vec<_>>();
        ensure!(!kills.is_empty(), "Required final-blow trigger is absent");
        if case == 1 {
            let states = kills
                .iter()
                .map(|c| p.u32(c.offset + 0x144))
                .collect::<Result<BTreeSet<_>, _>>()?;
            ensure!(
                states == BTreeSet::from([0xAC496004, 0x138F7272, 0x88C78BED]),
                "Repulsor Brace lost a killed-target debuff predicate"
            );
        }
        let roots = private_roots(&decoded, &p, &mut staged, &stock_manager, &output)?;
        let owners = closure(&mut staged, &staged_manager, &stock_manager, &roots)?;
        let native_dictionary =
            category_dictionary(&mut staged, &staged_manager, &stock_manager, &owners)?;
        let source_dictionary = source_reader.tag(0x80C3A713, Some(0x808097A2))?;
        let stock_dictionary = stock_reader.tag(0x80C70CA1, Some(0x808094B4))?;
        let source_action = source_reader.tag(
            if case == 0 { 0x80C30754 } else { 0x80C30718 },
            Some(0x8080B835),
        )?;
        let source_controller =
            parhelion_import::d2_mot::gameplay::perks::controller::read(&source_action)?;
        let source_kills = &source_controller.states[0].transitions[0].conditions;
        ensure!(
            source_kills.len() == kills.len(),
            "A final-blow alternative was lost"
        );
        let dictionaries = (&*source_dictionary, &native_dictionary, &*stock_dictionary);
        let kill_filters = source_kills
            .iter()
            .zip(&kills)
            .map(|(s, n)| {
                event_filters(
                    (&source_action, s.offset),
                    (&p, n.offset),
                    &[(0x60, 0x58), (0x140, 0x128)],
                    dictionaries,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let hosted_filters = if case == 0 {
            Some(hosted_filters(
                &mut source_reader,
                &mut staged,
                &staged_manager,
                &owners,
                dictionaries,
            )?)
        } else {
            None
        };
        let source_health = source_reader.tag(
            if case == 0 { 0x80C33FB1 } else { 0x80A5AE77 },
            Some(0x80809B06),
        )?;
        let health_owners = owners
            .values()
            .filter(|owner| {
                owner.pointer(24).ok().and_then(|d| owner.u32(d - 4).ok()) == Some(0x80804B8A)
            })
            .collect::<Vec<_>>();
        ensure!(
            health_owners.len() == 1,
            "Required private health component is absent or duplicated"
        );
        let health_evidence = health(&source_health, health_owners[0])?;
        let source_bank = source_reader.tag([0x80C33FDB, 0x80A5AE99][case], Some(0x80809B06))?;
        let bank_dependencies = bank_dependencies(&source_bank, &owners)?;
        let sequences = sequence_evidence(case, &mut source_reader, &owners)?;
        let timed_reports = if case == 1 {
            Some(timed_reports(&mut source_reader, &owners)?)
        } else {
            None
        };
        let shield_response = if case == 1 {
            Some(shield_response(
                &mut source_reader,
                &mut staged,
                &staged_manager,
                &owners,
            )?)
        } else {
            None
        };
        let mut policy_evidence = Vec::new();
        let source_sections =
            source_health.array(source_health.pointer(24)? + 0x210, 368, Some(0x80804091))?;
        let target = health_owners[0];
        let native_sections = target.array(target.pointer(24)? + 0x1B0, 312, Some(0x80804C11))?;
        ensure!(
            source_sections.len() == native_sections.len(),
            "Health sections were omitted"
        );
        for (&s, &n) in source_sections.iter().zip(&native_sections) {
            ensure!(
                source_health.u32(s + 16)? == target.u32(n + 16)?,
                "Health section identity differs"
            );
            let policy = source_reader.ref64(&source_health, s + 0xD0)?;
            let emitted = target.u32(n + 0xB8)?;
            if policy == u32::MAX {
                ensure!(
                    emitted == u32::MAX,
                    "Unexpected donor damage policy remains"
                );
                continue;
            }
            ensure!(
                stock_manager
                    .get_entry(tiger_pkg::TagHash(emitted))
                    .is_none(),
                "Shield damage policy remains a stock donor"
            );
            let source_policy = source_reader.tag(policy, Some(0x8080402D))?;
            let native_policy = staged.tag(emitted, Some(0x80804BFB))?;
            let source_rows = source_policy.array(8, 40, Some(0x80804032))?;
            let native_rows = native_policy.array(8, 40, Some(0x80804C00))?;
            ensure!(
                !source_rows.is_empty() && source_rows.len() == native_rows.len(),
                "Shield damage policy is empty or incomplete"
            );
            let mut multipliers = Vec::new();
            let mut filters = Vec::new();
            for (&a, &b) in source_rows.iter().zip(&native_rows) {
                ensure!(
                    source_policy.u32(a + 32)? == native_policy.u32(b + 32)?,
                    "Shield damage multiplier differs from source"
                );
                ensure!(
                    source_policy.bytes::<8>(a)? == native_policy.bytes::<8>(b)?
                        && source_policy.bytes::<8>(a + 24)? == native_policy.bytes::<8>(b + 24)?,
                    "Shield ability-filter flags differ"
                );
                let sm = source_policy.pointer(a + 16)?;
                let nm = native_policy.pointer(b + 16)?;
                ensure!(
                    source_policy.u32(sm - 4)? == 0x8080976E
                        && native_policy.u32(nm - 4)? == 0x808093F6,
                    "Shield category predicate is absent or has the wrong type"
                );
                ensure!(
                    source_policy.bytes::<4>(sm + 112)? == native_policy.bytes::<4>(nm + 80)?,
                    "Shield category predicate flags differ"
                );
                filters.push(category_masks(
                    (&source_policy, sm, &source_dictionary),
                    (&native_policy, nm, &native_dictionary),
                    &stock_dictionary,
                )?);
                multipliers.push(native_policy.f32(b + 32)?);
            }
            fs::write(
                output.join(format!("policy-{emitted:08X}.bin")),
                &native_policy.0,
            )?;
            policy_evidence.push(
                json!({"source":policy,"emitted":emitted,"multipliers":multipliers,"filters":filters,
                "sha256":hex::encode(Sha256::digest(&native_policy.0))}),
            );
        }
        ensure!(
            case != 1 || !policy_evidence.is_empty(),
            "Repulsor Brace damage policy was dropped"
        );
        for (tag, owner) in &owners {
            fs::write(output.join(format!("owner-{tag:08X}.bin")), &owner.0)?;
        }
        fs::write(
            output.join(format!("action-{case}.bin")),
            &runtime.action_payload,
        )?;
        evidence.push(json!({"plug":plug,"perk":indices[0],"action":runtime.action_tag.0,"roots":roots,
            "health":health_evidence,"damage_policies":policy_evidence,"owners":owners.keys().collect::<Vec<_>>(),
            "bank_dependencies":bank_dependencies,"sequences":sequences,
            "kill_filters":kill_filters,"hosted_filters":hosted_filters,
            "shield_response":shield_response,"timed_reports":timed_reports,
            "source_health_sha256":hex::encode(Sha256::digest(&source_health.0)),
            "sha256":hex::encode(Sha256::digest(&runtime.action_payload))}));
        ensure!(
            staged.tag(originals[case].action_tag.0, None)?.0 == originals[case].action_payload,
            "A stock donor action changed"
        );
    }
    staged.finish()?;
    stock_reader.finish()?;
    source_reader.finish()?;
    fs::write(
        output.join("verified.json"),
        serde_json::to_vec_pretty(&json!({
            "recipe":recipe_path,"cases":evidence,"particle_omissions":particle_omissions,
            "artifacts":built.artifacts,"gameplay_verified":false,
            "limits":"Package readback proves private routing. Gameplay must verify burst hits, shield absorption, refresh, switching and cleanup."
        }))?,
    )?;
    Ok(())
}
