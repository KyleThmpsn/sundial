//! A constant status host, its HUD packet and its separately timed Duration component.
use super::*;
mod conditional;
mod display;
pub(super) mod host;
mod lifecycle;

pub(super) fn pair(
    p: &Payload,
    owner: u32,
    instance: usize,
    definition: usize,
    ic: u32,
    dc: u32,
) -> Result<()> {
    ensure!(
        p.u32(instance)? == owner
            && p.u32(instance + 4)? == 0
            && p.u64(instance + 8)? == definition as u64
            && p.u32(instance + 16)? == dc
            && p.u32(instance + 20)? == 0
            && p.u32(definition)? == owner
            && p.u32(definition + 4)? == 0
            && p.u64(definition + 8)? == instance as u64
            && p.u32(definition + 16)? == ic
            && p.u32(definition + 20)? == 0,
        "Source status has a different embedded pair"
    );
    Ok(())
}

/// Settings `+B4` is the Float32 that source getter `010B5EA0` publishes through the `+90`
/// declaration. Native keeps one field for it and the compared priority, `80803A61 +14`, which the
/// writer fills with the HUD priority. So it is admitted when it is zero or already that priority.
/// The `+90` declaration is never an admitted export, so the source value is unobservable here.
fn scalar(p: &Payload, settings: usize, priority: i32) -> Result<()> {
    let value = p.f32(settings + 0xB4)?;
    ensure!(
        value == 0.0 || value == priority as f32,
        "Source status scalar differs from its competition priority"
    );
    Ok(())
}

fn constant(p: &Payload, at: usize, indefinite: bool) -> Result<[u8; 16]> {
    let code = p
        .array(at + 24, 1, Some(0x80800009))?
        .into_iter()
        .map(|a| p.u8(a))
        .collect::<Result<Vec<_>>>()?;
    let values = p.array(at + 40, 16, Some(0x80800090))?;
    ensure!(
        code == [0x34, 0, 0x3E, 0]
            && values.len() == 1
            && p.u64(at + 56)? == 1
            && p.u64(at + 64)? == 1
            && p.bytes::<16>(at + 72)? == [0; 16],
        "Source status needs expression translation"
    );
    // Only the lifetime expression has the trailing flags and name. The count
    // expression ends at +58, where the enclosing HUD's display-enable field starts.
    match p.u32(at + 16)? {
        0x8080084D => {}
        0x808008AC => ensure!(
            p.u64(at + 88)? == 0 && [0, 0x811C9DC5].contains(&p.u64(at + 96)?),
            "Source status lifetime has additional flags or a named input"
        ),
        _ => anyhow::bail!("Source status has another constant expression class"),
    }
    let bits = p.u32(values[0])?;
    ensure!(
        f32::from_bits(bits).is_finite()
            && (f32::from_bits(bits) >= 0.0 || (indefinite && f32::from_bits(bits) == -1.0))
            && (1..4).all(|n| p.u32(values[0] + n * 4).ok() == Some(bits)),
        "Source status needs a finite constant scalar"
    );
    p.bytes::<16>(values[0])
}

struct Status {
    competition: [u8; 8],
    hud: [u8; 12],
    duration: [u8; 20],
    timer: [u8; 8],
    count: [u8; 16],
    seconds: [u8; 16],
    display: bool,
    reset_owned: bool,
}

