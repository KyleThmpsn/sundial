//! Append clip descriptors while retaining existing bank ordinals and pose dispatch.
use crate::{
    presentation::{append, put},
    tiger::payload::Payload,
};
use anyhow::{Context, Result, ensure};
pub mod holding;

fn rows(p: &Payload, field: usize, stride: usize, class: u32) -> Result<Vec<u8>> {
    Ok(p.array(field, stride, Some(class))?
        .into_iter()
        .flat_map(|at| p.0[at..at + stride].iter().copied())
        .collect())
}

pub struct Addition {
    pub descriptor: usize,
    pub template: u32,
    pub clip: u32,
    pub duration: f32,
    pub dispatch: Dispatch,
}

/// What the pose consumers read for a new descriptor: the stock supplemental route whose edges
/// its own route row copies, and the aim and look aliases, each `FFFF` for none. Stock gives
/// every descriptor with a supplemental pose a route row of its own, and shares each alias
/// among descriptors.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub struct Dispatch {
    pub route: Option<u16>,
    pub aim: u16,
    pub look: u16,
}

impl Dispatch {
    /// A complete authored pose: no supplemental route and no direction correction.
    pub const NONE: Self = Self {
        route: None,
        aim: u16::MAX,
        look: u16::MAX,
    };
}

pub struct Extension {
    pub bank: Payload,
    pub layers: Payload,
    pub directions: Payload,
    pub descriptors: Vec<usize>,
}

/// Each addition supplies its original descriptor, clip tags and authored duration.
/// The caller allocates the clip payloads and owns resource enrollment.
pub fn extend(
    bank: &Payload,
    layers: &Payload,
    directions: &Payload,
    original: &Payload,
    original_layers: &Payload,
    original_directions: &Payload,
    additions: &[Addition],
) -> Result<Extension> {
    ensure!(
        !additions.is_empty(),
        "Character bank extension has no clips"
    );
    let mut clips = rows(bank, 8, 4, 0x80808F48)?;
    // One signed 32-bit word per clip, class 0x80800007. A clip without one holds -1.
    let mut auxiliary = rows(bank, 24, 4, 0x80800007)?;
    let mut descriptors = rows(bank, 0x68, 32, 0x80809002)?;
    let mut dispatch = rows(layers, 40, 2, 0x80800006)?;
    ensure!(
        clips.len() / 4 == auxiliary.len() / 4 && descriptors.len() / 32 == dispatch.len() / 2,
        "Extended character bank arrays disagree"
    );
    for (data, source, field, stride, class) in [
        (&clips, original, 8, 4, 0x80808F48),
        (&auxiliary, original, 24, 4, 0x80800007),
        (&descriptors, original, 0x68, 32, 0x80809002),
        (&dispatch, original_layers, 40, 2, 0x80800006),
    ] {
        ensure!(
            data.starts_with(&rows(source, field, stride, class)?),
            "Extended character bank changed a stock row"
        );
    }
    let source_descriptors = original.array(0x68, 32, Some(0x80809002))?;
    let source_clips = original.array(8, 4, Some(0x80808F48))?;
    let source_dispatch = original_layers.array(40, 2, Some(0x80800006))?;
    let mut direction_aliases = Vec::new();
    for field in [8, 0x38] {
        let aliases = rows(directions, field, 2, 0x80800006)?;
        let source = rows(original_directions, field, 2, 0x80800006)?;
        ensure!(
            aliases.len() / 2 == descriptors.len() / 32
                && source.len() / 2 == source_descriptors.len()
                && aliases.starts_with(&source),
            "Character direction aliases do not preserve and cover their bank"
        );
        direction_aliases.push((field, aliases));
    }
    let stock_routes = original_layers.array(56, 32, Some(0x808034EF))?.len();
    let mut routes = layers.array(56, 32, Some(0x808034EF))?;
    let copied_routes = routes.len();
    let mut indices = Vec::new();
    for &Addition {
        descriptor: index,
        template,
        clip: private,
        duration,
        dispatch: pose,
    } in additions
    {
        ensure!(
            ![0, u32::MAX].contains(&private),
            "Private character clip has an empty tag"
        );
        let row = *source_descriptors
            .get(index)
            .context("Character source descriptor missing")?;
        let mut descriptor = original.bytes::<32>(row)?;
        ensure!(
            descriptor[..16] == [0; 16]
                && matches!(original.u16(row + 26)?, 1 | 2)
                && original.u32(
                    *source_clips
                        .get(usize::from(original.u16(row + 24)?))
                        .context("Character source clip missing")?
                )? == template,
            "Character clip and source descriptor disagree"
        );
        descriptor[24..26].copy_from_slice(&u16::try_from(clips.len() / 4)?.to_le_bytes());
        ensure!(
            duration.is_finite() && duration > 0.,
            "Private character clip has an invalid duration"
        );
        descriptor[20..24].copy_from_slice(&duration.to_le_bytes());
        indices.push(descriptors.len() / 32);
        descriptors.extend(descriptor);
        clips.extend(private.to_le_bytes());
        auxiliary.extend(u32::MAX.to_le_bytes());
        // A complete authored body pose must not also inherit a donor stance or
        // action correction. Dynamic control parents and final constraints remain.
        ensure!(
            source_dispatch.get(index).is_some(),
            "Character pose dispatch missing"
        );
        match pose.route {
            None => dispatch.extend(u16::MAX.to_le_bytes()),
            // A supplemental pose takes a route row of its own with the stock route's edges.
            Some(route) => {
                ensure!(
                    usize::from(route) < stock_routes,
                    "Character pose route is not a stock route"
                );
                dispatch.extend(u16::try_from(routes.len())?.to_le_bytes());
                routes.push(routes[usize::from(route)]);
            }
        }
        for ((field, aliases), alias) in direction_aliases.iter_mut().zip([pose.aim, pose.look]) {
            // Both directional consumers index directly by descriptor. An absolute authored
            // pose has no donor direction correction. Any other alias is one stock uses.
            ensure!(
                alias == u16::MAX
                    || rows(original_directions, *field, 2, 0x80800006)?
                        .chunks_exact(2)
                        .any(|stock| stock == alias.to_le_bytes()),
                "Character direction alias is not a stock alias"
            );
            aliases.extend(alias.to_le_bytes());
        }
    }
    let mut bank = bank.clone();
    let mut layers = layers.clone();
    let mut directions = directions.clone();
    if routes.len() > copied_routes {
        holding::copy_rows(&mut layers, 56, 0x808034EF, 32, &routes, &[0, 16])?;
    }
    append(&mut bank.0, 8, 0x80808F48, &clips, 4)?;
    append(&mut bank.0, 24, 0x80800007, &auxiliary, 4)?;
    append(&mut bank.0, 0x68, 0x80809002, &descriptors, 32)?;
    append(&mut layers.0, 40, 0x80800006, &dispatch, 2)?;
    for (field, aliases) in direction_aliases {
        append(&mut directions.0, field, 0x80800006, &aliases, 2)?;
    }
    for payload in [&mut bank, &mut layers, &mut directions] {
        let length = u64::try_from(payload.0.len())?;
        put(&mut payload.0, 0, &length.to_le_bytes())?;
    }
    Ok(Extension {
        bank,
        layers,
        directions,
        descriptors: indices,
    })
}
