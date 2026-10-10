//! Shadowkeep Barrel spread patterns, using the native 86657 declarations and owner readback.
use super::WeaponComponentBinding;
use crate::package_payload::{
    bytes_at, native_array_at, relative_offset, rows_fit, u32_at, u64_at, write_bytes,
};

pub(super) const INSTANCE: u32 = 0x8080_3889;
pub(super) const DEFINITION: u32 = 0x8080_3865;
const PATTERN: u32 = 0x8080_888D;
const RING: u32 = 0x8080_888F;
const SPREAD: usize = 0xE60;

pub(super) struct Spread {
    pub slot: usize,
    pub pattern: Option<Layout>,
}

pub(super) struct Layout {
    pub offset: usize,
    pub rings: std::ops::Range<usize>,
    pub count: u64,
    pub pellets: u16,
}

impl Spread {
    pub fn read(owner: &[u8], binding: WeaponComponentBinding) -> Result<Self, String> {
        if binding.concrete_class != INSTANCE {
            return Err(format!(
                "Unsupported Barrel class 0x{:08X}",
                binding.concrete_class
            ));
        }
        let instance = usize::try_from(binding.resource_offset)
            .map_err(|_| "Barrel resource offset is too large")?;
        rows_fit(owner, instance, 1, 0x1760)?;
        if u64_at(owner, 0)? != owner.len() as u64
            || u32_at(owner, instance)? != binding.owner_tag
            || u32_at(owner, instance + 4)? != DEFINITION
        {
            return Err("Barrel owner or definition reference is invalid".into());
        }
        let definition = usize::try_from(u64_at(owner, instance + 8)?)
            .map_err(|_| "Barrel definition offset is too large")?;
        rows_fit(owner, definition, 1, 0x1DC0)?;
        let slot = definition + SPREAD;
        let delta = i64::from_le_bytes(bytes_at(owner, slot)?);
        if delta == 0 {
            return Ok(Self {
                slot,
                pattern: None,
            });
        }
        let pattern = relative_offset(slot, 0, delta)?;
        bytes_at::<0x68>(owner, pattern)?;
        let marker = pattern
            .checked_sub(4)
            .ok_or("Barrel spread has no class marker")?;
        if u32_at(owner, marker)? != PATTERN {
            return Err("Barrel spread pointer does not reach a supported pattern".into());
        }
        for ordinal in 0..3 {
            let at = pattern + ordinal * 0x18;
            if u32_at(owner, at)? != PATTERN
                || u32_at(owner, at + 4)? != ordinal as u32
                || u32_at(owner, at + 8)? != binding.owner_tag
                || u32_at(owner, at + 12)? != PATTERN
                || u64_at(owner, at + 16)? != pattern as u64
            {
                return Err("Barrel spread interface does not reference its own pattern".into());
            }
        }
        let (count, header, rows, class) = native_array_at(owner, pattern + 0x48)?;
        let marker = header
            .checked_sub(4)
            .ok_or("Barrel spread rings have no array marker")?;
        if class != RING || u32_at(owner, marker)? != 0x8080_9FBD || count == 0 {
            return Err("Barrel spread rings have an unsupported native array".into());
        }
        rows_fit(owner, rows, count, 20)?;
        let pellets = u32_at(owner, pattern + 0x60)?;
        if !(1..=0x7FFF).contains(&pellets) {
            return Err(format!(
                "Barrel spread pellet count {pellets} is unsupported"
            ));
        }
        let total = (0..count).try_fold(0_u64, |total, index| {
            u32_at(owner, rows + index * 20 + 12).map(|value| total + u64::from(value))
        })?;
        if total != u64::from(pellets) {
            return Err("Barrel spread rings and total pellet count disagree".into());
        }
        Ok(Self {
            slot,
            pattern: Some(Layout {
                offset: pattern,
                rings: rows..rows + count * 20,
                count: count as u64,
                pellets: pellets as u16,
            }),
        })
    }
}

