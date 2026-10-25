//! Checked scalar getter inputs, including their mutable bank storage and entity links.
use super::*;

pub(super) struct Input {
    pub ordinal: usize,
    name: u32,
    definition: usize,
}

fn empty_state(source: &Payload, at: usize) -> Result<()> {
    // Rise of Iron's unloaded extended references retain separate class words.
    let expected = [
        u32::MAX,
        0,
        0,
        0,
        u32::MAX,
        0,
        u32::MAX,
        0,
        0,
        0,
        u32::MAX,
        0,
    ];
    for (index, value) in expected.into_iter().enumerate() {
        ensure!(
            source.u32(at + 32 + index * 4)? == value,
            "Source incoming channel state is initialized"
        );
    }
    Ok(())
}

pub(super) fn read(source: &Payload, names: &[u32]) -> Result<Vec<Input>> {
    let instance = source.pointer(16)?;
    let schema = source.pointer(24)?;
    let inputs = source.array(instance + 0x30, 80, Some(0x80800895))?;
    let definitions = source.array(schema + 0xC8, 40, Some(0x80800668))?;
    let incoming = source.array(schema + 0x118, 12, Some(0x8080087B))?;
    let cache = source.array(instance + 0x60, 16, Some(0x80800090))?;
    ensure!(
        inputs.len() == definitions.len() && inputs.len() == incoming.len(),
        "Source incoming channel counts disagree"
    );
    ensure!(
        cache
            .iter()
            .all(|at| source.bytes::<16>(*at).is_ok_and(|bytes| bytes == [0; 16])),
        "Source incoming channel cache is initialized"
    );
    let mut lookup = BTreeMap::new();
    for at in incoming {
        let ordinal = usize::try_from(source.u32(at + 8)?)?;
        let name = source.u32(at + 4)?;
        ensure!(
            source.u32(at)? == 0x811C9DC5
                && names.get(ordinal) == Some(&name)
                && ordinal < inputs.len()
                && lookup.insert(ordinal, name).is_none(),
            "Source incoming channel does not address its cached output"
        );
    }
    let owner = source.u32(instance)?;
    inputs
        .into_iter()
        .zip(definitions)
        .enumerate()
        .map(|(ordinal, (input, definition))| {
            ensure!(
                source.u64(input)? == u64::from(owner)
                    && source.u64(definition)? == u64::from(owner)
                    && source.u64(input + 8)? == definition as u64
                    && source.u64(definition + 8)? == input as u64
                    && source.u64(input + 16)? == 0x80800668
                    && source.u64(definition + 16)? == 0x80800895
                    && source.pointer(input + 24)? == instance
                    && source.u64(definition + 24)? == 0
                    && source.u64(definition + 32)? == 0x80800539,
                "Source incoming getter envelope differs"
            );
            empty_state(source, input)?;
            Ok(Input {
                ordinal,
                name: *lookup.get(&ordinal).context("Source incoming name")?,
                definition,
            })
        })
        .collect()
}