#[allow(clippy::cognitive_complexity)]
fn validate(
    graph: &Payload,
    p: &Payload,
    owner: u32,
    companion: Option<&conditional::Companion>,
    bank: Option<(u32, usize)>,
    network: Option<u32>,
    audio: Option<&sequence::audio::Companion>,
) -> Result<Status> {
    let live = p.pointer(16)?;
    let settings = p.pointer(24)?;
    let hud = settings + 0xF0;
    let duration = hud + 0x160;
    let controller = duration + 0xB8;
    ensure!(
        p.u32(live - 4)? == 0x80802A75 && p.u32(settings - 4)? == 0x80802698,
        "Source status root differs"
    );
    for (offset, value) in [
        (0x18, 0),
        (0x20, u32::MAX as u64),
        (0x28, 0),
        (0x30, u32::MAX as u64),
        (0x38, u32::MAX as u64),
        (0x40, 0),
        (0x48, u32::MAX as u64),
        (0x50, 0),
        (0x58, 0),
    ] {
        ensure!(
            p.u64(settings + offset)? == value,
            "Source status host has additional state"
        );
    }
    let indices = graph.array(0x88, 2, Some(0x80800006))?;
    ensure!(
        indices.len() == 2 && indices.iter().all(|&a| graph.u16(a).ok() == Some(0)),
        "Source status owner indices differ"
    );
    for (i, d, ic, dc) in [
        (live, settings, 0x80802A75, 0x80802698),
        (live + 0x30, hud, 0x8080278D, 0x8080254F),
        (live + 0x50, duration, 0x8080255D, 0x8080223C),
        (live + 0x80, controller, 0x808027B5, 0x80802890),
        (live + 0xA0, duration + 0x138, 0x808008AC, 0x8080038D),
        (live + 0xE0, hud + 0xA0, 0x8080084D, 0x808005A0),
        (live + 0x110, hud + 0x108, 0x8080084D, 0x808005A0),
        (live + 0x170, settings + 0xB8, 0x808008AF, 0x80800456),
    ] {
        pair(p, owner, i, d, ic, dc)?;
    }
    // The only connected receiver admitted here is the separately checked audio
    // companion. Its complete sequence and resource obligations remain in the graph. Its bound
    // request is required (policy 0) in both builds, as the sound template `80BC5833`'s status
    // `81532CC9` holds it at `+60`, so a required request carries with the companion.
    let policy = p.u64(settings + 0xD0)?;
    ensure!(
        (policy == 2 || (policy == 0 && audio.is_some()))
            && p.u64(settings + 0xD8)? == 0x808003E3
            && p.u64(settings + 0xE0)? == 0
            && p.u64(settings + 0xE8)? == u32::MAX as u64
            && p.u32(settings + 0xA8)? == 0x3C18FBCE
            && p.u32(settings + 0xAC)? == 0
            && p.u32(settings + 0xB0)? <= 0xFF,
        "Source status host has a state program or unqualified competition flags"
    );
    // D1 keeps the competition mask on the host and uses the HUD's signed priority.
    // Native 3A61 owns both fields, with a Float32 priority. Keep integer ordering exact.
    // Both builds' activation and reevaluation predicates agree for every mask byte pair,
    // and so do the bit 2 and 3 counters, so any byte mask carries.
    let priority = i32::from_le_bytes(p.bytes::<4>(hud + 0x8C)?);
    ensure!(
        (-16_777_216..=16_777_216).contains(&priority),
        "Source status competition priority cannot be represented exactly"
    );
    scalar(p, settings, priority)?;
    let mut competition = [0; 8];
    competition[0] = p.u8(settings + 0xB0)?;
    competition[4..].copy_from_slice(&(priority as f32).to_le_bytes());
    let outputs = graph.array(0x30, 88, Some(0x8080054E))?;
    ensure!(
        outputs.len() == if companion.is_some() { 2 } else { 1 }
            && graph.bytes::<16>(0x40)? == [0; 16],
        "Source status has additional event connections"
    );
    let at = outputs[0];
    ensure!(
        graph.u32(at)? == 0x811C9DC5
            && graph.u32(at + 4)? == 0
            && graph.u32(at + 8)? == owner
            && graph.u32(at + 12)? == 0
            && graph.u64(at + 16)? == (settings + 0xB8) as u64
            && graph.u32(at + 24)? == 0x80800456
            && graph.u32(at + 28)? == 0
            && graph.u64(at + 32)? == 0
            && graph.u64(at + 40)? == 0x811C9DC5
            && graph.u64(at + 48)? == audio.map_or(u32::MAX as u64, |a| a.owner as u64)
            && graph.u64(at + 56)? == audio.map_or(0, |a| (a.definition + 0xA8) as u64)
            && graph.u64(at + 64)?
                == if audio.is_some() {
                    0x80800988
                } else {
                    u32::MAX as u64
                }
            && graph.u64(at + 72)? == if audio.is_some() { 1 } else { 65535 }
            && graph.u64(at + 80)? == 0,
        "Source status has an unqualified output receiver"
    );

    // Each allowed parameter is a declaration, followed by its literal selector.
    // Check the complete export set and reciprocal runtime references as well.
    let declarations: [(usize, usize, u32, u32, u64, u32); 18] = [
        (settings + 0x60, live, 0x80802A75, 0x80AAC44B, 0, 0),
        (settings + 0x78, live, 0x80802A75, 0x80AAC282, 0xFFA2, 0),
        (settings + 0x90, live, 0x80802A75, 0x80AAC44C, 0, 0),
        (settings + 0x418, live, 0x80802A75, 0x80AAC44A, 0, 0),
        (settings + 0x438, live, 0x80802A75, 0x80AAC283, 0, 0),
        (hud + 0x18, live + 0x30, 0x8080278D, 0x80AAC449, 0, 0),
        (hud + 0x38, live + 0x30, 0x8080278D, 0x80AAC447, 0, 0),
        (hud + 0x58, live + 0x30, 0x8080278D, 0x80AAC281, 0, 0),
        (hud + 0x70, live + 0x30, 0x8080278D, 0x80AAC448, 0, 1),
        (duration + 0x18, live + 0x50, 0x8080255D, 0x80AAC286, 0, 0),
        (duration + 0x38, live + 0x50, 0x8080255D, 0x80AAC284, 0, 0),
        (duration + 0x58, live + 0x50, 0x8080255D, 0x80AAEDB3, 0, 0),
        (duration + 0x70, live + 0x50, 0x8080255D, 0x80AAC288, 0, 0),
        (duration + 0x88, live + 0x50, 0x8080255D, 0x80AAC444, 0, 0),
        (
            controller + 0x18,
            live + 0x80,
            0x808027B5,
            0x80AAEDB4,
            0xFF9C,
            0,
        ),
        (controller + 0x30, live + 0x80, 0x808027B5, 0x80AAC285, 0, 2),
        (controller + 0x48, live + 0x80, 0x808027B5, 0x80AAC289, 0, 0),
        (controller + 0x60, live + 0x80, 0x808027B5, 0x80AAC287, 0, 0),
    ];
    for &(a, _, class, declaration, value, _) in &declarations {
        let container = match class {
            0x80802A75 => settings,
            0x8080278D => hud,
            0x8080255D => duration,
            0x808027B5 => controller,
            _ => unreachable!(),
        };
        ensure!(
            p.pointer(a)? == container,
            "Source status parameter has another path"
        );
        ensure!(
            p.u64(a + 8)? == declaration as u64 && p.u64(a + 16)? == value,
            "Source status parameter has an unqualified driver"
        );
    }
    let exports = graph.array(0x68, 40, Some(0x80800370))?;
    let refs = graph.array(0x78, 24, Some(0x80800790))?;
    // The unused host input at +90 is not exported by this compiled form.
    ensure!(
        exports.len()
            == declarations.len() - 1
                + if companion.is_some() { 4 } else { 0 }
                + bank.map_or(0, |(_, count)| count * 2 + 2)
                + if network.is_some() { 7 } else { 0 }
                + if audio.is_some() { 12 } else { 0 }
            && refs.len() == exports.len(),
        "Source status export count differs"
    );
    let mut seen = BTreeSet::new();
    for (&at, &reference) in exports.iter().zip(&refs) {
        if audio.is_some_and(|audio| graph.u32(at + 16).ok() == Some(audio.owner)) {
            // The audio reader checks the complete ordinal-one provider partition.
            continue;
        }
        if network.is_some_and(|owner| graph.u32(at + 16).ok() == Some(owner)) {
            // The network reader checks its complete provider and lifecycle contract.
            continue;
        }
        if bank.is_some_and(|(owner, _)| graph.u32(at + 16).ok() == Some(owner)) {
            // The modifier reader checks every reciprocal row and export separately.
            continue;
        }
        if let Some(companion) = companion
            && graph.u32(at + 16)? == companion.owner
        {
            // The companion reader validates its complete four-export set.
            continue;
        }
        let a = usize::try_from(graph.u64(at + 24)?)?;
        let &(_, instance, class, declaration, value, output) = declarations
            .iter()
            .find(|d| d.0 == a && a != settings + 0x90)
            .context("Unknown source status export")?;
        let selector = if value == 0 {
            0
        } else {
            (value as u16 as i16 as i32) as u32
        };
        ensure!(
            graph.u32(at)? == output << 16
                && graph.u32(at + 4)? as usize == instance - live
                && graph.u32(at + 8)? == selector
                && graph.u32(at + 12)? == class
                && graph.u64(at + 16)? == owner as u64
                && graph.u64(at + 32)? == declaration as u64
                && graph.u64(reference)? == owner as u64
                && graph.u64(reference + 8)? == instance as u64
                && graph.u64(reference + 16)? == 0
                && seen.insert(a),
            "Source status export targets another parameter"
        );
    }
    let reset_owned = companion.is_none()
        && bank.is_none()
        && network.is_none()
        && p.u32(duration + 0xA0)? == 9
        && p.u64(duration + 0xB0)? == 1
        && p.u64(controller + 0x78)? == 0;
    ensure!(
        p.u32(hud + 0x30)? == 0x80800955
            && p.u32(hud + 0x50)? == 0x80800652
            && p.u64(hud + 0x98)? == 0
            && p.u64(duration + 0x30)? == 0x8080299F
            && p.u64(duration + 0x50)? == 0x80802B20
            && (p.u32(duration + 0xA0)? == 5
                || (companion.is_none() && p.u32(duration + 0xA0)? == 9)
                || (network.is_some() && p.u32(duration + 0xA0)? == 11)
                || (companion.is_none() && network.is_none() && p.u32(duration + 0xA0)? == 0x17))
            && i32::from_le_bytes(p.bytes(duration + 0xA8)?) >= 0
            && (p.u64(duration + 0xB0)? == u64::from(companion.is_some())
                || (network.is_some() && p.u64(duration + 0xB0)? == 1)
                || reset_owned)
            && (p.u64(controller + 0x78)? == u64::from(companion.is_none()) || reset_owned),
        "Source status has additional presentation or duration policy"
    );
    let count = match companion {
        Some(companion) => companion.threshold,
        None => constant(p, hud + 0xA0, false)?,
    };
    // The connected audio envelope is qualified for the retained timed status.
    // Other reset policies need their own sequence activation and cleanup evidence.
    ensure!(
        audio.is_none()
            || (p.u32(duration + 0xA0)? == 9
                && p.u64(duration + 0xB0)? == 0
                && p.u64(controller + 0x78)? == 1),
        "Connected status audio needs the qualified duration policy"
    );
    let seconds = constant(p, duration + 0x138, bank.is_some())?;
    if reset_owned {
        // Duration construction deactivates this Timer before scalar initialization.
        // A matched message reactivates it without changing its retained zero, then
        // requests retirement. Zero alone does not mean an indefinite lifetime.
        ensure!(
            seconds == [0; 16],
            "Source reset-owned status needs a zero constant lifetime"
        );
        lifecycle::source(p, live)?;
    }
    if p.u32(duration + 0xA0)? == 0x17 {
        // Both refresh callbacks restore a fixed positive lifetime during countdown.
        // Native preserves a longer current timer, while D1 replaces it. Dynamic and
        // connected lifetime writers cannot use this route. Their absence is checked
        // through the complete owner, driver and connection validation above.
        ensure!(
            f32::from_le_bytes(seconds[..4].try_into()?) > 0.0,
            "Source refreshing status needs a positive constant lifetime"
        );
    }
    let display = display::read(p, hud)?;

    Ok(Status {
        competition,
        hud: p.bytes(hud + 0x88)?,
        duration: p.bytes(duration + 0xA0)?,
        timer: p.bytes(controller + 0x78)?,
        count,
        seconds,
        display,
        reset_owned,
    })
}

