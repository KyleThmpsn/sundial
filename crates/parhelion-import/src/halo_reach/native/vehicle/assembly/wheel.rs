//! Native pose layers that play converted source overlays from vehicle channels. Measured car
//! phase and steering reach wheel overlays as direct phase, and other overlays can integrate a
//! rate the same way.
mod output;
use super::super::curve::Program;
use super::*;
use crate::{
    halo_reach::{Scene, animation::native as source_motion},
    presentation::append,
    tiger::{animation, instance},
};

const BANK_SYMBOL: &str = "vehicle-wheel-channels";
/// A stock layer file holding one checked direct-phase envelope, the template of every pose layer.
const ENVELOPE: u32 = 0x80c0c493;
const NAMES: [&str; 3] = [
    "vehicle:ground_speed",
    "vehicle:ground_speed_rear",
    "vehicle:steering",
];

/// A root pose input appended to the carrier's pose controller, with the parameter that reads it.
pub(super) struct PoseInput {
    pub parameter: String,
    pub channel: Channel,
}

/// A parameter computed by a native four-lane program from appended parameters. A reference is
/// the index of an appended input or program parameter, inputs first.
pub(super) struct PoseProgram {
    pub parameter: &'static str,
    pub references: Vec<usize>,
    pub program: Program,
}

/// A layer playing one converted source overlay. `parameter` indexes the appended parameters,
/// inputs first. `mode` replaces the envelope's option byte `+0x3F`, so 3 integrates the
/// parameter over time where the envelope plays it as a direct phase. A layer that `yields` leaves
/// out the bones an earlier layer of the same call already plays.
pub(super) struct PoseLayer {
    pub clip: String,
    pub parameter: usize,
    pub mode: Option<u8>,
    pub yields: bool,
}

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

/// The duration in seconds of the envelope option every pose layer copies, and so of its clip.
pub(super) fn phase_duration(native: &mut Reader) -> Result<f32> {
    let envelope = native.tag(ENVELOPE, Some(0x808034eb))?;
    let (_, option) = phase_envelope(&envelope)?;
    let duration = envelope.f32(option + 0x34)?;
    ensure!(
        duration.is_finite() && duration > 0.,
        "Pose phase envelope duration differs"
    );
    Ok(duration)
}

/// Whether a sampled overlay track moves its rotation (`0`) or translation (`1`) channel. Samples
/// are `[x, y, z, w, tx, ty, tz, scale]` local poses, rotations kept on one hemisphere.
fn moves(samples: &[[f32; 8]], channel: usize) -> bool {
    let range = if channel == 0 { 0..4 } else { 4..7 };
    samples.iter().any(|sample| {
        range
            .clone()
            .any(|index| (sample[index] - samples[0][index]).abs() > 1e-5)
    })
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
    added: &[PoseInput],
    programs: Vec<PoseProgram>,
) -> Result<(Payload, usize)> {
    let old = original.array(8, 24, Some(0x80803650))?;
    let first = old.len();
    let count = added.len() + programs.len();
    ensure!(
        first + count < i16::MAX as usize,
        "Pose parameter index exceeds its native domain"
    );
    let mut p = original.clone();
    let mut records = bytes(original, &old, 24);
    records.resize(records.len() + count * 24, 0);
    append(&mut p.0, 8, 0x80803650, &records, 24)?;
    let rows = p.array(8, 24, Some(0x80803650))?;
    for (&from, &to) in old.iter().zip(&rows) {
        for offset in [8, 16] {
            if original.u64(from + offset)? != 0 {
                relative(&mut p, to + offset, original.pointer(from + offset)?)?;
            }
        }
    }
    // Kind 20 reads a root pose input by name and ordinal.
    for (index, input) in added.iter().enumerate() {
        let row = rows[first + index];
        put(&mut p.0, row, &name(&input.parameter).to_le_bytes())?;
        put(&mut p.0, row + 4, &[20, 2, 0, 0])?;
        let payload = node(&mut p, 0x80803655, 8)?;
        relative(&mut p, row + 8, payload)?;
        put(&mut p.0, payload, &input.channel.name.to_le_bytes())?;
        put(
            &mut p.0,
            payload + 4,
            &u32::try_from(inputs + index)?.to_le_bytes(),
        )?;
    }
    // Kind 21 evaluates a program over global parameters. Its inputs follow the reference order,
    // and its input count is the number of references, as in stock libraries.
    for (index, program) in programs.into_iter().enumerate() {
        let row = rows[first + added.len() + index];
        put(&mut p.0, row, &name(program.parameter).to_le_bytes())?;
        put(&mut p.0, row + 4, &[21, 2, 0, 0])?;
        let expression = node(&mut p, 0x80803653, 80)?;
        relative(&mut p, row + 8, expression)?;
        put(&mut p.0, expression + 64, &u32::MAX.to_le_bytes())?;
        let references = program
            .references
            .iter()
            .map(|&reference| {
                ensure!(
                    reference < count,
                    "Pose program reference is outside its parameters"
                );
                Ok(0x10000u32 | u32::try_from(first + reference)?)
            })
            .collect::<Result<Vec<_>>>()?;
        append(
            &mut p.0,
            expression,
            0x8080373d,
            &references
                .iter()
                .flat_map(|reference| reference.to_le_bytes())
                .collect::<Vec<_>>(),
            4,
        )?;
        program.program.write(&mut p, expression + 16)?;
    }
    let size = p.0.len() as u64;
    put(&mut p.0, 0, &size.to_le_bytes())?;
    Ok((p, first))
}

