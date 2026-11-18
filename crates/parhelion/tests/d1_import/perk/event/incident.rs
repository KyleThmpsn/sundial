//! A game event the importer renames must keep its definition in both games.
//!
//! Each game derives its kill incidents from the engine kill event through filter records, which
//! only the globals registries hold. The two events must reach the kill event through a shared
//! named ancestor, and differ from that ancestor in the same decoded restrictions and in nothing
//! else. The shared ancestors are matched by name and parent, since their undecoded scalars
//! cannot be compared across the two layouts.
use super::*;
use std::collections::BTreeSet;

/// The engine kill event, the root both games derive kill incidents from.
const KILL: u32 = 0x790C_2A95;
const SOURCE_REGISTRY: u32 = 0x8165_6529;
const NATIVE_REGISTRY: u32 = 0x80B9_E5BF;

struct Layout {
    /// The incident rows: descriptor offset, stride and class.
    rows: (usize, usize, u32),
    body: u32,
    /// The filter class and size.
    filter: (u32, usize),
    /// The scalar prefix before the first list.
    head: usize,
    /// The victim race byte, `FF` for any.
    race: usize,
    /// Decoded list descriptors: name, offset, row stride and row class.
    lists: &'static [(&'static str, usize, usize, u32)],
    /// List descriptors with no decoded counterpart, which must stay empty.
    other: &'static [usize],
    /// Regions that must equal the shared ancestor's bytes.
    fixed: &'static [(usize, usize)],
}

const SOURCE: Layout = Layout {
    rows: (0x20, 32, 0x8080_03D3),
    body: 0x8080_03C2,
    filter: (0x8080_21A3, 0x400),
    head: 0x40,
    race: 0x36,
    lists: &[
        ("damage types", 0x40, 1, 0x8080_4F9B),
        ("excluded damage types", 0x50, 1, 0x8080_4F9B),
        ("damage labels", 0x60, 24, 0x8080_0365),
        ("excluded damage labels", 0x70, 24, 0x8080_0365),
    ],
    other: &[
        0xD0, 0xE0, 0xF0, 0x100, 0x1A8, 0x1B8, 0x1C8, 0x1D8, 0x280, 0x290, 0x2A0, 0x2B0, 0x2C8,
        0x2D8, 0x2E8, 0x2F8, 0x3A0, 0x3B0, 0x3C0, 0x3D0, 0x3E8,
    ],
    fixed: &[],
};

const NATIVE: Layout = Layout {
    rows: (0x18, 40, 0x8080_8782),
    body: 0x8080_87F4,
    filter: (0x8080_6455, 0x2E0),
    head: 0x68,
    race: 0x5E,
    lists: &[
        ("damage types", 0x68, 1, 0x8080_6B02),
        ("excluded damage types", 0x78, 1, 0x8080_6B02),
        ("damage labels", 0x88, 24, 0x8080_94B3),
        ("excluded damage labels", 0xA8, 24, 0x8080_94B3),
    ],
    other: &[
        0x98, 0xB8, 0xF8, 0x108, 0x118, 0x128, 0x158, 0x168, 0x178, 0x188, 0x200, 0x210, 0x220,
        0x230,
    ],
    fixed: &[(0x1B8, 0x200), (0x260, 0x2E0)],
};

/// The restrictions an event adds to its shared ancestor.
#[derive(Debug, Default, PartialEq, Eq)]
struct Restrictions {
    races: BTreeSet<u8>,
    lists: BTreeMap<&'static str, BTreeSet<u32>>,
}

/// The event's ancestors up to the kill event, each with its filter offset.
fn chain(p: &Payload, layout: &Layout, event: u32) -> CheckResult<Vec<(u32, Option<usize>)>> {
    let (descriptor, stride, class) = layout.rows;
    let rows = p.array(descriptor, stride, Some(class))?;
    assert!(!rows.is_empty(), "Empty incident registry");
    let mut levels = Vec::new();
    let mut key = event;
    while key != KILL {
        assert!(levels.len() < 8, "Incident derivation exceeds its depth");
        let row = *rows
            .iter()
            .find(|&&row| p.u32(row).ok() == Some(key))
            .ok_or_else(|| format!("Incident {key:08X} is not registered"))?;
        let body = p.pointer(row + 8)?;
        assert_eq!(
            p.u32(body - 4)?,
            layout.body,
            "Incident {key:08X} is not derived"
        );
        let filter = if p.u64(body + 8)? == 0 {
            None
        } else {
            let filter = p.pointer(body + 8)?;
            assert_eq!(p.u32(filter - 4)?, layout.filter.0);
            Some(filter)
        };
        levels.push((key, filter));
        key = p.u32(body)?;
    }
    Ok(levels)
}

