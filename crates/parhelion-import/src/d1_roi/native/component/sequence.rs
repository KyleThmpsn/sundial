//! Scalar-only source effect sequences preserve scheduling and their material output providers.
use super::*;
use crate::tiger::{allocation::Row, sequence};
pub(super) mod output;
mod source;

fn pair(p: &mut Payload, owner: u32, i: usize, d: usize, ic: u32, dc: u32) -> Result<()> {
    for (at, class, twin) in [(i, dc, d), (d, ic, i)] {
        put(&mut p.0, at, &owner.to_le_bytes())?;
        put(&mut p.0, at + 4, &class.to_le_bytes())?;
        put(&mut p.0, at + 8, &(twin as u64).to_le_bytes())?;
    }
    Ok(())
}

impl Assembly<'_> {
    pub(super) fn scalar_sequence(
        &mut self,
        tag: u32,
        ordinal: usize,
        source: &Payload,
        entity: &Payload,
        symbol: &str,
        index: usize,
    ) -> Result<Component> {
        let names =
            light::inputs::names_at(self.source, source, entity, tag, ordinal, 0xB0, 0x1A0)?;
        let tree = source::tree(source, names.len())?;
        let mut component = self.component(0x80FC7EE1, 0x80C0DC18, symbol, index)?;
        let envelope = self.native.tag(0x80B36407, Some(0x80809C36))?;
        let count = output::write(self, source, &mut component, &envelope)?;
        let p = &mut component.payload;
        let i = p.pointer(16)?;
        let d = p.pointer(24)?;
        // These sequences belong to persistent appearance parts. Native root completion
        // at 58BB34 uses bit 1 to retire the entire owner, which would leave the appearance
        // controller holding a freed child. The controller owns final part retirement.
        p.0[d + 0x2E0] &= !2;
        for field in [0x148, 0x188, 0x200, 0x210, 0x220, 0x230, 0x2B8] {
            ensure!(
                p.u64(d + field)? == 0,
                "Scalar sequence envelope has additional dependencies"
            );
        }
        // Scalar events do not consume spatial locators. Remove the trail envelope's storage.
        for field in [0x1B0, 0x1C0, 0x1D0] {
            put(&mut p.0, d + field, &[0; 16])?;
        }
        for field in [0x170, 0x180, 0x190, 0x1A0] {
            put(&mut p.0, i + field, &[0; 16])?;
        }
        put(&mut p.0, d + 0x1E0, &[0; 16])?;
        let tr = *envelope
            .array(envelope.pointer(24)? + 0x188, 40, Some(0x80809789))?
            .first()
            .context("Native sequence scalar input absent")?;
        let ti = usize::try_from(envelope.u64(tr + 8)?)?;
        array(
            &mut p.0,
            i + 0xD0,
            0x80809788,
            &envelope.bytes::<96>(ti)?.repeat(names.len()),
            96,
        )?;
        array(
            &mut p.0,
            d + 0x188,
            0x80809789,
            &envelope.bytes::<40>(tr)?.repeat(names.len()),
            40,
        )?;
        for ((input, definition), name) in p
            .array(i + 0xD0, 96, Some(0x80809788))?
            .into_iter()
            .zip(p.array(d + 0x188, 40, Some(0x80809789))?)
            .zip(names)
        {
            pair(p, component.id, input, definition, 0x80809788, 0x80809789)?;
            sequence::relative(p, input + 16, i)?;
            put(&mut p.0, definition + 32, &name.to_le_bytes())?;
            component.inputs.push((name, definition));
        }
        let allocation_tag = p.u32(0x44)?;
        let mut allocation = (*self.native.tag(allocation_tag, Some(0x80809BBB))?).clone();
        Row::write(
            &[
                Row::named(0x86374C3C, 0x80809788, component.inputs.len(), Vec::new()),
                Row::named(771384421, 0x808084DE, count, Vec::new()),
            ],
            &mut allocation,
            32,
        )?;
        let written = sequence::write(p, &mut allocation, &tree, &[(*envelope).clone()])?;
        source::events(source, p, &written, count)?;
        activation(source, p)?;
        let allocation_symbol = format!("{symbol}-allocation");
        self.node(&allocation_symbol, allocation_tag, &allocation.0, None, &[])?;
        component.patch(0x44, &allocation_symbol)?;
        let carrier = self.native.tag(WEAPON_PART_ENTITY, Some(0x80809C0F))?;
        let row = carrier
            .array(0x20, 72, Some(0x80809BC9))?
            .into_iter()
            .find(|&at| carrier.u32(at + 12).ok() == Some(0x80809789))
            .context("Native scalar connection envelope absent")?;
        ensure!(
            component.entity.u64(0x20)? == 0,
            "Scalar sequence envelope has connected providers"
        );
        array(
            &mut component.entity.0,
            0x20,
            0x80809BC9,
            &carrier.bytes::<72>(row)?,
            72,
        )?;
        size(&mut component.entity.0)?;
        component.seal()?;
        self.evidence.push(json!({"source_component":tag,"scalar_sequence":{"outputs":count,"inputs":component.inputs.len(),"events":written.events.len()},"gameplay_verified":false}));
        Ok(component)
    }
}
