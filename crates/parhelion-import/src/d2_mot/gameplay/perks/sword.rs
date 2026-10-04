//! Checked source values for sword-specific native profile translation.

use anyhow::{Context, Result, ensure};

use crate::d2_mot::payload::Payload;

/// Source component 12 inputs 4 and 5 weight the sword's near and far
/// angular search windows. Keep the stored float bits for native vector lanes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AngularScales {
    pub near_bits: u32,
    pub far_bits: u32,
}

fn angular_row(owner: &Payload, at: usize) -> Result<(u16, u32)> {
    owner.bytes::<112>(at)?;
    ensure!(
        owner.u32(at + 4)? == 0x8080_2D32,
        "source modifier class differs"
    );
    let instance = usize::try_from(owner.u64(at + 8)?)?;
    ensure!(
        owner.u32(instance)? == owner.u32(at)?
            && owner.u32(instance + 4)? == 0x8080_2D33
            && owner.u64(instance + 8)? == at as u64,
        "source modifier pair differs"
    );
    ensure!(
        owner.u8(at + 20)? == 1
            && owner.bytes::<3>(at + 21)? == [0; 3]
            && i64::from_le_bytes(owner.bytes(at + 24)?) == -24
            && owner.u32(at + 36)? == 0
            && owner.u64(at + 40)? == 0
            && owner.u64(at + 48)? == 0
            && i64::from_le_bytes(owner.bytes(at + 56)?) == -56
            && owner.u32(at + 68)? == 0
            && owner.u64(at + 72)? == 1
            && owner.u64(at + 80)? == 0
            && owner.i16(at + 88)? == -1
            && owner.u32(at + 92)? == 0x811C_9DC5
            && owner.bytes::<3>(at + 97)? == [0; 3]
            && owner.u32(at + 100)? == 0
            && owner.u32(at + 104)? == u32::MAX
            && owner.u32(at + 108)? == 0,
        "source angular modifier metadata or operation differs"
    );
    for offset in [32, 64] {
        ensure!(
            (0x8080_0000..0x8200_0000).contains(&owner.u32(at + offset)?),
            "source angular modifier metadata reference differs"
        );
    }
    ensure!(owner.u8(at + 96)? == 12, "source angular component differs");
    let input = owner.u16(at + 90)?;
    ensure!(matches!(input, 4 | 5), "source angular input differs");
    let bits = owner.u32(at + 16)?;
    let scale = f32::from_bits(bits);
    ensure!(
        scale.is_finite() && (0.0..=16.0).contains(&scale),
        "source angular scale is invalid"
    );
    Ok((input, bits))
}

/// Read the two modern sword angular modifiers from their checked settings owner.
/// Their source component has no matching native modifier inputs. The importer
/// can instead place these bits in a private, keyed native sword profile.
pub fn angular_scales(owner: &Payload) -> Result<AngularScales> {
    ensure!(
        owner.u64(0)? == owner.0.len() as u64 && owner.u32(0xBC)? == 0x8080_2D2A,
        "unsupported source component owner root"
    );
    let settings = owner.pointer(24)?;
    ensure!(
        settings >= 4 && owner.u32(settings - 4)? == 0x8080_2D2B,
        "unsupported source component settings"
    );
    let descriptor = settings + 88;
    ensure!(
        owner.u64(descriptor)? == 2,
        "source angular settings need two rows"
    );
    let header = owner.pointer(descriptor + 8)?;
    let marker = header
        .checked_sub(4)
        .context("source angular array has no marker")?;
    ensure!(
        owner.u32(marker)? == 0x8080_9FB8,
        "source angular array marker differs"
    );
    let rows = owner.array(descriptor, 112, Some(0x8080_2D33))?;
    ensure!(rows.len() == 2, "source angular row count differs");

    let mut near_bits = None;
    let mut far_bits = None;
    for row in rows {
        let (input, bits) = angular_row(owner, row)?;
        let slot = if input == 4 {
            &mut near_bits
        } else {
            &mut far_bits
        };
        ensure!(slot.replace(bits).is_none(), "source angular input repeats");
    }
    Ok(AngularScales {
        near_bits: near_bits.context("source near angular scale is absent")?,
        far_bits: far_bits.context("source far angular scale is absent")?,
    })
}
