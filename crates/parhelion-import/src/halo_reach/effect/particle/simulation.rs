//! Source properties on native CPU phases. Section three precedes the host transform.
use super::*;
use crate::tiger::particle::Program;

fn register(bank: u8, slot: u8, lane: u8) -> Expr {
    Expr::Register(bank, slot, lane)
}

fn input(program: &mut Program, phase: usize, index: u8) -> Result<()> {
    if index == 12 {
        // 01274710 publishes the game clock at scoped scalar zero. Its
        // 0124FBE0 producer wraps this clock after 28,800 seconds.
        return program.code(phase, &[0x43, 0]);
    }
    let location = match index {
        0 => Some((1, 1, 7)),
        1 => Some((5, 0, 5)),
        2 => Some((0, 3, 4)),
        3 => Some((4, 0, 4)),
        4..=7 => Some((0, 1, index)),
        8..=9 => Some((4, 0, index - 3)),
        10 => Some((0, 0, 6)),
        11 => None,
        16 => Some((4, 0, 7)),
        // MCC 001FCC08 publishes four particle words at state +58 through +64
        // and two system floats at +68 and +6C. Reach retains unused index 21.
        22..=25 => Some((0, 2, index - 18)),
        26..=27 => Some((4, 1, index - 22)),
        _ => anyhow::bail!("Source particle input {index} has no qualified Reach state binding"),
    };
    if let Some((bank, slot, selector)) = location {
        if matches!(phase, 0 | 1 | 7) {
            ensure!(
                matches!(bank, 4 | 5),
                "Global emitter property reads particle state {index}"
            );
        }
        program.code(phase, &[0x3e, bank, slot, selector])
    } else {
        program.constant(phase, [1.; 4])
    }
}

fn write(program: &mut Program, phase: usize, target: (u8, u8, u8), value: Expr) -> Result<()> {
    value.emit(program, phase, &input)?;
    program.code(phase, &[0x3f, target.0, target.1, target.2])
}

fn vector(cache: &Cache, at: usize) -> Result<Expr> {
    let [x, y, z] = cache.floats::<3>(at)?;
    ensure!(
        [x, y, z].into_iter().all(f32::is_finite),
        "Nonfinite particle vector"
    );
    Ok(Expr::Value([x, y, z, 0.]))
}

fn transform(value: Expr, frame: &[f32; 16], translate: bool) -> Expr {
    let mut result = Expr::scalar(0.);
    for axis in 0..3 {
        let mut unit = [0.; 4];
        unit[axis] = 1.;
        let coordinate = value.clone().binary(
            0x0b,
            Expr::Value([frame[axis], frame[axis + 4], frame[axis + 8], 0.]),
        );
        result = result.add(Expr::Value(unit).mul(coordinate));
    }
    if translate {
        result = result.add(Expr::Value([frame[12], frame[13], frame[14], 0.]));
    }
    result
}

fn physics(cache: &Cache, at: usize, metres: f32, limits: &mut Vec<String>) -> Result<[Expr; 3]> {
    let mut parameters = std::array::from_fn(|_| Expr::scalar(0.));
    let mut found = false;
    for row in cache.block(at + 0x1ac, 0x18)? {
        let kind = cache.i16(row)?;
        ensure!(
            kind == 0,
            "Source particle movement kind {kind} requires collision, swarm, wind or turbulence translation"
        );
        ensure!(
            !found && cache.u8(row + 2)? & !1 == 0,
            "Source particle physics controller is duplicated or has unknown flags"
        );
        found = true;
        let rows = cache.block(row + 4, 0x24)?;
        ensure!(
            rows.len() == 3,
            "Source physics parameters are not fully indexed"
        );
        for (index, row) in rows.into_iter().enumerate() {
            ensure!(
                cache.u32(row)? == index as u32,
                "Source physics parameter ordinal differs"
            );
            parameters[index] = Property::read(cache, row + 4)?.formula()?;
        }
    }
    if found {
        // MCC 0019C588 initializes physics gravity to this source-unit value.
        // 0024C7E0 applies parameter zero to gravity, one to linear damping and
        // two to rotational damping. Scenario and console overrides are separate.
        parameters[0] = parameters[0]
            .clone()
            .mul(Expr::scalar(f32::from_bits(0x40857af5) * metres));
        limits.push("Particle gravity uses MCC's default gravity and the source controller multiplier. Scenario gravity zones and runtime overrides are not imported.".into());
    }
    Ok(parameters)
}

