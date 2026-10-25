//! Bounded event conditions rebuild native filters instead of copying compiled source masks.
use super::*;
use sundial::package_authoring::sandbox_perk::action::native::{Block, Graph, NodeKind};
mod accumulator;
mod group;
mod player;

pub(super) fn read(source: &Source, p: &Payload, at: usize) -> Result<Option<NativeNode>> {
    let class = p.u32(at - 4)?;
    let kind = p.u8(at + 5)?;
    if (class, kind) == (0x80802917, 32) {
        return group::read(source, p, at).map(Some);
    }
    if (class, kind) == (0x80801100, 36) {
        return group::predicate(source, p, at).map(Some);
    }
    if (class, kind) == (0x80803101, 20) {
        return player::read(p, at).map(Some);
    }
    if (class, kind) == (0x808030DB, 33) {
        ensure!(
            (0.0..=1.0).contains(&p.f32(at)?) && p.u8(at + 4)? == 255 && p.u8(at + 6)? == 0,
            "Source event-label condition has unqualified probability or state"
        );
        // Source 76DE10 reads the event object's label set directly. Native 107C790
        // reads it after the object's four-byte prefix. Both use the +58 predicate.
        let mut bytes = vec![0; 0x68];
        bytes[..8].copy_from_slice(&p.bytes::<8>(at)?);
        bytes[5] = 32;
        bytes[0x58] = 255;
        let mut graph = Graph {
            blocks: vec![Block {
                class: 0x80803DF8,
                count: None,
                bytes,
                links: BTreeMap::new(),
            }],
        };
        label::translate(source, p, at + 8, &mut graph, 8)?;
        graph
            .validate_node(NodeKind::Condition(32))
            .map_err(anyhow::Error::msg)?;
        return Ok(Some(NativeNode {
            kind: 32,
            bytes: graph.emit().map_err(anyhow::Error::msg)?,
        }));
    }
    if (class, kind) == (0x808030D4, 12) {
        ensure!(
            (0.0..=1.0).contains(&p.f32(at)?)
                && p.u8(at + 4)? == 0xFF
                && p.bytes::<2>(at + 6)? == [0; 2]
                && p.u32(at + 8)? == 0x6CEC7A87
                && p.u32(at + 12)? == 0x18CCBF24,
            "Source game event has no qualified native event and context pair"
        );
        // Source 76C450 and native 107DD50 compare the event value, then resolve
        // the context key. This pair is the shared Orb of Light pickup protocol.
        // Other source activity signals need their own native producer evidence.
        return Ok(Some(NativeNode {
            kind,
            bytes: p.bytes::<16>(at)?.to_vec(),
        }));
    }
    if (class, kind) == (0x8080320C, 26) {
        return accumulator::read(source, p, at).map(Some);
    }
    if (class, kind) == (0x808031D9, 22) {
        ensure!(
            (0.0..=1.0).contains(&p.f32(at)?)
                && p.u8(at + 4)? == 0xFF
                && p.u8(at + 6)? == 0
                && p.u8(at + 8)? <= 1
                && p.bytes::<3>(at + 9)? == [0; 3],
            "Source byte event has unsupported state or selectors"
        );
        // Original 76B200 and native 107C850 compare node +8 with event byte zero.
        // Native compilation supplies its class and rebuilds the condition ordinal.
        return Ok(Some(NativeNode {
            kind,
            bytes: p.bytes::<12>(at)?.to_vec(),
        }));
    }
    if (class, kind) == (0x8080307B, 4) {
        return hit::read(source, p, at).map(Some);
    }
    if matches!((class, kind), (0x808031FD, 7) | (0x80803147, 8)) {
        ensure!(
            (0.0..=1.0).contains(&p.f32(at)?)
                && p.u8(at + 4)? == 0xFF
                && p.u8(at + 6)? == 0
                && (kind == 8 || p.u8(at + 8)? & !7 == 0)
                && p.bytes::<7>(at + 9)? == [0; 7]
                && p.bytes::<16>(at + 0x10)? == [0; 16],
            "Source ability event has additional player restrictions"
        );
        // Both original callbacks select the event's ability slot with +8. An empty
        // player filter passes without consulting the player's numeric components.
        return Ok(Some(NativeNode {
            kind,
            bytes: p.bytes::<32>(at)?.to_vec(),
        }));
    }
    let weapon_class = match (class, kind) {
        (0x80803086, 14) => Some(0x80803DDE),
        (0x808031EA, 15) => Some(0x80803DFA),
        (0x80803157, 16) => Some(0x80803DF7),
        (0x808031AC, 17) => Some(0x80803DF5),
        (0x80802E9B, 18) => Some(0x80803DDB),
        (0x80802C2F, 19) => Some(0x80803DE2),
        _ => None,
    };
    if let Some(class) = weapon_class {
        return weapon(source, p, at, kind, class).map(Some);
    }
    let (native_class, size, labels) = match (class, kind) {
        (0x80803145, 2) => (0x80803DE7, 0x158, 8),
        (0x808030E6, 27) => (0x808029E0, 0x98, 0x10),
        _ => return Ok(None),
    };
    ensure!(
        (0.0..=1.0).contains(&p.f32(at)?) && p.u8(at + 6)? <= 1,
        "Unsupported source event probability or retained state"
    );
    let mut bytes = vec![0; size];
    bytes[..8].copy_from_slice(&p.bytes::<8>(at)?);
    if kind == 2 {
        // Supplemental source predicates remain empty. Damage-event labels are
        // translated separately from the victim labels at +08. Native-only keys stay unset.
        ensure!(
            p.bytes::<4>(at + 0xDC)? == [0; 4]
                && p.bytes::<0x48>(at + 0xE0)? == [0; 0x48]
                && p.u8(at + 0x1B4)? & !7 == 0
                && p.u8(at + 0x1B5)? <= 1
                && p.u8(at + 0x1B6)? == 0
                && p.u8(at + 0x1B7)? <= 2,
            "Source kill condition has supplemental predicates or unsupported ability restrictions"
        );
        let _ = p.f32(at + 0x1B8)?;
        let _ = p.f32(at + 0x1BC)?;
        // Both callbacks compare the damage event's ability identity with the selected
        // owner's current ability slots. The first three bits are grenade, super and melee.
        bytes[0x140] = p.u8(at + 0x1B4)?;
        bytes[0x141] = p.u8(at + 0x1B5)?;
        // Both callbacks select the current super. Mode 1 requires it active and
        // excludes its own damage identity, mode 2 requires it inactive. Missing
        // ability banks or interfaces bypass this gate in both original engines.
        bytes[0x14C] = p.u8(at + 0x1B7)?;
        for offset in [0x144, 0x148] {
            put(&mut bytes, offset, &0x811C9DC5u32.to_le_bytes())?;
        }
        put(&mut bytes, 0x150, &p.bytes::<8>(at + 0x1B8)?)?;
    } else {
        ensure!(
            p.u8(at + 8)? <= 1
                && p.u8(at + 9)? == 0
                && p.bytes::<6>(at + 10)? == [0; 6]
                && p.bytes::<3>(at + 0xE5)? == [0; 3],
            "Source shot condition has an unsupported slot mask or header"
        );
        // Source 76B510 mode 1 accepts a nonzero event flag and mode 2 accepts zero.
        // Native 107E300 reverses these two selectors. Mode 3 adds different target gates.
        bytes[0x90] = match p.u8(at + 0xE4)? {
            0 => 0,
            1 => 2,
            2 => 1,
            _ => anyhow::bail!("Source shot condition mode requires target translation"),
        };
        bytes[8] = p.u8(at + 8)?;
    }
    bytes[labels + 0x50] = 0xFF;
    let mut graph = Graph {
        blocks: vec![Block {
            class: native_class,
            count: None,
            bytes,
            links: BTreeMap::new(),
        }],
    };
    label::translate(source, p, at + labels, &mut graph, labels)?;
    if kind == 2 {
        filter::damage(source, p, at + 0x128, at + 0x48, &mut graph, 0xB0)?;
    }
    graph
        .validate_node(NodeKind::Condition(kind))
        .map_err(anyhow::Error::msg)?;
    Ok(Some(NativeNode {
        kind,
        bytes: graph.emit().map_err(anyhow::Error::msg)?,
    }))
}