/// A status lifetime: a replicated constant, a program over one named channel input, or a
/// program over constants and the builtin clock alone.
pub(crate) enum Lifetime {
    Constant([u8; 16]),
    Program {
        code: Vec<u8>,
        constants: Vec<[u8; 16]>,
        input: u32,
    },
    /// Destiny 1 tools write `3C 00 23 3E 00` for an unset program, the clock saturated to one
    /// second. Both interpreters, `003A4110` and `003B7A80`, run it identically.
    Builtin {
        code: Vec<u8>,
        constants: Vec<[u8; 16]>,
    },
}

/// A lifetime without a named input: a constant when it has the constant form, otherwise a
/// program over constants and the builtin clock.
fn unnamed(
    p: &Payload,
    at: usize,
    indefinite: bool,
    code: Vec<u8>,
    constants: Vec<[u8; 16]>,
) -> Result<Lifetime> {
    Ok(if code == [0x34, 0, 0x3E, 0] {
        Lifetime::Constant(constant(p, at, indefinite)?)
    } else {
        Lifetime::Builtin { code, constants }
    })
}

const ICON: &[u8] = b"abilities.ability_components.c_player_status_icon_definition\0";

/// Whether a source dynamic definition schema is the player status icon script,
/// `bs\abilities\ability_components.bs` on component base `80802361`. Two byte-identical copies
/// exist under different tags, so the class name decides.
pub(super) fn is_icon(schema: &Payload) -> bool {
    schema.u32(16).ok() == Some(0x80802361)
        && schema.0.windows(ICON.len()).any(|window| window == ICON)
}

