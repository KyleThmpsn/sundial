//! Native supplemental layer tables (`808034EB`): clip storage sizes, appended direct-phase
//! layers and donor layers kept off complete source actions.
use crate::{
    presentation::{append, put},
    tiger::{payload::Payload, reader::Reader},
};
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, BTreeSet};

pub fn clip_sizes(payload: &mut Payload, sizes: &[u32]) -> Result<()> {
    for (field, layer_class, choice_class, option_class, stride) in [
        (8, 0x80803724, 0x8080372E, 0x80803730, 64),
        (24, 0x80803721, 0x80803729, 0x8080372B, 72),
    ] {
        for layer in payload.array(field, 24, Some(layer_class))? {
            for choice in payload.array(layer, 16, Some(choice_class))? {
                for option in payload.array(choice, stride, Some(option_class))? {
                    for weight in payload.array(option, 12, Some(0x80803737))? {
                        let index = usize::try_from(payload.u32(weight)?)?;
                        let size = sizes.get(index).context("Layer clip is outside its bank")?;
                        put(&mut payload.0, weight + 8, &size.to_le_bytes())?;
                    }
                }
            }
        }
    }
    Ok(())
}

/// One clip played by direct phase. Option byte `+3F` equal to 2 reads the option's parameter,
/// clamps it to `[0, 1]` and multiplies it by the option's duration for the playback time.
pub struct Phase {
    pub name: u32,
    /// Packed parameter reference. Bit 16 selects the controller's global parameter array.
    pub parameter: u32,
    /// Bank clip ordinal and its storage size word.
    pub clip: u32,
    pub size: u32,
    pub duration: f32,
}

fn rows(p: &Payload, fields: &[usize], stride: usize) -> Vec<u8> {
    fields
        .iter()
        .flat_map(|&at| p.0[at..at + stride].iter().copied())
        .collect()
}

fn relative(p: &mut Payload, field: usize, target: usize) -> Result<()> {
    put(
        &mut p.0,
        field,
        &(target as i64 - field as i64).to_le_bytes(),
    )
}

/// Appends a first-family layer that plays `phase` on every route, after every existing layer.
/// Its option copies the flags of the table's first direct-phase option, without that option's
/// clips or conditions. Descriptors without supplemental layers take a new route holding only
/// this layer. Returns the table and the new layer's index.
pub fn append_phase(table: &Payload, phase: &Phase) -> Result<(Payload, usize)> {
    ensure!(
        phase.duration.is_finite() && phase.duration > 0.,
        "Direct-phase duration must be positive"
    );
    let old = table.array(8, 24, Some(0x80803724))?;
    for &layer in &old {
        ensure!(
            table.u32(layer + 16)? != phase.name,
            "Appended pose layer name repeats"
        );
    }
    let mut template = None;
    'layers: for &layer in &old {
        for choice in table.array(layer, 16, Some(0x8080372e))? {
            for option in table.array(choice, 64, Some(0x80803730))? {
                if table.0[option + 63] == 2 {
                    template = Some((layer, option));
                    break 'layers;
                }
            }
        }
    }
    let (layer, option) = template.context("Pose layers have no direct-phase option")?;
    let mut p = table.clone();
    let mut data = rows(table, &old, 24);
    data.resize(data.len() + 24, 0);
    append(&mut p.0, 8, 0x80803724, &data, 24)?;
    let written = p.array(8, 24, Some(0x80803724))?;
    for (&from, &to) in old.iter().zip(&written) {
        if table.u64(from)? != 0 {
            relative(&mut p, to + 8, table.pointer(from + 8)?)?;
        }
    }
    let order = old
        .iter()
        .map(|&row| table.u16(row + 20))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .max()
        .context("Empty pose layers")?;
    let index = old.len();
    let row = written[index];
    put(&mut p.0, row + 16, &phase.name.to_le_bytes())?;
    put(
        &mut p.0,
        row + 20,
        &order
            .checked_add(1)
            .context("Pose layer order overflow")?
            .to_le_bytes(),
    )?;
    put(&mut p.0, row + 22, &table.bytes::<2>(layer + 22)?)?;
    append(&mut p.0, row, 0x8080372e, &[0; 16], 16)?;
    let choice = p.array(row, 16, Some(0x8080372e))?[0];
    // The first 32 bytes hold the clip and condition arrays.
    let mut record = table.bytes::<64>(option)?;
    record[..32].fill(0);
    record[52..56].copy_from_slice(&phase.duration.to_le_bytes());
    record[56..60].copy_from_slice(&phase.parameter.to_le_bytes());
    append(&mut p.0, choice, 0x80803730, &record, 64)?;
    let selection = p.array(choice, 64, Some(0x80803730))?[0];
    let mut weight = [0; 12];
    weight[..4].copy_from_slice(&phase.clip.to_le_bytes());
    weight[4..8].copy_from_slice(&1f32.to_le_bytes());
    weight[8..].copy_from_slice(&phase.size.to_le_bytes());
    append(&mut p.0, selection, 0x80803737, &weight, 12)?;
    let routes = table.array(56, 32, Some(0x808034ef))?;
    let mut data = rows(table, &routes, 32);
    data.resize(data.len() + 32, 0);
    append(&mut p.0, 56, 0x808034ef, &data, 32)?;
    let written = p.array(56, 32, Some(0x808034ef))?;
    let edge = [0u16.to_le_bytes(), u16::try_from(index)?.to_le_bytes()].concat();
    for (route, &row) in written.iter().enumerate() {
        let mut edges = if let Some(&old) = routes.get(route) {
            if table.u64(old + 16)? != 0 {
                relative(&mut p, row + 24, table.pointer(old + 24)?)?;
            }
            rows(table, &table.array(old, 4, Some(0x808034f1))?, 4)
        } else {
            Vec::new()
        };
        edges.extend(&edge);
        append(&mut p.0, row, 0x808034f1, &edges, 4)?;
    }
    let fallback = u16::try_from(routes.len())?.to_le_bytes();
    for at in table.array(40, 2, Some(0x80800006))? {
        if table.u16(at)? == u16::MAX {
            put(&mut p.0, at, &fallback)?;
        }
    }
    Ok((p, index))
}

