//! Measured car phase and steering reach source overlays through native pose inputs.
mod output;
use super::super::curve::Program;
use super::*;
use crate::{
    halo_reach::{Scene, animation::native as source_motion},
    presentation::append,
    tiger::{animation, instance},
};

const BANK_SYMBOL: &str = "vehicle-wheel-channels";
const NAMES: [&str; 3] = [
    "vehicle:ground_speed",
    "vehicle:ground_speed_rear",
    "vehicle:steering",
];

fn phase_envelope(envelope: &Payload) -> Result<(usize, usize)> {
    let mut candidates = Vec::new();
    for layer in envelope.array(8, 24, Some(0x80803724))? {
        if envelope.u16(layer + 22)? != 2 {
            continue;
        }
        let choices = envelope.array(layer, 16, Some(0x8080372e))?;
        if choices.len() != 1 {
            continue;
        }
        let options = envelope.array(choices[0], 64, Some(0x80803730))?;
        if options.len() != 1 {
            continue;
        }
        let option = options[0];
        if envelope.u64(option + 16)? == 0
            && envelope.u32(option + 32)? == 1
            && envelope.f32(option + 36)? == 1.
            && envelope.0[option + 63] == 2
        {
            candidates.push((layer, option));
        }
    }
    ensure!(
        candidates.len() == 1,
        "Wheel phase envelope is absent or ambiguous"
    );
    Ok(candidates[0])
}

fn name(value: &str) -> u32 {
    value.bytes().fold(0x811c9dc5u32, |hash, byte| {
        hash.wrapping_mul(0x01000193) ^ u32::from(byte)
    })
}

fn bytes(p: &Payload, fields: &[usize], stride: usize) -> Vec<u8> {
    fields
        .iter()
        .flat_map(|&at| p.0[at..at + stride].iter().copied())
        .collect()
}

fn node(p: &mut Payload, class: u32, size: usize) -> Result<usize> {
    let at = (p.0.len() + 19) & !15;
    p.0.resize(at + size, 0);
    put(&mut p.0, at - 4, &class.to_le_bytes())?;
    Ok(at)
}

fn relative(p: &mut Payload, field: usize, target: usize) -> Result<()> {
    put(
        &mut p.0,
        field,
        &(target as i64 - field as i64).to_le_bytes(),
    )
}

fn parameters(
    original: &Payload,
    inputs: usize,
    input_names: &[u32; 3],
) -> Result<(Payload, usize)> {
    let old = original.array(8, 24, Some(0x80803650))?;
    let first = old.len();
    ensure!(
        first + 4 < i16::MAX as usize,
        "Wheel parameter index exceeds its native domain"
    );
    let mut p = original.clone();
    let mut records = bytes(original, &old, 24);
    records.resize(records.len() + 4 * 24, 0);
    append(&mut p.0, 8, 0x80803650, &records, 24)?;
    let rows = p.array(8, 24, Some(0x80803650))?;
    for (&from, &to) in old.iter().zip(&rows) {
        for offset in [8, 16] {
            if original.u64(from + offset)? != 0 {
                relative(&mut p, to + offset, original.pointer(from + offset)?)?;
            }
        }
    }
    for (index, input_name) in input_names.iter().enumerate() {
        let row = rows[first + index];
        put(&mut p.0, row, &name(NAMES[index]).to_le_bytes())?;
        put(&mut p.0, row + 4, &[20, 2, 0, 0])?;
        let input = node(&mut p, 0x80803655, 8)?;
        relative(&mut p, row + 8, input)?;
        put(&mut p.0, input, &input_name.to_le_bytes())?;
        put(
            &mut p.0,
            input + 4,
            &u32::try_from(inputs + index)?.to_le_bytes(),
        )?;
    }
    let row = rows[first + 3];
    put(&mut p.0, row, &name("source/steering_phase").to_le_bytes())?;
    put(&mut p.0, row + 4, &[21, 2, 0, 0])?;
    let expression = node(&mut p, 0x80803653, 80)?;
    relative(&mut p, row + 8, expression)?;
    put(&mut p.0, expression + 64, &u32::MAX.to_le_bytes())?;
    append(
        &mut p.0,
        expression,
        0x8080373d,
        &(0x10000u32 | u32::try_from(first + 2)?).to_le_bytes(),
        4,
    )?;
    Program::ramp([-1., 1.], [0., 1.])?.write(&mut p, expression + 16)?;
    let size = p.0.len() as u64;
    put(&mut p.0, 0, &size.to_le_bytes())?;
    Ok((p, first))
}

