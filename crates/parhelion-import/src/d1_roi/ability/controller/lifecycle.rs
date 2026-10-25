//! Qualified lifecycle lowering for retained controller and embedded effect helpers.
use crate::{
    d1_roi::lifecycle::Lifecycle,
    tiger::{entity::lifecycle as native, payload::Payload},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

fn concrete(owner: &Payload, target: usize, source: bool) -> Result<u32> {
    let definition = usize::try_from(owner.u64(target + 8)?)?;
    ensure!(
        owner.u64(definition + 8)? == target as u64
            && owner.u32(definition)? == owner.u32(target)?,
        "Controller lifecycle pair is not reciprocal"
    );
    owner.u32(definition + if source { 16 } else { 4 })
}

/// Reconstruct the full ordered initializer table after the graph builder has constructed all
/// helper pairs. Offsets come from that builder's object map, never a particular asset identity.
pub fn initialize(
    source: &Payload,
    target: &mut Payload,
    objects: &BTreeMap<usize, usize>,
) -> Result<Value> {
    let source_root = source.pointer(16)?;
    let native_root = target.pointer(16)?;
    ensure!(
        concrete(source, source_root, true)? == 0x808025B8
            && concrete(target, native_root, false)? == 0x80804176
            && objects.get(&source_root) == Some(&native_root),
        "Controller lifecycle root differs"
    );
    let lifecycle = Lifecycle::read(source)?;
    let expected = [
        (0x8080286F, 0x80802860, u16::MAX, 0),
        (0x8080286F, 0x8080286F, u16::MAX, 2),
        (0x8080234A, 0x8080234A, 1, 2),
        (0x8080232E, 0x80802860, u16::MAX, 0),
        (0x808025B8, 0x808020C7, 11, 13),
        (0x808025B8, 0x808025B8, 0, 1),
    ];
    ensure!(
        lifecycle
            .methods
            .iter()
            .map(|row| (row.class, row.implementation, row.constructor, row.cleanup))
            .eq(expected),
        "Controller has an unqualified lifecycle contract"
    );
    ensure!(
        objects.keys().copied().collect::<BTreeSet<_>>()
            == lifecycle.methods.iter().map(|row| row.target).collect(),
        "Controller object map does not cover exactly its lifecycle targets"
    );
    let mut mappings = BTreeMap::new();
    for method in &lifecycle.methods {
        let at = objects[&method.target];
        let class = concrete(target, at, false)?;
        let (required, implementation, constructor, cleanup) =
            match (method.class, method.implementation) {
                (0x8080286F, 0x80802860) => (0x80804589, 0x80804590, u16::MAX, 0),
                (0x8080286F, 0x8080286F) => (0x80804589, 0x80804589, u16::MAX, 1),
                (0x8080232E, 0x80802860) => (0x80804585, 0x80804590, u16::MAX, 0),
                (0x808025B8, 0x808020C7) => (0x80804176, 0x80803BB7, 13, 15),
                (0x808025B8, 0x808025B8) => (0x80804176, 0x80804176, 0, 1),
                (0x8080234A, 0x8080234A) => {
                    // Source 772560/774180 operate on 234A. Native F40160 constructs the
                    // embedded 92A4 through A4DA20. Stock 4147 owners serialize its cleanup
                    // separately, before the containing 422D constructor.
                    let source_parent = source.pointer(method.target + 0x18)?;
                    let parent = target.pointer(at + 0x10)?;
                    ensure!(
                        class == 0x808092A4
                            && concrete(source, source_parent, true)? == 0x808027D5
                            && concrete(target, parent, false)? == 0x8080422D
                            && method.target == source_parent + 0x20
                            && at == parent + 0x40
                            && source_parent == source_root + 0xE10
                            && parent == native_root + 0xCF0,
                        "Controller shape helper has a different containing object"
                    );
                    mappings.insert(
                        *method,
                        vec![
                            native::Method {
                                target: at,
                                implementation: 0x808092A4,
                                constructor: u16::MAX,
                                cleanup: 0,
                            },
                            native::Method {
                                target: parent,
                                implementation: 0x8080422D,
                                constructor: 0,
                                cleanup: u16::MAX,
                            },
                        ],
                    );
                    continue;
                }
                _ => anyhow::bail!("Controller lifecycle class is unsupported"),
            };
        ensure!(
            class == required,
            "Controller lifecycle helper has the wrong native class"
        );
        mappings.insert(
            *method,
            vec![native::Method {
                target: at,
                implementation,
                constructor,
                cleanup,
            }],
        );
    }
    let mut output = target.clone();
    let boundary = output.pointer(24)?;
    lifecycle.write_expanded(&mut output, &mappings, boundary)?;
    let methods = native::read(&output).context("Read back controller initialization")?;
    let evidence = json!({"source":lifecycle,"native":methods,"gameplay_verified":false});
    *target = output;
    Ok(evidence)
}