/// First-family layers that couple the donor arms' own actions to each other. Each is additive
/// and authored against the donor's clips. The procedural look, movement, velocity and crouch
/// layers stay.
///
/// - `FC23AA50`, the action correction, plays on idle and reload but not firing. It holds idle at
///   the end pose of firing: on scout rifle base `62629194` its idle clip `80BC5F7D` moves each
///   arm control by exactly the 0.05 to 0.14 m between idle's start and firing's end.
/// - `D3B8D0AC`, `charge_fire`, plays on idle routes at a phase that rises while firing. On
///   sidearm Buzzard `3005A7F1` it moves the grip up to 0.053 m and carries hand targets.
/// - `E4E61EE3` plays on firing routes. On Buzzard it kicks the grip up to 0.074 m and 23 degrees.
/// - `4D467CC1` plays the donor's sprint sway on sprint loop and exit routes, phased by
///   `sprint_state`, over a loop clip that barely moves. On Buzzard it moves arm controls up to
///   0.156 m and carries hand targets.
pub const ACTION_COUPLED: [u32; 4] = [0xfc23aa50, 0xd3b8d0ac, 0xe4e61ee3, 0x4d467cc1];

/// The largest translation (metres) and rotation (degrees) by which a translated clip may differ
/// from the native clip it replaced, on a layer's controls, for that layer to still fit it.
const SAME_POSE: (f32, f32) = (0.01, 2.0);

/// A complete clip a translated descriptor plays and the native clip it replaced.
pub struct Translated {
    pub clip: Payload,
    pub template: Payload,
    /// The coupled layers the source authored for this descriptor, by layer name, with the choices
    /// of that layer its own route takes. An empty list means its route leaves the layer out, as
    /// for a native layer whose offset a source layer folds in, or one the source table lacks and
    /// whose motion the source clips already hold. Each replaces the donor's same-named layer on
    /// the route, whether that fits or not, since the source's own coupling was authored against
    /// its own clips.
    pub own: BTreeMap<u32, Vec<u16>>,
}

/// One option of an appended layer choice, with no conditions.
pub struct LayerOption {
    /// Weighted bank clips: ordinal, cumulative weight and storage size ([`super::clip::size`]).
    pub clips: Vec<(u32, f32, u32)>,
    /// Option bytes `+20..+40`: driver kind, constant, driver parameter, rate, duration, phase
    /// parameter, flags and playback mode at `+3F`.
    pub playback: [u8; 32],
}