fn weapon(source: &Source, p: &Payload, at: usize, kind: u8, class: u32) -> Result<NativeNode> {
    ensure!(
        (0.0..=1.0).contains(&p.f32(at)?)
            && p.u8(at + 4)? == 0xFF
            && p.u8(at + 6)? == 0
            && p.u8(at + 8)? <= 1
            && p.u8(at + 9)? & !7 == 0
            && p.bytes::<2>(at + 10)? == [0; 2]
            && if kind == 19 {
                p.f32(at + 12)?.is_finite() && p.f32(at + 12)? >= 0.0
            } else {
                p.u32(at + 12)? == 0
            },
        "Source weapon event has unsupported flags, slots or time restrictions"
    );
    let native_kind = if kind == 19 { 19 } else { kind - 1 };
    let mut bytes = vec![0; 0x70];
    bytes[..8].copy_from_slice(&p.bytes::<8>(at)?);
    bytes[5] = native_kind;
    bytes[8] = p.u8(at + 8)?;
    if kind == 19 {
        // D1's reload callback has no event-subtype gate. Native +9 and +A admit
        // the two variants separately, and move the slot mask from +9 to +B.
        bytes[9..11].fill(1);
        bytes[11] = p.u8(at + 9)?;
        // Both callbacks compare elapsed time since the weapon's kill timestamp.
        // Keep seconds, letting the receiving engine convert its own clock units.
        bytes[12..16].copy_from_slice(&p.bytes::<4>(at + 12)?);
    } else {
        bytes[9] = p.u8(at + 9)?;
    }
    bytes[0x60] = 0xFF;
    let mut graph = Graph {
        blocks: vec![Block {
            class,
            count: None,
            bytes,
            links: BTreeMap::new(),
        }],
    };
    // The source record includes compiled masks through +E3. Checking only the
    // native-sized +70 prefix would miss those masks and their empty flags.
    label::translate(source, p, at + 0x10, &mut graph, 0x10)?;
    graph
        .validate_node(NodeKind::Condition(native_kind))
        .map_err(anyhow::Error::msg)?;
    Ok(NativeNode {
        kind: native_kind,
        bytes: graph.emit().map_err(anyhow::Error::msg)?,
    })
}
