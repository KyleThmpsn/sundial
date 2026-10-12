//! Refresh clip storage sizes through the native supplemental layer references.
use crate::{presentation::put, tiger::payload::Payload};
use anyhow::{Context, Result};

pub fn clip_sizes(payload: &mut Payload, sizes: &[u32]) -> Result<()> {
    for (field, layer_class, choice_class, option_class) in [
        (8, 0x80803724, 0x8080372E, 0x80803730),
        (24, 0x80803721, 0x80803729, 0x8080372B),
    ] {
        for layer in payload.array(field, 24, Some(layer_class))? {
            for choice in payload.array(layer, 16, Some(choice_class))? {
                for option in payload.array(choice, 64, Some(option_class))? {
                    for weight in payload.array(option, 12, Some(0x80803737))? {
                        let index = usize::try_from(payload.u32(weight)?)?;
                        let size = sizes.get(index).context("Layer clip is outside its bank")?;
                        put(&mut payload.0, weight + 8, &size.to_le_bytes())?;
                    }
                }
            }
        }
    }
    Ok(())
}