fn values(p: &Payload, at: usize, stride: usize, class: u32) -> CheckResult<BTreeSet<u32>> {
    let mut values = BTreeSet::new();
    for row in p.array(at, stride, Some(class))? {
        values.insert(if stride == 1 {
            u32::from(p.u8(row)?)
        } else {
            p.u32(row)?
        });
    }
    Ok(values)
}

/// What the levels above `ancestor` add to it, with every undecoded byte equal to its filter.
fn restrictions(
    p: &Payload,
    layout: &Layout,
    levels: &[(u32, Option<usize>)],
    ancestor: usize,
) -> CheckResult<Restrictions> {
    let base = levels[ancestor].1.ok_or("Shared ancestor has no filter")?;
    let mut found = Restrictions::default();
    for (index, &(key, filter)) in levels.iter().enumerate().take(ancestor + 1) {
        let filter = filter.ok_or_else(|| format!("Incident {key:08X} has no filter"))?;
        for &(name, at, stride, class) in layout.lists {
            let values = values(p, filter + at, stride, class)?;
            if index == ancestor {
                assert!(values.is_empty(), "Shared ancestor restricts {name}");
            } else if !values.is_empty() {
                found.lists.entry(name).or_default().extend(values);
            }
        }
        for &at in layout.other {
            assert_eq!(
                p.u64(filter + at)?,
                0,
                "Incident {key:08X} has an undecoded list at {at:X}"
            );
        }
        let race = p.u8(filter + layout.race)?;
        if index == ancestor {
            assert_eq!(race, 0xFF, "Shared ancestor restricts the victim race");
        } else if race != 0xFF {
            found.races.insert(race);
        }
        let mut regions = vec![(0, layout.race), (layout.race + 1, layout.head)];
        regions.extend_from_slice(layout.fixed);
        for (from, to) in regions {
            assert_eq!(
                p.0[filter + from..filter + to],
                p.0[base + from..base + to],
                "Incident {key:08X} differs from its ancestor at {from:X}"
            );
        }
        assert!(filter + layout.filter.1 <= p.0.len());
    }
    Ok(found)
}

/// Source event `from` and native event `to` have one definition.
pub(super) fn twin(reader: &Source, stock: &mut Reader, from: u32, to: u32) -> CheckResult<Value> {
    let source = reader.read(SOURCE_REGISTRY)?;
    let native = stock.tag(NATIVE_REGISTRY, None)?;
    let original = chain(&source, &SOURCE, from)?;
    let emitted = chain(&native, &NATIVE, to)?;
    let (s, n) = original
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(s, (key, _))| {
            emitted
                .iter()
                .skip(1)
                .position(|(other, _)| other == key)
                .map(|n| (s, n + 1))
        })
        .ok_or("Renamed incident shares no ancestor with its native event")?;
    // Below the shared ancestor both games name the same events down to the kill event.
    assert_eq!(
        original[s..].iter().map(|l| l.0).collect::<Vec<_>>(),
        emitted[n..].iter().map(|l| l.0).collect::<Vec<_>>()
    );
    let expected = restrictions(&source, &SOURCE, &original, s)?;
    let found = restrictions(&native, &NATIVE, &emitted, n)?;
    assert!(
        !expected.races.is_empty() || !expected.lists.is_empty(),
        "Renamed incident adds nothing to its ancestor"
    );
    assert!(expected.races.len() <= 1, "Source incident names two races");
    assert_eq!(expected, found, "Renamed incident changed its definition");
    Ok(json!({
        "source": format!("{from:08X}"),
        "native": format!("{to:08X}"),
        "ancestor": format!("{:08X}", original[s].0),
        "races": expected.races,
        "lists": expected.lists.iter().map(|(name, values)| {
            (name.to_string(), values.iter().map(|v| format!("{v:08X}")).collect::<Vec<_>>())
        }).collect::<BTreeMap<_, _>>(),
    }))
}

