//! Universal armor ornaments: plugs that any armor piece of one class and slot accepts. Each class
//! and slot has its own plug category, the FNV-1 hash of `armor_skins_{class}_{slot}`, in
//! Shadowkeep and modern Destiny 2 alike, so a plug's category names the class and slot it serves.
use crate::tiger::payload::Payload;
use anyhow::Result;
use std::sync::LazyLock;

/// The class and armor slot a universal ornament serves.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Slot {
    /// 0 Titan, 1 Hunter, 2 Warlock.
    pub class: u8,
    /// The inventory bucket of the armor pieces it applies to.
    pub bucket: u32,
}

const CLASSES: [(u8, &str); 3] = [(0, "titan"), (1, "hunter"), (2, "warlock")];
/// Helmet, gauntlets, chest armor, leg armor and class armor.
const SLOTS: [(u32, &str); 5] = [
    (3448274439, "head"),
    (3551918588, "arms"),
    (14239492, "chest"),
    (20886954, "legs"),
    (1585787867, "class"),
];
/// A modern definition's plug block pointer and the block's class word. The category is the
/// block's first word.
const MODERN_PLUG: usize = 0x40;
const MODERN_PLUG_CLASS: u32 = 0x808073A1;

static CATEGORIES: LazyLock<Vec<(u32, Slot)>> = LazyLock::new(|| {
    CLASSES
        .iter()
        .flat_map(|&(class, class_name)| {
            SLOTS.iter().map(move |&(bucket, slot_name)| {
                let name = format!("armor_skins_{class_name}_{slot_name}");
                let category = name.bytes().fold(0x811C9DC5u32, |hash, byte| {
                    hash.wrapping_mul(16777619) ^ u32::from(byte)
                });
                (category, Slot { class, bucket })
            })
        })
        .collect()
});

/// The class and slot of a universal ornament category, or `None` for any other category.
pub fn slot(category: u32) -> Option<Slot> {
    CATEGORIES
        .iter()
        .find(|(candidate, _)| *candidate == category)
        .map(|&(_, slot)| slot)
}

/// A modern definition's plug category, when the definition is a plug.
pub(crate) fn modern_category(item: &Payload) -> Result<Option<u32>> {
    if item.u64(MODERN_PLUG)? == 0 {
        return Ok(None);
    }
    let plug = item.pointer(MODERN_PLUG)?;
    if plug < 4 || item.u32(plug - 4)? != MODERN_PLUG_CLASS {
        return Ok(None);
    }
    Ok(Some(item.u32(plug)?))
}

/// The category and slot of a modern universal ornament definition.
pub(crate) fn modern_slot(item: &Payload) -> Result<Option<(u32, Slot)>> {
    Ok(modern_category(item)?.and_then(|category| Some((category, slot(category)?))))
}

/// Whether a definition, in the modern or Shadowkeep layout, is a universal ornament. Its art
/// follows the character like the armor it applies to.
pub(crate) fn is_universal(item: &Payload, modern: bool) -> Result<bool> {
    let category = if modern {
        modern_category(item)?
    } else {
        crate::tiger::shadowkeep::plug_category(item)?
    };
    Ok(category.and_then(slot).is_some())
}
