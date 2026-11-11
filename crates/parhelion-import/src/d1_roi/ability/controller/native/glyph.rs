//! The source Super's HUD glyph. Both builds name it by key in the energy controller, and every
//! Rise of Iron glyph key is also a row of the Shadowkeep glyph table.
use super::Prepared;
use crate::{presentation::put, tiger::payload::Payload};
use anyhow::{Context, Result, ensure};
use serde_json::json;
use std::collections::BTreeSet;
use sundial::package_authoring::ability_hud::GLYPH_FIELD;

impl Prepared {
    /// Name the source controller's HUD glyph in the retained controller. Rise of Iron resets
    /// its instance key and suppress byte from definition `+170` and `+174` (`004818A0`) and its
    /// HUD writer reads them back (`0048A0A3`). Shadowkeep keeps the key at `+1C8` of the
    /// instance and of both definition views. A suppressed source glyph leaves the native one.
    pub fn source_glyph(&mut self, source: &Payload, native_keys: &BTreeSet<u32>) -> Result<()> {
        let definition = source.pointer(24)?;
        let key = source.u32(definition + 0x170)?;
        let suppressed = source.u8(definition + 0x174)? != 0;
        let live = self.payload.pointer(16)?;
        let target = usize::try_from(self.payload.u64(live + 8)?)
            .context("Native glyph definition offset")?;
        let schema = self.payload.pointer(24)?;
        let native = self.payload.u32(live + GLYPH_FIELD)?;
        // The retained controller already names a native glyph in every view, which places the
        // field before it is replaced.
        ensure!(
            native_keys.contains(&native)
                && self.payload.u32(target + GLYPH_FIELD)? == native
                && self.payload.u32(schema + GLYPH_FIELD)? == native,
            "Native controller glyph field differs"
        );
        if suppressed {
            self.evidence["source_glyph"] = json!({"key":key,"suppressed":true,"native":native});
            return Ok(());
        }
        // A key outside the native table would need a private row with the source art.
        ensure!(
            native_keys.contains(&key),
            "Source Super glyph {key:08X} has no native row"
        );
        for at in [live, target, schema] {
            put(&mut self.payload.0, at + GLYPH_FIELD, &key.to_le_bytes())?;
        }
        self.evidence["source_glyph"] = json!({"key":key,"suppressed":false,"replaced":native,
            "limits":["The native row's art and theme color are used. They match the source art for keys checked in the HUD glyph route."]});
        Ok(())
    }
}