fn layers(
    original: &Payload,
    envelope: &Payload,
    first_parameter: usize,
    clip_indices: &[usize; 3],
    option: usize,
    layer: usize,
) -> Result<Payload> {
    let mut p = original.clone();
    let old = original.array(8, 24, Some(0x80803724))?;
    let mut data = bytes(original, &old, 24);
    data.resize(data.len() + 3 * 24, 0);
    append(&mut p.0, 8, 0x80803724, &data, 24)?;
    let rows = p.array(8, 24, Some(0x80803724))?;
    let last_order = old
        .iter()
        .map(|&row| original.u16(row + 20))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .max()
        .context("Empty pose layers")?;
    for (&from, &to) in old.iter().zip(&rows) {
        relative(&mut p, to + 8, original.pointer(from + 8)?)?;
    }
    for index in 0..3 {
        let row = rows[old.len() + index];
        put(&mut p.0, row + 16, &name(NAMES[index]).to_le_bytes())?;
        put(
            &mut p.0,
            row + 20,
            &last_order
                .checked_add(index as u16 + 1)
                .context("Pose layer order overflow")?
                .to_le_bytes(),
        )?;
        put(&mut p.0, row + 22, &envelope.bytes::<2>(layer + 22)?)?;
        append(&mut p.0, row, 0x8080372e, &[0; 16], 16)?;
        let choice = p.array(row, 16, Some(0x8080372e))?[0];
        let mut record = envelope.bytes::<64>(option)?;
        record[..32].fill(0);
        record[56..60].copy_from_slice(
            &(0x10000u32 | u32::try_from(first_parameter + if index == 2 { 3 } else { index })?)
                .to_le_bytes(),
        );
        append(&mut p.0, choice, 0x80803730, &record, 64)?;
        let selection = p.array(choice, 64, Some(0x80803730))?[0];
        let weighted = envelope.array(option, 12, Some(0x80803737))?;
        ensure!(
            weighted.len() == 1 && envelope.f32(weighted[0] + 4)? == 1.,
            "Wheel clip weight differs"
        );
        let mut weight = envelope.bytes::<12>(weighted[0])?;
        put(
            &mut weight,
            0,
            &u32::try_from(clip_indices[index])?.to_le_bytes(),
        )?;
        append(&mut p.0, selection, 0x80803737, &weight, 12)?;
    }
    let routes = original.array(56, 32, Some(0x808034ef))?;
    let mut data = bytes(original, &routes, 32);
    data.resize(data.len() + 32, 0);
    append(&mut p.0, 56, 0x808034ef, &data, 32)?;
    let written = p.array(56, 32, Some(0x808034ef))?;
    for (index, &row) in written.iter().enumerate() {
        let mut edges = if let Some(&old) = routes.get(index) {
            if original.u64(old + 16)? != 0 {
                relative(&mut p, row + 24, original.pointer(old + 24)?)?;
            }
            bytes(original, &original.array(old, 4, Some(0x808034f1))?, 4)
        } else {
            Vec::new()
        };
        for index in 0..3 {
            edges.extend(0u16.to_le_bytes());
            edges.extend(u16::try_from(old.len() + index)?.to_le_bytes());
        }
        append(&mut p.0, row, 0x808034f1, &edges, 4)?;
    }
    for at in original.array(40, 2, Some(0x80800006))? {
        if original.u16(at)? == u16::MAX {
            put(&mut p.0, at, &u16::try_from(routes.len())?.to_le_bytes())?;
        }
    }
    Ok(p)
}