fn geometry(
    program: &mut Program,
    cache: &Cache,
    at: usize,
    property: &BTreeMap<&str, Property>,
    metres: f32,
    frame: &[f32; 16],
) -> Result<()> {
    // The original 011E7E00 calls section three before it transforms routes six
    // and seven into the emitter's world frame. This permits source local geometry.
    let scalar = |name| property[name].formula();
    let shape = cache.u8(at + 4)?;
    // MCC 00240F28 uses independent azimuth and polar draws. The authored
    // axis angle controls model orientation, not billboard emission azimuth.
    let axis = register(0, 6, 6);
    let cone = scalar("angle")?.mul(Expr::scalar(1. / 360.));
    let angle = if shape == 4 {
        cone
    } else {
        cone.mul(register(0, 6, 7))
    };
    let radius = scalar("radius")?.mul(Expr::scalar(metres));
    let sine = angle.clone().unary(0x1e);
    let cosine = angle.unary(0x1f);
    let y = sine.clone().mul(axis.clone().unary(0x1e));
    let z = sine.mul(axis.clone().unary(0x1f));
    let offset = vector(cache, at + 0x68)?
        .lerp(vector(cache, at + 0x74)?, scalar("offset")?)
        .mul(Expr::scalar(metres));
    // State row seven temporarily holds the sampled source direction and radius.
    for (lane, value) in [(4, cosine), (5, y), (6, z), (7, radius.clone())] {
        write(program, 3, (0, 7, lane), value)?;
    }
    let cone_direction = register(0, 7, 1);
    // Globe and implode choose one of MCC's 1,026 stored unit directions.
    // A continuous uniform sphere retains the geometry and isotropy, while
    // the discrete source table and random sequence remain a reported limit.
    let x = register(0, 6, 7)
        .mul(Expr::scalar(2.))
        .add(Expr::scalar(-1.));
    let squared = Expr::scalar(1.).binary(2, x.clone().mul(x.clone()));
    let radial = Expr::select(
        squared.clone().binary(0x0a, Expr::scalar(0.)),
        Expr::scalar(0.),
        squared
            .binary(9, Expr::scalar(1e-12))
            .unary(0x25)
            .mul(Expr::scalar(0.5))
            .unary(0x24),
    );
    let sphere = Expr::Value([1., 0., 0., 0.])
        .mul(x)
        .add(
            Expr::Value([0., 1., 0., 0.])
                .mul(radial.clone())
                .mul(axis.clone().unary(0x1e)),
        )
        .add(
            Expr::Value([0., 0., 1., 0.])
                .mul(radial)
                .mul(axis.clone().unary(0x1f)),
        );
    let ring = Expr::Value([0., 1., 0., 0.])
        .mul(axis.clone().unary(0x1e))
        .add(Expr::Value([0., 0., 1., 0.]).mul(axis.unary(0x1f)));
    let axis_scale = vector(cache, at + 0x34)?;
    let position = match shape {
        0 => offset,
        1 | 4 => ring.clone().mul(radius).mul(axis_scale).add(offset),
        2 | 3 | 5 => sphere.clone().mul(radius).mul(axis_scale).add(offset),
        _ => anyhow::bail!("Source particle shape {shape} needs a geometry adapter"),
    };
    let direction = match shape {
        1 => ring,
        2 => sphere,
        3 => sphere.unary(0x1d),
        _ => cone_direction,
    };
    write(program, 3, (1, 2, 1), transform(position, frame, true))?;
    write(
        program,
        3,
        (1, 1, 1),
        transform(
            direction.mul(scalar("velocity")?).mul(Expr::scalar(metres)),
            frame,
            false,
        ),
    )
}

fn routes(program: &mut Program) -> Result<()> {
    // Bank zero is private particle state. Bank one is the eight-row draw record.
    // Bank four is the fixed emitter storage selected by the native phase caller.
    for (route, bank, scalar) in [
        (0, 1, 7),
        (2, 5, 1),
        (5, 0, 1),
        (6, 1, 8),
        (7, 1, 4),
        (8, 2, 12),
        (9, 2, 8),
        (10, 4, 8),
        (16, 4, 9),
        (17, 4, 10),
        (18, 0, 16),
        (19, 6, 0),
        (20, 0, 20),
        (21, 6, 0),
        (22, 0, 23),
        (23, 0, 27),
        (27, 1, 11),
        (38, 0, 2),
        (42, 0, 0),
        (43, 0, 3),
    ] {
        if bank != 6 {
            program.route(route, bank, scalar)?;
        }
    }
    // Native angular update always consumes route 23, even with zero inputs.
    // Keep its state in row six W, separate from the source spin in row six Z.
    // Route 24 initializes it, while 25 and 26 leave native integration neutral.
    for route in [11, 12, 13, 14, 15, 24, 25, 26, 31, 36] {
        if route != 31 {
            program.default_value(route, [0.; 4])?;
        }
    }
    for route in [19, 21] {
        program.default_value(route, [1.; 4])?;
    }
    Ok(())
}