pub(super) fn write(
    inputs: &[Input],
    source: &Payload,
    component: &mut Component,
    template: &Payload,
) -> Result<()> {
    let ti = template.pointer(16)?;
    let td = template.pointer(24)?;
    let input = *template
        .array(ti + 0x30, 80, Some(0x808097A7))?
        .first()
        .context("Native getter input template")?;
    let definition = *template
        .array(td + 0xC8, 32, Some(0x808097A8))?
        .first()
        .context("Native getter definition template")?;
    ensure!(
        template.u32(input + 4)? == 0x808097A8
            && template.u64(input + 8)? == definition as u64
            && template.u32(definition + 4)? == 0x808097A7
            && template.u64(definition + 8)? == input as u64
            && template.pointer(input + 16)? == ti
            && template.u64(definition + 16)? == 0
            && template.u64(definition + 24)? == 0x80809AE2,
        "Native getter input envelope differs"
    );
    let p = &mut component.payload;
    let instance = p.pointer(16)?;
    let schema = p.pointer(24)?;
    array(
        &mut p.0,
        instance + 0x30,
        0x808097A7,
        &template.0[input..input + 80].repeat(inputs.len()),
        80,
    )?;
    array(
        &mut p.0,
        schema + 0xC8,
        0x808097A8,
        &template.0[definition..definition + 32].repeat(inputs.len()),
        32,
    )?;
    let emitted = p.array(instance + 0x30, 80, Some(0x808097A7))?;
    let definitions = p.array(schema + 0xC8, 32, Some(0x808097A8))?;
    for ((source_input, input), definition) in inputs.iter().zip(emitted).zip(definitions) {
        for (at, class, paired) in [
            (input, 0x808097A8u32, definition),
            (definition, 0x808097A7, input),
        ] {
            put(&mut p.0, at, &component.id.to_le_bytes())?;
            put(&mut p.0, at + 4, &class.to_le_bytes())?;
            put(&mut p.0, at + 8, &(paired as u64).to_le_bytes())?;
        }
        put(
            &mut p.0,
            input + 16,
            &(instance as i64 - (input + 16) as i64).to_le_bytes(),
        )?;
        component.incoming.push((
            source.u32(source.pointer(16)?)?,
            source_input.definition,
            definition,
        ));
    }
    let mut lookup = inputs
        .iter()
        .map(|input| (0x811C9DC5, input.name, input.ordinal))
        .collect::<Vec<_>>();
    lookup.sort_unstable();
    array(
        &mut p.0,
        schema + 0x118,
        0x808097A0,
        &super::lookup(&lookup)?,
        12,
    )?;
    Ok(())
}

pub(super) fn allocate(
    allocation: &mut Payload,
    input_template: &Payload,
    count: usize,
) -> Result<()> {
    let existing = allocation.array(0x20, 40, Some(0x80808852))?;
    let mut rows = Vec::new();
    for at in existing {
        ensure!(
            allocation.u64(at + 8)? == 0 && allocation.u64(at + 24)? == 0,
            "Channel allocation has indirect children"
        );
        rows.extend(allocation.bytes::<40>(at)?);
    }
    let input = input_template
        .array(0x20, 40, Some(0x80808852))?
        .into_iter()
        .find(|at| input_template.u32(*at).ok() == Some(0xA8333EC9))
        .context("Native input allocation template")?;
    ensure!(
        input_template.u32(input + 16)? == 0x808097A7
            && input_template.u64(input + 8)? == 0
            && input_template.u64(input + 24)? == 0,
        "Native input allocation differs"
    );
    let mut row = input_template.bytes::<40>(input)?;
    put(&mut row, 20, &u32::try_from(count)?.to_le_bytes())?;
    rows.extend(row);
    array(&mut allocation.0, 0x20, 0x80808852, &rows, 40)?;
    Ok(())
}

pub(super) fn allocate_history(allocation: &mut Payload, count: usize) -> Result<()> {
    let mut rows = Vec::new();
    for at in allocation.array(0x20, 40, Some(0x80808852))? {
        ensure!(
            allocation.u64(at + 8)? == 0 && allocation.u64(at + 24)? == 0,
            "Channel allocation has indirect children"
        );
        // The local template has no history. Rebuild this member once after incoming inputs
        // have been allocated, so callback state cannot alias an unrelated cached input.
        if allocation.u32(at)? != 0xB6515162 {
            rows.extend(allocation.bytes::<40>(at)?);
        }
    }
    if count == 0 {
        array(&mut allocation.0, 0x20, 0x80808852, &rows, 40)?;
        return Ok(());
    }
    let cache = allocation
        .array(0x20, 40, Some(0x80808852))?
        .into_iter()
        .find(|at| allocation.u32(*at).ok() == Some(0xFC2F3D6F))
        .context("Native vector allocation template")?;
    ensure!(
        allocation.u32(cache + 16)? == 0x80800090,
        "Native vector allocation differs"
    );
    let mut row = allocation.bytes::<40>(cache)?;
    put(&mut row, 0, &0xB6515162u32.to_le_bytes())?;
    put(&mut row, 20, &u32::try_from(count)?.to_le_bytes())?;
    rows.extend(row);
    array(&mut allocation.0, 0x20, 0x80808852, &rows, 40)?;
    Ok(())
}

