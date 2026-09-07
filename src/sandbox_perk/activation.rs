//! Experimental activation adapters, deliberately restricted to mapped stock actions.
//!
//! Build 86657: Outlaw's two 0x80803DE7 nodes each have a precision-label filter
//! at +0xD0 and a weapon-source flag at +0x141. Live native definitions confirm
//! the 0x158-byte node and 0x18-byte 0x808094B3 label rows. Precision/grenade
//! hashes match their FNV-1 names; the melee label set is copied from Grave
//! Robber and Swashbuckler. These are package-verified candidates, not a promise
//! of in-game activation on every weapon family. No arbitrary action is accepted.
use crate::package_payload::{
    bytes_at, i64_at, native_array_at, relative_offset, u32_at, u64_at, write_bytes,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerkActivation {
    WeaponKill,
    PrecisionWeaponKill,
    MeleeKill,
    GrenadeKill,
    AnyKill,
}

impl PerkActivation {
    pub const ALL: [Self; 5] = [
        Self::WeaponKill,
        Self::PrecisionWeaponKill,
        Self::MeleeKill,
        Self::GrenadeKill,
        Self::AnyKill,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::WeaponKill => "Weapon Kill",
            Self::PrecisionWeaponKill => "Precision Weapon Kill",
            Self::MeleeKill => "Melee Kill",
            Self::GrenadeKill => "Grenade Kill",
            Self::AnyKill => "Any Credited Kill",
        }
    }

    const fn labels(self) -> &'static [u32] {
        match self {
            Self::PrecisionWeaponKill => &[0x962E_A19B],
            Self::MeleeKill => &[0xBF39_E12B, 0xE175_76C9, 0x5D3A_7C84],
            Self::GrenadeKill => &[0xC20D_D425],
            Self::WeaponKill | Self::AnyKill => &[],
        }
    }

    const fn requires_weapon(self) -> bool {
        matches!(self, Self::WeaponKill | Self::PrecisionWeaponKill)
    }
}

/// UI hint only; compilation additionally checks the actual action and native layout.
#[must_use]
pub const fn supports_activation(perk_index: u16) -> bool {
    matches!(perk_index, 421 | 422)
}

const OUTLAW_ACTION: u32 = 0x80BB_C7B4;
const NODE_CLASS: u32 = 0x8080_3DE7;
const NODE_STARTS: [usize; 2] = [0x100, 0x410];
const LABEL_CLASS: u32 = 0x8080_94B3;
const LABEL_GLOBALS: u32 = 0x80C7_0CA1;
const LABEL_PATH: &[u8] = b"content/common/native/sandbox/label_globals.label_globals.tft\0";

/// Returns an independently owned action, changing only the kill filter and source
/// gate. Effect graphs, event channels, duration, sibling perks and stock data stay intact.
pub fn with_activation(
    action_tag: u32,
    source: &[u8],
    activation: PerkActivation,
) -> Result<Vec<u8>, String> {
    let paths = validate_outlaw(action_tag, source)?;
    if activation == PerkActivation::PrecisionWeaponKill {
        return Ok(source.to_vec());
    }
    let mut result = source.to_vec();
    for (start, path) in NODE_STARTS.into_iter().zip(paths) {
        let descriptor = start + 0xD0;
        let labels = activation.labels();
        if labels.len() <= 1 {
            let (_, _, row, _) = native_array_at(source, descriptor)?;
            if let Some(label) = labels.first() {
                write_bytes(&mut result, row, &label.to_le_bytes())?;
            } else {
                write_bytes(&mut result, descriptor, &[0; 16])?;
            }
        } else {
            // Out-of-line rows keep every existing offset stable. Rebase the debug
            // path pointer in each new row; copying its old relative value is invalid.
            result.resize(result.len().next_multiple_of(16), 0);
            let header = result.len();
            result.extend_from_slice(&(labels.len() as u64).to_le_bytes());
            result.extend_from_slice(&LABEL_CLASS.to_le_bytes());
            result.extend_from_slice(&[0; 4]);
            for label in labels {
                let row = result.len();
                result.extend_from_slice(&label.to_le_bytes());
                result.extend_from_slice(&[0; 4]);
                result.extend_from_slice(&((path as i64) - (row + 8) as i64).to_le_bytes());
                result.extend_from_slice(&u64::from(LABEL_GLOBALS).to_le_bytes());
            }
            write_bytes(
                &mut result,
                descriptor,
                &(labels.len() as u64).to_le_bytes(),
            )?;
            write_bytes(
                &mut result,
                descriptor + 8,
                &((header as i64) - (descriptor + 8) as i64).to_le_bytes(),
            )?;
        }
        write_bytes(
            &mut result,
            start + 0x141,
            &[u8::from(activation.requires_weapon())],
        )?;
    }
    let size = result.len() as u64;
    write_bytes(&mut result, 0, &size.to_le_bytes())?;
    Ok(result)
}