fn allocation(p: &mut Payload, inputs: usize, parameters: usize, layers: usize) -> Result<()> {
    let mut found = BTreeSet::new();
    let mut pending = p.array(0x20, 40, Some(0x80808852))?;
    let mut seen = BTreeSet::new();
    while let Some(row) = pending.pop() {
        ensure!(
            seen.insert(row) && seen.len() < 1024,
            "Wheel pose allocation repeats a node"
        );
        for (key, class, old, new) in [
            (0x86374c3c, 0x80809788, inputs, inputs + 3),
            (0xcd06e535, 0x8080000f, parameters, parameters + 4),
            (0x6b6ba194, 0x80800090, parameters, parameters + 4),
            (0xe5b438c2, 0x8080371a, layers, layers + 3),
        ] {
            if p.u32(row)? == key && p.u32(row + 16)? == class && p.u32(row + 20)? as usize == old {
                ensure!(found.insert(key), "Wheel pose allocation is ambiguous");
                put(&mut p.0, row + 20, &u32::try_from(new)?.to_le_bytes())?;
            }
        }
        if p.u64(row + 8)? != 0 {
            pending.push(p.pointer(row + 8)?);
        }
        pending.extend(p.array(row + 24, 40, Some(0x80808852))?);
    }
    ensure!(found.len() == 4, "Wheel pose allocation is incomplete");
    Ok(())
}

impl Assembly {
    fn wheel_bank(
        &mut self,
        native: &mut Reader,
        graph: &mut Graph,
        motion: &motion::Motion,
    ) -> Result<(links::Endpoint, [usize; 3], [u32; 3])> {
        let declared = patches(graph, "vehicle-motion")?;
        let mut sources = Vec::new();
        for method in [10u32, 11, 9] {
            let symbol = format!("vehicle-car-output-{}", method - 6);
            let at = declared
                .iter()
                .find(|p| p["symbol"] == symbol)
                .context("Car phase provider")?["offset"]
                .as_u64()
                .context("Car phase field")?
                - 8;
            let object = links::Object {
                owner: motion::OWNER,
                class: 0x80809ae1,
                offset: at,
            };
            let contract =
                links::Interface::read(&motion.payload, object, &graph.read(&symbol)?, false)?;
            ensure!(
                contract.methods.len() == 1
                    && contract.methods[0].index == method
                    && contract.instance_class == 0x80809ae2,
                "Car wheel output contract differs"
            );
            sources.push((object, 0));
        }
        let channels = self.channel_bridge(native, graph, BANK_SYMBOL, &sources)?;
        Ok((
            channels[0].provider.clone(),
            std::array::from_fn(|i| channels[i].index as usize),
            std::array::from_fn(|i| channels[i].name),
        ))
    }

