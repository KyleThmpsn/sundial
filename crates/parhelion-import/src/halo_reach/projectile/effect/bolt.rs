//! The Fallen arc bolt's flight effects, recolored, as the presentation of additive source
//! contrails.
//!
//! The Fallen arc projectile `80C1BF02` carries flight owner `80FF0231`. Its root starts five
//! particle events and a light at spawn: a core streak `80FF021F` (geometry 5), a repeated ribbon
//! `80FF0223` (geometry 3) whose strands twist about the path, a single ribbon `80FF0227`
//! (geometry 2), two glow sprites and a light. The streak and both ribbons play in the carrier's
//! own effect flow, as `host` describes. One sprite takes its color from a palette texture, the
//! other from a particle program, and the light is teal, so they stay out.
//!
//! Private copies recolor the three systems from the source contrail's color stops at the donor's
//! luminance. The streak's pixel program `80C1BF08` and the repeated ribbon's `80FF0238` each hold
//! red and green in one immediate beside a blue the compiled program keeps at one, so the copy
//! writes red and green over blue and scales the intensity constants by blue: constant 7 X, which
//! the streak's section 5 writes to row 2 W, and constants 10 and 11 X, which the ribbon's section
//! 4 interpolates into row 4 X. The single ribbon takes an age gradient between definition
//! constants 12 and 13, written to row 3 XYZ. Systems, definitions and materials are private
//! copies, and each material carries private programs, textures and samplers with their data
//! entries, since a container that loads only a header leaves its data unloaded.
//!
//! The stock streak holds one particle for a second. Its copy is re-emitted as the flight beams in
//! `volume` are, so a streak ends with the `attached` gate instead of staying on a stuck projectile.
use super::*;
use crate::halo_reach::native::material::bytecode;
use crate::tiger::sequence;

const OWNER: u32 = 0x80ff_0231;
/// FNV-1 of `root` and `world_space`, the bolt's two locator names.
const ROOT: u32 = 0x7bd0_5f71;
const WORLD_SPACE: u32 = 0x9199_59c0;
/// FNV-1 of `attached`, the projectile channel that reaches one once the projectile sticks.
const ATTACHED: u32 = 0xfce5_65b9;
const SYSTEM_CLASS: u32 = 0x8080_6e28;
const MATERIAL_CLASS: u32 = 0x8080_71e8;
const STAGES: [usize; 6] = [0x48, 0xe8, 0x188, 0x228, 0x2c8, 0x368];

/// A system tinted through its pixel program's immediate, and its intensity constants.
struct Literal {
    system: u32,
    name: &'static str,
    tint: [f32; 4],
    section: usize,
    program: &'static [u8],
    constants: &'static [usize],
    /// A beam holding one particle for its whole life, re-emitted by `reemit`.
    beam: bool,
}

const LITERALS: [Literal; 2] = [
    Literal {
        system: 0x80ff_021f,
        name: "streak",
        tint: [
            0.,
            0.,
            f32::from_bits(0x3da3_c105),
            f32::from_bits(0x3f5b_f62b),
        ],
        section: 5,
        program: &[0x34, 0x07, 0x3e, 1, 0, 7],
        constants: &[7],
        beam: true,
    },
    Literal {
        system: 0x80ff_0223,
        name: "sway",
        tint: [
            0.,
            0.,
            f32::from_bits(0x3ea5_3ae7),
            f32::from_bits(0x3f38_3fd5),
        ],
        section: 4,
        program: &[0x35, 0x0a, 0x3f, 1, 4, 4],
        constants: &[10, 11],
        beam: false,
    },
];
/// The end of the streak's section 5: the intensity product, then its store into row 2 W.
const INTENSITY_STORE: [u8; 5] = [0x03, 0x3f, 1, 2, 7];
/// The single ribbon and its color program: normalized age through a curve between constants
/// 12 and 13 into row 3 XYZ.
const STRAND: u32 = 0x80ff_0227;
const STRAND_COLOR: [u8; 19] = [
    0x3e, 1, 1, 7, 0x23, 0x22, 0, 0x34, 0x0b, 0x0f, 0x23, 0x22, 0, 0x35, 0x0c, 0x3f, 1, 3, 1,
];
const STRAND_STOPS: [usize; 2] = [12, 13];

