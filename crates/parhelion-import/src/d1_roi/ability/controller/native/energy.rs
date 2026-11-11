//! Source energy settings with matched common-controller consumers in both builds.
use super::Prepared;
use crate::{presentation::put, tiger::payload::Payload};
use anyhow::{Context, Result, ensure};
use serde_json::json;
use sha2::{Digest, Sha256};

impl Prepared {
    /// Translate only the qualified energy fields. Recharge programs, presentation and the
    /// remaining common settings still belong to the containing graph's native adaptation.
    pub fn source_energy(&mut self, tag: u32, source: &Payload) -> Result<()> {
        let runtime = source.pointer(16)?;
        let definition = source.pointer(24)?;
        ensure!(
            runtime >= 4
                && definition >= 4
                && source.u32(runtime - 4)? == 0x808025B8
                && source.u32(definition - 4)? == 0x80802A4A
                && source.u32(runtime)? == tag
                && source.u32(definition)? == tag
                && source.u32(runtime + 16)? == 0x80802A4A
                && source.u32(definition + 16)? == 0x808025B8
                && source.u64(runtime + 8)? == definition as u64
                && source.u64(definition + 8)? == runtime as u64,
            "Source energy settings have another controller ownership contract"
        );
        let charges = source.u32(definition + 0xE8)?;
        let cost = source.f32(definition + 0xEC)?;
        let drain = source.f32(definition + 0xF4)?;
        let flags = source.u16(definition + 0x64)?;
        ensure!(
            charges > 0
                && charges <= i32::MAX as u32
                && cost.is_finite()
                && cost >= 0.0
                && drain.is_finite()
                && drain > 0.0
                && flags & !0x112 == 0,
            "Source controller has unqualified energy values or flags"
        );
        // Source 487ABB and native BB16B5 suppress recharge while active with bits 1/0.
        // Source 481EF7 and native BA0CC5 initialize charged state with bits 4/3.
        // Source 48A208 and native B9610C gate the charge-refill branch with bits 8/7.
        // Other source flags remain rejected, including the removed source low bit.
        let native_flags = flags >> 1;
        let live = self.payload.pointer(16)?;
        let target = usize::try_from(self.payload.u64(live + 8)?)
            .context("Native energy definition offset")?;
        ensure!(
            self.payload.u32(live - 4)? == 0x80804176
                && self.payload.u32(target - 4)? == 0x80804177
                && self.payload.u64(target + 8)? == live as u64,
            "Native energy settings have another controller ownership contract"
        );
        // Sealing keeps the reciprocal definition and an immutable schema view after the
        // mutable span. Both views must carry the same translated scalar configuration.
        let schema = self.payload.pointer(24)?;
        ensure!(
            self.payload.u32(schema - 4)? == 0x80804177
                && self.payload.bytes::<16>(schema)? == self.payload.bytes::<16>(target)?,
            "Native energy schema view differs from its reciprocal definition"
        );
        // Reset 4865A0/BA1C70 supplies charge count, activation cost and active drain.
        // The selected bank can subsequently replace the drain, as Fireborn does.
        for definition in [target, schema] {
            for (offset, bytes) in [
                (0xC8, charges.to_le_bytes()),
                (0xCC, cost.to_le_bytes()),
                (0xD0, drain.to_le_bytes()),
            ] {
                put(&mut self.payload.0, definition + offset, &bytes)?;
            }
            put(
                &mut self.payload.0,
                definition + 0x4C,
                &native_flags.to_le_bytes(),
            )?;
        }
        self.evidence["source_energy"] = json!({
            "controller":tag,"source_sha256":hex::encode(Sha256::digest(&source.0)),
            "charges":charges,"activation_cost":cost,"active_drain":drain,
            "source_flags":flags,"native_flags":native_flags,
            "limits":["Selected bank application can replace the base drain.",
                "Remaining costs, numeric inputs and recharge retain the native adaptation.",
                "These fields do not establish completed activation, revival or cleanup."]
        });
        Ok(())
    }
}
