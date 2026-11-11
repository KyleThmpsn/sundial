//! Native Shadowkeep character customization and the saved Sunrise appearance header.
mod catalog;
pub(crate) use catalog::{Catalog, Field, Group};
#[cfg(test)]
pub(crate) mod tests;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Appearance {
    pub race: u8,
    pub gender: u8,
    header: [u8; 36],
}

impl Appearance {
    pub fn from_saved(race: u8, gender: u8, header: [u8; 36]) -> Result<Self, String> {
        if race > 2 || gender > 1 {
            return Err("The character's race or gender is outside the native domain".into());
        }
        Ok(Self {
            race,
            gender,
            header,
        })
    }

    pub fn header(&self) -> &[u8] {
        &self.header
    }

    pub fn value(&self, field: Field) -> [u16; 3] {
        let (offset, count, byte) = field.layout();
        let mut values = [u16::MAX; 3];
        for (index, value) in values.iter_mut().take(count).enumerate() {
            *value = if byte {
                u16::from(self.header[offset])
            } else {
                u16::from_le_bytes(
                    self.header[offset + index * 2..offset + index * 2 + 2]
                        .try_into()
                        .unwrap(),
                )
            };
        }
        values
    }

    fn set(&mut self, field: Field, values: [u16; 3]) {
        let (offset, count, byte) = field.layout();
        for (index, value) in values.into_iter().take(count).enumerate() {
            if byte {
                self.header[offset] = value as u8;
            } else {
                self.header[offset + index * 2..offset + index * 2 + 2]
                    .copy_from_slice(&value.to_le_bytes());
            }
        }
    }
}
