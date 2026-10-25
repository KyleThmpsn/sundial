//! Root numeric input identities remain stable when controller slot ordinals change.
use super::*;

pub(super) struct Inputs {
    source: Vec<u32>,
    native: Vec<u32>,
}

impl Inputs {
    pub(super) fn read(
        sr: &mut Reader,
        nr: &mut Reader,
        source: &Value,
        native: &Value,
    ) -> Result<Self> {
        fn names(reader: &mut Reader, rig: &Value, modern: bool) -> Result<Vec<u32>> {
            let (tag, _, _) = controller(reader, rig, modern)?;
            let owner = reader.tag(tag, None)?;
            owner
                .array(
                    owner.pointer(24)? + if modern { 0xC8 } else { 0xA8 },
                    40,
                    Some(if modern { 0x80809591 } else { 0x80809789 }),
                )?
                .into_iter()
                .map(|row| {
                    ensure!(
                        owner.u32(row + 36)? == 0,
                        "Pose numeric input has unsupported identity flags"
                    );
                    owner.u32(row + 32)
                })
                .collect()
        }
        Ok(Self {
            source: names(sr, source, true)?,
            native: names(nr, native, false)?,
        })
    }

    pub(super) fn translate(&self, source: &Payload, at: usize) -> Result<([u8; 16], Value)> {
        let mut driver = source.bytes::<16>(at)?;
        match source.u32(at)? {
            1 => {
                ensure!(
                    source.f32(at + 4)? == 1.0
                        && source.u32(at + 8)? == 0xFFFF
                        && source.u32(at + 12)? == 0,
                    "Profile pose has an unsupported constant weight"
                );
                Ok((driver, json!({"constant":1.0})))
            }
            2 => {
                ensure!(
                    source.u32(at + 4)? == 0
                        && source.u16(at + 10)? == 0
                        && source.u32(at + 12)? == 0,
                    "Profile pose weight requires a non-root numeric driver"
                );
                let index = usize::from(source.u16(at + 8)?);
                let name = *self.source.get(index).context("Source pose weight input")?;
                ensure!(
                    ![0, u32::MAX, 0x811C9DC5].contains(&name)
                        && self.source.iter().filter(|&&n| n == name).count() == 1,
                    "Source pose weight input identity is missing or ambiguous"
                );
                let matches = self
                    .native
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| **n == name)
                    .map(|(i, _)| i)
                    .collect::<Vec<_>>();
                ensure!(
                    matches.len() == 1,
                    "Native pose weight input identity is missing or ambiguous"
                );
                driver[8..10].copy_from_slice(&u16::try_from(matches[0])?.to_le_bytes());
                Ok((
                    driver,
                    json!({"input_name":name,"source_index":index,"native_index":matches[0]}),
                ))
            }
            kind => anyhow::bail!("Unsupported profile pose weight kind {kind}"),
        }
    }
}
