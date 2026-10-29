//! Preserve light parameter arithmetic while retaining the native draw shader and pass layout.
use super::*;
use crate::d1_roi::expression;

fn bytes(p: &Payload, field: usize, stride: usize, class: u32) -> Result<Vec<u8>> {
    let range = p.array_range(field, stride, Some(class))?;
    Ok(p.0[range].to_vec())
}

impl Assembly<'_> {
    pub(super) fn light_program(
        &mut self,
        draw: &Payload,
        native: &Payload,
        names: &[u32],
        name: &str,
    ) -> Result<(String, String)> {
        let source_tag = draw.u32(0x98)?;
        ensure!(
            self.source.reference(source_tag)? == 0x80801AF2,
            "Source light parameter class differs"
        );
        let source = self.source.read(source_tag)?;
        let template_tag = native.u32(0xA0)?;
        let mut target = (*self.native.tag(template_tag, Some(0x808071DC))?).clone();
        let witness = bytes(&target, 0x30, 1, 0x80800009)?;
        ensure!(
            witness == hex::decode("34003c04403c0107030e430534013c04403c0107030e4307")?,
            "Native light exposure procedure differs"
        );
        ensure!(
            source.bytes::<40>(8)? == target.bytes::<40>(8)?
                && source.u32(0x80)? as usize == names.len()
                && source.bytes::<12>(0x84)? == target.bytes::<12>(0x84)?,
            "Source light parameter layout differs"
        );
        let code = bytes(&source, 0x30, 1, 0x80800009)?;
        let constants = bytes(&source, 0x40, 16, 0x80800090)?;
        let values = bytes(&source, 0x60, 16, 0x80800090)?;
        ensure!(
            values.len() == 128,
            "Source light needs eight parameter vectors"
        );
        ensure!(
            constants
                .chunks_exact(4)
                .chain(values.chunks_exact(4))
                .all(|b| f32::from_le_bytes(b.try_into().expect("Float32 lane")).is_finite()),
            "Nonfinite source light parameter vector"
        );
        let empty = Default::default();
        let program = expression::translate(
            &code,
            constants,
            &expression::Inputs {
                globals: &self.globals,
                dye: expression::Dye::Unset(None),
                plates: &empty,
                detail: &Default::default(),
                objects: names.len(),
                object_channels: &[],
                outputs: 8,
                samplers: 0,
                stage: 1,
                reticle: false,
                light: true,
            },
        )?;
        ensure!(
            program.textures.is_empty() && program.defaulted_globals.is_empty(),
            "Light parameters require an unavailable renderer input"
        );
        array(&mut target.0, 0x30, 0x80800009, &program.code, 1)?;
        array(&mut target.0, 0x40, 0x80800090, &program.constants, 16)?;
        array(&mut target.0, 0x60, 0x80800090, &values, 16)?;
        put(&mut target.0, 0x80, &source.bytes::<4>(0x80)?)?;
        size(&mut target.0)?;
        let symbol = format!("{name}-parameters");
        self.node(&symbol, template_tag, &target.0, None, &[])?;
        let supplemental_tag = native.u32(0xA4)?;
        let mut supplemental = (*self.native.tag(supplemental_tag, Some(0x808071DC))?).clone();
        ensure!(
            supplemental.u64(0x30)? == 0
                && supplemental.u64(0x40)? == 0
                && supplemental.u32(0x80)? == 0,
            "Native supplemental light program differs"
        );
        let rows = supplemental.array(0x60, 16, Some(0x80800090))?;
        ensure!(rows.len() == 4, "Native supplemental light values differ");
        put(&mut supplemental.0, rows[0], &values[32..44])?;
        let extra = format!("{name}-supplemental");
        self.node(&extra, supplemental_tag, &supplemental.0, None, &[])?;
        Ok((symbol, extra))
    }
}