fn luminance(rgb: [f32; 3]) -> f32 {
    0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]
}

/// `rgb`'s hue and saturation at `reference`'s luminance.
pub(super) fn matched(rgb: [f32; 3], reference: [f32; 3]) -> Result<[f32; 3]> {
    let own = luminance(rgb);
    ensure!(own > 1e-3, "Source contrail color is black");
    Ok(rgb.map(|v| v * luminance(reference) / own))
}

/// Bytecode section `index` of particle definition `d`.
fn section(d: &Payload, index: usize) -> Result<Vec<u8>> {
    let code = d.array(0x50, 1, Some(0x8080_0009))?;
    let lengths = (0..8)
        .map(|i| Ok(usize::from(d.u16(0x70 + i * 2)?)))
        .collect::<Result<Vec<_>>>()?;
    let start = lengths[..index].iter().sum::<usize>();
    let rows = code
        .get(start..start + lengths[index])
        .context("Particle definition section extent")?;
    rows.iter().map(|&at| d.u8(at)).collect()
}

fn set_constant(d: &mut Payload, index: usize, rgb: [f32; 3]) -> Result<()> {
    let rows = d.array(0x60, 16, Some(0x8080_0090))?;
    let row = *rows.get(index).context("Particle definition constant")?;
    for (lane, value) in rgb.into_iter().enumerate() {
        ensure!(value.is_finite(), "Bolt color is not finite");
        put(&mut d.0, row + lane * 4, &value.to_le_bytes())?;
    }
    Ok(())
}

fn constant(d: &Payload, index: usize) -> Result<[f32; 4]> {
    let rows = d.array(0x60, 16, Some(0x8080_0090))?;
    let row = *rows.get(index).context("Particle definition constant")?;
    Ok([
        d.f32(row)?,
        d.f32(row + 4)?,
        d.f32(row + 8)?,
        d.f32(row + 12)?,
    ])
}