/// Pellets per shot in the final Barrel. A null spread fires one, malformed data is an error.
pub fn barrel_pellets(owner: &[u8], binding: WeaponComponentBinding) -> Result<u16, String> {
    Spread::read(owner, binding).map(|spread| spread.pattern.map_or(1, |pattern| pattern.pellets))
}

/// Native polar ring. Radii are fractions of the Barrel's spread, rotation is in radians,
/// and randomness is the fraction of each pellet's angular sector available for jitter.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ring {
    pub pellets: u16,
    pub inner_radius_bits: u32,
    pub outer_radius_bits: u32,
    pub rotation_bits: u32,
    pub randomness_bits: u32,
}

/// Geometry understood by Shadowkeep's 0x8080888D spread implementation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pattern {
    pub rings: Vec<Ring>,
    /// The native conversion factor applied to the Barrel's spread input.
    pub scale_bits: u32,
}

impl Pattern {
    /// A centered single pellet or a filled circular spread for multiple pellets.
    pub fn circular(pellets: u16) -> Self {
        Self {
            rings: vec![Ring {
                pellets,
                inner_radius_bits: 0.0_f32.to_bits(),
                outer_radius_bits: if pellets == 1 { 0.0_f32 } else { 1.0_f32 }.to_bits(),
                rotation_bits: 0.0_f32.to_bits(),
                randomness_bits: 1.0_f32.to_bits(),
            }],
            // Native degrees-to-radians factor in the supported stock ring patterns.
            scale_bits: 0x3C8E_FA35,
        }
    }

    /// Writer bounds are authoring limits, not evidence of the engine's maximum.
    pub fn validate(&self) -> Result<u16, String> {
        if self.rings.is_empty() || self.rings.len() > 0x7FFF {
            return Err("A Barrel pattern needs between 1 and 32767 rings".into());
        }
        let scale = f32::from_bits(self.scale_bits);
        if !scale.is_finite() || scale < 0.0 {
            return Err("Barrel spread must be finite and nonnegative".into());
        }
        let mut total = 0_u32;
        for (index, ring) in self.rings.iter().enumerate() {
            let inner = f32::from_bits(ring.inner_radius_bits);
            let outer = f32::from_bits(ring.outer_radius_bits);
            let rotation = f32::from_bits(ring.rotation_bits);
            let randomness = f32::from_bits(ring.randomness_bits);
            if !inner.is_finite()
                || !outer.is_finite()
                || inner < 0.0
                || outer < inner
                || !(outer * scale).is_finite()
                || !rotation.is_finite()
                || !randomness.is_finite()
                || !(0.0..=1.0).contains(&randomness)
            {
                return Err(format!(
                    "Ring {} needs finite radii with 0 <= inner <= outer, a finite rotation, and randomness between 0 and 1",
                    index + 1
                ));
            }
            total += u32::from(ring.pellets);
        }
        if !(1..=0x7FFF).contains(&total) {
            return Err("Pellets per shot must be between 1 and 32767".into());
        }
        Ok(total as u16)
    }
}

