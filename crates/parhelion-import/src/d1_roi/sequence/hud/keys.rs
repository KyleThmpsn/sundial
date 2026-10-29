//! Named UI keys accumulate group lengths after the 128 built-in bit positions.
use super::*;
use crate::tiger::reader::Reader;

/// Checked native name dictionary. No numeric source ordinal is treated as a native key.
pub struct Keys {
    names: BTreeMap<u32, u16>,
}

impl Keys {
    pub fn read(native: &mut Reader) -> Result<Self> {
        ensure!(native.is_native(), "HUD key target must be Shadowkeep");
        let tags = native.classes(0x808047C1);
        ensure!(
            tags.len() == 1,
            "Native UI configuration is missing or ambiguous"
        );
        let config = native.tag(tags[0], Some(0x808047C1))?;
        Ok(Self {
            names: dictionary(&config, 0x80809F7F, 0x530)?,
        })
    }

    pub(super) fn key(&self, name: u32) -> Result<u16> {
        self.names
            .get(&name)
            .copied()
            .with_context(|| format!("Source HUD name {name:08X} has no native key"))
    }
}

pub(super) fn source(source: &Source) -> Result<BTreeMap<u16, u32>> {
    let (_, config) = source.table(0x8080287D)?;
    Ok(dictionary(&config, 0x80804065, 0x480)?
        .into_iter()
        .map(|(name, key)| (key, name))
        .collect())
}

fn dictionary(config: &Payload, group_class: u32, prefix: usize) -> Result<BTreeMap<u32, u16>> {
    ensure!(
        config.0.len() >= prefix && config.u64(0)? == config.0.len() as u64,
        "UI configuration extent differs"
    );
    let mut names = BTreeMap::new();
    let mut key = 128u16;
    for group in config.array(8, 16, Some(group_class))? {
        let mut previous = None;
        for row in config.array(group, 4, Some(0x80800070))? {
            let name = config.u32(row)?;
            ensure!(
                previous.is_none_or(|last| name > last) && key < 256,
                "UI names are unsorted or exceed the native bitset"
            );
            ensure!(
                names.insert(name, key).is_none(),
                "UI name is ambiguous across groups"
            );
            previous = Some(name);
            key += 1;
        }
    }
    ensure!(!names.is_empty(), "UI name dictionary is empty");
    Ok(names)
}