/// The streak beam `d` as a re-emitted flight beam (see `volume`): new life (route 5) and rate
/// (route 10) defaults, each a lane no other route reads, the life ceiling, and section 5
/// multiplying the intensity it stores in row 2 W by the age weight. Route 0 publishes the
/// normalized age to row 1 W, which each beam record copies.
fn reemit(d: &mut Payload) -> Result<Value> {
    let route = |d: &Payload, index: usize| -> Result<(u8, u8)> {
        Ok((d.u8(0x80 + index * 2)?, d.u8(0x81 + index * 2)?))
    };
    ensure!(
        d.u8(0x14b)? == 5 && route(d, 0)? == (1, 7),
        "Fallen bolt streak is not a beam with a published age"
    );
    let defaults = d.array(8, 16, Some(0x8080_0090))?;
    for (index, value) in [
        (5, volume::BEAM_LIFE),
        (10, volume::BEAM_OVERLAP / volume::BEAM_LIFE),
    ] {
        let (bank, scalar) = route(d, index)?;
        ensure!(
            bank == 6,
            "Fallen bolt streak route {index} does not read a default"
        );
        let readers = (0..56)
            .filter(|&other| route(d, other).ok() == Some((6, scalar)))
            .count();
        ensure!(
            readers == 1,
            "Fallen bolt streak route {index} shares its default"
        );
        let row = *defaults
            .get(usize::from(scalar) / 4)
            .context("Fallen bolt streak default")?;
        put(
            &mut d.0,
            row + usize::from(scalar % 4) * 4,
            &value.to_le_bytes(),
        )?;
    }
    put(&mut d.0, 0x120, &(volume::BEAM_LIFE + 0.05).to_le_bytes())?;

    let mut constants = d
        .array(0x60, 16, Some(0x8080_0090))?
        .into_iter()
        .map(|row| {
            d.0.get(row..row + 16)
                .map(<[u8]>::to_vec)
                .context("Particle definition constant")
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        constants.len() + 2 <= 256,
        "Particle constant table exceeds its byte index"
    );
    let first = u8::try_from(constants.len())?;
    let step = 1. / volume::BEAM_OVERLAP;
    for value in [-step, step] {
        constants.push([value; 4].iter().flat_map(|v| v.to_le_bytes()).collect());
    }
    let mut sections = (0..8)
        .map(|index| section(d, index))
        .collect::<Result<Vec<_>>>()?;
    let intensity = &mut sections[5];
    ensure!(
        intensity.ends_with(&INTENSITY_STORE),
        "Fallen bolt streak intensity store differs"
    );
    // Before the store: age (row 1 W), opcode 1F's cosine of turns, then cos * -step + step.
    let store = intensity.split_off(intensity.len() - 4);
    intensity.extend([
        0x3e,
        1,
        1,
        7,
        0x1f,
        0x34,
        first,
        0x34,
        first + 1,
        0x12,
        0x03,
    ]);
    intensity.extend(store);
    let code = sections.concat();
    ensure!(
        code.len() <= usize::from(u16::MAX),
        "Particle program exceeds native cumulative phase offsets"
    );
    crate::presentation::append(&mut d.0, 0x50, 0x8080_0009, &code, 1)?;
    crate::presentation::append(&mut d.0, 0x60, 0x8080_0090, &constants.concat(), 16)?;
    for (index, section) in sections.iter().enumerate() {
        put(
            &mut d.0,
            0x70 + index * 2,
            &u16::try_from(section.len())?.to_le_bytes(),
        )?;
    }
    Ok(json!({"particle_life_seconds":volume::BEAM_LIFE,
        "overlapping_particles":volume::BEAM_OVERLAP}))
}

/// A private header and the data entry its package reference names. `data` replaces the stock
/// data when given.
pub(super) fn paired(
    native: &mut Reader,
    graph: &mut Graph,
    symbol: &str,
    header: u32,
    data: Option<&[u8]>,
) -> Result<String> {
    if !graph.contains(symbol) {
        let source = native.reference(header)?;
        let bytes = match data {
            Some(bytes) => bytes.to_vec(),
            None => native.tag(source, None)?.0.clone(),
        };
        let raw = format!("{symbol}-data");
        graph.add(
            symbol,
            header,
            &native.tag(header, None)?.0.clone(),
            Some(&raw),
            vec![],
        )?;
        graph.add(&raw, source, &bytes, Some(symbol), vec![])?;
    }
    Ok(symbol.into())
}

/// A private material whose programs, textures, samplers and external constant buffers are
/// private copies. `pixel` names an already placed pixel program.
pub(in crate::halo_reach) fn material(
    native: &mut Reader,
    graph: &mut Graph,
    prefix: &str,
    tag: u32,
    pixel: Option<String>,
) -> Result<String> {
    let symbol = format!("{prefix}-material");
    let mut bytes = (*native.tag(tag, Some(MATERIAL_CLASS))?).clone();
    let mut patches = Vec::new();
    for stage in STAGES {
        let program = bytes.u32(stage)?;
        if program != u32::MAX {
            let name = match (&pixel, stage) {
                (Some(pixel), 0x2c8) => pixel.clone(),
                _ => paired(
                    native,
                    graph,
                    &format!("ribbon-program-{program:08X}"),
                    program,
                    None,
                )?,
            };
            put(&mut bytes.0, stage, &u32::MAX.to_le_bytes())?;
            patches.push(json!({"offset":stage,"symbol":name}));
        }
        for row in bytes.array(stage + 8, 8, Some(0x8080_7211))? {
            let texture = bytes.u32(row + 4)?;
            let name = paired(
                native,
                graph,
                &format!("ribbon-texture-{texture:08X}"),
                texture,
                None,
            )?;
            put(&mut bytes.0, row + 4, &u32::MAX.to_le_bytes())?;
            patches.push(json!({"offset":row + 4,"symbol":name}));
        }
        for row in bytes.array(stage + 0x40, 16, None)? {
            let sampler = native.ref64(&bytes, row)?;
            let name = paired(
                native,
                graph,
                &format!("ribbon-sampler-{sampler:08X}"),
                sampler,
                None,
            )?;
            put(&mut bytes.0, row, &u32::MAX.to_le_bytes())?;
            put(&mut bytes.0, row + 4, &1u32.to_le_bytes())?;
            put(&mut bytes.0, row + 8, &0u64.to_le_bytes())?;
            patches.push(json!({"offset":row,"symbol":name}));
        }
        // A stage's external constant buffer is a header naming its data entry, as a texture is.
        let buffer = bytes.u32(stage + 0x84)?;
        if !matches!(buffer, 0 | u32::MAX | 0x811c_9dc5) {
            let name = paired(
                native,
                graph,
                &format!("ribbon-buffer-{buffer:08X}"),
                buffer,
                None,
            )?;
            put(&mut bytes.0, stage + 0x84, &u32::MAX.to_le_bytes())?;
            patches.push(json!({"offset":stage + 0x84,"symbol":name}));
        }
    }
    graph.add(&symbol, tag, &bytes.0, None, patches)?;
    Ok(symbol)
}

/// A private copy of particle system `tag` naming the private `definition` and `material`. A
/// system with a computational stage is refused.
pub(in crate::halo_reach) fn system(
    graph: &mut Graph,
    native: &mut Reader,
    prefix: &str,
    tag: u32,
    definition: String,
    material: String,
) -> Result<String> {
    let symbol = format!("{prefix}-system");
    let mut bytes = (*native.tag(tag, Some(SYSTEM_CLASS))?).clone();
    for at in [4, 8, 0x10] {
        ensure!(
            bytes.u32(at)? == u32::MAX,
            "Bolt system has a computational stage"
        );
    }
    put(&mut bytes.0, 0, &u32::MAX.to_le_bytes())?;
    put(&mut bytes.0, 0x14, &u32::MAX.to_le_bytes())?;
    graph.add(
        &symbol,
        tag,
        &bytes.0,
        None,
        vec![
            json!({"offset":0,"symbol":definition}),
            json!({"offset":0x14,"symbol":material}),
        ],
    )?;
    Ok(symbol)
}

pub(super) struct Bolt {
    pub allocation: (u32, Payload),
    /// Each kept event's system field and the private system it names.
    pub patches: Vec<(usize, String)>,
    pub report: Value,
}

/// The flow event that starts `system` alone.
fn event_of(owner: &Payload, system: u32) -> Result<usize> {
    let definition = owner.pointer(24)?;
    let mut found = Vec::new();
    for row in owner.array(definition + 0x168, 24, Some(0x8080_93e6))? {
        let event = owner.pointer(row + 16)?;
        if owner.u32(event - 4)? != 0x8080_6cc6 {
            continue;
        }
        let systems = owner.array(event + 0x40, 24, Some(0x8080_6cc8))?;
        if systems.len() == 1 && owner.u32(systems[0] + 16)? == system {
            found.push(event);
        }
    }
    let [event] = found[..] else {
        anyhow::bail!("Fallen bolt flow starts {system:08X} in no event or several");
    };
    Ok(event)
}

fn literal(
    native: &mut Reader,
    graph: &mut Graph,
    prefix: &str,
    spec: &Literal,
    color: [f32; 3],
) -> Result<(String, Value)> {
    // The program keeps blue at one, so red and green are written relative to blue and blue
    // moves into the intensity constants.
    let tint = matched(color, [spec.tint[2], spec.tint[3], 1.])?;
    ensure!(
        tint[2] > 1e-3,
        "Source contrail color has no blue for the bolt's tint"
    );
    let name = format!("{prefix}-{}", spec.name);
    let definition_tag = native.tag(spec.system, Some(SYSTEM_CLASS))?.u32(0)?;
    let mut definition = (*native.tag(definition_tag, Some(0x8080_6e2c))?).clone();
    ensure!(
        section(&definition, spec.section)?
            .windows(spec.program.len())
            .filter(|window| *window == spec.program)
            .count()
            == 1,
        "Fallen bolt {} intensity program differs",
        spec.name
    );
    for &index in spec.constants {
        let [x, y, z, _] = constant(&definition, index)?;
        set_constant(&mut definition, index, [x * tint[2], y, z])?;
    }
    let beam = if spec.beam {
        Some(reemit(&mut definition)?)
    } else {
        None
    };
    graph.add(
        &format!("{name}-definition"),
        definition_tag,
        &definition.0,
        None,
        vec![],
    )?;
    let material_tag = native.tag(spec.system, None)?.u32(0x14)?;
    let pixel_tag = native.tag(material_tag, Some(MATERIAL_CLASS))?.u32(0x2c8)?;
    let code = native.tag(native.reference(pixel_tag)?, None)?;
    let edited = crate::tiger::shader::dxbc::replace_immediate(
        &code.0,
        spec.tint,
        [0., 0., tint[0] / tint[2], tint[1] / tint[2]],
    )?;
    let pixel = format!("{name}-pixel");
    bytecode(
        native,
        graph,
        &pixel,
        pixel_tag,
        &edited,
        &format!(
            "// Native program {pixel_tag:08X} with its red and green tint immediate replaced."
        ),
    )?;
    let material_symbol = material(native, graph, &name, material_tag, Some(pixel))?;
    let symbol = system(
        graph,
        native,
        &name,
        spec.system,
        format!("{name}-definition"),
        material_symbol,
    )?;
    Ok((
        symbol,
        json!({"donor_system":format!("{:08X}", spec.system),"written_tint":tint,"beam":beam}),
    ))
}

fn strand(
    native: &mut Reader,
    graph: &mut Graph,
    prefix: &str,
    [first, last]: [[f32; 3]; 2],
) -> Result<(String, Value)> {
    let name = format!("{prefix}-strand");
    let definition_tag = native.tag(STRAND, Some(SYSTEM_CLASS))?.u32(0)?;
    let mut definition = (*native.tag(definition_tag, Some(0x8080_6e2c))?).clone();
    ensure!(
        section(&definition, 2)? == STRAND_COLOR,
        "Fallen bolt strand color program differs"
    );
    let mut written = Vec::new();
    for (index, stop) in STRAND_STOPS.into_iter().zip([first, last]) {
        let [r, g, b, _] = constant(&definition, index)?;
        let color = matched(stop, [r, g, b])?;
        set_constant(&mut definition, index, color)?;
        written.push(color);
    }
    graph.add(
        &format!("{name}-definition"),
        definition_tag,
        &definition.0,
        None,
        vec![],
    )?;
    let material_tag = native.tag(STRAND, None)?.u32(0x14)?;
    let material_symbol = material(native, graph, &name, material_tag, None)?;
    let symbol = system(
        graph,
        native,
        &name,
        STRAND,
        format!("{name}-definition"),
        material_symbol,
    )?;
    Ok((
        symbol,
        json!({"donor_system":format!("{STRAND:08X}"),"written_stops":written}),
    ))
}

/// The Fallen bolt's streak and ribbons, recolored to the source contrail, as the only events
/// `host`, the carrier's own effect flow, starts. `marker` names the host's emitter marker.
///
/// The bolt binds its systems to kind 0 `root` as emitter and, for the ribbons, kind 1
/// `world_space` as the frame. `root` is the projectile on the Fallen bolt, which has no model, and
/// is not on a modeled host. Native grenade trails bind the carrier's kind 2 marker locator beside
/// kind 1 `world_space`, so each system takes the host's two such locators in the bolt's order. An
/// owner of its own beside the host drew nothing in game. The host's controls are rebuilt with one
/// persistent event per system, so its other events, a grenade carrier's glow and detonation
/// among them, are no longer reachable. `flight` names converted source flight systems that join
/// them, each bound to the marker as its emitter, and a world-space one also to `world_space` as
/// its particles' frame.
///
/// A palettized profile's color stops are multiplied by the palette's mean color, as MCC
/// multiplies the profile color by the palette texel its base map selects.
#[expect(
    clippy::too_many_arguments,
    reason = "The bolt joins a source profile and its decoded palette with the native host"
)]
pub(super) fn host(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    profile: &trail::Profile,
    prefix: &str,
    host: &mut Payload,
    (marker, flight): (u32, &[super::flight::Flight]),
) -> Result<Bolt> {
    let tint = profile.palette_tint(cache, pages)?;
    let stops = profile
        .color
        .stops()?
        .into_iter()
        .map(|stop| [0, 1, 2].map(|i| stop[i] * tint[i]))
        .collect::<Vec<_>>();
    let first = *stops.first().context("Source contrail color stops")?;
    let last = *stops.last().context("Source contrail color stops")?;
    let mean = stops.iter().fold([0.; 3], |sum, stop| {
        [0, 1, 2].map(|i| sum[i] + stop[i] / stops.len() as f32)
    });
    let donor = native.tag(OWNER, Some(0x8080_9c36))?;
    let names = donor.array(donor.pointer(24)? + 0x1d0, 4, Some(0x8080_8614))?;
    ensure!(
        names.len() == 3 && donor.u32(names[0])? == ROOT && donor.u32(names[1])? == WORLD_SPACE,
        "Fallen bolt locators differ"
    );
    // Locator ordinals of each system's event, in the bolt's order.
    let bindings = |system: u32| -> Result<Vec<u8>> {
        let event = event_of(&donor, system)?;
        let row = donor.array(event + 0x40, 24, Some(0x8080_6cc8))?[0];
        donor
            .array(row, 1, Some(0x8080_0009))?
            .into_iter()
            .map(|at| donor.u8(at))
            .collect()
    };
    let mut systems = Vec::new();
    let mut reports = Vec::new();
    for spec in &LITERALS {
        let (symbol, report) = literal(native, graph, prefix, spec, mean)?;
        systems.push((symbol, bindings(spec.system)?));
        reports.push(report);
    }
    let (symbol, report) = strand(native, graph, prefix, [first, last])?;
    systems.push((symbol, bindings(STRAND)?));
    reports.push(report);
    // The bolt's ordinal 0, its emitter, maps to the host marker below and 1 to `world_space`, the
    // frame of a world-space flight system's particles.
    systems.extend(flight.iter().map(|flight| {
        let ordinals = if flight.world { vec![0, 1] } else { vec![0] };
        (flight.symbol.clone(), ordinals)
    }));

    let definition = host.pointer(24)?;
    let locators = host.array(definition + 0x1c0, 16, Some(0x8080_8615))?;
    let host_names = host.array(definition + 0x1d0, 4, Some(0x8080_8614))?;
    ensure!(
        locators.len() == host_names.len(),
        "Host locator names differ from its locators"
    );
    let find = |kind: u8, name: u32, by_row: bool| -> Result<u8> {
        let found = locators
            .iter()
            .zip(&host_names)
            .enumerate()
            .filter(|(_, (row, named))| {
                host.u8(**row).ok() == Some(kind)
                    && host.u32(if by_row { **row + 8 } else { **named }).ok() == Some(name)
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let [index] = found[..] else {
            anyhow::bail!("Host has no single kind {kind} locator named {name:08X}")
        };
        Ok(u8::try_from(index)?)
    };
    // The bolt's ordinal 0 is its emitter and 1 its frame.
    let map = [find(2, marker, true)?, find(1, WORLD_SPACE, false)?];
    let systems = systems
        .into_iter()
        .map(|(symbol, bindings)| {
            let bindings = bindings
                .into_iter()
                .map(|ordinal| {
                    map.get(usize::from(ordinal))
                        .copied()
                        .context("Fallen bolt binds another locator")
                })
                .collect::<Result<Vec<_>>>()?;
            Ok((symbol, bindings))
        })
        .collect::<Result<Vec<_>>>()?;

    let stock = host.clone();
    let allocation_tag = host.u32(0x44)?;
    let mut allocation = (*native.tag(allocation_tag, Some(0x8080_9bbb))?).clone();
    let name = |index: usize| crate::tiger::audio::bank::hash(&format!("fallen/bolt/{index}"));
    // The flight systems play only while the projectile is unattached. A stuck needle otherwise
    // dragged its contrail behind the target it rode. The host flow already reads `attached`
    // (FNV-1 `FCE565B9`), which the projectile's own bank declares, and its stock controls gate
    // the flight loop sounds on `[-FLT_MAX, 0.9999)` of it and the stuck sound on the rest.
    let attached = host
        .array(definition + 0x188, 40, None)?
        .into_iter()
        .map(|row| host.u32(row + 0x20))
        .collect::<Result<Vec<_>>>()?
        .iter()
        .position(|&input| input == ATTACHED)
        .context("Bolt host flow does not read attached")?;
    let root = sequence::Node {
        name: crate::tiger::audio::bank::hash("fallen/bolt/root"),
        timing: sequence::Timing::default(),
        kind: sequence::Kind::Parallel(vec![sequence::Node {
            name: crate::tiger::audio::bank::hash("fallen/bolt/flight"),
            timing: sequence::Timing {
                flags: 0x10,
                ..sequence::Timing::default()
            },
            kind: sequence::Kind::Range {
                input: u32::try_from(attached)?,
                bounds: [-f32::MAX, 0.9999],
                children: (0..systems.len())
                    .map(|index| sequence::Node {
                        name: name(index),
                        // Flag 8 holds the event until its owner retires, as authored trail
                        // events do. Leaving the gate retires it. Its system stops emitting but
                        // draws its live particles to the end of their life, which is why the
                        // beams re-emit short particles.
                        timing: sequence::Timing {
                            duration: [1.; 2],
                            flags: 8,
                            ..sequence::Timing::default()
                        },
                        kind: sequence::Kind::Event(0x8080_6cc6),
                    })
                    .collect(),
            },
        }]),
    };
    let written = sequence::write(host, &mut allocation, &root, std::slice::from_ref(&stock))?;
    let mut declared = Vec::new();
    for (index, (symbol, bindings)) in systems.iter().enumerate() {
        for (offset, symbol) in sequence::particle::write_bound(
            host,
            written.events[&name(index)],
            &[(symbol.clone(), bindings.clone())],
        )? {
            declared.push(json!({"offset":offset,"symbol":symbol}));
        }
    }
    // The rebuilt controls and events are runtime records past the stock schema, which the
    // client's instance copy leaves out without a definition view after them.
    let end = host.0.len();
    crate::tiger::instance::seal_span(host, &mut declared, end)?;
    let patches = declared
        .iter()
        .map(|patch| {
            Ok((
                usize::try_from(patch["offset"].as_u64().context("Bolt patch offset")?)?,
                patch["symbol"]
                    .as_str()
                    .context("Bolt patch symbol")?
                    .to_owned(),
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Bolt {
        allocation: (allocation_tag, allocation),
        patches,
        report: json!({"donor_owner":format!("{OWNER:08X}"),"source_stops":stops,"palette_tint":tint,"systems":reports,
            "bindings":systems.iter().map(|(symbol, bindings)| json!({"system":symbol,"locators":bindings})).collect::<Vec<_>>(),
            "host_marker":format!("{marker:08X}"),
            "omitted":["Glow sprites 80FF022D and 80FF022E","Light"],"gameplay_verified":false,
            "limits":["Width, lifetime, twist and texture stay the donor's. The source profile's width, lifetime, alpha, emission and texture are not carried."]}),
    })
}