    pub fn wheels(
        &mut self,
        native: &mut Reader,
        graph: &mut Graph,
        scene: &Scene,
        bones: &Value,
        motion: &motion::Motion,
    ) -> Result<Value> {
        let first = usize::try_from(
            bones["source_bone_first"]
                .as_u64()
                .context("Wheel source bones were not appended")?,
        )?;
        let total = bones["bones"].as_array().context("Wheel skeleton")?.len();
        let owner = self.owner(0x8080348f)?;
        let original = self.payloads[&owner].clone();
        let instance = original.pointer(16)?;
        let definition = usize::try_from(original.u64(instance + 8)?)?;
        let layer_tag = original.u32(definition + 0x11c)?;
        let layer_symbol = patches(graph, &self.symbols[&owner])?
            .into_iter()
            .find(|patch| patch["offset"] == definition + 0x11c)
            .and_then(|patch| patch["symbol"].as_str().map(str::to_owned));
        let layer_source = if let Some(symbol) = layer_symbol {
            graph.read(&symbol)?
        } else {
            (*native.tag(layer_tag, Some(0x808034eb))?).clone()
        };
        let layers_before = layer_source.array(8, 24, Some(0x80803724))?;
        // Use a checked native direct-phase envelope independently of the
        // selected carrier. Its original layers and bank ordinals stay intact.
        let envelope = native.tag(0x80c0c493, Some(0x808034eb))?;
        let (layer, option) = phase_envelope(&envelope)?;
        let weighted = envelope.array(option, 12, Some(0x80803737))?;
        ensure!(
            weighted.len() == 1,
            "Wheel phase envelope has multiple clips"
        );
        let original_bank = native.tag(0x80c0c48f, Some(0x808036f6))?;
        let bank_rows = original_bank.array(8, 4, Some(0x80808f48))?;
        let clip_tag = original_bank.u32(
            *bank_rows
                .get(envelope.u32(weighted[0])? as usize)
                .context("Wheel phase clip ordinal")?,
        )?;
        let template = native.tag(clip_tag, Some(0x80808f49))?;
        let controls = graph.read("motion-controls")?;
        let frames = usize::from(template.u16(0x13c)?);
        let mut bank = graph.read("motion-bank")?;
        let old_rows = bank.array(8, 4, Some(0x80808f48))?;
        let clip_indices = std::array::from_fn(|index| old_rows.len() + index);
        let mut bank_data = bytes(&bank, &old_rows, 4);
        bank_data.extend([0xff; 12]);
        append(&mut bank.0, 8, 0x80808f48, &bank_data, 4)?;
        let new_rows = bank.array(8, 4, Some(0x80808f48))?;
        let mut bank_patches = patches(graph, "motion-bank")?;
        for patch in &mut bank_patches {
            let old = patch["offset"].as_u64().context("Animation bank patch")? as usize;
            let index = old_rows
                .iter()
                .position(|&row| row == old)
                .context("Animation bank patch is outside its clip array")?;
            patch["offset"] = json!(new_rows[index]);
        }
        let mut clip_report = Vec::new();
        for (index, source) in NAMES.iter().enumerate() {
            let tracks = source_motion::rotations(scene, &graph.root, source, first, frames)?;
            let clip = animation::clip::rotations(&template, total, &controls, &tracks)?;
            let symbol = format!("vehicle-wheel-clip-{index}");
            graph.add(&symbol, clip_tag, &clip.0, None, Vec::new())?;
            bank_patches.push(json!({"offset":new_rows[clip_indices[index]],"symbol":symbol}));
            clip_report.push(json!({"source":source,"symbol":symbol,"bank_ordinal":clip_indices[index],"slots":tracks.keys().collect::<Vec<_>>() }));
        }
        graph.replace("motion-bank", &bank.0, bank_patches)?;
        let old_inputs = original.array(instance + 0x30, 96, Some(0x80809788))?;
        let old_definitions = original.array(definition + 0xa8, 40, Some(0x80809789))?;
        ensure!(
            !old_inputs.is_empty() && old_inputs.len() == old_definitions.len(),
            "Wheel pose input counts differ"
        );
        let (provider, channels, input_names) = self.wheel_bank(native, graph, motion)?;
        let parameter_tag = original.u32(definition + 0x110)?;
        let parameter_source = native.tag(parameter_tag, None)?;
        let (parameters, first_parameter) =
            parameters(&parameter_source, old_inputs.len(), &input_names)?;
        let mut layer_payload = layers(
            &layer_source,
            &envelope,
            first_parameter,
            &clip_indices,
            option,
            layer,
        )?;
        let declared = patches(graph, "motion-bank")?;
        let sizes = new_rows
            .iter()
            .map(|row| {
                let patch = declared
                    .iter()
                    .find(|patch| patch["offset"] == *row)
                    .context("Wheel bank clip")?;
                let symbol = patch["symbol"].as_str().context("Wheel bank clip symbol")?;
                animation::clip::size(&graph.read(symbol)?)
            })
            .collect::<Result<Vec<_>>>()?;
        animation::layer::clip_sizes(&mut layer_payload, &sizes)?;
        graph.add(
            "vehicle-wheel-parameters",
            parameter_tag,
            &parameters.0,
            None,
            Vec::new(),
        )?;
        graph.add(
            "vehicle-wheel-layers",
            layer_tag,
            &layer_payload.0,
            None,
            Vec::new(),
        )?;
        self.resource(owner, definition + 0x110, "vehicle-wheel-parameters".into())?;
        self.resource(owner, definition + 0x11c, "vehicle-wheel-layers".into())?;
        let p = self.payloads.get_mut(&owner).unwrap();
        let mut input_data = bytes(&original, &old_inputs, 96);
        let mut definition_data = bytes(&original, &old_definitions, 40);
        for _ in 0..3 {
            input_data.extend(original.bytes::<96>(old_inputs[0])?);
            definition_data.extend(original.bytes::<40>(old_definitions[0])?);
        }
        append(&mut p.0, instance + 0x30, 0x80809788, &input_data, 96)?;
        append(
            &mut p.0,
            definition + 0xa8,
            0x80809789,
            &definition_data,
            40,
        )?;
        let inputs = p.array(instance + 0x30, 96, Some(0x80809788))?;
        let definitions = p.array(definition + 0xa8, 40, Some(0x80809789))?;
        for (index, (&input, &data)) in inputs.iter().zip(&definitions).enumerate() {
            put(&mut p.0, input + 8, &(data as u64).to_le_bytes())?;
            relative(p, input + 16, instance + 0x30)?;
            put(&mut p.0, data + 8, &(input as u64).to_le_bytes())?;
            if index >= old_inputs.len() {
                put(
                    &mut p.0,
                    data + 32,
                    &input_names[index - old_inputs.len()].to_le_bytes(),
                )?;
            }
        }
        for row in self
            .links
            .connections
            .iter_mut()
            .chain(&mut self.links.named_connections)
        {
            if let Some(consumer) = row.consumer.object.as_mut().filter(|v| v.owner == owner)
                && let Some(index) = old_definitions
                    .iter()
                    .position(|&at| consumer.offset == at as u64)
            {
                consumer.offset = definitions[index] as u64;
            }
        }
        let ordinal = self
            .links
            .components
            .iter()
            .position(|&tag| tag == owner)
            .context("Wheel pose ordinal")?;
        for index in 0..3 {
            self.links.connections.push(links::Connection {
                consumer: links::Endpoint {
                    namespace: 0x811c9dc5,
                    object: Some(links::Object {
                        owner,
                        class: 0x80809789,
                        offset: definitions[old_inputs.len() + index] as u64,
                    }),
                    selector: ordinal as u64,
                },
                provider: provider.clone(),
                channel: channels[index] as u32,
                flags: 0,
            });
        }
        ensure!(
            original.u32(definition + 0x144)? as usize == first_parameter
                && original.u32(definition + 0x148)? as usize == first_parameter,
            "Wheel parameter definition counts differ"
        );
        for (field, stride, class) in [(0x1f0, 4, 0x8080000f), (0x200, 16, 0x80800090)] {
            let old = original.array(instance + field, stride, Some(class))?;
            ensure!(
                old.len() == first_parameter,
                "Wheel parameter state count differs"
            );
            let mut data = bytes(&original, &old, stride);
            data.resize(data.len() + 4 * stride, 0);
            append(&mut p.0, instance + field, u64::from(class), &data, stride)?;
        }
        ensure!(
            first_parameter.div_ceil(32) == (first_parameter + 4).div_ceil(32),
            "Wheel parameters require a larger dirty mask"
        );
        for field in [0x144, 0x148] {
            put(
                &mut p.0,
                definition + field,
                &u32::try_from(first_parameter + 4)?.to_le_bytes(),
            )?;
        }
        let states = original.array(instance + 0x500, 120, Some(0x8080371a))?;
        ensure!(
            states.len() == layers_before.len(),
            "Wheel layer state count differs"
        );
        let mut state_data = bytes(&original, &states, 120);
        state_data.resize(state_data.len() + 3 * 120, 0);
        append(&mut p.0, instance + 0x500, 0x8080371a, &state_data, 120)?;
        put(
            &mut p.0,
            definition + 0x180,
            &u32::try_from(states.len() + 3)?.to_le_bytes(),
        )?;
        let allocation_tag = original.u32(0x44)?;
        let mut allocation_payload = (*native.tag(allocation_tag, Some(0x80809bbb))?).clone();
        allocation(
            &mut allocation_payload,
            old_inputs.len(),
            first_parameter,
            states.len(),
        )?;
        graph.add(
            "vehicle-wheel-pose-allocation",
            allocation_tag,
            &allocation_payload.0,
            None,
            Vec::new(),
        )?;
        self.allocations
            .insert(owner, "vehicle-wheel-pose-allocation".into());
        let symbol = &self.symbols[&owner];
        let mut declared = patches(graph, symbol)?;
        declared.extend(
            self.resources
                .remove(&owner)
                .unwrap()
                .into_iter()
                .map(|(offset, symbol)| json!({"offset":offset,"symbol":symbol})),
        );
        let end = p.0.len();
        let seal = instance::seal_span(p, &mut declared, end)?;
        graph.replace(symbol, &p.0, declared)?;
        let output = output::build(self, native, graph, states.len())?;
        Ok(
            json!({"clips":clip_report,"front_phase_method":10,"rear_phase_method":11,"steering_method":9,
            "parameters":first_parameter+4,"layers":states.len()+3,"input_scheduler":"Primary vehicle bank","seal":seal,"output":output,
            "limits":["These source rotation overlays follow native car travel and steering."]}),
        )
    }
}