/// A source player status icon: the HUD block a status embeds, `8080254F` at definition `+60`,
/// with its Duration and Timer, and no host. Its runtime twins sit where a status's do. The icon
/// has no host-state input, so it carries as a native status whose lifetime receiver is optional,
/// policy 2, with the competition the HUD priority gives. The graph's own rows never name it.
pub(super) fn icon(p: &Payload, owner: u32) -> Result<Connected> {
    let live = p.pointer(16)?;
    let settings = p.pointer(24)?;
    let hud = settings + 0x60;
    let duration = hud + 0x160;
    let controller = duration + 0xB8;
    ensure!(
        p.u32(live)? == owner && p.u32(settings)? == owner,
        "Source status icon root differs"
    );
    for (i, d, ic, dc) in [
        (live + 0x30, hud, 0x8080278D, 0x8080254F),
        (live + 0x50, duration, 0x8080255D, 0x8080223C),
        (live + 0x80, controller, 0x808027B5, 0x80802890),
        (live + 0xA0, duration + 0x138, 0x808008AC, 0x8080038D),
        (live + 0xE0, hud + 0xA0, 0x8080084D, 0x808005A0),
        (live + 0x110, hud + 0x108, 0x8080084D, 0x808005A0),
    ] {
        pair(p, owner, i, d, ic, dc)?;
    }
    let flags = p.u32(duration + 0xA0)?;
    ensure!(
        p.u32(hud + 0x30)? == 0x80800955
            && p.u32(hud + 0x50)? == 0x80800652
            && p.u64(hud + 0x98)? == 0
            && p.u64(duration + 0x30)? == 0x8080299F
            && p.u64(duration + 0x50)? == 0x80802B20
            && flags & !0x1F == 0
            && i32::from_le_bytes(p.bytes(duration + 0xA8)?) >= 0
            && p.u64(duration + 0xB0)? <= 1
            && p.u64(controller + 0x78)? <= 1,
        "Source status icon has additional presentation or duration policy"
    );
    let priority = i32::from_le_bytes(p.bytes::<4>(hud + 0x8C)?);
    ensure!(
        (-16_777_216..=16_777_216).contains(&priority),
        "Source status icon priority cannot be represented exactly"
    );
    let mut competition = [0; 8];
    competition[4..].copy_from_slice(&(priority as f32).to_le_bytes());
    let lifetime_at = duration + 0x138;
    ensure!(
        p.u64(lifetime_at + 88)? == 0 && [0, 0x811C9DC5].contains(&p.u64(lifetime_at + 96)?),
        "Source status icon lifetime has additional flags or a named input"
    );
    let (code, constants, input) = program(p, lifetime_at)?;
    ensure!(
        input.is_none(),
        "Source status icon lifetime reads an input"
    );
    let indefinite = p.u64(controller + 0x78)? == 1;
    let lifetime = unnamed(p, lifetime_at, indefinite, code, constants)?;
    if flags & 0x10 != 0 {
        ensure!(
            matches!(&lifetime, Lifetime::Constant(seconds)
                if f32::from_le_bytes(seconds[..4].try_into()?) > 0.0),
            "Source refreshing status icon needs a positive constant lifetime"
        );
    }
    Ok(Connected {
        competition,
        hud: p.bytes(hud + 0x88)?,
        duration: p.bytes(duration + 0xA0)?,
        timer: p.bytes(controller + 0x78)?,
        count: constant(p, hud + 0xA0, false)?,
        lifetime,
        lifetime_input: None,
        provider_input: usize::MAX,
        receiver_policy: 2,
    })
}

/// A source Timer, definition `80802890` at `definition` with runtime `808027B5` at `live`: its
/// flag word and lifetime, as a status's Timer holds them at its Duration `+B8`.
pub(super) fn timer(
    p: &Payload,
    owner: u32,
    live: usize,
    definition: usize,
) -> Result<([u8; 8], Lifetime)> {
    pair(p, owner, live, definition, 0x808027B5, 0x80802890)?;
    let at = definition + 0x80;
    pair(p, owner, live + 0x20, at, 0x808008AC, 0x8080038D)?;
    let flag = p.u64(definition + 0x78)?;
    ensure!(
        flag <= 1 && p.u64(at + 88)? == 0 && [0, 0x811C9DC5].contains(&p.u64(at + 96)?),
        "Source Timer has unknown flags or a named lifetime"
    );
    let (code, constants, input) = program(p, at)?;
    ensure!(input.is_none(), "Source Timer lifetime reads an input");
    Ok((
        p.bytes(definition + 0x78)?,
        unnamed(p, at, flag == 1, code, constants)?,
    ))
}

/// The source fields of a status host whose lifetime sequence is another graph owner.
pub(crate) struct Connected {
    pub competition: [u8; 8],
    pub hud: [u8; 12],
    pub duration: [u8; 20],
    pub timer: [u8; 8],
    pub count: [u8; 16],
    pub lifetime: Lifetime,
    /// Offsets in the source owner of the lifetime input definition and the lifetime-sequence
    /// receiver, which graph connection rows name.
    pub lifetime_input: Option<usize>,
    pub provider_input: usize,
    /// Lifetime-sequence receiver policy at host `+D0`: zero required, two optional. Native
    /// connected statuses keep it at root `+70`.
    pub receiver_policy: u64,
}

