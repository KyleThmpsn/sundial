//! Item families are investment bucket contracts, independent of localized display names.
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    Weapon,
    Armor,
    GhostShell,
    Ship,
    Sparrow,
    Shader,
    Emblem,
}

impl Family {
    /// Current definitions keep the inventory bucket at 0x98. Equipment restrictions
    /// supply the class and slot. Display classification is not stable across gear sets.
    pub(crate) fn source(
        item: &crate::d2_mot::payload::Payload,
        strings: &crate::d2_mot::payload::Payload,
    ) -> Result<Option<(Self, u32, Option<u8>)>> {
        let inventory = item.u8(0x98)?;
        let bucket = match inventory {
            0 => 1498876634,
            1 => 2465295065,
            2 => 953998645,
            3 => 3448274439,
            4 => 3551918588,
            5 => 14239492,
            6 => 20886954,
            7 => 1585787867,
            8 => 4023194814,
            9 => 2025709351,
            10 => 284967655,
            25 => 4274335291,
            32 if shader(item)? => return Ok(Some((Self::Shader, 2973005342, None))),
            _ => return Ok(None),
        };
        let family = Self::from_bucket(bucket).context("source inventory family")?;
        let class = if item.u64(0x18)? != 0 {
            let equipment = item.pointer(0x18)?;
            ensure!(
                equipment >= 4 && item.u32(equipment - 4)? == 0x808077E7,
                "Unsupported source equipment block"
            );
            let slot = item.u8(equipment + 0x0D)?;
            let compatible = match inventory {
                0..=2 => (6..=8).contains(&slot),
                3..=7 => slot == inventory - 2,
                8 => slot == 11,
                9 => slot == 10,
                10 => slot == 9,
                25 => slot == 12,
                _ => false,
            };
            ensure!(compatible, "Source inventory and equipment slots disagree");
            let class = item.u8(equipment + 0x0C)?;
            (family == Self::Armor && class <= 2).then_some(class)
        } else {
            // Display-only definitions remain browsable as dummy items.
            match strings.u32(0xC0)? {
                0xD60E6BA5 if family == Self::Armor => Some(0),
                0xD74EBFB3 if family == Self::Armor => Some(1),
                0x82E15A90 if family == Self::Armor => Some(2),
                _ => None,
            }
        };
        Ok(Some((family, bucket, class)))
    }

    pub fn from_bucket(bucket: u32) -> Option<Self> {
        Some(match bucket {
            1498876634 | 2465295065 | 953998645 => Self::Weapon,
            3448274439 | 3551918588 | 14239492 | 20886954 | 1585787867 => Self::Armor,
            4023194814 => Self::GhostShell,
            284967655 => Self::Ship,
            2025709351 => Self::Sparrow,
            2973005342 => Self::Shader,
            4274335291 => Self::Emblem,
            _ => return None,
        })
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Weapon => "weapon",
            Self::Armor => "armor",
            Self::GhostShell => "ghost_shell",
            Self::Ship => "ship",
            Self::Sparrow => "sparrow",
            Self::Shader => "shader",
            Self::Emblem => "emblem",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Weapon => "Weapon",
            Self::Armor => "Armor",
            Self::GhostShell => "Ghost Shell",
            Self::Ship => "Ship",
            Self::Sparrow => "Sparrow",
            Self::Shader => "Shader",
            Self::Emblem => "Emblem",
        }
    }

    pub const fn is_model_gear(self) -> bool {
        matches!(
            self,
            Self::Armor | Self::GhostShell | Self::Ship | Self::Sparrow
        )
    }
}

fn shader(item: &crate::d2_mot::payload::Payload) -> Result<bool> {
    if item.u64(0x40)? == 0 || item.u64(0x70)? == 0 {
        return Ok(false);
    }
    let plug = item.pointer(0x40)?;
    ensure!(
        plug >= 4 && item.u32(plug - 4)? == 0x808073A1,
        "Unsupported source plug block"
    );
    // Standard and holographic shader categories. These are plug contracts, not item hashes
    // or localized names. Engine trails and ornaments can also contain dye rows.
    if !matches!(item.u32(plug)?, 0xB134761E | 0x948B3BCF) {
        return Ok(false);
    }
    let translation = item.pointer(0x70)?;
    ensure!(
        translation >= 4 && item.u32(translation - 4)? == 0x80807377,
        "Unsupported source shader translation block"
    );
    Ok(item.array(translation, 4, Some(0x8080737D))?.is_empty()
        && [0x28, 0x38, 0x48]
            .into_iter()
            .any(|offset| item.u64(translation + offset).is_ok_and(|count| count > 0)))
}

impl Weapon {
    pub fn family(&self) -> Family {
        self.bucket_hash
            .and_then(Family::from_bucket)
            .unwrap_or_else(|| {
                // Old service callers and saved selections predate structural family metadata.
                if self.weapon_type == "Shader" {
                    Family::Shader
                } else {
                    Family::Weapon
                }
            })
    }

    pub fn accepts_gear_donor(&self, bucket: u64, class: Option<u8>) -> bool {
        self.family().is_model_gear()
            && self.bucket_hash.map(u64::from) == Some(bucket)
            && (self.family() != Family::Armor
                || self
                    .class_type
                    .is_some_and(|source| source <= 2 && class == Some(source)))
    }
}