/// Appends `choices` to the first-family layer named `name`, each with its options in order.
/// Existing choices keep their indices, so routes stay valid. Returns the table, the layer's index
/// and the index of the first appended choice. The layer count is unchanged, so the controller's
/// per-layer state needs no growth.
pub fn append_choices(
    table: &Payload,
    name: u32,
    choices: &[Vec<LayerOption>],
) -> Result<(Payload, usize, u16)> {
    let layers = table.array(8, 24, Some(0x80803724))?;
    let named = layers
        .iter()
        .enumerate()
        .filter(|&(_, &at)| table.u32(at + 16).ok() == Some(name))
        .collect::<Vec<_>>();
    ensure!(
        named.len() == 1,
        "Pose layer {name:08X} is absent or repeated"
    );
    let (index, &layer) = named[0];
    let old = table.array(layer, 16, Some(0x8080372e))?;
    let first = old.len();
    ensure!(
        first + choices.len() <= i16::MAX as usize,
        "Pose layer choices exceed native limits"
    );
    let mut p = table.clone();
    let mut data = rows(table, &old, 16);
    data.resize(data.len() + choices.len() * 16, 0);
    append(&mut p.0, layer, 0x8080372e, &data, 16)?;
    let written = p.array(layer, 16, Some(0x8080372e))?;
    for (&from, &to) in old.iter().zip(&written) {
        if table.u64(from)? != 0 {
            relative(&mut p, to + 8, table.pointer(from + 8)?)?;
        }
    }
    for (options, &choice) in choices.iter().zip(&written[first..]) {
        ensure!(!options.is_empty(), "Appended pose layer choice is empty");
        let mut records = Vec::new();
        for option in options {
            // The condition array at +10 stays empty, so the option always applies.
            let mut record = [0; 64];
            record[32..].copy_from_slice(&option.playback);
            records.extend(record);
        }
        append(&mut p.0, choice, 0x80803730, &records, 64)?;
        for (option, row) in options.iter().zip(p.array(choice, 64, Some(0x80803730))?) {
            ensure!(
                !option.clips.is_empty(),
                "Appended pose layer option is empty"
            );
            let weights = option
                .clips
                .iter()
                .flat_map(|&(clip, weight, size)| {
                    [clip.to_le_bytes(), weight.to_le_bytes(), size.to_le_bytes()].concat()
                })
                .collect::<Vec<_>>();
            append(&mut p.0, row, 0x80803737, &weights, 12)?;
        }
    }
    Ok((p, index, u16::try_from(first)?))
}

/// Adds the controls layer clip `clip` moves at any frame, sampled as offsets, by at least the
/// [`SAME_POSE`] tolerance. A quantized identity rotation decodes up to 0.9 degrees from identity,
/// so a lower threshold counts controls the layer does not move. The sprint layer `4D467CC1`
/// touches a finger slot only by that much, so a clip whose finger ends 2.7 degrees from the
/// template's would otherwise lose the layer that moves its upper arms 13 to 16 cm.
fn moved(clip: &Payload, slots: &mut BTreeSet<usize>) -> Result<()> {
    let frames = usize::from(clip.u16(0x13C)?);
    let identity = vec![[0., 0., 0., 1., 0., 0., 0., 1.]; usize::from(clip.u16(0x140)?)];
    for frame in 0..frames {
        for (slot, pose) in super::clip::sample(clip, &identity, frame)?
            .iter()
            .enumerate()
        {
            let translation = pose[4..7].iter().map(|v| v * v).sum::<f32>().sqrt();
            let angle = 2. * pose[3].abs().clamp(0., 1.).acos().to_degrees();
            if translation >= SAME_POSE.0 || angle >= SAME_POSE.1 {
                slots.insert(slot);
            }
        }
    }
    Ok(())
}

