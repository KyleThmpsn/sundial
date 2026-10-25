//! Constant named-property writes, the effect perks use to switch a weapon channel on while
//! they are active, such as the rocket channel that releases tracking cluster missiles.
use anyhow::{Result, ensure};
use sundial::package_authoring::sandbox_perk::program::Action;

use crate::d2_mot::payload::Payload;

/// The modern code that pushes constant zero and returns, as in every constant value program
/// this effect carries. Shadowkeep's constant program is `34 00 3E 00`, which the native
/// Property action writes itself.
const CONSTANT_PROGRAM: [u8; 4] = [0x42, 0x00, 0x4C, 0x00];

/// Lower a modern `80802093` named property effect to the native Named Property node.
///
/// The modern node keeps the native head through `+0x48`: kind 10 and its retained byte,
/// target `+02`, flag `+03`, ability mask `+04`, key `+08`, then the value program's code and
/// constant arrays at `+18` and `+28` and the two unit words at `+38` and `+40`. It inserts 24
/// bytes before the native `+48` block, sixteen zero bytes, an `FF` selector at `+58` and an
/// empty key at `+5C`, so the input, operation and removal bytes sit at `+60` to `+62` and the
/// removal value at `+64`. Only a single constant pushed to all four lanes is accepted.
pub fn named_property(source: &Payload, at: usize) -> Result<Action> {
    ensure!(
        at >= 4 && source.u32(at - 4)? == 0x8080_2093,
        "unsupported source named property class"
    );
    ensure!(
        source.u8(at)? == 10 && source.u8(at + 1)? == 1 && source.u32(at + 12)? == 0,
        "source named property header differs"
    );
    let key = source.u32(at + 8)?;
    ensure!(
        ![0, u32::MAX, 0x811C_9DC5].contains(&key),
        "source named property has no key"
    );
    ensure!(
        source.u64(at + 16)? == 0
            && source.u64(at + 56)? == 1
            && source.u64(at + 64)? == 1
            && source.bytes::<16>(at + 72)? == [0; 16]
            && source.bytes::<4>(at + 88)? == [0xFF, 0, 0, 0]
            && source.u32(at + 92)? == 0x811C_9DC5
            && source.u8(at + 99)? == 0,
        "source named property has value inputs or policy fields that need translation"
    );
    let code = source.array_range(at + 24, 1, Some(0x8080_0009))?;
    ensure!(
        source.0.get(code) == Some(&CONSTANT_PROGRAM[..]),
        "source named property value is not a constant"
    );
    let constants = source.array(at + 40, 16, Some(0x8080_0090))?;
    let [lanes] = constants.as_slice() else {
        anyhow::bail!("source named property needs one constant vector");
    };
    let value_bits = source.u32(*lanes)?;
    ensure!(
        (1..4).all(|lane| source.u32(lanes + lane * 4).ok() == Some(value_bits))
            && f32::from_bits(value_bits).is_finite(),
        "source named property constant differs between lanes"
    );
    let restore_bits = source.u32(at + 100)?;
    ensure!(
        f32::from_bits(restore_bits).is_finite(),
        "source named property removal value is not finite"
    );
    Ok(Action::Property {
        key,
        target: source.u8(at + 2)?,
        operation_byte: source.u8(at + 97)?,
        removal: source.u8(at + 98)?,
        value_bits,
        restore_bits,
        ability_mask: source.u32(at + 4)?,
        input: source.u8(at + 96)?,
        flag: source.u8(at + 3)?,
    })
}