/// The source layout with the victim label filter decoded, for an incident that becomes a kill
/// condition.
const ADAPTED: Layout = Layout {
    lists: &[
        ("damage types", 0x40, 1, 0x8080_4F9B),
        ("excluded damage types", 0x50, 1, 0x8080_4F9B),
        ("damage labels", 0x60, 24, 0x8080_0365),
        ("excluded damage labels", 0x70, 24, 0x8080_0365),
        ("victim labels", 0x2C8, 24, 0x8080_0365),
        ("required victim labels", 0x2D8, 24, 0x8080_0365),
        ("excluded victim labels", 0x2E8, 24, 0x8080_0365),
        ("jointly excluded victim labels", 0x2F8, 24, 0x8080_0365),
    ],
    other: &[
        0xD0, 0xE0, 0xF0, 0x100, 0x1A8, 0x1B8, 0x1C8, 0x1D8, 0x280, 0x290, 0x2A0, 0x2B0, 0x3A0,
        0x3B0, 0x3C0, 0x3D0, 0x3E8,
    ],
    ..SOURCE
};
/// The player-kills-combatant incident, and an artifact kill talent on the same targets.
const BASE: u32 = 0x9B60_AB3F;
const SIBLING: u32 = 0x80A7_E088;

/// A source kill incident with no Shadowkeep event, emitted as a kill condition holding the
/// incident's own damage filter and victim labels. An incident without victim labels takes the
/// victim list a kill talent of the same artifacts declares, read here from that talent.
pub(super) fn kill(
    reader: &Source,
    stock: &mut Reader,
    source: &Payload,
    s: usize,
    native: &Payload,
    n: usize,
) -> CheckResult<Value> {
    assert_eq!(source.u32(s + 12)?, 0x4FE1_9919, "Another context key");
    assert_eq!(source.bytes::<5>(s)?, native.bytes::<5>(n)?);
    assert_eq!(native.bytes::<2>(n + 5)?, [2, 0]);
    assert_eq!(native.bytes::<4>(n + 0x140)?, [0; 4]);
    assert_eq!(native.u32(n + 0x144)?, 0x811C_9DC5);
    assert_eq!(native.u32(n + 0x148)?, 0x811C_9DC5);
    assert_eq!(native.u32(n + 0x14C)?, 0);
    let registry = reader.read(SOURCE_REGISTRY)?;
    let event = source.u32(s + 8)?;
    let levels = chain(&registry, &ADAPTED, event)?;
    assert_eq!(
        levels.iter().map(|level| level.0).collect::<Vec<_>>(),
        [event, BASE],
        "Adapted incident is not a child of the combatant kill"
    );
    let found = restrictions(&registry, &ADAPTED, &levels, 1)?;
    assert!(found.races.is_empty(), "Adapted incident names a race");
    assert!(
        found.lists.keys().any(|name| name.contains("damage")),
        "Adapted incident has no damage restriction"
    );
    let own = levels[0].1.ok_or("Incident filter")?;
    filter::damage(reader, stock, &registry, own + 0x40, native, n + 0xB0)?;
    let sibling = reader.read(SIBLING)?;
    let condition = sibling
        .array(0x20, 16, None)?
        .into_iter()
        .map(|row| sibling.pointer(row))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .find(|&at| sibling.u8(at + 5).ok() == Some(2))
        .ok_or("Sibling kill condition")?;
    // The sibling's two trailing floats, which every artifact kill talent stores.
    assert_eq!(
        native.bytes::<8>(n + 0x150)?,
        sibling.bytes::<8>(condition + 0x1B8)?
    );
    let victims = found.lists.keys().any(|name| name.contains("victim"));
    if victims {
        labels(reader, stock, &registry, own + 0x2C8, native, n + 8)?;
    } else {
        let expected = sibling
            .array(condition + 8, 24, Some(0x8080_0365))?
            .into_iter()
            .map(|row| sibling.u32(row))
            .collect::<Result<Vec<_>>>()?;
        assert!(!expected.is_empty(), "Sibling names no victims");
        for operation in 0..4 {
            let emitted = native
                .array(n + 8 + operation * 16, 24, Some(0x8080_94B3))?
                .into_iter()
                .map(|row| native.u32(row))
                .collect::<Result<Vec<_>>>()?;
            if operation == 0 {
                assert_eq!(
                    emitted, expected,
                    "Adapted victims differ from the kill talents'"
                );
            } else {
                assert!(
                    emitted.is_empty(),
                    "Adapted incident gained a victim operation"
                );
            }
        }
        // The sibling translation is checked by its own case. This list must compile.
        assert_ne!(
            native.u8(n + 0x58)?,
            255,
            "Adapted victim list has no compiled mask"
        );
    }
    Ok(json!({
        "source": format!("{event:08X}"),
        "native": "kill condition",
        "own_victim_labels": victims,
        "lists": found.lists.iter().map(|(name, values)| {
            (name.to_string(), values.iter().map(|v| format!("{v:08X}")).collect::<Vec<_>>())
        }).collect::<BTreeMap<_, _>>(),
        "limits": "Stored condition only. The incident listener is replaced, and the base incident's scalar restriction becomes a victim label list.",
    }))
}
