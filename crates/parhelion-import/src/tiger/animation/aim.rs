//! Native turret aim: the pose definition's aim and look table and the aim clips it names.
//!
//! In Shadowkeep 86657 pose definition `80803640 +0x114` names an `8080348E` table. Its family at
//! `+0x08` aims and the one at `+0x38` looks. A family (`808032F6`) holds one signed alias per bank
//! descriptor at `+0x00` (`80800006`, `-1` leaves that descriptor unaimed), 16-byte `80803301`
//! choices at `+0x10` and 28-byte `808034AF` rows at `+0x20`. The loader reads neither alias array
//! against its length, so both must cover every descriptor. A row keeps two yaw bounds at `+0x00`
//! and `+0x04`, which every stock row leaves at the float maximum and its negative, the pitch bounds
//! in radians at `+0x08` and `+0x0C`, the aim mode at `+0x14` and the bank ordinals of the yaw and
//! pitch clips at `+0x16` and `+0x18`. `1049C70` copies the selected row into the animation state.
//! Mode 2 then plays the yaw and pitch clips under the bank names `aim_yaw` and `aim_pitch`
//! (`103FFB0`, `1041DD0`), and a bank without those names skips that axis.
//!
//! An aim clip is additive. Its blend space is a self-relative pointer at clip `+0x50` to an open
//! `80808F5F` or closed `80808F63` chain. Its vertex array at `+0x00` holds one (yaw, pitch) pair in
//! radians per frame and its edge array at `+0x10` four signed bytes per edge: the edge's two frames
//! and its previous and next edges. Clip byte `+0x143` names the kind, 1 for an open chain and 2 for
//! a closed one. `C81C70` projects the relative aim onto an open chain and clamps it at both ends.
//! `C82120` adds the closing edge. The relative yaw is the aim yaw less the entity's own facing.
use crate::{
    presentation::{append, put},
    tiger::payload::{Payload, compact},
};
use anyhow::{Context, Result, ensure};

pub const AIM_YAW: u32 = 0x2E2B_3ED4;
pub const AIM_PITCH: u32 = 0x843A_7A97;
/// The aim and look table reference in the pose definition.
pub const TABLE_FIELD: usize = 0x114;
const TABLE: u32 = 0x8080_348E;
const ALIAS: u32 = 0x8080_0006;
const CHOICE: u32 = 0x8080_3301;
const ROW: (u32, usize) = (0x8080_34AF, 0x1C);
const BANK_NAMES: (usize, u32) = (0x58, 0x8080_8F1C);
const BANK_DESCRIPTORS: usize = 0x68;
const BLEND_SPACE: usize = 0x50;
const KIND: usize = 0x143;
const VERTEX: u32 = 0x8080_8F6E;
const OPEN: (u32, u32) = (0x8080_8F5F, 0x8080_8F69);
const CLOSED: (u32, u32) = (0x8080_8F63, 0x8080_8F6C);

/// The aim row of a table whose aim family selects one constant row, as every stock aim table does.
#[derive(Clone, Copy, Debug)]
pub struct Row {
    pub mode: u8,
    pub yaw_clip: u16,
    pub pitch_clip: u16,
    pub pitch: [f32; 2],
}

/// The pose definition of a pose controller owner and the aim table it names, if any.
pub fn table_tag(pose: &Payload) -> Result<(usize, Option<u32>)> {
    let definition = pose.pointer(24)?;
    ensure!(
        definition >= 4 && pose.u32(definition - 4)? == 0x8080_3640,
        "Aim table requires a native pose definition"
    );
    let tag = pose.u32(definition + TABLE_FIELD)?;
    Ok((definition, (tag != u32::MAX).then_some(tag)))
}

fn single_row(table: &Payload) -> Result<usize> {
    let choices = table.array(0x18, 16, Some(CHOICE))?;
    let rows = table.array(0x28, ROW.1, Some(ROW.0))?;
    ensure!(
        choices.len() == 1 && rows.len() == 1 && table.u32(choices[0])? == 2,
        "Native aim family has another choice layout"
    );
    Ok(rows[0])
}

