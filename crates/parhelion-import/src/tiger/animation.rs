//! Native FK extensions and clip channels shared by source importers.
pub mod clip;
pub mod control;
pub mod pose;
pub mod rig;
pub mod state;

pub fn bank_field(payload: &crate::tiger::payload::Payload) -> anyhow::Result<usize> {
    let data = payload.pointer(24)?;
    anyhow::ensure!(data >= 4, "Animation consumer data header missing");
    match payload.u32(data - 4)? {
        0x80803640 => Ok(data + 0x108),
        0x808036CF => Ok(payload.pointer(16)? + 0xD0),
        class => anyhow::bail!("Unsupported animation bank consumer {class:08X}"),
    }
}