/// Read a supported Barrel's geometry. `None` is the native single-projectile default.
pub fn read_pattern(
    owner: &[u8],
    binding: WeaponComponentBinding,
) -> Result<Option<Pattern>, String> {
    let Some(layout) = Spread::read(owner, binding)?.pattern else {
        return Ok(None);
    };
    let rings = (layout.rings.start..layout.rings.end)
        .step_by(20)
        .map(|at| {
            Ok(Ring {
                inner_radius_bits: u32_at(owner, at)?,
                outer_radius_bits: u32_at(owner, at + 4)?,
                rotation_bits: u32_at(owner, at + 8)?,
                pellets: u16::try_from(u32_at(owner, at + 12)?)
                    .map_err(|_| "Ring count is too large")?,
                randomness_bits: u32_at(owner, at + 16)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let pattern = Pattern {
        rings,
        scale_bits: u32_at(owner, layout.offset + 0x58)?,
    };
    pattern.validate()?;
    Ok(Some(pattern))
}

/// Replace geometry atomically. Existing interface locations and unknown pattern fields stay
/// intact. A fresh array avoids moving shared native records or overwriting other objects.
pub fn write_pattern(
    owner: &mut Vec<u8>,
    binding: WeaponComponentBinding,
    pattern: &Pattern,
) -> Result<(), String> {
    let pellets = pattern.validate()?;
    let layout = Spread::read(owner, binding)?;
    let mut edited = owner.clone();
    let at = if let Some(layout) = layout.pattern {
        layout.offset
    } else {
        let start = aligned(edited.len())?;
        let at = start
            .checked_add(8)
            .ok_or("Barrel pattern offset overflowed")?;
        edited.resize(
            at.checked_add(0x68)
                .ok_or("Barrel pattern size overflowed")?,
            0,
        );
        write_bytes(&mut edited, at - 4, &PATTERN.to_le_bytes())?;
        for ordinal in 0_u32..3 {
            let interface = at + ordinal as usize * 0x18;
            for (offset, value) in [
                (0, PATTERN),
                (4, ordinal),
                (8, binding.owner_tag),
                (12, PATTERN),
            ] {
                write_bytes(&mut edited, interface + offset, &value.to_le_bytes())?;
            }
            write_bytes(&mut edited, interface + 16, &(at as u64).to_le_bytes())?;
        }
        write_relative(&mut edited, layout.slot, at)?;
        at
    };
    let header = aligned(edited.len())?
        .checked_add(16)
        .ok_or("Barrel rings offset overflowed")?;
    let end = pattern
        .rings
        .len()
        .checked_mul(20)
        .and_then(|size| header.checked_add(16 + size))
        .ok_or("Barrel rings size overflowed")?;
    edited.resize(aligned(end)?, 0);
    write_bytes(&mut edited, header - 4, &0x8080_9FBD_u32.to_le_bytes())?;
    write_bytes(
        &mut edited,
        header,
        &(pattern.rings.len() as u64).to_le_bytes(),
    )?;
    write_bytes(&mut edited, header + 8, &RING.to_le_bytes())?;
    write_bytes(
        &mut edited,
        at + 0x48,
        &(pattern.rings.len() as u64).to_le_bytes(),
    )?;
    write_relative(&mut edited, at + 0x50, header)?;
    let mut outer = 0.0_f32;
    for (index, ring) in pattern.rings.iter().enumerate() {
        let row = header + 16 + index * 20;
        for (offset, value) in [
            (0, ring.inner_radius_bits),
            (4, ring.outer_radius_bits),
            (8, ring.rotation_bits),
            (12, u32::from(ring.pellets)),
            (16, ring.randomness_bits),
        ] {
            write_bytes(&mut edited, row + offset, &value.to_le_bytes())?;
        }
        outer = outer.max(f32::from_bits(ring.outer_radius_bits));
    }
    write_bytes(&mut edited, at + 0x58, &pattern.scale_bits.to_le_bytes())?;
    write_bytes(
        &mut edited,
        at + 0x5C,
        &(outer * f32::from_bits(pattern.scale_bits)).to_le_bytes(),
    )?;
    write_bytes(&mut edited, at + 0x60, &u32::from(pellets).to_le_bytes())?;
    let length = edited.len() as u64;
    write_bytes(&mut edited, 0, &length.to_le_bytes())?;
    *owner = edited;
    Ok(())
}

fn aligned(length: usize) -> Result<usize, String> {
    length
        .checked_add(15)
        .map(|value| value & !15)
        .ok_or_else(|| "Barrel owner size overflowed".into())
}

fn write_relative(bytes: &mut [u8], slot: usize, target: usize) -> Result<(), String> {
    let delta = i64::try_from(target)
        .ok()
        .zip(i64::try_from(slot).ok())
        .and_then(|(target, slot)| target.checked_sub(slot))
        .ok_or("Barrel pointer overflowed")?;
    write_bytes(bytes, slot, &delta.to_le_bytes())
}