pub fn row(table: &Payload) -> Result<Row> {
    let row = single_row(table)?;
    Ok(Row {
        mode: table.u8(row + 0x14)?,
        yaw_clip: table.u16(row + 0x16)?,
        pitch_clip: table.u16(row + 0x18)?,
        pitch: [table.f32(row + 8)?, table.f32(row + 0xC)?],
    })
}

/// A private copy of a stock aim table that aims every one of a bank's `descriptors` through its
/// row, with the row's pitch bounds set to `pitch`, low then high, in radians. The look family keeps
/// its own aliases, extended with `-1`.
pub fn aliased(table: &Payload, descriptors: usize, pitch: [f32; 2]) -> Result<Payload> {
    ensure!(
        descriptors > 0
            && descriptors < i16::MAX as usize
            && pitch.iter().all(|v| v.is_finite())
            && pitch[0] < pitch[1],
        "Aim table needs descriptors and ordered pitch bounds"
    );
    let row = single_row(table)?;
    ensure!(
        table.u8(row + 0x14)? == 2,
        "Native aim row does not play separate yaw and pitch clips"
    );
    let mut out = table.clone();
    for (bound, value) in pitch.into_iter().enumerate() {
        put(&mut out.0, row + 8 + bound * 4, &value.to_le_bytes())?;
    }
    let aimed = vec![0u8; descriptors * 2];
    append(&mut out.0, 0x08, u64::from(ALIAS), &aimed, 2)?;
    let mut looked = table
        .array(0x38, 2, Some(ALIAS))?
        .into_iter()
        .map(|at| table.i16(at))
        .collect::<Result<Vec<_>>>()?;
    looked.resize(descriptors.max(looked.len()), -1);
    let looked = looked
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect::<Vec<_>>();
    append(&mut out.0, 0x38, u64::from(ALIAS), &looked, 2)?;
    Ok(compact(&out, TABLE)?.payload)
}

/// The vertices of an aim clip's blend space and whether its chain closes.
pub fn vertices(clip: &Payload) -> Result<(Vec<[f32; 2]>, bool)> {
    ensure!(clip.u64(BLEND_SPACE)? != 0, "Clip has no aim blend space");
    let space = clip.pointer(BLEND_SPACE)?;
    let closed = match clip.u32(space - 4)? {
        class if class == OPEN.0 => false,
        class if class == CLOSED.0 => true,
        class => anyhow::bail!("Unsupported aim blend space {class:08X}"),
    };
    let rows = clip.array(space, 8, Some(VERTEX))?;
    ensure!(
        rows.len() == usize::from(clip.u16(0x13C)?) && clip.u8(KIND)? == 1 + u8::from(closed),
        "Aim blend space differs from its clip"
    );
    Ok((
        rows.into_iter()
            .map(|at| Ok([clip.f32(at)?, clip.f32(at + 4)?]))
            .collect::<Result<_>>()?,
        closed,
    ))
}

fn axis_angle(q: [f32; 4]) -> ([f32; 3], f32) {
    let q = if q[3] < 0. { q.map(|v| -v) } else { q };
    let angle = 2. * q[3].clamp(-1., 1.).acos();
    let s = (1. - q[3] * q[3]).max(0.).sqrt();
    if s < 1e-6 {
        ([0.; 3], 0.)
    } else {
        ([q[0] / s, q[1] / s, q[2] / s], angle)
    }
}

pub fn rotation(axis: [f32; 3], angle: f32) -> [f32; 4] {
    let (s, c) = (angle * 0.5).sin_cos();
    [axis[0] * s, axis[1] * s, axis[2] * s, c]
}