/// Opcodes of the scalar dialect that both builds interpret identically in the qualified
/// dynamic programs. Their operand widths match the source program validator.
#[allow(clippy::type_complexity)]
pub(super) fn program(
    p: &Payload,
    at: usize,
) -> Result<(Vec<u8>, Vec<[u8; 16]>, Option<(u32, usize)>)> {
    let code = p
        .array(at + 24, 1, Some(0x80800009))?
        .into_iter()
        .map(|a| p.u8(a))
        .collect::<Result<Vec<_>>>()?;
    let constants = p
        .array(at + 40, 16, Some(0x80800090))?
        .into_iter()
        .map(|a| p.bytes::<16>(a))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        constants.iter().all(|row| row
            .chunks_exact(4)
            .all(|lane| f32::from_le_bytes(lane.try_into().unwrap()).is_finite())),
        "Source status program has a nonfinite constant"
    );
    let inputs = p.array(at + 72, 48, Some(0x80800927))?;
    let mut position = 0;
    let mut reads = false;
    while position < code.len() {
        let op = code[position];
        // Opcode 12 is `a*b + c`. Original interpreters 3A4110 and 3B7A80 give equal bits for
        // Defensive Reflex's lifetime program over twelve supplied input and constant cases.
        let operand = match op {
            0x01 | 0x03 | 0x12 | 0x23 => None,
            0x34 | 0x3C | 0x3E => {
                Some(*code.get(position + 1).context("Truncated status program")?)
            }
            _ => anyhow::bail!("Source status program opcode {op:02X} needs a native contract"),
        };
        match (op, operand) {
            (0x34, Some(index)) => ensure!(
                usize::from(index) < constants.len(),
                "Source status program constant is undeclared"
            ),
            // Input zero is the builtin both builds supply to a lifetime: the game clock in
            // seconds, wrapped at 28800, from source `003A6550` and native `003BA320`. Input
            // one is the first declared input.
            (0x3C, Some(0)) => {}
            (0x3C, Some(index)) => {
                ensure!(
                    index == 1 && inputs.len() == 1,
                    "Source status program reads an undeclared input"
                );
                reads = true;
            }
            (0x3E, Some(index)) => ensure!(
                index == 0 && position + 2 == code.len(),
                "Source status program has another output"
            ),
            _ => {}
        }
        position += 1 + usize::from(operand.is_some());
    }
    ensure!(
        code.ends_with(&[0x3E, 0])
            && p.u64(at + 56)? == if reads { 2 } else { 1 }
            && p.u64(at + 64)? == 1,
        "Source status program slots differ"
    );
    let input = if reads {
        let row = inputs[0];
        let owner = p.u32(row)?;
        pair(
            p,
            owner,
            usize::try_from(p.u64(row + 8)?)?,
            row,
            0x8080077D,
            0x80800927,
        )?;
        ensure!(
            p.u64(row + 24)? == 0 && p.u64(row + 32)? == 0x808003E8,
            "Source status program input is not a direct named channel"
        );
        Some((p.u32(row + 40)?, row))
    } else {
        ensure!(inputs.is_empty(), "Source status program has unused inputs");
        None
    };
    Ok((code, constants, input))
}