/// Whether `clip` starts and ends where `template` does on `slots`, so a layer authored against
/// the template still fits it.
fn same_pose(clip: &Payload, template: &Payload, slots: &BTreeSet<usize>) -> Result<bool> {
    if clip.u8(0x181)? != template.u8(0x181)? || clip.u16(0x140)? != template.u16(0x140)? {
        return Ok(false);
    }
    let identity = vec![[0., 0., 0., 1., 0., 0., 0., 1.]; usize::from(clip.u16(0x140)?)];
    let last = |p: &Payload| -> Result<usize> { Ok(usize::from(p.u16(0x13C)?).saturating_sub(1)) };
    for (a, b) in [(0, 0), (last(clip)?, last(template)?)] {
        let ours = super::clip::sample(clip, &identity, a)?;
        let theirs = super::clip::sample(template, &identity, b)?;
        for &slot in slots {
            let (Some(x), Some(y)) = (ours.get(slot), theirs.get(slot)) else {
                return Ok(false);
            };
            let translation = (0..3)
                .map(|i| (x[4 + i] - y[4 + i]).powi(2))
                .sum::<f32>()
                .sqrt();
            let dot = (0..4).map(|i| x[i] * y[i]).sum::<f32>().abs().clamp(0., 1.);
            if translation > SAME_POSE.0 || 2. * dot.acos().to_degrees() > SAME_POSE.1 {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// The additive [`ACTION_COUPLED`] layers of `table`, by layer index, with their names and the
/// controls their clips move. A layer that plays any replacement or absolute clip is left out.
fn coupled_layers(
    native: &mut Reader,
    table: &Payload,
    bank: &Payload,
) -> Result<BTreeMap<usize, (u32, BTreeSet<usize>)>> {
    let clips = bank.array(8, 4, Some(0x80808f48))?;
    let mut coupled = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for (index, at) in table
        .array(8, 24, Some(0x80803724))?
        .into_iter()
        .enumerate()
    {
        let name = table.u32(at + 16)?;
        if !ACTION_COUPLED.contains(&name) {
            continue;
        }
        ensure!(seen.insert(name), "Repeated action-coupled pose layer");
        let mut additive = true;
        let mut slots = BTreeSet::new();
        for choice in table.array(at, 16, Some(0x8080372e))? {
            for option in table.array(choice, 64, Some(0x80803730))? {
                for weight in table.array(option, 12, Some(0x80803737))? {
                    let slot = usize::try_from(table.u32(weight)?)?;
                    let tag = bank.u32(*clips.get(slot).context("Coupled layer clip missing")?)?;
                    let clip = native.tag(tag, Some(0x80808f49))?;
                    additive &= clip.u8(0x181)? == 1;
                    if additive {
                        moved(&clip, &mut slots)?;
                    }
                }
            }
        }
        if additive {
            coupled.insert(index, (name, slots));
        }
    }
    Ok(coupled)
}

/// What routing one translated descriptor does to its route's first-family edges.
struct Rerouted {
    edges: Vec<u8>,
    omitted: BTreeSet<u32>,
    kept: BTreeSet<u32>,
}

/// The first-family edges of `route` for a descriptor playing `translated`. Layers the source
/// authored for it take its own choices in table order. Other coupled layers stay only where they
/// fit the translated clip.
fn reroute(
    route: &[u8],
    names: &[u32],
    coupled: &BTreeMap<usize, (u32, BTreeSet<usize>)>,
    translated: Option<&Translated>,
) -> Result<Rerouted> {
    let mut edges = Vec::new();
    let mut omitted = BTreeSet::new();
    let mut kept = BTreeSet::new();
    let own = translated.map(|translated| &translated.own);
    for edge in route.chunks_exact(4) {
        let layer = usize::from(u16::from_le_bytes([edge[2], edge[3]]));
        let name = *names
            .get(layer)
            .context("Pose route layer is outside its table")?;
        if own.is_some_and(|own| own.contains_key(&name)) {
            continue;
        }
        if let Some((name, slots)) = coupled.get(&layer) {
            let fits = match translated {
                Some(translated) => same_pose(&translated.clip, &translated.template, slots)?,
                None => false,
            };
            if !fits {
                omitted.insert(*name);
                continue;
            }
            kept.insert(*name);
        }
        edges.push((layer, edge.to_vec()));
    }
    for (name, choices) in own.into_iter().flatten() {
        let layer = names
            .iter()
            .position(|other| other == name)
            .context("Source-authored pose layer is absent from the native table")?;
        for &choice in choices {
            // Routes list their layers in table order.
            let at = edges
                .iter()
                .position(|(other, _)| *other > layer)
                .unwrap_or(edges.len());
            let edge = [choice.to_le_bytes(), u16::try_from(layer)?.to_le_bytes()].concat();
            edges.insert(at, (layer, edge));
        }
    }
    Ok(Rerouted {
        edges: edges.into_iter().flat_map(|(_, edge)| edge).collect(),
        omitted,
        kept,
    })
}

/// Routes `descriptors` through copies of their routes without the [`ACTION_COUPLED`] layers that
/// no longer fit them. A layer fits a descriptor whose translated clip starts and ends in its
/// template's pose on every control the layer moves, as a source that revises the donor's own
/// motion does. A descriptor without a template omits them all. Other descriptors and their routes
/// are unchanged. Only a layer whose clips, read through `bank`, are all additive is omitted. One
/// that plays a replacement or absolute clip, as the fusion rifle donor `9527F0F4` does, stays on
/// every route, since removing it would change which pose its bones take rather than drop an
/// offset.
///
/// A coupled layer the source authored for a descriptor ([`Translated::own`]) instead takes the
/// source's own choices on that descriptor, whatever the donor's layer is, and a descriptor the
/// donor dispatches to no route gets one holding only those choices. Returns the table, the names
/// omitted from some route by the fit, the names kept on some translated route and the number of
/// routes added.
pub fn omit_action_coupled(
    native: &mut Reader,
    table: &Payload,
    bank: &Payload,
    descriptors: &BTreeMap<usize, Option<Translated>>,
) -> Result<(Payload, BTreeSet<u32>, BTreeSet<u32>, usize)> {
    let aliases = table.array(40, 2, Some(0x80800006))?;
    ensure!(
        descriptors
            .keys()
            .all(|&descriptor| descriptor < aliases.len()),
        "Source action dispatch is outside its table"
    );
    let coupled = coupled_layers(native, table, bank)?;
    let own = descriptors.values().flatten().any(|t| !t.own.is_empty());
    if descriptors.is_empty() || (coupled.is_empty() && !own) {
        return Ok((table.clone(), BTreeSet::new(), BTreeSet::new(), 0));
    }
    let names = table
        .array(8, 24, Some(0x80803724))?
        .into_iter()
        .map(|at| table.u32(at + 16))
        .collect::<Result<Vec<_>>>()?;
    let mut routes = table
        .array(56, 32, Some(0x808034ef))?
        .into_iter()
        .map(|at| {
            Ok([
                rows(table, &table.array(at, 4, Some(0x808034f1))?, 4),
                rows(table, &table.array(at + 16, 4, Some(0x808034f1))?, 4),
            ])
        })
        .collect::<Result<Vec<_>>>()?;
    let before = routes.len();
    let mut out = table.clone();
    let mut replacements = BTreeMap::new();
    let mut omitted = BTreeSet::new();
    let mut kept = BTreeSet::new();
    for (&descriptor, translated) in descriptors {
        let alias = aliases[descriptor];
        let original = table.u16(alias)?;
        let route = if original == u16::MAX {
            [Vec::new(), Vec::new()]
        } else {
            routes
                .get(usize::from(original))
                .context("Source action dispatch is outside its routes")?
                .clone()
        };
        let rerouted = reroute(&route[0], &names, &coupled, translated.as_ref())?;
        omitted.extend(rerouted.omitted);
        kept.extend(rerouted.kept);
        if rerouted.edges == route[0] {
            continue;
        }
        let key = (original, rerouted.edges);
        let replacement = if let Some(&index) = replacements.get(&key) {
            index
        } else {
            let index = u16::try_from(routes.len())?;
            ensure!(
                index <= i16::MAX as u16,
                "Source action pose routes exceed native limits"
            );
            routes.push([key.1.clone(), route[1].clone()]);
            replacements.insert(key, index);
            index
        };
        put(&mut out.0, alias, &replacement.to_le_bytes())?;
    }
    let added = routes.len() - before;
    if added == 0 {
        return Ok((table.clone(), BTreeSet::new(), kept, 0));
    }
    append(&mut out.0, 56, 0x808034ef, &vec![0; routes.len() * 32], 32)?;
    for (at, route) in out.array(56, 32, Some(0x808034ef))?.into_iter().zip(routes) {
        for (family, edges) in route.into_iter().enumerate() {
            append(&mut out.0, at + family * 16, 0x808034f1, &edges, 4)?;
        }
    }
    Ok((out, omitted, kept, added))
}
