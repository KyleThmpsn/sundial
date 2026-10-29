//! Retained revival-controller construction over a checked native common ability base.
//! Presentation connections and bank selection remain explicit graph-level obligations.
use super::{defaults, owner::Owner};
use crate::{
    presentation::put,
    tiger::{
        allocation::Row,
        entity::{lifecycle, links},
        payload::Payload,
        sequence::{input, object, relative},
    },
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use sundial::package_authoring::{PackageManager, runtime};
use tiger_pkg::TagHash;
mod query;
mod storage;
pub use storage::Offsets;

const CLASSES: (u32, u32) = (0x80804176, 0x80804177);
const BASE: (u32, u32) = (0x80803BB7, 0x80803BF6);

pub struct Prepared {
    pub payload: Payload,
    pub patches: Vec<(usize, String)>,
    pub allocation: Payload,
    /// Private common-ability dispatch with retained pending-revival and slot-binding methods.
    pub metadata: Payload,
    pub metadata_template: u32,
    pub provider: usize,
    pub offsets: Offsets,
    /// New consumers use the native common start, active and retirement providers.
    /// Each pair is (existing common definition, new mode-specific definition).
    pub script_aliases: Vec<(usize, usize)>,
    pub evidence: Value,
}

fn layout(class: u32) -> Result<(usize, Vec<(usize, u32)>)> {
    runtime::native_relocation_layout(class)
        .map_err(anyhow::Error::msg)?
        .with_context(|| format!("Native controller declaration {class:08X} is absent"))
}

fn read(manager: &PackageManager, tag: u32, class: u32) -> Result<Payload> {
    ensure!(
        manager
            .get_entry(TagHash(tag))
            .is_some_and(|entry| entry.reference == class),
        "Native controller resource {tag:08X} has another class"
    );
    Ok(Payload(
        manager.read_tag(TagHash(tag)).map_err(anyhow::Error::msg)?,
    ))
}

/// Construct the retained native implementation with native common ability state. This is
/// deliberately separate from a complete D1-controller translation. The containing graph must
/// connect the returned script aliases, publish the correct private bank and retain all common
/// providers before the owner can be loaded.
#[allow(clippy::cognitive_complexity)]
pub fn prepare(manager: &PackageManager, template: u32, symbol: &str) -> Result<Prepared> {
    let original = read(manager, template, 0x80809C36)?;
    let live = original.pointer(16)?;
    let old_definition = original.pointer(24)?;
    let definition = old_definition;
    ensure!(
        live >= 4 && definition >= 4,
        "Native controller lacks root markers"
    );
    let old = (original.u32(live - 4)?, original.u32(definition - 4)?);
    let runtime_size = layout(old.0)?.0;
    let definition_size = layout(old.1)?.0;
    ensure!(
        runtime::native_type_inherits(old.0, BASE.0)
            && runtime::native_type_inherits(old.1, BASE.1)
            && runtime_size >= 0xAE0
            && definition_size >= 0x630
            && live + runtime_size <= definition
            && original.u32(live)? == template
            && original.u32(definition)? == template
            && original.u64(live + 8)? == definition as u64
            && original.u64(definition + 8)? == live as u64
            && original.u32(live + 4)? == old.1
            && original.u32(definition + 4)? == old.0,
        "Native controller does not provide the complete common ability storage"
    );
    let methods = lifecycle::read(&original)?;
    ensure!(
        methods
            .iter()
            .filter(|method| method.target == live && method.implementation == BASE.0)
            .copied()
            .collect::<Vec<_>>()
            == vec![lifecycle::Method {
                target: live,
                implementation: BASE.0,
                constructor: 13,
                cleanup: 15,
            }],
        "Native donor has another common-controller lifecycle"
    );
    let original_callbacks = original
        .array(0x20, 8, Some(0x8080919F))?
        .into_iter()
        .map(|row| {
            let reference = original.pointer(row)?;
            let at = reference.checked_sub(8).context("Native callback prefix")?;
            ensure!(
                original.u32(reference)? == template,
                "Native callback has an external owner"
            );
            Ok((
                at,
                original.u32(at)?,
                original.u32(at + 4)?,
                usize::try_from(original.u64(reference + 8)?)?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        !original_callbacks.is_empty(),
        "Native common controller has no callbacks"
    );

    let (mut payload, offsets) =
        storage::grow(manager, template, &original, live, definition, old)?;
    let definition = offsets.map(definition)?;
    // Common storage and its out-of-line expressions retain their checked targets through
    // relocation. Only the derived body is replaced. No source-build pointer is copied.
    put(
        &mut payload.0,
        live + 0xAE0,
        &defaults::runtime_controller()[0xAE0..],
    )?;
    put(
        &mut payload.0,
        definition + 0x630,
        &defaults::definition_controller()[0x630..],
    )?;
    put(&mut payload.0, 0x20, &[0; 32])?;
    put(&mut payload.0, live - 4, &CLASSES.0.to_le_bytes())?;
    put(&mut payload.0, definition - 4, &CLASSES.1.to_le_bytes())?;
    put(&mut payload.0, live + 4, &CLASSES.1.to_le_bytes())?;
    put(&mut payload.0, definition + 4, &CLASSES.0.to_le_bytes())?;
    let mut owner = Owner::adopt(payload, symbol)?;

    // Native declaration names pair these embedded objects. Children point to the containing
    // runtime, while each definition points back to its own runtime object.
    for (at, settings, classes, parent) in [
        (0xCC0, 0x7C8, (0x80808518, 0x80808519), 0),
        (0xCF0, 0x7F0, (0x8080422D, 0x8080422E), 0),
        (0xD30, 0x800, (0x808092A4, 0x808092A1), 0xCF0),
        (0xE00, 0x8E8, (0x808089F5, 0x808089F6), 0xCF0),
        (0xE30, 0xA20, (0x808089F5, 0x808089F6), 0),
        (0xE70, 0xA80, (0x80804585, 0x80804586), 0),
        (0xE90, 0xAA8, (0x808089F5, 0x808089F6), 0xE70),
    ] {
        ensure!(
            at + layout(classes.0)?.0 <= 0xF40 && settings + layout(classes.1)?.0 <= 0xB30,
            "Retained helper leaves its declaration"
        );
        owner.pair(
            live + at,
            definition + settings,
            classes,
            Some(live + parent),
        )?;
    }

    // 422D:0 calls A4DA20 with an absent external query handle. That constructor requires
    // 92A1 +10 to name geometry even when this presentation-only helper has no damage resource.
    // The native 4147 helper uses this bounded 92B1 kind-2 geometry, radius 1 and no nested
    // provider. Declaration defaults alone leave the pointer null and reject the entire graph.
    ensure!(
        layout(0x808092B1)?.0 == 16
            && runtime::native_type_inherits(0x808092B1, 0x808092B8)
            && owner.payload.u64(definition + 0x810)? == 0,
        "Retained query geometry declaration differs"
    );
    let geometry = object(&mut owner.payload, 0x808092B1, 16)?;
    put(&mut owner.payload.0, geometry, &2u32.to_le_bytes())?;
    put(&mut owner.payload.0, geometry + 4, &1.0f32.to_le_bytes())?;
    relative(&mut owner.payload, definition + 0x810, geometry)?;
    owner.size()?;

    let mut script_aliases = Vec::new();
    for (at, settings) in [(0xAE0, 0x6C0), (0xBD0, 0x630)] {
        for index in 0..3 {
            let written = input::write(
                &mut owner.payload,
                live + at + index * 0x50,
                definition + settings + index * 0x30,
                live,
                symbol,
            )?;
            let common = definition + 0x208 + index * 0x30;
            // An absent donor sequence can be optional. Native connection setup reads +10
            // before rejecting an unbound consumer, so aliases must inherit this policy as
            // well as the graph endpoint. A fresh required input would reject optional gaps.
            let policy = owner.payload.bytes::<8>(common + 16)?;
            put(&mut owner.payload.0, written.definition + 16, &policy)?;
            owner.patches.extend(written.patches);
            script_aliases.push((common, written.definition));
        }
    }

    // The common providers dispatch the native base. A donor-specific method table cannot be
    // carried onto a different concrete implementation merely because its wrapper fits.
    for offset in [
        0x418, 0x430, 0x448, 0x460, 0x478, 0x490, 0x4B0, 0x4D0, 0x4F0, 0x510, 0x530, 0x550, 0x590,
        0x5B0, 0x5D0, 0x5F0, 0x610,
    ] {
        let at = definition + offset;
        let metadata = read(manager, owner.payload.u32(at + 8)?, 0x80809C54)?;
        let provider = links::Interface::read(
            &owner.payload,
            links::Object {
                owner: template,
                class: metadata.u32(8)?,
                offset: at as u64,
            },
            &metadata,
            false,
        )?;
        ensure!(
            provider.definition_offset == definition
                && !provider.methods.is_empty()
                && provider
                    .methods
                    .iter()
                    .all(|method| runtime::native_type_inherits(
                        CLASSES.0,
                        method.implementation_class
                    )),
            "Native controller provider dispatch is donor-specific"
        );
    }

    // Interface 3BEA row 14 dispatches slot assignment, signature 46672D02. The retained
    // implementation calls the common setup, binds all six specialized sequences and copies
    // the pending player revival request. Keeping base method 30 would omit those steps.
    // Property application remains the separate root callback +890, not this interface.
    let provider = definition + 0x430;
    let metadata_template = owner.payload.u32(provider + 8)?;
    let mut metadata = read(manager, metadata_template, 0x80809C54)?;
    let dispatch = metadata.array(16, 24, Some(0x80809C56))?;
    ensure!(
        metadata.u32(8)? == 0x80803BEA && metadata.u32(12)? == 0x80803C0D && dispatch.len() == 71,
        "Native common ability dispatch has another interface contract"
    );
    // Row 5 dispatches 3BB7:54 (B96FA0). Retained method 9 handles a pending revival latch
    // before tail-calling that base method, with the same 74BC09D7 signature. D1's primary
    // ability interface independently selects its retained method 9 at row 5.
    for (row, original, replacement) in [(5, 54u32, 9u32), (14, 30, 2)] {
        let binding = dispatch[row];
        ensure!(
            metadata.u32(binding)? == BASE.0
                && metadata.u32(binding + 4)? == original
                && metadata.bytes::<16>(binding + 8)? == [0; 16],
            "Native ability dispatch has another implementation"
        );
        put(&mut metadata.0, binding, &CLASSES.0.to_le_bytes())?;
        put(&mut metadata.0, binding + 4, &replacement.to_le_bytes())?;
    }
    owner
        .patches
        .push((provider + 8, format!("{symbol}-interface")));

    let references = storage::fields(manager, template, &owner.payload)?;
    let mut reachable = BTreeSet::from([live]);
    for (at, kind) in references {
        let fresh = (live + 0xAE0..live + 0xF40).contains(&at)
            || (definition + 0x630..definition + 0xB30).contains(&at);
        let target = match kind {
            1..=3 if owner.payload.u64(at)? != 0 => Some(owner.payload.pointer(at)?),
            9 if owner.payload.u32(at)? == template => {
                Some(usize::try_from(owner.payload.u64(at + 8)?)?)
            }
            _ => None,
        };
        if !fresh && let Some(target) = target {
            ensure!(
                !(live + 0xAE0..offsets.map(live + runtime_size)?).contains(&target)
                    && !(definition + 0x630..offsets.map(old_definition + definition_size)?)
                        .contains(&target),
                "Native common state refers to replaced donor-specific storage"
            );
        }
        if !matches!(kind, 4 | 9) || owner.payload.u32(at)? != template {
            continue;
        }
        owner.patches.push((at, symbol.into()));
        if kind != 9 {
            continue;
        }
        let target = usize::try_from(owner.payload.u64(at + 8)?)?;
        if target == live && owner.payload.u32(at + 4)? == old.0 {
            put(&mut owner.payload.0, at + 4, &CLASSES.0.to_le_bytes())?;
        }
        if target == definition && owner.payload.u32(at + 4)? == old.1 {
            put(&mut owner.payload.0, at + 4, &CLASSES.1.to_le_bytes())?;
        }
        if target >= live
            && target.checked_add(16).is_some_and(|end| end <= definition)
            && owner.payload.u32(target)? == template
            && let Ok(twin) = usize::try_from(owner.payload.u64(target + 8)?)
            && twin
                .checked_add(16)
                .is_some_and(|end| end <= owner.payload.0.len())
            && owner.payload.u64(twin + 8)? == target as u64
        {
            reachable.insert(target);
        }
    }

    // Property application is callback +890 (signature 328ABD87), not method 24 of
    // the common ability provider. +788 copies the revival latch between runtime instances.
    let overrides = BTreeMap::from([
        (0x680, 6),
        (0x698, 7),
        (0x6B0, 8),
        (0x758, 10),
        (0x788, 4),
        (0x7A0, 5),
        (0x848, 11),
        (0x860, 12),
        (0x878, 13),
        (0x890, 3),
        (0x8A8, 14),
    ]);
    let mut retained = Vec::new();
    for (at, implementation, method, target) in original_callbacks {
        // The old derived implementation and its private callbacks are replaced as a whole.
        if (live + 0xAE0..live + runtime_size).contains(&at) {
            continue;
        }
        let at = offsets.map(at)?;
        let target = offsets.map(target)?;
        if target == live {
            if at == live + 0x6C8 {
                owner.callback(at, BASE.0, 91, live)?;
                continue;
            }
            if at >= live && overrides.contains_key(&(at - live)) {
                continue;
            }
            ensure!(
                at >= live
                    && at + 24 <= live + 0xAE0
                    && runtime::native_type_inherits(BASE.0, implementation),
                "Native donor has an unqualified root callback"
            );
        } else if !reachable.contains(&target) {
            continue;
        }
        ensure!(
            at + 24 <= live + 0xAE0 || at >= offsets.map(live + runtime_size)?,
            "Native common callback overlaps replaced derived storage"
        );
        owner.callback(at, implementation, method, target)?;
        retained.push(
            json!({"offset":at,"implementation":implementation,"method":method,"target":target}),
        );
    }
    for (offset, method) in overrides {
        owner.callback(live + offset, CLASSES.0, method, live)?;
    }

    for mut method in methods {
        if method.target == live {
            continue;
        }
        if (live + 0xAE0..live + runtime_size).contains(&method.target) {
            continue;
        }
        method.target = offsets.map(method.target)?;
        if reachable.contains(&method.target) {
            owner.initializers.push(method);
        }
    }
    let query = query::write(&mut owner)?;
    owner.initializers.extend([
        lifecycle::Method {
            target: live + 0xD30,
            implementation: 0x808092A4,
            constructor: u16::MAX,
            cleanup: 0,
        },
        lifecycle::Method {
            target: live + 0xCF0,
            implementation: 0x8080422D,
            constructor: 0,
            cleanup: u16::MAX,
        },
        lifecycle::Method {
            target: live + 0xE70,
            implementation: 0x80804590,
            constructor: u16::MAX,
            cleanup: 0,
        },
        lifecycle::Method {
            target: live,
            implementation: BASE.0,
            constructor: 13,
            cleanup: 15,
        },
        lifecycle::Method {
            target: live,
            implementation: CLASSES.0,
            constructor: 0,
            cleanup: 1,
        },
    ]);
    let mut allocation = read(manager, original.u32(0x44)?, 0x80809BBB)?;
    ensure!(
        allocation.u64(0)? == allocation.0.len() as u64
            && allocation.u32(8)? == 0x811C9DC5
            && allocation.u32(24)? == u32::MAX
            && allocation.u32(28)? == 0
            && allocation.u64(16)? != 0,
        "Native controller allocation has another root contract"
    );
    // Preserve the common-base inheritance, replacing donor-derived fields with the retained
    // query's action and recipient tracking allocations.
    Row::write(&[query.allocation], &mut allocation, 32)?;
    let allocation_size = allocation.0.len() as u64;
    put(&mut allocation.0, 0, &allocation_size.to_le_bytes())?;
    put(&mut owner.payload.0, 0x44, &u32::MAX.to_le_bytes())?;
    owner.patches.push((0x44, format!("{symbol}-allocation")));
    put(
        &mut owner.payload.0,
        0x48,
        &u64::try_from(definition - live)?.to_le_bytes(),
    )?;
    owner.finish(query.mutable_end)?;
    owner.patches.sort();
    owner.patches.dedup();
    let evidence = json!({"template":template,"native_classes":[CLASSES.0,CLASSES.1],
        "template_sha256":hex::encode(Sha256::digest(&original.0)),
        "retained_callbacks":retained,"script_aliases":script_aliases,
        "query_geometry":{"class":0x808092B1u32,"kind":2,"radius":1.0,"nested_provider":false},
        "query_action":query.evidence,
        "sequence_policy":"Each specialized input retains its common donor's connection policy.",
        "slot_binding":{"interface":0x80803BEAu32,"row":14,"implementation":CLASSES.0,"method":2},
        "pending_revival":{"interface":0x80803BEAu32,"row":5,"implementation":CLASSES.0,"method":9},
        "gameplay_verified":false,
        "limits":["Uses native common ability behavior and presentation providers.",
            "Requires graph connection assembly and selected private revival properties.",
            "Does not establish request authority, Super consumption or completed resurrection."]});
    Ok(Prepared {
        payload: owner.payload,
        patches: owner.patches,
        allocation,
        metadata,
        metadata_template,
        provider,
        offsets,
        script_aliases,
        evidence,
    })
}