/// Read a status host whose required lifetime-sequence receiver is another owner of its graph.
/// Every field the native status stores is returned. The graph's other owners are validated
/// by their own readers. HUD display state is checked here and translated by the caller.
#[allow(clippy::cognitive_complexity)]
pub(super) fn connected(
    graph: &Payload,
    p: &Payload,
    owner: u32,
    ordinal: usize,
) -> Result<Connected> {
    let live = p.pointer(16)?;
    let settings = p.pointer(24)?;
    let hud = settings + 0xF0;
    let duration = hud + 0x160;
    let controller = duration + 0xB8;
    ensure!(
        p.u32(live - 4)? == 0x80802A75 && p.u32(settings - 4)? == 0x80802698,
        "Source status root differs"
    );
    for (offset, value) in [
        (0x18, 0),
        (0x20, u32::MAX as u64),
        (0x28, 0),
        (0x30, u32::MAX as u64),
        (0x38, u32::MAX as u64),
        (0x40, 0),
        (0x48, u32::MAX as u64),
        (0x50, 0),
        (0x58, 0),
    ] {
        ensure!(
            p.u64(settings + offset)? == value,
            "Source status host has additional state"
        );
    }
    for (i, d, ic, dc) in [
        (live, settings, 0x80802A75, 0x80802698),
        (live + 0x30, hud, 0x8080278D, 0x8080254F),
        (live + 0x50, duration, 0x8080255D, 0x8080223C),
        (live + 0x80, controller, 0x808027B5, 0x80802890),
        (live + 0xA0, duration + 0x138, 0x808008AC, 0x8080038D),
        (live + 0xE0, hud + 0xA0, 0x8080084D, 0x808005A0),
        (live + 0x110, hud + 0x108, 0x8080084D, 0x808005A0),
        (live + 0x170, settings + 0xB8, 0x808008AF, 0x80800456),
    ] {
        pair(p, owner, i, d, ic, dc)?;
    }
    // Policy zero marks the lifetime-sequence receiver required. Its provider is checked with
    // the graph's connections. Policy two leaves it optional, as for a status with no lifetime
    // sequence, such as Eliksni Immunity's, whose request stays unbound. The native template
    // keeps its own receiver policy, and its unbound status rows stand in for the request.
    ensure!(
        matches!(p.u64(settings + 0xD0)?, 0 | 2)
            && p.u64(settings + 0xD8)? == 0x808003E3
            && p.u64(settings + 0xE0)? == 0
            && p.u64(settings + 0xE8)? == u32::MAX as u64
            && p.u32(settings + 0xA8)? == 0x3C18FBCE
            && p.u32(settings + 0xAC)? == 0
            && p.u32(settings + 0xB0)? <= 0xFF,
        "Source connected status has a state program or unqualified competition flags"
    );
    let priority = i32::from_le_bytes(p.bytes::<4>(hud + 0x8C)?);
    ensure!(
        (-16_777_216..=16_777_216).contains(&priority),
        "Source status competition priority cannot be represented exactly"
    );
    scalar(p, settings, priority)?;
    let mut competition = [0; 8];
    competition[0] = p.u8(settings + 0xB0)?;
    competition[4..].copy_from_slice(&(priority as f32).to_le_bytes());
    let declarations: [(usize, usize, u32, u32, u64, u32); 17] = [
        (settings + 0x60, live, 0x80802A75, 0x80AAC44B, 0, 0),
        (settings + 0x78, live, 0x80802A75, 0x80AAC282, 0xFFA2, 0),
        (settings + 0x418, live, 0x80802A75, 0x80AAC44A, 0, 0),
        (settings + 0x438, live, 0x80802A75, 0x80AAC283, 0, 0),
        (hud + 0x18, live + 0x30, 0x8080278D, 0x80AAC449, 0, 0),
        (hud + 0x38, live + 0x30, 0x8080278D, 0x80AAC447, 0, 0),
        (hud + 0x58, live + 0x30, 0x8080278D, 0x80AAC281, 0, 0),
        (hud + 0x70, live + 0x30, 0x8080278D, 0x80AAC448, 0, 1),
        (duration + 0x18, live + 0x50, 0x8080255D, 0x80AAC286, 0, 0),
        (duration + 0x38, live + 0x50, 0x8080255D, 0x80AAC284, 0, 0),
        (duration + 0x58, live + 0x50, 0x8080255D, 0x80AAEDB3, 0, 0),
        (duration + 0x70, live + 0x50, 0x8080255D, 0x80AAC288, 0, 0),
        (duration + 0x88, live + 0x50, 0x8080255D, 0x80AAC444, 0, 0),
        (
            controller + 0x18,
            live + 0x80,
            0x808027B5,
            0x80AAEDB4,
            0xFF9C,
            0,
        ),
        (controller + 0x30, live + 0x80, 0x808027B5, 0x80AAC285, 0, 2),
        (controller + 0x48, live + 0x80, 0x808027B5, 0x80AAC289, 0, 0),
        (controller + 0x60, live + 0x80, 0x808027B5, 0x80AAC287, 0, 0),
    ];
    let exports = graph.array(0x68, 40, Some(0x80800370))?;
    let refs = graph.array(0x78, 24, Some(0x80800790))?;
    ensure!(
        exports.len() == refs.len(),
        "Source status export references differ"
    );
    let mut seen = BTreeSet::new();
    for (&at, &reference) in exports.iter().zip(&refs) {
        if graph.u32(at + 16)? != owner {
            continue;
        }
        let a = usize::try_from(graph.u64(at + 24)?)?;
        let &(_, instance, class, declaration, value, output) = declarations
            .iter()
            .find(|d| d.0 == a)
            .context("Unknown source connected status export")?;
        let container = match class {
            0x80802A75 => settings,
            0x8080278D => hud,
            0x8080255D => duration,
            _ => controller,
        };
        let selector = if value == 0 {
            0
        } else {
            (value as u16 as i16 as i32) as u32
        };
        // Provider exclusion bits are numbered across the whole graph, network first, so a
        // connected status holds one bit per exclusive provider rather than fixed values.
        let flags = graph.u32(at)?;
        let exclusion = if output == 0 {
            flags == 0
        } else {
            flags & 0xFFFF == 0 && (flags >> 16).is_power_of_two()
        };
        ensure!(
            p.pointer(a)? == container
                && p.u64(a + 8)? == declaration as u64
                && p.u64(a + 16)? == value
                && exclusion
                && graph.u32(at + 4)? as usize == instance - live
                && graph.u32(at + 8)? == selector
                && graph.u32(at + 12)? == class
                && graph.u64(at + 32)? == declaration as u64
                && graph.u64(reference)? == owner as u64
                && graph.u64(reference + 8)? == instance as u64
                && graph.u64(reference + 16)? == ordinal as u64
                && seen.insert(a),
            "Source connected status export targets another parameter"
        );
    }
    ensure!(
        seen.len() == declarations.len(),
        "Source connected status exports are incomplete"
    );
    // Every Duration flag bit has the same consumer in both builds: bit 0 gates retirement on
    // the entity bitmap test (source 7B690A and 7B710B, native CA071B and C9FC46), bit 1 keeps a
    // matched Duration from retiring before its count falls (7B6FFA, C9FDB2), bits 2 and 3 select
    // the priority rule, and bit 4 refreshes the lifetime, checked below. Native connected status
    // 80FEF720 stores 07 in the same graph form. The match count at `+A8` carries as stored, zero
    // included, as stock Duration host 8157808D stores it, since both builds decrement and test it
    // on the same matching paths.
    let flags = p.u32(duration + 0xA0)?;
    ensure!(
        p.u32(hud + 0x30)? == 0x80800955
            && p.u32(hud + 0x50)? == 0x80800652
            && p.u64(hud + 0x98)? == 0
            && p.u64(duration + 0x30)? == 0x8080299F
            && p.u64(duration + 0x50)? == 0x80802B20
            && flags & !0x1F == 0
            && i32::from_le_bytes(p.bytes(duration + 0xA8)?) >= 0
            && p.u64(duration + 0xB0)? <= 1
            && p.u64(controller + 0x78)? <= 1,
        "Source connected status has additional presentation or duration policy"
    );
    let lifetime_at = duration + 0x138;
    ensure!(
        p.u32(lifetime_at + 16)? == 0x808008AC
            && p.u64(lifetime_at + 88)? == 0
            && [0, 0x811C9DC5].contains(&p.u64(lifetime_at + 96)?),
        "Source status lifetime has additional flags or a named input"
    );
    let (code, constants, input) = program(p, lifetime_at)?;
    // With the Timer flag set, a nonpositive lifetime deactivates the Timer in both builds:
    // source method one clears its active byte, and native setter EDF2E0 calls EDF1E0 with zero,
    // which stores the active byte at +B1. The status then lasts until it is removed, so the
    // indefinite -1 literal carries, as on the elemental armor perks' statuses.
    let indefinite = p.u64(controller + 0x78)? == 1;
    let lifetime = match input {
        None => unnamed(p, lifetime_at, indefinite, code, constants)?,
        Some((name, _)) => Lifetime::Program {
            code,
            constants,
            input: name,
        },
    };
    if flags & 0x10 != 0 {
        // Source refresh assigns the evaluated lifetime, native refresh only raises a shorter
        // remainder. A constant positive lifetime keeps them equal. An indefinite status keeps
        // its Timer deactivated in both builds: source refresh assigns the nonpositive lifetime
        // again, which deactivates it, and native refresh has no remainder to raise, so the
        // status lasts until it is removed, as Tempered Light's lasts while its sword is held.
        let kept = match &lifetime {
            Lifetime::Constant(seconds) => {
                let seconds = f32::from_le_bytes(seconds[..4].try_into()?);
                seconds > 0.0 || (indefinite && seconds <= 0.0)
            }
            Lifetime::Program { .. } | Lifetime::Builtin { .. } => false,
        };
        ensure!(
            kept,
            "Source refreshing status needs a positive or indefinite constant lifetime"
        );
    }
    Ok(Connected {
        competition,
        hud: p.bytes(hud + 0x88)?,
        duration: p.bytes(duration + 0xA0)?,
        timer: p.bytes(controller + 0x78)?,
        count: constant(p, hud + 0xA0, false)?,
        lifetime,
        lifetime_input: input.map(|(_, row)| row),
        provider_input: settings + 0xB8,
        receiver_policy: p.u64(settings + 0xD0)?,
    })
}