fn layers(
    original: &Payload,
    envelope: &Payload,
    first_parameter: usize,
    specs: &[(&PoseLayer, usize)],
    option: usize,
    layer: usize,
) -> Result<Payload> {
    let mut p = original.clone();
    let old = original.array(8, 24, Some(0x80803724))?;
    let mut data = bytes(original, &old, 24);
    data.resize(data.len() + specs.len() * 24, 0);
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
    for (index, (spec, clip)) in specs.iter().enumerate() {
        let row = rows[old.len() + index];
        put(&mut p.0, row + 16, &name(&spec.clip).to_le_bytes())?;
        put(
            &mut p.0,
            row + 20,
            &last_order
                .checked_add(u16::try_from(index + 1)?)
                .context("Pose layer order overflow")?
                .to_le_bytes(),
        )?;
        put(&mut p.0, row + 22, &envelope.bytes::<2>(layer + 22)?)?;
        append(&mut p.0, row, 0x8080372e, &[0; 16], 16)?;
        let choice = p.array(row, 16, Some(0x8080372e))?[0];
        let mut record = envelope.bytes::<64>(option)?;
        record[..32].fill(0);
        record[56..60].copy_from_slice(
            &(0x10000u32 | u32::try_from(first_parameter + spec.parameter)?).to_le_bytes(),
        );
        if let Some(mode) = spec.mode {
            // 0104C9D0 reads the time source at +0x3F. Mode 3 adds the parameter times the
            // frame time and wraps over the duration at +0x34. Stock integrating options keep
            // the unused mode 1 rate at one.
            record[63] = mode;
            if mode != 2 {
                record[48..52].copy_from_slice(&1f32.to_le_bytes());
            }
        }
        append(&mut p.0, choice, 0x80803730, &record, 64)?;
        let selection = p.array(choice, 64, Some(0x80803730))?[0];
        let weighted = envelope.array(option, 12, Some(0x80803737))?;
        ensure!(
            weighted.len() == 1 && envelope.f32(weighted[0] + 4)? == 1.,
            "Pose clip weight differs"
        );
        let mut weight = envelope.bytes::<12>(weighted[0])?;
        put(&mut weight, 0, &u32::try_from(*clip)?.to_le_bytes())?;
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
        for index in 0..specs.len() {
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

fn allocation(
    p: &mut Payload,
    inputs: usize,
    parameters: usize,
    layers: usize,
    added: [usize; 3],
) -> Result<()> {
    let mut found = BTreeSet::new();
    let mut pending = p.array(0x20, 40, Some(0x80808852))?;
    let mut seen = BTreeSet::new();
    while let Some(row) = pending.pop() {
        ensure!(
            seen.insert(row) && seen.len() < 1024,
            "Pose allocation repeats a node"
        );
        for (key, class, old, new) in [
            (0x86374c3c, 0x80809788, inputs, inputs + added[0]),
            (0xcd06e535, 0x8080000f, parameters, parameters + added[1]),
            (0x6b6ba194, 0x80800090, parameters, parameters + added[1]),
            (0xe5b438c2, 0x8080371a, layers, layers + added[2]),
        ] {
            if p.u32(row)? == key && p.u32(row + 16)? == class && p.u32(row + 20)? as usize == old {
                ensure!(found.insert(key), "Pose allocation is ambiguous");
                put(&mut p.0, row + 20, &u32::try_from(new)?.to_le_bytes())?;
            }
        }
        if p.u64(row + 8)? != 0 {
            pending.push(p.pointer(row + 8)?);
        }
        pending.extend(p.array(row + 24, 40, Some(0x80808852))?);
    }
    ensure!(found.len() == 4, "Pose allocation is incomplete");
    Ok(())
}

impl Assembly {
    fn wheel_bank(
        &mut self,
        native: &mut Reader,
        graph: &mut Graph,
        motion: &motion::Motion,
        suspension: &[contact::Suspension],
    ) -> Result<Vec<Channel>> {
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
        // Each added ground sensor reports its suspension phase through its own numeric provider.
        let physical = self.owner(0x80803d4c)?;
        for sensor in suspension {
            sources.push((
                links::Object {
                    owner: physical,
                    class: 0x80809ae1,
                    offset: sensor.provider as u64,
                },
                0,
            ));
        }
        self.channel_bridge(native, graph, BANK_SYMBOL, &sources)
    }

    pub fn wheels(
        &mut self,
        native: &mut Reader,
        graph: &mut Graph,
        scene: &Scene,
        bones: &Value,
        motion: &motion::Motion,
        suspension: &[contact::Suspension],
    ) -> Result<Value> {
        let channels = self.wheel_bank(native, graph, motion, suspension)?;
        let parameters = NAMES
            .iter()
            .map(|&name| name.to_owned())
            .chain(
                suspension
                    .iter()
                    .map(|sensor| format!("source/{}", sensor.label)),
            )
            .collect::<Vec<_>>();
        ensure!(
            channels.len() == parameters.len(),
            "Wheel bank channel count differs"
        );
        let inputs = parameters
            .into_iter()
            .zip(channels)
            .map(|(parameter, channel)| PoseInput { parameter, channel })
            .collect::<Vec<_>>();
        let steering = inputs.len();
        let programs = vec![PoseProgram {
            parameter: "source/steering_phase",
            references: vec![2],
            program: Program::ramp([-1., 1.], [0., 1.])?,
        }];
        // A suspension overlay plays its sensor's phase directly and yields the bones the spin
        // and steering overlays already play, such as the Mongoose's rear wheels, so those keep
        // whatever the native layer blend does with one bone in two layers.
        let layers = (0..NAMES.len())
            .map(|index| PoseLayer {
                clip: NAMES[index].to_owned(),
                parameter: if index == 2 { steering } else { index },
                mode: None,
                yields: false,
            })
            .chain(
                suspension
                    .iter()
                    .enumerate()
                    .map(|(index, sensor)| PoseLayer {
                        clip: sensor.clip.clone(),
                        parameter: NAMES.len() + index,
                        mode: None,
                        yields: true,
                    }),
            )
            .collect();
        let mut report = self.pose_layers(
            native,
            graph,
            scene,
            bones,
            "vehicle-wheel",
            inputs,
            programs,
            layers,
        )?;
        for (key, value) in [
            ("front_phase_method", json!(10)),
            ("rear_phase_method", json!(11)),
            ("steering_method", json!(9)),
            ("input_scheduler", json!("Primary vehicle bank")),
            (
                "suspension",
                json!(
                    suspension
                        .iter()
                        .map(|sensor| json!({"label":sensor.label,"clip":sensor.clip,"provider":sensor.provider}))
                        .collect::<Vec<_>>()
                ),
            ),
            (
                "limits",
                json!([
                    "These source rotation overlays follow native car travel and steering.",
                    "Suspension overlays follow their ground sensors. Bones the spin and steering overlays play stay with those layers."
                ]),
            ),
        ] {
            report[key] = value;
        }
        Ok(report)
    }

    /// Append source overlay layers to the carrier's pose controller, read from bank channels.
    /// Each source overlay becomes a native clip in the vehicle bank. The layers use the checked
    /// native direct-phase envelope independently of the carrier, whose original layers and bank
    /// ordinals stay intact.
    #[expect(
        clippy::too_many_arguments,
        reason = "Pose layers join source motion, bank channels and the carrier controller"
    )]
    pub(super) fn pose_layers(
        &mut self,
        native: &mut Reader,
        graph: &mut Graph,
        scene: &Scene,
        bones: &Value,
        prefix: &str,
        inputs: Vec<PoseInput>,
        programs: Vec<PoseProgram>,
        specs: Vec<PoseLayer>,
    ) -> Result<Value> {
        ensure!(
            !inputs.is_empty() && !specs.is_empty(),
            "Pose layers need an input and a clip"
        );
        let added_parameters = inputs.len() + programs.len();
        ensure!(
            specs.iter().all(|spec| spec.parameter < added_parameters),
            "Pose layer parameter is outside its appended parameters"
        );
        let first = usize::try_from(
            bones["source_bone_first"]
                .as_u64()
                .context("Pose source bones were not appended")?,
        )?;
        let total = bones["bones"].as_array().context("Pose skeleton")?.len();
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
        let envelope = native.tag(ENVELOPE, Some(0x808034eb))?;
        let (layer, option) = phase_envelope(&envelope)?;
        let weighted = envelope.array(option, 12, Some(0x80803737))?;
        ensure!(
            weighted.len() == 1,
            "Pose phase envelope has multiple clips"
        );
        let original_bank = native.tag(0x80c0c48f, Some(0x808036f6))?;
        let bank_rows = original_bank.array(8, 4, Some(0x80808f48))?;
        let clip_tag = original_bank.u32(
            *bank_rows
                .get(envelope.u32(weighted[0])? as usize)
                .context("Pose phase clip ordinal")?,
        )?;
        let template = native.tag(clip_tag, Some(0x80808f49))?;
        let controls = graph.read("motion-controls")?;
        let frames = usize::from(template.u16(0x13c)?);
        // Earlier layers claim the channels their overlays move, rotation and translation apart.
        // These are mode two layers, which replace only the channels their clips map, so a layer
        // that yields maps only the moving channels no earlier layer claims. The electric cart's
        // suspension then moves its wheel bones while the spin overlay turns them. A layer left
        // with no channel, or whose overlay moves none, is dropped and reported.
        let (kept, dropped) = moving_layers(scene, &graph.root, (first, frames), specs)?;
        let (specs, tracks): (Vec<_>, Vec<_>) = kept
            .into_iter()
            .map(|(spec, tracks, yielded)| ((spec, yielded), tracks))
            .unzip();
        let (specs, yields): (Vec<PoseLayer>, Vec<Value>) = specs.into_iter().unzip();
        let mut bank = graph.read("motion-bank")?;
        let old_rows = bank.array(8, 4, Some(0x80808f48))?;
        let clip_indices = (0..specs.len())
            .map(|index| old_rows.len() + index)
            .collect::<Vec<_>>();
        let mut bank_data = bytes(&bank, &old_rows, 4);
        bank_data.extend(std::iter::repeat_n(0xff, 4 * specs.len()));
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
        for (index, ((spec, (tracks, channels)), yielded)) in
            specs.iter().zip(&tracks).zip(&yields).enumerate()
        {
            let clip = animation::clip::mapped(&template, total, &controls, tracks, channels)?;
            let symbol = format!("{prefix}-clip-{index}");
            graph.add(&symbol, clip_tag, &clip.0, None, Vec::new())?;
            bank_patches.push(json!({"offset":new_rows[clip_indices[index]],"symbol":symbol}));
            clip_report.push(json!({"source":spec.clip,"symbol":symbol,"bank_ordinal":clip_indices[index],
                "parameter":spec.parameter,"yielded_slots":yielded,"mode":spec.mode.unwrap_or(envelope.0[option + 63]),
                "duration_seconds":envelope.f32(option + 0x34)?,"slots":tracks.keys().collect::<Vec<_>>() }));
        }
        graph.replace("motion-bank", &bank.0, bank_patches)?;
        let old_inputs = original.array(instance + 0x30, 96, Some(0x80809788))?;
        let old_definitions = original.array(definition + 0xa8, 40, Some(0x80809789))?;
        ensure!(
            !old_inputs.is_empty() && old_inputs.len() == old_definitions.len(),
            "Pose input counts differ"
        );
        let parameter_tag = original.u32(definition + 0x110)?;
        let parameter_source = native.tag(parameter_tag, None)?;
        let program_count = programs.len();
        let (parameters, first_parameter) =
            parameters(&parameter_source, old_inputs.len(), &inputs, programs)?;
        let layer_specs = specs
            .iter()
            .zip(&clip_indices)
            .map(|(spec, &clip)| (spec, clip))
            .collect::<Vec<_>>();
        let mut layer_payload = layers(
            &layer_source,
            &envelope,
            first_parameter,
            &layer_specs,
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
                    .context("Pose bank clip")?;
                let symbol = patch["symbol"].as_str().context("Pose bank clip symbol")?;
                animation::clip::size(&graph.read(symbol)?)
            })
            .collect::<Result<Vec<_>>>()?;
        animation::layer::clip_sizes(&mut layer_payload, &sizes)?;
        let parameter_symbol = format!("{prefix}-parameters");
        let layer_symbol = format!("{prefix}-layers");
        graph.add(
            &parameter_symbol,
            parameter_tag,
            &parameters.0,
            None,
            Vec::new(),
        )?;
        graph.add(&layer_symbol, layer_tag, &layer_payload.0, None, Vec::new())?;
        self.resource(owner, definition + 0x110, parameter_symbol)?;
        self.resource(owner, definition + 0x11c, layer_symbol)?;
        let p = self.payloads.get_mut(&owner).unwrap();
        let mut input_data = bytes(&original, &old_inputs, 96);
        let mut definition_data = bytes(&original, &old_definitions, 40);
        for _ in 0..inputs.len() {
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
        let instance_inputs = p.array(instance + 0x30, 96, Some(0x80809788))?;
        let definitions = p.array(definition + 0xa8, 40, Some(0x80809789))?;
        for (index, (&input, &data)) in instance_inputs.iter().zip(&definitions).enumerate() {
            put(&mut p.0, input + 8, &(data as u64).to_le_bytes())?;
            relative(p, input + 16, instance + 0x30)?;
            put(&mut p.0, data + 8, &(input as u64).to_le_bytes())?;
            if index >= old_inputs.len() {
                put(
                    &mut p.0,
                    data + 32,
                    &inputs[index - old_inputs.len()].channel.name.to_le_bytes(),
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
            .context("Pose controller ordinal")?;
        for (index, input) in inputs.iter().enumerate() {
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
                provider: input.channel.provider.clone(),
                channel: input.channel.index,
                flags: 0,
            });
        }
        ensure!(
            original.u32(definition + 0x144)? as usize == first_parameter
                && original.u32(definition + 0x148)? as usize == first_parameter,
            "Pose parameter definition counts differ"
        );
        for (field, stride, class) in [(0x1f0, 4, 0x8080000f), (0x200, 16, 0x80800090)] {
            let old = original.array(instance + field, stride, Some(class))?;
            ensure!(
                old.len() == first_parameter,
                "Pose parameter state count differs"
            );
            let mut data = bytes(&original, &old, stride);
            data.resize(data.len() + added_parameters * stride, 0);
            append(&mut p.0, instance + field, u64::from(class), &data, stride)?;
        }
        ensure!(
            first_parameter.div_ceil(32) == (first_parameter + added_parameters).div_ceil(32),
            "Pose parameters require a larger dirty mask"
        );
        for field in [0x144, 0x148] {
            put(
                &mut p.0,
                definition + field,
                &u32::try_from(first_parameter + added_parameters)?.to_le_bytes(),
            )?;
        }
        let states = original.array(instance + 0x500, 120, Some(0x8080371a))?;
        ensure!(
            states.len() == layers_before.len(),
            "Pose layer state count differs"
        );
        let mut state_data = bytes(&original, &states, 120);
        state_data.resize(state_data.len() + specs.len() * 120, 0);
        append(&mut p.0, instance + 0x500, 0x8080371a, &state_data, 120)?;
        put(
            &mut p.0,
            definition + 0x180,
            &u32::try_from(states.len() + specs.len())?.to_le_bytes(),
        )?;
        let allocation_tag = original.u32(0x44)?;
        let mut allocation_payload = (*native.tag(allocation_tag, Some(0x80809bbb))?).clone();
        allocation(
            &mut allocation_payload,
            old_inputs.len(),
            first_parameter,
            states.len(),
            [inputs.len(), added_parameters, specs.len()],
        )?;
        let allocation_symbol = format!("{prefix}-pose-allocation");
        graph.add(
            &allocation_symbol,
            allocation_tag,
            &allocation_payload.0,
            None,
            Vec::new(),
        )?;
        self.allocations.insert(owner, allocation_symbol);
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
        let output = output::build(self, native, graph, states.len(), specs.len(), prefix)?;
        Ok(
            json!({"clips":clip_report,"dropped_layers":dropped,"parameters":first_parameter + added_parameters,
            "programs":program_count,"layers":states.len() + specs.len(),"seal":seal,"output":output}),
        )
    }
}

type LayerTracks = (BTreeMap<usize, Vec<[f32; 8]>>, BTreeMap<usize, [bool; 2]>);
type MovingLayer = (PoseLayer, LayerTracks, Value);

fn moving_layers(
    scene: &Scene,
    root: &std::path::Path,
    (first, frames): (usize, usize),
    specs: Vec<PoseLayer>,
) -> Result<(Vec<MovingLayer>, Vec<Value>)> {
    let mut claimed: [BTreeSet<usize>; 2] = Default::default();
    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    for spec in specs {
        let mut tracks = if spec.yields {
            source_motion::overlay(scene, root, &spec.clip, first, frames)?
        } else {
            source_motion::rotations(scene, root, &spec.clip, first, frames)?
        };
        let before = tracks.len();
        let mut channels = BTreeMap::new();
        if spec.yields {
            tracks.retain(|slot, samples| {
                let mapped = std::array::from_fn(|channel| {
                    moves(samples, channel) && !claimed[channel].contains(slot)
                });
                channels.insert(*slot, mapped);
                mapped.iter().any(|&channel| channel)
            });
            channels.retain(|slot, _| tracks.contains_key(slot));
        }
        if tracks.is_empty() {
            dropped.push(json!({"source":spec.clip,"moving_slots":before}));
            continue;
        }
        for (slot, samples) in &tracks {
            let mapped = channels.get(slot).copied().unwrap_or([true; 2]);
            for channel in 0..2 {
                if mapped[channel] && moves(samples, channel) {
                    claimed[channel].insert(*slot);
                }
            }
        }
        let partial = channels
            .iter()
            .filter(|(_, mapped)| !mapped.iter().all(|&channel| channel))
            .map(|(slot, mapped)| json!({"slot":slot,"rotation":mapped[0],"translation":mapped[1]}))
            .collect::<Vec<_>>();
        let yielded = json!({"slots":before - tracks.len(),"partial":partial});
        kept.push((spec, (tracks, channels), yielded));
    }
    ensure!(!kept.is_empty(), "Pose layers have no moving clip");
    Ok((kept, dropped))
}