impl Assembly<'_> {
    fn scalar_output(
        &mut self,
        source_tag: u32,
        offset: u64,
        component: &Component,
    ) -> Result<usize> {
        let source = self.source.read(source_tag)?;
        let s = source.pointer(24)?;
        if source.u32(s - 4)? == 0x80800610 {
            return super::super::sequence::output::provider(self, &source, offset, component);
        }
        let p = &component.payload;
        let n = usize::try_from(p.u64(p.pointer(16)? + 8)?)?;
        ensure!(
            source.u32(s - 4)? == 0x808027FC
                && offset == (s + 0x2B0) as u64
                && source.pointer(s + 0x2B0)? == s
                && p.u32(n - 4)? == 0x8080393B
                && p.pointer(n + 0x268)? == n
                && source.u32(s + 0x2C8)? == p.u32(n + 0x280)?,
            "Incoming channel requires a translated optic output"
        );
        let a = self.source.read(source.u32(s + 0x2B8)?)?;
        let b = self.native.tag(p.u32(n + 0x270)?, None)?;
        for (metadata, implementation, abstract_class, row_class, source_class, method) in [
            (&a, 0x8080053B, 0x80800539, 0x808005C0, 0x80802170, 3),
            (&*b, 0x80809AE1, 0x80809AE2, 0x80809C56, 0x80803941, 2),
        ] {
            let rows = metadata.array(16, 24, Some(row_class))?;
            ensure!(
                metadata.u32(8)? == implementation
                    && metadata.u32(12)? == abstract_class
                    && rows.len() == 1
                    && metadata.u32(rows[0])? == source_class
                    && metadata.u32(rows[0] + 4)? == method
                    && metadata.bytes::<16>(rows[0] + 8)? == [0; 16],
                "Optic getter dispatch differs"
            );
        }
        Ok(n + 0x268)
    }

    pub(in super::super) fn channel_connections(
        &mut self,
        entity: &Payload,
        components: &[Component],
        owners: &BTreeMap<usize, usize>,
    ) -> Result<Vec<Vec<u8>>> {
        let mut result = Vec::new();
        for (index, component) in components.iter().enumerate() {
            for &(source_owner, source_definition, definition) in &component.incoming {
                let matches = entity
                    .array(0x30, 88, Some(0x8080054E))?
                    .into_iter()
                    .filter(|at| {
                        entity.u32(*at + 8).ok() == Some(source_owner)
                            && entity.u64(*at + 16).ok() == Some(source_definition as u64)
                    })
                    .collect::<Vec<_>>();
                let [at] = matches.as_slice() else {
                    anyhow::bail!("Incoming channel provider is absent or ambiguous");
                };
                ensure!(
                    entity.u64(*at)? == 0x811C9DC5
                        && entity.u64(*at + 24)? == 0x80800668
                        && entity.u64(*at + 40)? == 0x811C9DC5
                        && entity.u64(*at + 64)? == 0x8080053B
                        && entity.u64(*at + 80)? == 0,
                    "Incoming channel connection requires translation"
                );
                let provider = entity.u32(*at + 48)?;
                let ordinal = usize::try_from(entity.u64(*at + 72)?)?;
                let listed = entity.array(0x20, 12, Some(0x80800715))?;
                let entry = *listed
                    .get(ordinal)
                    .context("Incoming channel provider ordinal")?;
                ensure!(
                    entity.u32(entry)? == provider,
                    "Incoming channel provider ordinal names another component"
                );
                let provider_index = *owners
                    .get(&ordinal)
                    .context("Incoming channel provider is not translated")?;
                let output = self.scalar_output(
                    provider,
                    entity.u64(*at + 56)?,
                    &components[provider_index],
                )?;
                let mut row = vec![0; 72];
                for (at, value) in [
                    (0, 0x811C9DC5),
                    (8, component.id),
                    (12, 0x808097A8),
                    (32, 0x811C9DC5),
                    (40, components[provider_index].id),
                    (44, 0x80809AE1),
                ] {
                    put(&mut row, at, &value.to_le_bytes())?;
                }
                for (at, value) in [
                    (16, definition),
                    (24, index),
                    (48, output),
                    (56, provider_index),
                ] {
                    put(&mut row, at, &(value as u64).to_le_bytes())?;
                }
                result.push(row);
            }
        }
        Ok(result)
    }
}
