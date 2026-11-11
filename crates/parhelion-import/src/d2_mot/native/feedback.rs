//! Modern feedback resources, rewritten as Shadowkeep feedback resources.
//!
//! A feedback event names a feedback resource, which names its members through 8-byte rows with
//! the member tag at `+04`. Each modern kind has a Shadowkeep twin of the same size and layout:
//!
//! | Modern | Shadowkeep | Role |
//! | --- | --- | --- |
//! | `808031DE` | `80803EB0` | Feedback resource |
//! | `808031C7` | `80803E99` | Member |
//! | `808031D3` | `80803EA5` | Member |
//! | `808031D8` | `80803EAA` | Member |
//!
//! Translation keeps every byte but three kinds of word. Each array's marker, the word before its
//! 16-aligned header, becomes Shadowkeep's (`80809FB8` to `80809FBD`). Member rows change class
//! (`80803005` to `80803D95`). Each numeric program's bytecode is lowered to Shadowkeep's opcodes,
//! which keeps its length. A program record is 64 bytes: bytecode (`80800009`), constants
//! (`80800090`, empty in some), its input count at `+20` and one at `+28`. Member rows are 0x24
//! bytes, and every tag in one must name a member kind. Member tags are left for the caller,
//! which binds the translated members. Twin resources of 788 weapons present in
//! both versions differ in exactly these words. Tuning values are the source's own.
use crate::d2_mot::{entity::sequence::lower_expression, payload::Payload, reader::Reader};
use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeSet;

const KINDS: [(u32, u32); 4] = [
    (0x8080_31DE, 0x8080_3EB0),
    (0x8080_31C7, 0x8080_3E99),
    (0x8080_31D3, 0x8080_3EA5),
    (0x8080_31D8, 0x8080_3EAA),
];
const MARKER: (u32, u32) = (0x8080_9FB8, 0x8080_9FBD);
const CODE: u32 = 0x8080_0009;
const CONSTANTS: u32 = 0x8080_0090;
const MEMBERS: (u32, u32) = (0x8080_3005, 0x8080_3D95);

/// One translated resource. `members` are the offsets of member tags in `payload` with the source
/// member each names, to be replaced by the translated member's tag.
pub struct Resource {
    pub source: u32,
    pub class: u32,
    pub payload: Payload,
    pub members: Vec<(usize, u32)>,
}

/// An array descriptor at `at`: its row count and header. Only a descriptor whose header is
/// 16-aligned, repeats the count and carries the modern marker is one.
fn descriptor(p: &Payload, at: usize) -> Option<(usize, usize)> {
    let count = p.u64(at).ok()?;
    if count == 0 || count > 100_000 {
        return None;
    }
    let header = p.pointer(at + 8).ok()?;
    (header > at
        && header % 16 == 0
        && header + 16 <= p.0.len()
        && p.u64(header).ok()? == count
        && p.u32(header - 4).ok()? == MARKER.0)
        .then_some((usize::try_from(count).ok()?, header))
}

fn put(p: &mut Payload, at: usize, bytes: &[u8]) -> Result<()> {
    p.0.get_mut(at..at + bytes.len())
        .context("feedback write outside payload")?
        .copy_from_slice(bytes);
    Ok(())
}

/// The lowered bytecode of the program record at `at`, whose `count` code bytes start at `rows`.
fn program(
    original: &Payload,
    source: u32,
    at: usize,
    rows: usize,
    count: usize,
) -> Result<Vec<u8>> {
    ensure!(
        original.u64(at + 40)? == 1,
        "feedback program {source:08X}+{at:X} dimensions differ"
    );
    let constants = match descriptor(original, at + 16) {
        Some((constants, h)) if original.u32(h + 8)? == CONSTANTS => constants,
        None if original.u64(at + 16)? == 0 => 0,
        _ => bail!("feedback program {source:08X}+{at:X} constants differ"),
    };
    let inputs = usize::try_from(original.u64(at + 32)?)?;
    let code = original
        .0
        .get(rows..rows + count)
        .context("feedback program extent")?;
    let lowered = lower_expression(code, constants, inputs)
        .with_context(|| format!("feedback program {source:08X}+{at:X}"))?;
    ensure!(
        lowered.len() == code.len(),
        "feedback program {source:08X}+{at:X} changes length"
    );
    Ok(lowered)
}

/// Translate one payload in place.
fn translate(source: u32, class: u32, original: &Payload) -> Result<Resource> {
    ensure!(
        original.u64(0)? == original.0.len() as u64,
        "feedback resource {source:08X} size differs"
    );
    let mut payload = original.clone();
    let mut members = Vec::new();
    let mut headers = BTreeSet::new();
    for at in (0..original.0.len().saturating_sub(16)).step_by(8) {
        let Some((count, header)) = descriptor(original, at) else {
            continue;
        };
        headers.insert(header);
        put(&mut payload, header - 4, &MARKER.1.to_le_bytes())?;
        let rows = header + 16;
        match original.u32(header + 8)? {
            CODE => {
                let lowered = program(original, source, at, rows, count)?;
                put(&mut payload, rows, &lowered)?;
            }
            CONSTANTS => {}
            class if class == MEMBERS.0 => {
                put(&mut payload, header + 8, &MEMBERS.1.to_le_bytes())?;
                // A row is 0x24 bytes with up to three member slots, unset ones `FFFFFFFF`, as
                // Shadowkeep's own rows are.
                for word in (rows..rows + count * 0x24).step_by(4) {
                    let tag = original.u32(word)?;
                    if (0x80A0_0000..0x8200_0000).contains(&tag) {
                        members.push((word, tag));
                    }
                }
            }
            other => bail!("feedback resource {source:08X} has array class {other:08X}"),
        }
    }
    // A marker this walk did not reach would leave a modern array in the native payload.
    for at in (12..payload.0.len().saturating_sub(3)).step_by(16) {
        ensure!(
            payload.u32(at)? != MARKER.0 || headers.contains(&(at + 4)),
            "feedback resource {source:08X} has an unread array at {:X}",
            at + 4
        );
    }
    Ok(Resource {
        source,
        class,
        payload,
        members,
    })
}

/// The feedback resource `root` and every member it reaches, root first, each once.
pub fn convert(reader: &mut Reader, root: u32) -> Result<Vec<Resource>> {
    let mut pending = vec![root];
    let mut seen = BTreeSet::new();
    let mut converted = Vec::new();
    while let Some(tag) = pending.pop() {
        if !seen.insert(tag) {
            continue;
        }
        let modern = reader.reference(tag)?;
        let (_, class) = *KINDS
            .iter()
            .find(|(source, _)| *source == modern)
            .with_context(|| format!("feedback resource {tag:08X} has class {modern:08X}"))?;
        if tag == root {
            ensure!(
                class == KINDS[0].1,
                "feedback event names a {modern:08X} resource"
            );
        }
        let payload = reader.tag(tag, Some(modern))?;
        let resource = translate(tag, class, &payload)?;
        pending.extend(resource.members.iter().rev().map(|&(_, member)| member));
        converted.push(resource);
    }
    Ok(converted)
}