/// The rotation `b` relative to `a`, `conj(a) * b`, for unit quaternions `[x, y, z, w]`.
fn relative(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = [-a[0], -a[1], -a[2], a[3]];
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// The controls a stock aim clip turns and, for each, the local axis that a positive vertex angle
/// turns it about. `axis` picks the vertex lane, 0 for yaw and 1 for pitch. The frame whose angle is
/// furthest from both zero and a half turn names the axis without ambiguity. Each control's turn is
/// measured from the clip's neutral frame, the vertex nearest zero, so a control the clip holds at a
/// constant rotation, such as a root in its rest frame, does not count as turning.
pub fn axes(clip: &Payload, axis: usize) -> Result<Vec<(usize, [f32; 3])>> {
    ensure!(clip.u8(0x181)? == 1 && axis < 2, "Aim clip is not additive");
    let (vertices, _) = vertices(clip)?;
    let (frame, value) = vertices
        .iter()
        .enumerate()
        .map(|(frame, v)| (frame, v[axis]))
        .max_by(|a, b| a.1.sin().abs().total_cmp(&b.1.sin().abs()))
        .context("Aim clip has no vertices")?;
    let (neutral, neutral_value) = vertices
        .iter()
        .enumerate()
        .map(|(frame, v)| (frame, v[axis]))
        .min_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
        .context("Aim clip has no vertices")?;
    let turn = value - neutral_value;
    ensure!(
        value.sin().abs() > 0.1 && turn.abs() > 0.1,
        "Aim clip vertices do not turn"
    );
    let slots = usize::from(clip.u16(0x140)?);
    let identity = vec![[0., 0., 0., 1., 0., 0., 0., 1.]; slots];
    let pose = super::clip::sample(clip, &identity, frame)?;
    let rest = super::clip::sample(clip, &identity, neutral)?;
    let mut found = Vec::new();
    for (slot, (pose, rest)) in pose.iter().zip(&rest).enumerate() {
        let (direction, angle) = axis_angle(relative(rest[..4].try_into()?, pose[..4].try_into()?));
        // A held control drifts by its compression error, as the tank yaw clip's root does by
        // 1.3 degrees over a 99.7 degree sweep. An aim control turns by the sweep itself.
        if angle.to_degrees() < 0.5 || angle < turn.abs() * 0.1 {
            continue;
        }
        // The turn from the neutral frame equals the vertex change for a single-axis aim clip.
        ensure!(
            (angle - turn.abs()).abs().to_degrees() < 2.,
            "Aim clip control {slot} turns {:.1} degrees from frame {neutral} to {frame} where its vertices turn {:.1}",
            angle.to_degrees(),
            turn.to_degrees()
        );
        let sign = turn.signum();
        found.push((slot, direction.map(|v| v * sign)));
    }
    ensure!(!found.is_empty(), "Aim clip turns no control");
    Ok(found)
}

/// An additive aim clip on the rig of `controls`: `turns` rotate each listed control by the vertex
/// angle in lane `axis` about its local axis, and every other control holds the identity delta.
/// The template is a stock aim clip of the same family, which supplies the duration, events and
/// envelope. The frame count follows the vertices.
pub fn clip(
    template: &Payload,
    controls: &Payload,
    bones: usize,
    turns: &[(usize, [f32; 3])],
    axis: usize,
    vertices: &[[f32; 2]],
    closed: bool,
) -> Result<Payload> {
    let frames = vertices.len();
    ensure!(
        (2..=127).contains(&frames)
            && axis < 2
            && vertices.iter().flatten().all(|v| v.is_finite())
            && template.u8(0x181)? == 1
            && [0x38, 0x40, 0x48]
                .into_iter()
                .all(|field| template.u64(field).is_ok_and(|v| v == 0)),
        "Aim clip needs an additive template without constraints and up to 127 vertices"
    );
    let slots = controls
        .array(controls.pointer(24)? + 0xA0, 52, Some(0x8080_9076))?
        .len();
    let identity = [0., 0., 0., 1., 0., 0., 0., 1.];
    let mut deltas = vec![vec![identity; frames]; slots];
    for &(slot, direction) in turns {
        let track = deltas
            .get_mut(slot)
            .context("Aim control is outside its rig")?;
        for (pose, vertex) in track.iter_mut().zip(vertices) {
            pose[..4].copy_from_slice(&rotation(direction, vertex[axis]));
        }
    }
    let mut envelope = template.clone();
    put(
        &mut envelope.0,
        0x13C,
        &u16::try_from(frames)?.to_le_bytes(),
    )?;
    let mut out = super::clip::replace_deltas_for(&envelope, &deltas, bones, controls)?.0;
    let (class, edge) = if closed { CLOSED } else { OPEN };
    let space = (out.len() + 11) & !7;
    out.resize(space - 4, 0);
    out.extend(class.to_le_bytes());
    out.resize(space + 0x20, 0);
    put(
        &mut out,
        BLEND_SPACE,
        &(space as i64 - BLEND_SPACE as i64).to_le_bytes(),
    )?;
    let points = vertices
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect::<Vec<_>>();
    append(&mut out, space, u64::from(VERTEX), &points, 8)?;
    let count = if closed { frames } else { frames - 1 };
    let mut edges = Vec::with_capacity(count * 4);
    for index in 0..count {
        let previous = if index == 0 {
            if closed {
                count - 1
            } else {
                usize::from(u8::MAX)
            }
        } else {
            index - 1
        };
        let next = if index + 1 == count {
            if closed { 0 } else { usize::from(u8::MAX) }
        } else {
            index + 1
        };
        for value in [index, (index + 1) % frames, previous, next] {
            edges.push(u8::try_from(value)?);
        }
    }
    append(&mut out, space + 0x10, u64::from(edge), &edges, 4)?;
    out[KIND] = 1 + u8::from(closed);
    let length = out.len() as u64;
    put(&mut out, 0, &length.to_le_bytes())?;
    Ok(compact(&Payload(out), 0x8080_8F49)?.payload)
}

/// The bank with `names` in its name list, or `None` when it already has them. Clip references
/// keep their offsets, since the new list is appended.
pub fn with_names(bank: &Payload, names: &[u32]) -> Result<Option<Payload>> {
    let present = bank
        .array(BANK_NAMES.0, 4, Some(BANK_NAMES.1))?
        .into_iter()
        .map(|at| bank.u32(at))
        .collect::<Result<Vec<_>>>()?;
    let missing = names
        .iter()
        .filter(|name| !present.contains(name))
        .copied()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(None);
    }
    let rows = present
        .iter()
        .chain(&missing)
        .flat_map(|v| v.to_le_bytes())
        .collect::<Vec<_>>();
    let mut out = bank.clone();
    append(&mut out.0, BANK_NAMES.0, u64::from(BANK_NAMES.1), &rows, 4)?;
    Ok(Some(out))
}

/// The descriptor count of a native bank, which both alias families must cover.
pub fn descriptors(bank: &Payload) -> Result<usize> {
    Ok(bank.array(BANK_DESCRIPTORS, 32, Some(0x8080_9002))?.len())
}

/// Evenly spaced vertices over `bounds` in lane `axis`, at most `step` radians apart and at least
/// two. A closed chain starts one degree past its low bound, as the stock yaw chains start at -179
/// degrees, so its closing edge joins two distinct vertices of the same pose.
pub fn sweep(axis: usize, bounds: [f32; 2], step: f32, closed: bool) -> Result<Vec<[f32; 2]>> {
    ensure!(
        axis < 2 && step > 0. && bounds[0] < bounds[1],
        "Aim sweep bounds are invalid"
    );
    let span = bounds[1] - bounds[0];
    let intervals = ((span / step).ceil() as usize).max(1);
    Ok((0..=intervals)
        .map(|index| {
            let mut value = bounds[0] + span * index as f32 / intervals as f32;
            if closed && index == 0 {
                value += 1f32.to_radians().min(span / intervals as f32 / 2.);
            }
            let mut vertex = [0.; 2];
            vertex[axis] = value;
            vertex
        })
        .collect())
}
