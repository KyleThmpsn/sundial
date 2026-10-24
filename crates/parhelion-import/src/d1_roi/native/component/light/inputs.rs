//! Bind light parameter inputs to the source body's declared channel providers.
use super::*;

pub(super) fn names(
    source: &Source,
    p: &Payload,
    entity: &Payload,
    owner: u32,
    ordinal: usize,
) -> Result<Vec<u32>> {
    names_at(source, p, entity, owner, ordinal, 0xC8, 0x1D0)
}

pub(in crate::d1_roi::native::component) fn names_at(
    source: &Source,
    p: &Payload,
    entity: &Payload,
    owner: u32,
    ordinal: usize,
    instance_field: usize,
    definition_field: usize,
) -> Result<Vec<u32>> {
    let r = p.pointer(24)?;
    let inputs = p.array(p.pointer(16)? + instance_field, 88, Some(0x8080077D))?;
    let definitions = p.array(r + definition_field, 48, Some(0x80800927))?;
    ensure!(
        inputs.len() == definitions.len(),
        "Source light input count differs"
    );
    let links = entity.array(0x30, 88, Some(0x8080054E))?;
    let owners = entity.array(0x20, 12, Some(0x80800715))?;
    let mut names = Vec::new();
    for (input, def) in inputs.into_iter().zip(definitions) {
        ensure!(
            p.u64(input + 8)? == def as u64
                && p.u64(def + 8)? == input as u64
                && p.u64(def + 32)? == 0x808003E8,
            "Source light input definition differs"
        );
        let name = p.u32(def + 40)?;
        let matching = links
            .iter()
            .copied()
            .filter(|&a| {
                entity.u32(a + 8).ok() == Some(owner)
                    && entity.u64(a + 32).ok() == Some(ordinal as u64)
                    && entity.u32(a + 24).ok() == Some(0x80800927)
                    && entity.u64(a + 16).ok() == Some(def as u64)
            })
            .collect::<Vec<_>>();
        ensure!(
            matching.len() == 1,
            "Source light input has no unique provider"
        );
        let a = matching[0];
        let ordinal = usize::try_from(entity.u64(a + 72)?)?;
        let provider = entity.u32(a + 48)?;
        ensure!(
            entity.u32(*owners.get(ordinal).context("Source light provider index")?)? == provider
                && entity.u32(a + 40)? == 0x811C9DC5
                && entity.u32(a + 64)? == 0x808005AB,
            "Source light input provider differs"
        );
        let bank = source.read(provider)?;
        let schema = bank.pointer(24)?;
        ensure!(
            bank.u32(schema - 4)? == 0x808008F6 && entity.u64(a + 56)? == (schema + 0x60) as u64,
            "Source light input is not a body channel"
        );
        let rows = bank.array(schema + 0xD8, 112, Some(0x80800883))?;
        let channel = usize::try_from(entity.u64(a + 80)?)?;
        ensure!(
            bank.u32(*rows.get(channel).context("Source light channel index")?)? == name,
            "Source light input name differs from its provider"
        );
        names.push(name);
    }
    Ok(names)
}

impl Assembly<'_> {
    pub(super) fn light_inputs(&mut self, component: &mut Component, names: &[u32]) -> Result<()> {
        if names.is_empty() {
            return Ok(());
        }
        let donor = self.native.tag(0x80B7C5D4, Some(0x80809C36))?;
        let old_inputs = donor.array(donor.pointer(16)? + 0xD0, 96, Some(0x80809788))?;
        let old_defs = donor.array(donor.pointer(24)? + 0x1A8, 40, Some(0x80809789))?;
        ensure!(
            old_inputs.len() == 4 && old_defs.len() == 4,
            "Native light input template differs"
        );
        let input_bytes = donor.bytes::<96>(old_inputs[0])?.repeat(names.len());
        let def_bytes = donor.bytes::<40>(old_defs[0])?.repeat(names.len());
        let allocation_tag = donor.u32(0x44)?;
        let i = component.payload.pointer(16)?;
        let r = component.payload.pointer(24)?;
        array(
            &mut component.payload.0,
            i + 0xD0,
            0x80809788,
            &input_bytes,
            96,
        )?;
        array(
            &mut component.payload.0,
            r + 0x1A8,
            0x80809789,
            &def_bytes,
            40,
        )?;
        let inputs = component.payload.array(i + 0xD0, 96, Some(0x80809788))?;
        let defs = component.payload.array(r + 0x1A8, 40, Some(0x80809789))?;
        for ((input, def), &name) in inputs.into_iter().zip(defs).zip(names) {
            put(&mut component.payload.0, input, &component.id.to_le_bytes())?;
            put(
                &mut component.payload.0,
                input + 8,
                &(def as u64).to_le_bytes(),
            )?;
            put(
                &mut component.payload.0,
                input + 16,
                &((i + 0xB0) as i64 - (input + 16) as i64).to_le_bytes(),
            )?;
            put(&mut component.payload.0, def, &component.id.to_le_bytes())?;
            put(
                &mut component.payload.0,
                def + 8,
                &(input as u64).to_le_bytes(),
            )?;
            put(&mut component.payload.0, def + 32, &name.to_le_bytes())?;
            component.inputs.push((name, def));
        }
        let mut allocation = (*self.native.tag(allocation_tag, None)?).clone();
        ensure!(
            crate::presentation::channel::input_allocation(&mut allocation, 0x20, 4, names.len())?
                == 1,
            "Native light input allocation absent or ambiguous"
        );
        let symbol = format!("{}-allocation", component.symbol);
        self.node(&symbol, allocation_tag, &allocation.0, None, &[])?;
        component.patch(0x44, &symbol)?;
        // Reuse the native typed getter connection, preserving the light's skeleton binding.
        let carrier = self.native.tag(WEAPON_PART_ENTITY, Some(0x80809C0F))?;
        let row = carrier
            .array(0x20, 72, Some(0x80809BC9))?
            .into_iter()
            .find(|&a| carrier.u32(a + 12).ok() == Some(0x80809789))
            .context("Native material connection template absent")?;
        let mut connections = component
            .entity
            .array(0x20, 72, Some(0x80809BC9))?
            .into_iter()
            .map(|a| component.entity.bytes::<72>(a))
            .collect::<Result<Vec<_>>>()?;
        connections.push(carrier.bytes::<72>(row)?);
        array(
            &mut component.entity.0,
            0x20,
            0x80809BC9,
            &connections.concat(),
            72,
        )?;
        size(&mut component.entity.0)
    }
}