/// The complete private graph edit and its private HUD entry.
pub(super) type Hosted = (
    NativeAssetPatch,
    Option<sundial::package_authoring::sandbox_perk::program::HudStatus>,
);

pub(super) fn modifier_bank(
    source: &Source,
    graph: &Payload,
) -> Result<Option<(u32, Vec<Modifier>)>> {
    let owners = graph.array(32, 12, Some(0x80800715))?;
    if !matches!(owners.len(), 2 | 3) {
        return Ok(None);
    }
    let tag = graph.u32(owners[owners.len() - 1])?;
    let p = source.read(tag)?;
    if p.u32(p.pointer(24)? - 4)? != 0x808020DE {
        return Ok(None);
    }
    Ok(Some((tag, modifier_owner(source, graph, tag)?)))
}

#[allow(clippy::cognitive_complexity)]
pub(super) fn read(
    source: &Source,
    native: &mut Reader,
    graph: &Payload,
    p: &Payload,
    owner: u32,
) -> Result<Hosted> {
    let audio = sequence::audio::read(source, native, graph)?;
    let bank = modifier_bank(source, graph)?;
    let combined = graph.u64(32)? == 3 && audio.is_none();
    let network = if audio.is_some() {
        Some(network::sound(source, native, graph)?)
    } else if bank.is_none() || combined {
        network::read(source, native, graph)?
    } else {
        None
    };
    ensure!(
        !combined || (bank.is_some() && network.is_some()),
        "Source three-owner status requires a qualified network and modifier bank"
    );
    let companion = if graph.u64(32)? == 2 && bank.is_none() && network.is_none() {
        Some(conditional::read(source, graph, p, owner)?)
    } else {
        None
    };
    let status = validate(
        graph,
        p,
        owner,
        companion.as_ref(),
        bank.as_ref().map(|(tag, rows)| (*tag, rows.len())),
        network,
        audio.as_ref(),
    )?;
    let name = u32::from_le_bytes(status.hud[..4].try_into()?);
    let hud_status = hud::read(source, native, name)?;
    let tag = if audio.is_some() {
        sequence::audio::GRAPH
    } else if combined {
        network::BANK_GRAPH
    } else if let Some((_, rows)) = &bank {
        match rows.len() {
            1 => 0x80FF05F0,
            2 => 0x8161E50E,
            3 => 0x8161E47B,
            4 => 0x8161E480,
            5 => 0x80C19A4F,
            _ => anyhow::bail!("Status modifier count needs a native allocation"),
        }
    } else if companion.is_some() {
        0x80BC5880
    } else if network.is_some() {
        network::GRAPH
    } else {
        0x80BC31BC
    };
    let graph = native.tag(tag, Some(0x80809C0F))?;
    let owners = graph.array(16, 12, Some(0x80809C04))?;
    ensure!(
        owners.len()
            == if combined || audio.is_some() {
                3
            } else if companion.is_some() || bank.is_some() || network.is_some() {
                2
            } else {
                1
            },
        "Native status envelope has companion components"
    );
    let target_owner = graph.u32(owners[0])?;
    let target = native.tag(target_owner, Some(0x80809C36))?;
    let root = target.pointer(24)?;
    ensure!(
        !(combined || audio.is_some()) || target.u64(root + 0x440)? == 0,
        "Native replicated status contains an additional embedded bank"
    );
    let h = root + 0xE0;
    let d = h + 0x148;
    if status.reset_owned {
        lifecycle::native(native, &graph, &target, target_owner, d)?;
    }
    let sequence = root + 0x48;
    let sequence_live = usize::try_from(target.u64(sequence + 8)?)?;
    ensure!(
        target.u32(sequence)? == target_owner
            && target.u32(sequence + 4)? == 0x80803A60
            && target.u32(sequence_live)? == target_owner
            && target.u32(sequence_live + 4)? == 0x80803A61
            && target.u64(sequence_live + 8)? == sequence as u64,
        "Native status competition pair differs"
    );
    // The status's lifetime request policy at `+70`. The sound template `80BC5833` binds that
    // request to its sound sequence `80EFADEC` with policy 0 (required), the source sound form's
    // own policy, where the other templates leave it at 2.
    ensure!(
        target.u32(root - 4)? == 0x80804205
            && target.u32(h + 4)? == 0x80804211
            && target.u32(d + 4)? == 0x80803B32
            && target.u64(root + 0x58)? == if audio.is_some() { 4 } else { 0 }
            && target.u64(root + 0x70)? == if audio.is_some() { 0 } else { 2 }
            && target.u64(root + 0xC0)? == 0x3C18FBCE
            && target.u64(root + 0x408)? == 0
            && target.u64(root + 0x410)? == 0
            && target.u64(root + 0x418)? == 0
            && target.u64(root + 0x420)? == 0
            && target.u32(root + 0x428)? == u32::MAX
            && target.u64(h + 0xE8)? == 0x811C9DC5,
        "Native status envelope has an active sequence or display transform"
    );
    let outputs = graph.array(32, 72, Some(0x80809BC9))?;
    ensure!(
        outputs.len() == if companion.is_some() { 3 } else { 2 },
        "Native status envelope outputs differ"
    );
    for at in outputs {
        if audio.is_some() {
            // Both complete rows, including the unbound requester, were checked
            // against the source connection by the retained audio-envelope reader.
            continue;
        }
        if companion.is_some() && graph.u32(at + 40)? != u32::MAX {
            // The conditional patcher validates the one channel connection.
            continue;
        }
        ensure!(
            graph.u32(at + 40)? == u32::MAX && graph.u64(at + 48)? == 0,
            "Native status envelope has an external receiver"
        );
    }
    let bindings = weapon_component_bindings(&graph.0, 0x9E50D3EB).map_err(anyhow::Error::msg)?;
    ensure!(
        bindings.len() == 1
            && bindings[0].owner_tag == target_owner
            && bindings[0].concrete_class == 0x80804209,
        "Native status binding differs"
    );
    let binding = &bindings[0];
    let mut patches = Vec::new();
    let mut replace = |at: usize, bytes: Vec<u8>| -> Result<()> {
        let expected = target
            .0
            .get(at..at + bytes.len())
            .context("Truncated native status")?
            .to_vec();
        patches.push(NativeAssetResourcePatch {
            binding_hash: binding.binding_hash,
            resource_index: u16::try_from(binding.resource_index)?,
            offset: u32::try_from(
                at.checked_sub(usize::try_from(binding.resource_offset)?)
                    .context("Native status field precedes instance")?,
            )?,
            expected,
            bytes,
            imported_particle: None,
        });
        Ok(())
    };
    replace(root + 0x58, status.competition.to_vec())?;
    if hud_status.is_some() {
        // The private HUD builder patches the name separately. Leave its expected stock
        // bytes intact and avoid overlapping that patch with priority and display flags.
        replace(h + 0x84, status.hud[4..].to_vec())?;
    } else {
        replace(h + 0x80, status.hud.to_vec())?;
    }
    replace(d + 0x98, status.duration.to_vec())?;
    replace(d + 0xB0 + 0x70, status.timer.to_vec())?;
    for (at, bytes) in [(h + 0x98, status.count), (d + 0x128, status.seconds)] {
        let values = target.array(at + 0x20, 16, Some(0x80800090))?;
        let code = target
            .array(at + 0x10, 1, Some(0x80800009))?
            .into_iter()
            .map(|a| target.u8(a))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            (code == [0x34, 0, 0x3E, 0]
                || (companion.is_some()
                    && at == h + 0x98
                    && code == [0x3C, 1, 0x34, 0, 0x08, 0x3E, 0]))
                && values.len() == 1,
            "Native status envelope has a dynamic expression"
        );
        replace(values[0], bytes.to_vec())?;
    }
    if status.display {
        display::patch(&target, h, &mut replace)?;
    }
    if let Some(audio) = audio {
        patches.push(audio.patch);
    }
    let mut appends = Vec::new();
    if let Some(companion) = companion {
        let (extra, append) =
            conditional::patch(native, &graph, &target, target_owner, &companion)?;
        patches.extend(extra);
        appends.push(append);
    }
    let mut components = Vec::new();
    let mut remove_owners = Vec::new();
    if let Some((_, rows)) = bank {
        if combined {
            // The trailing owner is replaced as a complete allocation. Its replica
            // row comes from a checked native graph with that exact storage shape.
            let replication = match rows.len() {
                1 => 0x80C3DDE2,
                2 => 0x80C0020A,
                _ => anyhow::bail!("Status modifier count needs a native replication witness"),
            };
            let (source_graph, patches) = modifier_patch(native, &rows)?;
            connected::allocation(native, source_graph, rows.len())?;
            components.push(NativeAssetComponent {
                source_graph,
                patches,
                replication: Some(replication),
            });
            remove_owners.push(graph.u32(owners[2])?);
        } else {
            patches.extend(modifier_patches(
                native,
                &graph,
                graph.u32(owners[1])?,
                &rows,
            )?);
        }
    }
    Ok((
        NativeAssetPatch {
            action_index: 0,
            source_graph: tag,
            patches,
            appends,
            remove_owners,
            components,
            programs: Vec::new(),
        },
        hud_status,
    ))
}
