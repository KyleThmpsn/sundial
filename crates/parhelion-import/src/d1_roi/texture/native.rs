//! Native resident headers shared by equipment and particle material resources.
use super::{Texture, surface};
use crate::{presentation::put, tiger::payload::Payload};
use anyhow::{Result, ensure};

impl Texture {
    pub(crate) fn subtype(&self) -> u8 {
        if self.depth > 1 {
            3
        } else if self.layers == 6 {
            2
        } else {
            1
        }
    }

    /// Preserve the checked native volume topology flags, then describe the complete
    /// translated mip payload. Ordinary 2D and cube descriptors are authored directly.
    pub(crate) fn native_header(&self, volume: Option<&Payload>) -> Result<Vec<u8>> {
        let mut header = if self.depth > 1 {
            let native = volume
                .ok_or_else(|| anyhow::anyhow!("Native volume texture template is absent"))?;
            ensure!(
                native.0.len() == 40
                    && native.u16(12)? == 0xCAFE
                    && native.u16(18)? > 1
                    && native.u16(20)? == 1,
                "Native volume texture template differs"
            );
            native.0.clone()
        } else {
            vec![0; 40]
        };
        put(
            &mut header,
            0,
            &u32::try_from(self.bytes.len())?.to_le_bytes(),
        )?;
        put(&mut header, 4, &self.format.to_le_bytes())?;
        put(&mut header, 12, &0xCAFEu16.to_le_bytes())?;
        for (at, value) in [
            (14, self.width),
            (16, self.height),
            (18, self.depth),
            (20, self.layers),
        ] {
            put(&mut header, at, &value.to_le_bytes())?;
        }
        header[22] = u8::try_from(surface(self.format, 4, 4)?.1 / 2)?;
        header[23] = self.mips;
        if self.depth == 1 {
            header[25] = 1;
            header[31] = 1;
            header[32] = if self.layers == 6 { 2 } else { 0 };
            header[33] = 3;
            header[34] = 1;
        }
        put(&mut header, 36, &u32::MAX.to_le_bytes())?;
        crate::tiger::texture::resident(&mut header, self.bytes.len())?;
        Ok(header)
    }
}