fn validate_outlaw(action_tag: u32, source: &[u8]) -> Result<[usize; 2], String> {
    let invalid = || {
        "Activation conditions currently require the mapped stock Outlaw action (build 86657); this source is unsupported or has changed.".to_owned()
    };
    if action_tag != OUTLAW_ACTION
        || source.len() != 1782
        || u64_at(source, 0)? != source.len() as u64
    {
        return Err(invalid());
    }
    let nodes = (0..source.len().saturating_sub(3))
        .step_by(4)
        .filter(|offset| u32_at(source, *offset).ok() == Some(NODE_CLASS))
        .map(|offset| offset + 4)
        .collect::<Vec<_>>();
    if nodes != NODE_STARTS {
        return Err(invalid());
    }
    let mut paths = [0; 2];
    for (index, start) in NODE_STARTS.into_iter().enumerate() {
        // No hidden companion requirements may be discarded or contradicted.
        for offset in [
            0x08, 0x18, 0x28, 0x38, 0x68, 0x78, 0x88, 0x98, 0xB0, 0xC0, 0xE0, 0xF0, 0x100,
        ] {
            if bytes_at::<16>(source, start + offset)? != [0; 16] {
                return Err(invalid());
            }
        }
        if bytes_at::<2>(source, start + 0x140)? != [0, 1] {
            return Err(invalid());
        }
        // These are optional predicate references, not array descriptors.
        if u64_at(source, start + 0x58)? != 0xFF
            || u64_at(source, start + 0x60)? != 0
            || u64_at(source, start + 0x128)? != 0
        {
            return Err(invalid());
        }
        let predicate = relative_offset(start, 0x130, i64_at(source, start + 0x130)?)?;
        let mut expected = [0; 84];
        expected[1] = 2;
        expected[81] = 1;
        if predicate < 4
            || u32_at(source, predicate - 4)? != 0x8080_93F6
            || bytes_at::<84>(source, predicate)? != expected
        {
            return Err(invalid());
        }
        let (count, _, row, class) = native_array_at(source, start + 0xD0)?;
        if count != 1
            || class != LABEL_CLASS
            || u64_at(source, row)? != 0x962E_A19B
            || u64_at(source, row + 16)? != u64::from(LABEL_GLOBALS)
        {
            return Err(invalid());
        }
        let path = relative_offset(row, 8, i64_at(source, row + 8)?)?;
        if source.get(path..path.saturating_add(LABEL_PATH.len())) != Some(LABEL_PATH) {
            return Err(invalid());
        }
        paths[index] = path;
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        let mut b = vec![0; 1782];
        write_bytes(&mut b, 0, &1782u64.to_le_bytes()).unwrap();
        let path = 1700;
        write_bytes(&mut b, path, LABEL_PATH).unwrap();
        for (start, header) in NODE_STARTS.into_iter().zip([0x260, 0x570]) {
            write_bytes(&mut b, start - 4, &NODE_CLASS.to_le_bytes()).unwrap();
            write_bytes(&mut b, start + 0x141, &[1]).unwrap();
            write_bytes(&mut b, start + 0x58, &0xFFu64.to_le_bytes()).unwrap();
            let predicate = header + 44;
            write_bytes(
                &mut b,
                start + 0x130,
                &((predicate as i64) - (start + 0x130) as i64).to_le_bytes(),
            )
            .unwrap();
            write_bytes(&mut b, predicate - 4, &0x8080_93F6u32.to_le_bytes()).unwrap();
            b[predicate + 1] = 2;
            b[predicate + 81] = 1;
            write_bytes(&mut b, start + 0xD0, &1u64.to_le_bytes()).unwrap();
            write_bytes(
                &mut b,
                start + 0xD8,
                &((header as i64) - (start + 0xD8) as i64).to_le_bytes(),
            )
            .unwrap();
            write_bytes(&mut b, header, &1u64.to_le_bytes()).unwrap();
            write_bytes(&mut b, header + 8, &LABEL_CLASS.to_le_bytes()).unwrap();
            write_bytes(&mut b, header + 16, &0x962E_A19Bu64.to_le_bytes()).unwrap();
            write_bytes(
                &mut b,
                header + 24,
                &((path as i64) - (header + 24) as i64).to_le_bytes(),
            )
            .unwrap();
            write_bytes(&mut b, header + 32, &u64::from(LABEL_GLOBALS).to_le_bytes()).unwrap();
        }
        b
    }

    fn assert_activation_filter(result: &[u8], start: usize, condition: PerkActivation) {
        assert_eq!(result[start + 0x141], u8::from(condition.requires_weapon()));
        if condition.labels().is_empty() {
            assert_eq!(&result[start + 0xD0..start + 0xE0], &[0; 16]);
            return;
        }
        let (count, _, rows, class) = native_array_at(result, start + 0xD0).unwrap();
        assert_eq!(count, condition.labels().len());
        assert_eq!(class, LABEL_CLASS);
        for (index, label) in condition.labels().iter().enumerate() {
            let row = rows + index * 24;
            assert_eq!(u32_at(result, row).unwrap(), *label);
            let path = relative_offset(row, 8, i64_at(result, row + 8).unwrap()).unwrap();
            assert_eq!(&result[path..path + LABEL_PATH.len()], LABEL_PATH);
            assert_eq!(u64_at(result, row + 16).unwrap(), u64::from(LABEL_GLOBALS));
        }
    }

    #[test]
    fn activation_changes_both_filters_and_preserves_effect_bytes() {
        let source = fixture();
        for condition in PerkActivation::ALL {
            let result = with_activation(OUTLAW_ACTION, &source, condition).unwrap();
            for start in NODE_STARTS {
                assert_activation_filter(&result, start, condition);
            }
            for (i, byte) in source.iter().enumerate().skip(8) {
                let edited = NODE_STARTS
                    .iter()
                    .any(|start| (start + 0xD0..start + 0xE0).contains(&i) || i == start + 0x141)
                    || [0x270..0x274, 0x580..0x584]
                        .iter()
                        .any(|range| range.contains(&i));
                if !edited {
                    assert_eq!(result[i], *byte, "effect byte {i:X}");
                }
            }
            assert_eq!(u64_at(&result, 0).unwrap(), result.len() as u64);
        }
        assert_eq!(
            with_activation(OUTLAW_ACTION, &source, PerkActivation::PrecisionWeaponKill).unwrap(),
            source
        );
    }

    #[test]
    fn activation_rejects_unmapped_or_changed_sources_without_mutation() {
        let source = fixture();
        assert!(with_activation(0, &source, PerkActivation::AnyKill).is_err());
        for offset in [0, 0xFC, 0x108, 0x1D0, 0x270, 0x280, 0x241, 0x580] {
            let mut changed = source.clone();
            changed[offset] ^= 1;
            let before = changed.clone();
            assert!(
                with_activation(OUTLAW_ACTION, &changed, PerkActivation::AnyKill).is_err(),
                "{offset:X}"
            );
            assert_eq!(before, changed);
        }
    }
}
