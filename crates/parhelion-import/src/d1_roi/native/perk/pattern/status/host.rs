//! A standalone Duration host, the Duration of a status without its HUD.
//!
//! Rise of Iron host `808029C3` (instance `8080222E`) embeds Duration `8080223C` at `+60`, its
//! Timer `80802890` at `+118` and lifetime expression `8080038D` at `+198`, at the offsets the
//! connected status embeds them after its HUD. Shadowkeep host `80803B30` (instance `80803B2F`)
//! embeds Duration `80803B33` at `+48`, its Timer at `+F8` and expression `808089F6` at `+170`.
//! The invisibility hosts of both builds hold the same Duration key `02B4C507`, priority 1 and
//! lifetime program form, so the host reads the status's Duration fields and lifetime rules.
use super::*;

/// The source values a native Duration host stores, with the lifetime input row that graph
/// connections name.
pub(crate) struct Host {
    pub duration: [u8; 20],
    pub timer: [u8; 8],
    pub lifetime: Lifetime,
    pub lifetime_input: Option<usize>,
}

pub(crate) fn read(graph: &Payload, p: &Payload, owner: u32, ordinal: usize) -> Result<Host> {
    let (live, s) = (p.pointer(16)?, p.pointer(24)?);
    let d = s + 0x60;
    let timer = d + 0xB8;
    ensure!(
        p.u32(s - 4)? == 0x808029C3 && p.u32(live - 4)? == 0x8080222E,
        "Source Duration host root differs"
    );
    for (i, def, ic, dc) in [
        (live, s, 0x8080222E, 0x808029C3),
        (live + 0x30, d, 0x8080255D, 0x8080223C),
        (live + 0x60, timer, 0x808027B5, 0x80802890),
        (live + 0x80, d + 0x138, 0x808008AC, 0x8080038D),
    ] {
        pair(p, owner, i, def, ic, dc)?;
    }
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
            p.u64(s + offset)? == value,
            "Source Duration host has additional state"
        );
    }
    let declarations: [(usize, usize, u32, u32, u64, u32); 9] = [
        (d + 0x18, live + 0x30, 0x8080255D, 0x80AAC286, 0, 0),
        (d + 0x38, live + 0x30, 0x8080255D, 0x80AAC284, 0, 0),
        (d + 0x58, live + 0x30, 0x8080255D, 0x80AAEDB3, 0, 0),
        (d + 0x70, live + 0x30, 0x8080255D, 0x80AAC288, 0, 0),
        (d + 0x88, live + 0x30, 0x8080255D, 0x80AAC444, 0, 0),
        (timer + 0x18, live + 0x60, 0x808027B5, 0x80AAEDB4, 0xFF9C, 0),
        (timer + 0x30, live + 0x60, 0x808027B5, 0x80AAC285, 0, 1),
        (timer + 0x48, live + 0x60, 0x808027B5, 0x80AAC289, 0, 0),
        (timer + 0x60, live + 0x60, 0x808027B5, 0x80AAC287, 0, 0),
    ];
    let exports = graph.array(0x68, 40, Some(0x80800370))?;
    let refs = graph.array(0x78, 24, Some(0x80800790))?;
    ensure!(
        exports.len() == refs.len(),
        "Source Duration host export references differ"
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
            .context("Unknown source Duration host export")?;
        let container = if class == 0x8080255D { d } else { timer };
        let selector = if value == 0 {
            0
        } else {
            (value as u16 as i16 as i32) as u32
        };
        // As on connected statuses, exclusion bits number every exclusive provider of the graph.
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
            "Source Duration host export targets another parameter"
        );
    }
    ensure!(
        seen.len() == declarations.len(),
        "Source Duration host exports are incomplete"
    );
    // The Duration flags and Timer byte have the consumers recorded on connected statuses.
    let flags = p.u32(d + 0xA0)?;
    ensure!(
        p.u64(d + 0x30)? == 0x8080299F
            && p.u64(d + 0x50)? == 0x80802B20
            && flags & !0x1F == 0
            && i32::from_le_bytes(p.bytes(d + 0xA8)?) >= 0
            && p.u64(d + 0xB0)? <= 1
            && p.u64(timer + 0x78)? <= 1,
        "Source Duration host has another matching or timer policy"
    );
    let lifetime_at = d + 0x138;
    ensure!(
        p.u32(lifetime_at + 16)? == 0x808008AC
            && p.u64(lifetime_at + 88)? == 0
            && [0, 0x811C9DC5].contains(&p.u64(lifetime_at + 96)?),
        "Source Duration host lifetime has additional flags or a named input"
    );
    let (code, constants, input) = program(p, lifetime_at)?;
    let indefinite = p.u64(timer + 0x78)? == 1;
    let lifetime = match input {
        None => unnamed(p, lifetime_at, indefinite, code, constants)?,
        Some((name, _)) => Lifetime::Program {
            code,
            constants,
            input: name,
        },
    };
    if flags & 0x10 != 0 {
        let positive = match &lifetime {
            Lifetime::Constant(seconds) => f32::from_le_bytes(seconds[..4].try_into()?) > 0.0,
            Lifetime::Program { .. } | Lifetime::Builtin { .. } => false,
        };
        ensure!(
            positive,
            "Source refreshing Duration host needs a positive constant lifetime"
        );
    }
    Ok(Host {
        duration: p.bytes(d + 0xA0)?,
        timer: p.bytes(timer + 0x78)?,
        lifetime,
        lifetime_input: input.map(|(_, row)| row),
    })
}
