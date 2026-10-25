//! Constant ammunition adjustments retain their source operands.
use super::*;

pub(super) fn ammo_modifiers(p: &Payload, at: usize, kind: u8) -> Result<Action> {
    let size = if kind == 10 { 48 } else { 28 };
    ensure!(
        p.bytes::<3>(at + 1)? == [0; 3],
        "Source ammunition modifiers have an unsupported lifecycle or header"
    );
    for offset in (4..size).step_by(4) {
        ensure!(
            p.f32(at + offset)?.is_finite(),
            "Source ammunition modifier is not finite"
        );
    }
    // Source 71C380/71C4E0 and native 108B010/108BF50 add and subtract two
    // shared values plus three ammo triples. Source 71C0E0/71C200 and native
    // 108AEF0/108BE30 do the same for three ammo pairs. Both resolve the player's
    // component, not the perk's item. Preserve every lane and the zero state byte.
    let mut bytes =
        p.0.get(at..at + size)
            .context("Ammunition modifier extent")?
            .to_vec();
    bytes[0] = kind + 1;
    Ok(Action::Native {
        node: NativeNode {
            kind: kind + 1,
            bytes,
        },
    })
}

pub(super) fn rounds(p: &Payload, at: usize) -> Result<Action> {
    ensure!(
        p.u8(at + 1)? == 0 && p.bytes::<2>(at + 6)? == [0; 2],
        "Unknown fixed ammunition flags"
    );
    ensure!(
        (2..=5).all(|i| p.u8(at + i).is_ok_and(|v| v <= 1)),
        "Unknown fixed ammunition selector"
    );
    ensure!(
        p.bytes::<12>(at + 12)? == [0; 12],
        "Source slot ammunition amounts require separate slot translation"
    );
    // Source 71AAF0 and native 108A1A0 consume the signed owning-weapon amount,
    // destination, overflow, unit-scaling and action-scaling operands in this order.
    Ok(Action::AddRounds {
        rounds: i32::from_le_bytes(p.bytes::<4>(at + 8)?),
        target: AmmunitionTarget::OwningWeapon,
        store: AmmunitionStore::from_byte(p.u8(at + 2)?).context("Ammunition store")?,
        overflow: p.u8(at + 3)? != 0,
        unit_scaled: p.u8(at + 4)? != 0,
        action_scaled: p.u8(at + 5)? != 0,
    })
}