pub(super) fn build(
    cache: &Cache,
    at: usize,
    properties: &BTreeMap<&str, Property>,
    metres: f32,
    frame: &[f32; 16],
    limits: &mut Vec<String>,
) -> Result<Program> {
    ensure!(
        metres.is_finite() && metres > 0.,
        "Invalid particle world-unit scale"
    );
    let mut program = Program::default();
    routes(&mut program)?;
    // Eight stable emitter random values survive both spawn and update phases.
    for scalar in 0..8u8 {
        program.code(7, &[0x3d, 0x3f, 4, scalar / 4, 4 + scalar % 4])?;
    }
    for (name, phase, lane) in [
        ("starting_count", 0, 5),
        ("maximum_count", 0, 6),
        ("rate", 0, 4),
        ("rate", 1, 4),
    ] {
        write(
            &mut program,
            phase,
            (4, 2, lane),
            properties[name].formula()?,
        )?;
    }
    // A particle's correlation draws are stable across all of its properties.
    for slot in 1..=3 {
        for selector in 4..8 {
            program.code(6, &[0x3d, 0x3f, 0, slot, selector])?;
        }
    }
    for selector in 6..8 {
        program.code(6, &[0x3d, 0x3f, 0, 6, selector])?;
    }
    write(&mut program, 6, (1, 1, 7), Expr::scalar(0.))?;
    write(&mut program, 2, (0, 0, 5), properties["life"].formula()?)?;
    for (lane, name) in [(4, "rotation_rate"), (5, "animation_rate")] {
        write(&mut program, 2, (0, 6, lane), properties[name].formula()?)?;
    }
    for (slot, lane) in [(0, 4), (4, 0), (5, 0)] {
        write(&mut program, 2, (0, slot, lane), Expr::scalar(0.))?;
    }
    // Native preparation already supplied emit time and the particle ordinal.
    // Preserve life, emit time and ordinal when resetting the normalized age.
    program.route(38, 0, 2)?;
    geometry(&mut program, cache, at, properties, metres, frame)?;
    // Shape draws are finished before row six Z becomes integrated source spin
    // and W becomes native angular state. Row four W retains the previous age.
    for target in [(0, 6, 6), (0, 6, 7), (0, 4, 7)] {
        write(&mut program, 3, target, Expr::scalar(0.))?;
    }
    let acceleration = transform(
        vector(cache, at + 0x1e8)?
            .lerp(
                vector(cache, at + 0x1f4)?,
                properties["acceleration"].formula()?,
            )
            .mul(Expr::scalar(metres)),
        frame,
        false,
    );
    let [gravity, drag, rotation_drag] = physics(cache, at, metres, limits)?;
    for phase in [3, 4] {
        let prop = |name| properties[name].formula();
        write(
            &mut program,
            phase,
            (1, 0, 1),
            prop("tint")?.mul(prop("color")?),
        )?;
        write(
            &mut program,
            phase,
            (1, 0, 7),
            prop("alpha")?.mul(prop("particle_alpha")?),
        )?;
        let size = prop("size")?.mul(prop("scale")?).mul(Expr::scalar(metres));
        write(
            &mut program,
            phase,
            (1, 3, 4),
            size.clone().mul(prop("aspect")?),
        )?;
        write(&mut program, phase, (1, 3, 5), size)?;
        write(&mut program, phase, (1, 3, 6), prop("black_point")?)?;
        write(&mut program, phase, (1, 3, 7), prop("intensity")?)?;
        write(&mut program, phase, (1, 4, 4), prop("frame")?)?;
        write(&mut program, phase, (1, 4, 5), prop("palette")?)?;
        write(
            &mut program,
            phase,
            (1, 4, 6),
            register(1, 1, 7).mul(register(0, 0, 5)),
        )?;
        write(&mut program, phase, (1, 4, 7), register(0, 6, 5))?;
        write(&mut program, phase, (1, 7, 0), register(0, 3, 0))?;
        if phase == 4 {
            let delta = register(1, 4, 6)
                .binary(2, register(0, 4, 7))
                .binary(9, Expr::scalar(0.));
            write(
                &mut program,
                phase,
                (0, 6, 4),
                register(0, 6, 4).mul(
                    Expr::scalar(1.)
                        .binary(2, rotation_drag.clone().mul(delta.clone()).unary(0x23)),
                ),
            )?;
            write(
                &mut program,
                phase,
                (0, 6, 6),
                register(0, 6, 6).add(register(0, 6, 4).mul(delta)),
            )?;
            write(&mut program, phase, (0, 4, 7), register(1, 4, 6))?;
        }
        write(
            &mut program,
            phase,
            (1, 2, 7),
            prop("rotation")?
                .add(register(0, 6, 6))
                .mul(Expr::scalar(std::f32::consts::TAU)),
        )?;
        write(&mut program, phase, (0, 4, 1), acceleration.clone())?;
        write(
            &mut program,
            phase,
            (0, 5, 1),
            Expr::Value([0., 0., -1., 0.]).mul(gravity.clone()),
        )?;
        write(&mut program, phase, (0, 5, 7), drag.clone())?;
    }
    limits.push("Sprayer and tube angles follow the MCC CPU branches. Globe and implode use continuous uniform directions in place of MCC's discrete direction table. Native random streams differ.".into());
    limits.push("Time properties use Shadowkeep's game clock, which wraps after 28,800 seconds. Source game-clock phase is not retained.".into());
    Ok(program)
}
