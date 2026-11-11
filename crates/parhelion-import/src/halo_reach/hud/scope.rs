//! Resolve scoped artwork through the weapon's MCC HUD, including atlas sprites.
use super::super::{
    cache::{Cache, Tag},
    material::Mapping,
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};

/// HUD units per texel of a bitmap. MCC's 4K HUD art marks its scale in the bitmap's flags word at
/// `+04`: bit 11 for double-sized and bit 12 for triple-sized art, so a texel covers a half or a
/// third of a HUD unit. The editing kit's tags name these flags "bitmap is double sized" and
/// "bitmap is triple sized".
fn texel_units(c: &Cache, bitmap: usize) -> Result<f32> {
    Ok(match c.u16(bitmap + 4)? & 0x1800 {
        0 => 1.,
        0x800 => 0.5,
        0x1000 => 1. / 3.,
        _ => anyhow::bail!("Scope bitmap is marked both double and triple sized"),
    })
}

/// The full-screen widescreen HUD frame: the HUD attributes row with resolution flag bit 0.
/// Layout runs in a virtual canvas centred on the screen. Its normalized coordinates, -1 to 1 across
/// the canvas with Y down, pass through a biquadratic curve that lands the canvas's corners, edge
/// midpoints and centre on the nine curvature points, and then scale by the global safe frame to
/// screen coordinates.
#[derive(Serialize)]
pub(crate) struct Frame {
    /// Virtual canvas width and height in HUD units.
    pub size: [f32; 2],
    /// Curvature points TL, CL, BL, TM, CM, BM, TR, CR, BR.
    pub curvature: [[f32; 2]; 9],
    pub safe: [f32; 2],
}

fn frame(c: &Cache, globals: usize) -> Result<Frame> {
    let row = c
        .block(globals + 0x2dc, 0x10c)?
        .into_iter()
        .find(|&row| c.u8(row).is_ok_and(|flags| flags & 1 != 0))
        .context("Source HUD has no full-screen widescreen attributes")?;
    let mut curvature = [[0.; 2]; 9];
    for (i, point) in curvature.iter_mut().enumerate() {
        *point = c.floats::<2>(row + 4 + i * 8)?;
    }
    let size = [
        f32::from(u16::try_from(c.u32(row + 0x94)?)?),
        f32::from(u16::try_from(c.u32(row + 0x98)?)?),
    ];
    let safe = c.floats::<2>(row + 0xd0)?;
    ensure!(
        size.iter().all(|v| *v > 0.)
            && safe.iter().all(|v| (0.5..=1.).contains(v))
            && curvature[4] == [0., 0.]
            && curvature[7][0] > curvature[1][0]
            && curvature[5][1] > curvature[3][1],
        "Unsupported source HUD attributes"
    );
    Ok(Frame {
        size,
        curvature,
        safe,
    })
}

#[derive(Serialize)]
pub(crate) struct Layer {
    pub name: String,
    pub mapping: Mapping,
    pub uv: [f32; 4],
    /// The sampled rectangle in HUD units, before placement scale.
    pub size: [f32; 2],
    /// The sampled rectangle in texels.
    pub texels: [f32; 2],
    pub origin: [f32; 2],
    pub offset: [f32; 2],
    pub scale: [f32; 2],
    /// Radians about the registration point, in the HUD's Y-down frame.
    pub rotation: f32,
    /// Animation scale about the registration point, applied before rotation.
    pub stretch: [f32; 2],
    pub flags: u32,
    pub shader: u8,
    pub opacity: f32,
    pub color: [[f32; 3]; 3],
}

/// A widget's pose from its animation: offset, rotation and scale about its registration point,
/// and opacity.
#[derive(Clone, Copy, PartialEq)]
struct Pose {
    offset: [f32; 2],
    rotation: f32,
    scale: [f32; 2],
    opacity: f32,
}

impl Default for Pose {
    fn default() -> Self {
        Self {
            offset: [0.; 2],
            rotation: 0.,
            scale: [1.; 2],
            opacity: 1.,
        }
    }
}

/// Where the zoomed view at rest samples an animation.
#[derive(Clone, Copy)]
enum At {
    /// The frame an input selects at rest, between keys by linear interpolation and holding the
    /// first and last keys outside them.
    Frame(f32),
    /// The last frame, which a one-shot time animation holds once it ends.
    End(f32),
    /// Any frame, which only an animation with constant keyframes defines.
    Constant,
}

/// An external input's value while the weapon is at rest. `zero`, weapon heat, weapon barrel pinned
/// error scale, weapon autoaim target, weapon charge and locking amount are zero. Weapon barrel error
/// scale is `error`, the resting error. The numbers are MCC's, paired with the editing kit's names
/// through widgets that both define. Other inputs change in play.
fn rest_input(input: u16, error: f32) -> Option<f32> {
    match input {
        0 | 50 | 56 | 57 | 66 | 70 => Some(0.),
        55 => Some(error),
        _ => None,
    }
}

/// The barrel error scale at rest: the first barrel's lower error angle over its upper one, the
/// Float32 pair at `+0x74` of the 0x184-byte barrel rows at `weap +0x408`. A barrel's error runs
/// between those angles, so at rest it sits at the lower one. In Reach's tag test the magnum's
/// resting crosshair ticks cross the ring about 9 to 19 HUD units out. Its animation sampled at
/// this scale, 0.6 over 4 degrees, places them within a unit of that, where frame zero left them
/// inside the ring. The pinned error scale is taken as zero, as the magnum's resting circle is
/// whole there.
fn rest_error(c: &Cache, weapon: usize) -> Result<f32> {
    let Some(&barrel) = c.block(weapon + 0x408, 0x184)?.first() else {
        return Ok(0.);
    };
    let [low, high] = c.floats::<2>(barrel + 0x74)?;
    Ok(if high > 0. && low >= 0. {
        (low / high).min(1.)
    } else {
        0.
    })
}

enum Channel<const N: usize> {
    Absent,
    Value([f32; N]),
    Dynamic,
}

/// A `chad` channel's value where `at` samples it. Keyframes are a frame number followed by `N`
/// values. Several curves, unordered keyframes, or a curve keyed off the sampled frame are not
/// reproduced.
fn key<const N: usize>(c: &Cache, field: usize, stride: usize, at: At) -> Result<Channel<N>> {
    let curves = c.block(field, 32)?;
    let curve = match curves.as_slice() {
        [] => return Ok(Channel::Absent),
        [curve] => *curve,
        _ => return Ok(Channel::Dynamic),
    };
    let keys = c
        .block(curve, stride)?
        .into_iter()
        .map(|frame| Ok((c.f32(frame)?, c.floats::<N>(frame + 4)?)))
        .collect::<Result<Vec<_>>>()?;
    let (Some(first), Some(last)) = (keys.first(), keys.last()) else {
        return Ok(Channel::Dynamic);
    };
    if keys.windows(2).any(|pair| {
        matches!(
            pair[0].0.partial_cmp(&pair[1].0),
            None | Some(std::cmp::Ordering::Greater)
        )
    }) {
        return Ok(Channel::Dynamic);
    }
    if keys.iter().all(|key| key.1 == first.1) {
        return Ok(Channel::Value(first.1));
    }
    Ok(match at {
        At::Frame(frame) if frame <= first.0 => Channel::Value(first.1),
        At::Frame(frame) if frame >= last.0 => Channel::Value(last.1),
        At::Frame(frame) => {
            let after = keys
                .iter()
                .position(|key| key.0 > frame)
                .context("Animation keys around frame")?;
            let (a, b) = (keys[after - 1], keys[after]);
            let t = (frame - a.0) / (b.0 - a.0);
            Channel::Value(std::array::from_fn(|i| a.1[i] + (b.1[i] - a.1[i]) * t))
        }
        At::End(end) if last.0 == end => Channel::Value(last.1),
        At::End(_) | At::Constant => Channel::Dynamic,
    })
}

/// The pose an animation defines where `at` samples it, or `None` when it uses other channels
/// or changes there. Error moves the MCC magnum's crosshair ticks outward through position keys
/// and fades its circle through alpha keys, and its top and bottom ticks hold a constant quarter
/// turn. The needle rifle's corner ticks also scale.
fn pose(c: &Cache, animation: usize, at: At) -> Result<Option<Pose>> {
    for field in [0x28, 0x40, 0x4c, 0x58] {
        if !c.block(animation + field, 32)?.is_empty() {
            return Ok(None);
        }
    }
    let mut pose = Pose::default();
    match key::<3>(c, animation + 4, 16, at)? {
        Channel::Absent => {}
        Channel::Value([x, y, 0.]) => pose.offset = [x, y],
        Channel::Value(_) | Channel::Dynamic => return Ok(None),
    }
    match key::<3>(c, animation + 0x10, 16, at)? {
        Channel::Absent => {}
        Channel::Value([angle, pitch, roll]) if pitch == 0. && roll == 0. => pose.rotation = angle,
        Channel::Value(_) | Channel::Dynamic => return Ok(None),
    }
    match key::<2>(c, animation + 0x1c, 12, at)? {
        Channel::Absent => {}
        Channel::Value(scale) if scale.iter().all(|v| v.is_finite() && *v > 0.) => {
            pose.scale = scale;
        }
        Channel::Value(_) | Channel::Dynamic => return Ok(None),
    }
    match key::<1>(c, animation + 0x34, 8, at)? {
        Channel::Absent => {}
        Channel::Value([alpha]) if (0.0..=1.).contains(&alpha) => pose.opacity = alpha,
        Channel::Value(_) | Channel::Dynamic => return Ok(None),
    }
    Ok(Some(pose))
}

/// Where the zoomed view at rest samples the animation in a widget's slot. The five slots play
/// while initializing, active, flashing, readying and unreadying. A slot's byte `+01` selects its
/// input: time, the widget's external input A or B (`+08` and `+0A`), or zero. The active slot of
/// an input known at rest sits at that fraction of the animation's length at `+70`, as the input's
/// identity function over 0 to 1 maps it. A one-shot time animation, with no slot flags and no
/// loop or ping-pong bits (1 and 2 of the `chad` flags), holds its last frame once it ends.
fn sample(
    c: &Cache,
    (widget, slot, row): (usize, usize, usize),
    animation: usize,
    error: f32,
) -> Result<At> {
    let input = match c.u8(row + 1)? {
        0 => None,
        1 => Some(c.u16(widget + 8)?),
        2 => Some(c.u16(widget + 10)?),
        3 => Some(0),
        _ => return Ok(At::Constant),
    };
    let end = c.f32(animation + 0x70)?;
    Ok(match input {
        Some(input) if slot == 1 => {
            rest_input(input, error).map_or(At::Constant, |value| At::Frame(value * end))
        }
        None if c.u8(row)? == 0 && c.u8(animation)? & 6 == 0 && end > 0. => At::End(end),
        _ => At::Constant,
    })
}

/// The zoomed view a weapon shows.
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum View {
    /// The weapon's own zoom levels, drawn by the weapon's HUD.
    Weapon,
    /// The player's binoculars, drawn by the player's HUD.
    Binoculars,
}

#[derive(Serialize)]
pub(crate) struct Scope {
    pub view: View,
    pub hud: Tag,
    pub zoom_levels: u16,
    pub magnification: [f32; 2],
    /// Layers are placed about the crosshair at the frame's centre.
    pub frame: Frame,
    pub layers: Vec<Layer>,
    pub excluded: Vec<Value>,
    /// Layers drawn at the final frame of an entry transition that is not reproduced.
    pub settled: Vec<Value>,
}

/// Weapon flags bit 10 at `+0x204`, Allows Binoculars.
const BINOCULARS: u32 = 1 << 10;

/// Whether a weapon without zoom levels of its own zooms through the player's binoculars. It
/// must allow them and fire. A weapon without barrels keeps the donor's view, as a sword's guard
/// does. In the inspected caches this selects every handheld firearm without its own zoom.
/// Turrets and vehicle weapons do not allow binoculars, and swords, hammers and objectives have
/// no barrels.
fn binocular(flags: u32, barrels: usize) -> bool {
    flags & BINOCULARS != 0 && barrels > 0
}

/// Whether the object read's gameplay record zooms, through its own levels or the binoculars.
pub(crate) fn zooms(gameplay: &Value) -> bool {
    gameplay["zoom_levels"]
        .as_u64()
        .is_some_and(|levels| levels > 0)
        || binocular(
            gameplay["flags"]
                .as_u64()
                .and_then(|flags| u32::try_from(flags).ok())
                .unwrap_or(0),
            gameplay["barrels"].as_array().map_or(0, Vec::len),
        )
}

/// The player representation and information rows of the globals that the binoculars use: the
/// first of each. The first representation is the Spartan's in every inspected cache, the
/// information rows agree, and Shadowkeep's Guardian has no second player kind to choose.
struct Player {
    /// Its HUD, `matg +0x474` rows of 0x6C bytes, reference at `+0x04`.
    hud: Tag,
    /// Binoculars Zoom Count `+0x94` and Zoom Range `+0x98` of the `matg +0x468` rows of 0xF4
    /// bytes, 1 and 3 to 3 in the inspected multiplayer and campaign caches.
    levels: u16,
    magnification: [f32; 2],
}

fn player(c: &Cache) -> Result<Player> {
    let globals = c.only("matg")?.address()?;
    let information = *c
        .block(globals + 0x468, 0xf4)?
        .first()
        .context("Source globals have no player information")?;
    let representation = *c
        .block(globals + 0x474, 0x6c)?
        .first()
        .context("Source globals have no player representation")?;
    Ok(Player {
        hud: c
            .reference(representation + 4, Some("chdt"))?
            .context("The source player has no HUD")?,
        levels: u16::try_from(c.u32(information + 0x94)?)?,
        magnification: c.floats::<2>(information + 0x98)?,
    })
}

struct Zoom {
    view: View,
    levels: u16,
    magnification: [f32; 2],
}

fn zoom(c: &Cache, weapon: &Tag) -> Result<Option<Zoom>> {
    ensure!(weapon.group == "weap", "Zoom owner is not a weapon");
    let base = weapon.address()?;
    let levels = c.u16(base + 0x2ca)?;
    let zoom = if levels > 0 {
        Zoom {
            view: View::Weapon,
            levels,
            magnification: c.floats::<2>(base + 0x2cc)?,
        }
    } else if binocular(c.u32(base + 0x204)?, c.block(base + 0x408, 0x184)?.len()) {
        let player = player(c)?;
        ensure!(
            player.levels > 0,
            "The source weapon allows binoculars that have no zoom levels"
        );
        Zoom {
            view: View::Binoculars,
            levels: player.levels,
            magnification: player.magnification,
        }
    } else {
        return Ok(None);
    };
    ensure!(
        zoom.levels <= 2
            && zoom.magnification[..usize::from(zoom.levels)]
                .iter()
                .all(|value| (1.0..=10.0).contains(value)),
        "Unsupported source zoom range"
    );
    Ok(Some(zoom))
}

pub(crate) fn aim(c: &Cache, weapon: &Tag) -> Result<Value> {
    let zoom = zoom(c, weapon)?;
    let source = zoom.as_ref().map_or(&[][..], |zoom| {
        &zoom.magnification[..usize::from(zoom.levels)]
    });
    Ok(json!({
        "zoom_levels":source.len(),
        "view":zoom.as_ref().map(|zoom| zoom.view),
        "source_magnification":source,
        "magnification":source.first().copied().unwrap_or(1.0),
        "method":"private_weapon_controller_zoom_translation",
        "limits":if source.len() > 1 {
            vec!["The first source zoom level is selected. Switching to additional source zoom levels is not translated."]
        } else { Vec::<&str>::new() },
        "gameplay_verified":false,
    }))
}

/// Whether a HUD condition holds in the translated zoomed view. A weapon's view is its first zoom
/// level, with the weapon in the right hand, no lock and no headshot under the crosshair. The
/// numbers are MCC's, paired with the editing kit's names through widgets that both define: 49
/// binoculars enabled, 83 zoomed, 84 unzoomed, 85 and 86 zoom levels 1 and 2, 97 and 98 weapon in
/// the right and left hand, 103 crosshair enemy headshot, 114 locking on available and 115 lock
/// available. The binocular view reads only the player's HUD, whose binocular group alone names
/// 49. Other conditions change in play.
fn condition(value: u32, view: View) -> Option<bool> {
    match (view, value) {
        (View::Weapon, 83 | 85 | 97) | (View::Binoculars, 49) => Some(true),
        (View::Weapon, 84 | 86 | 98 | 103 | 114 | 115) => Some(false),
        _ => None,
    }
}

/// Whether a condition selects the view's zoom.
fn zooming(value: u32, view: View) -> bool {
    match view {
        View::Weapon => (83..=86).contains(&value),
        View::Binoculars => value == 49,
    }
}

/// A widget's activation in the translated zoomed view.
struct Activation {
    /// `None` when it depends on a condition that changes in play.
    shown: Option<bool>,
    /// A satisfied term names a zoom condition.
    zoomed: bool,
}

// MCC's active groups are ANDs of ORs. A term's flag bit 0 inverts its condition, as the
// rocket launcher's crosshair shows while no lock is available. Flash and hidden groups change
// in play.
fn state(c: &Cache, widget: usize, view: View) -> Result<Activation> {
    let rows = c.block(widget + 0x1c, 0x38)?;
    ensure!(rows.len() <= 1, "HUD widget has multiple state records");
    let mut result = Activation {
        shown: Some(true),
        zoomed: false,
    };
    for row in rows {
        if !c.block(row + 12, 12)?.is_empty() || !c.block(row + 24, 12)?.is_empty() {
            result.shown = None;
            return Ok(result);
        }
        for group in c.block(row, 12)? {
            let terms = c.block(group, 8)?;
            ensure!(!terms.is_empty(), "HUD activation has an empty term group");
            let mut any = Some(false);
            for term in terms {
                let flags = c.u32(term)?;
                let value = c.u32(term + 4)?;
                let holds = (flags & !1 == 0)
                    .then(|| condition(value, view).map(|v| v != (flags & 1 != 0)))
                    .flatten();
                result.zoomed |= holds == Some(true) && zooming(value, view);
                any = match (any, holds) {
                    (Some(true), _) | (_, Some(true)) => Some(true),
                    (None, _) | (_, None) => None,
                    _ => Some(false),
                };
            }
            result.shown = match (result.shown, any) {
                (Some(false), _) | (_, Some(false)) => Some(false),
                (None, _) | (_, None) => None,
                _ => Some(true),
            };
        }
    }
    Ok(result)
}

/// The state values a widget's activation groups name, inverted ones negative, for reporting.
fn states(c: &Cache, widget: usize) -> Result<Vec<i64>> {
    let mut values = Vec::new();
    for row in c.block(widget + 0x1c, 0x38)? {
        for group in c.block(row, 12)? {
            for term in c.block(group, 8)? {
                let value = i64::from(c.u32(term + 4)?);
                values.push(if c.u32(term)? & 1 == 0 { value } else { -value });
            }
        }
    }
    Ok(values)
}

/// A widget's placement in the full-screen view. Parents and most widgets keep their first row, the
/// default. A bitmap widget anchored to the crosshair (17) takes its last row on that anchor. The editing kit's
/// magnum ticks carry sideways origins of 1, -2, 2 and 2 half-sizes in their default row and 0 in
/// all three split-screen rows, and its circle a 0.2-unit offset there and none in the others.
/// Reach's tag test draws the ticks centred on their axes and the circle on the crosshair in the
/// full-screen view, as a capture of MCC's does within a pixel. The scope parent keeps its first
/// row, whose scale of 1 the same capture's mask edge confirms. Which row the engine selects is
/// otherwise not established.
fn placement(c: &Cache, widget: usize, bitmap: bool) -> Result<[f32; 6]> {
    let rows = c.block(widget + 0x38, 0x1c)?;
    if rows.is_empty() {
        return Ok([0.5, 0.5, 0., 0., 1., 1.]);
    }
    let first = *rows
        .first()
        .context("HUD widget has no full-screen placement")?;
    let row = if bitmap && c.u8(first + 1)? == 17 {
        let mut last = first;
        for &row in &rows {
            if c.u8(row + 1)? == 17 {
                last = row;
            }
        }
        last
    } else {
        first
    };
    ensure!(
        (row == first && c.u8(row)? <= 1 || c.u8(row + 1)? == 17)
            && matches!(c.u8(row + 1)?, 0 | 17)
            && c.u16(row + 2)? == 0,
        "Scope placement is not an unrotated parent or crosshair anchor"
    );
    let values = c.floats::<6>(row + 4)?;
    ensure!(
        values.iter().all(|v| v.is_finite()) && values[4..].iter().all(|v| v.abs() > 0.0001),
        "Degenerate scope placement"
    );
    Ok(values)
}

fn colors(c: &Cache, render: usize, globals: usize) -> Result<[[f32; 3]; 3]> {
    let mut result = [[0.; 3]; 3];
    for (i, color) in result.iter_mut().enumerate() {
        let selector = usize::from(c.u16(render + 0x54 + i * 2)?);
        *color = match selector {
            0..=3 => c.floats::<3>(render + 8 + selector * 16)?,
            18..=49 => c.floats::<3>(globals + 8 + (selector - 18) * 16)?,
            _ => anyhow::bail!("Unsupported scope color selector {selector}"),
        };
        ensure!(
            color.iter().all(|v| v.is_finite() && *v >= 0.),
            "Invalid scope color"
        );
    }
    Ok(result)
}

/// Whether an unmirrored layer lies entirely outside the virtual canvas, the HUD's layout space.
/// The MCC DMR's `distance_slider` has origin X -23.375, which puts its 22-unit sprite about 514
/// HUD units from the pivot, beyond the canvas edge, while the range arc it rotates along lies
/// about 215 units out. The unit of such an origin is not established, so the layer is reported
/// rather than drawn there.
fn outside(layer: &Layer, frame: &Frame) -> bool {
    if layer.flags & 11 != 0 {
        return false;
    }
    let (sin, cos) = layer.rotation.sin_cos();
    let corners = [[0., 0.], [1., 0.], [0., 1.], [1., 1.]].map(|[u, v]: [f32; 2]| {
        let s = [
            (u - layer.origin[0]) * layer.size[0] * layer.stretch[0],
            (v - layer.origin[1]) * layer.size[1] * layer.stretch[1],
        ];
        // The inverse of the overlay's sampling: rotate, then apply the placement scale.
        [
            layer.offset[0] + (s[0] * cos - s[1] * sin) * layer.scale[0],
            layer.offset[1] + (s[0] * sin + s[1] * cos) * layer.scale[1],
        ]
    });
    (0..2).any(|axis| {
        let half = frame.size[axis] / 2.;
        corners.iter().all(|c| c[axis] > half) || corners.iter().all(|c| c[axis] < -half)
    })
}

fn layer(c: &Cache, widget: usize, parent: [f32; 6], globals: usize) -> Result<Layer> {
    let bitmap = c
        .reference(widget + 0x80, Some("bitm"))?
        .context("Scope bitmap is missing")?;
    let sequence = usize::from(c.u8(widget + 0x90)?);
    let units = texel_units(c, bitmap.address()?)?;
    let sequences = c.block(bitmap.address()? + 0x70, 0x40)?;
    let (frame, uv) = if sequences.is_empty() {
        (sequence, [0., 1., 0., 1.])
    } else {
        let sequence = *sequences
            .get(sequence)
            .context("Scope sequence is outside its bitmap")?;
        let sprites = c.block(sequence + 0x34, 0x20)?;
        if let [sprite] = sprites.as_slice() {
            (
                usize::try_from(c.i16(*sprite)?)?,
                c.floats::<4>(*sprite + 8)?,
            )
        } else {
            ensure!(
                sprites.is_empty() && c.i16(sequence + 0x22)? == 1,
                "Scope sequence requires animated sprite selection"
            );
            (usize::try_from(c.i16(sequence + 0x20)?)?, [0., 1., 0., 1.])
        }
    };
    ensure!(
        uv.iter().all(|v| (0.0..=1.).contains(v)) && uv[0] < uv[1] && uv[2] < uv[3],
        "Invalid scope sprite rectangle"
    );
    let images = c.block(bitmap.address()? + 0x7c, 0x38)?;
    let image = *images.get(frame).context("Scope bitmap frame is absent")?;
    let texels = [
        f32::from(c.u16(image)?) * (uv[1] - uv[0]),
        f32::from(c.u16(image + 2)?) * (uv[3] - uv[2]),
    ];
    let p = placement(c, widget, true)?;
    let render = c.block(widget + 0x70, 0x78)?;
    let [render] = render.as_slice() else {
        anyhow::bail!("Scope requires one render state")
    };
    let flags = c.u32(widget + 0x7c)?;
    ensure!(
        flags & !11 == 0,
        "Unsupported scope bitmap flags {flags:#x}"
    );
    Ok(Layer {
        name: c.string_id(c.u32(widget)?)?,
        mapping: Mapping {
            // An extended border reads only its bitmap's top level.
            role: if flags & 8 != 0 {
                "scope_border"
            } else {
                "scope"
            }
            .into(),
            bitmap: Some(bitmap),
            transform: [1., 1., 0., 0.],
            frame,
            address: 0,
            filter: 0,
            functions: Vec::new(),
        },
        uv,
        size: texels.map(|t| t * units),
        texels,
        origin: [p[0], p[1]],
        offset: [parent[2] + p[2] * parent[4], parent[3] + p[3] * parent[5]],
        scale: [parent[4] * p[4], parent[5] * p[5]],
        rotation: 0.,
        stretch: [1.; 2],
        flags,
        shader: c.u8(*render)?,
        opacity: 1.,
        color: colors(c, *render, globals)?,
    })
}

#[allow(clippy::cognitive_complexity)]
pub(crate) fn read(c: &Cache, weapon: &Tag) -> Result<Option<Scope>> {
    let Some(zoom) = zoom(c, weapon)? else {
        return Ok(None);
    };
    let base = weapon.address()?;
    let error = rest_error(c, base)?;
    let hud = match zoom.view {
        View::Weapon => c
            .reference(base + 0x3c4, Some("chdt"))?
            .context("Scoped weapon has no HUD")?,
        View::Binoculars => player(c)?.hud,
    };
    let global = c.only("chgd")?;
    let globals = c
        .block(global.address()?, 0x534)?
        .into_iter()
        .find(|&row| c.u32(row).ok() == Some(0))
        .context("Source Spartan HUD globals are absent")?;
    let mut result = Scope {
        view: zoom.view,
        hud,
        zoom_levels: zoom.levels,
        magnification: zoom.magnification,
        frame: frame(c, globals)?,
        layers: Vec::new(),
        excluded: Vec::new(),
        settled: Vec::new(),
    };
    for parent in c.block(result.hud.address()?, 0xdc)? {
        let group = state(c, parent, zoom.view)?;
        if group.shown != Some(true) {
            continue;
        }
        for widget in c.block(parent + 0x98, 0x9c)? {
            let name = c.string_id(c.u32(widget)?)?;
            let renders = c.block(widget + 0x70, 0x78)?;
            if renders.len() != 1 {
                continue;
            }
            let shader = c.u8(renders[0])?;
            let activation = state(c, widget, zoom.view)?;
            // A weapon's flat-color reticles show in every zoom state. Reach's binocular view
            // shows no weapon reticle, so it takes only the binocular group.
            if !group.zoomed
                && !(activation.shown == Some(true) && activation.zoomed)
                && !(zoom.view == View::Weapon && shader == 6)
            {
                continue;
            }
            let simple = shader <= 1 || shader == 6;
            let mut animated = false;
            let mut settled = Vec::new();
            let mut posed = None;
            for row in c.block(widget + 0x54, 0x64)? {
                for slot in 0..5 {
                    let Some(animation) = c.reference(row + slot * 20 + 4, Some("chad"))? else {
                        continue;
                    };
                    let address = animation.address()?;
                    let at = sample(c, (widget, slot, row + slot * 20), address, error)?;
                    // The active slot holds the widget's pose. Neutral reticles respond to firing
                    // error there, and their zero-error pose is carried without inventing a native
                    // error input. Other slots must leave the widget unchanged.
                    match pose(c, address, at)? {
                        Some(pose) if slot == 1 && posed.is_none() => posed = Some(pose),
                        Some(pose) if pose == Pose::default() => {}
                        _ => animated = true,
                    }
                    if matches!(at, At::End(_)) {
                        settled.push(animation.path);
                    }
                }
            }
            if activation.shown != Some(true) || !simple || animated {
                let reason = if activation.shown == Some(false) {
                    "Shown only in HUD states that the zoomed view at rest does not reach"
                } else {
                    "Dynamic state, meter or animation requires a separate native input mapping"
                };
                result.excluded.push(json!({"widget":name,"reason":reason}));
                continue;
            }
            let pose = posed.unwrap_or_default();
            if pose.opacity <= 0. {
                result
                    .excluded
                    .push(json!({"widget":name,"reason":"Transparent in the zoomed view at rest"}));
                continue;
            }
            let mut frame = placement(c, parent, false)?;
            if zoom.view == View::Binoculars {
                // Reach lowers its crosshair to 0.17 below the screen's middle, the Player Control
                // crosshair location at `matg +0x414` row `+0x24`, and the binocular group's offset
                // of 120 HUD units raises it from there toward the middle. Shadowkeep's crosshair
                // is at the middle, so the group centres on it with its reticle, at the group's
                // origin, on the aim point.
                frame[2] = 0.;
                frame[3] = 0.;
            }
            let mut layer = layer(c, widget, frame, globals)?;
            // Animation positions move the widget within its parent's placement.
            layer.offset[0] += pose.offset[0] * frame[4];
            layer.offset[1] += pose.offset[1] * frame[5];
            layer.rotation = pose.rotation;
            layer.stretch = pose.scale;
            layer.opacity = pose.opacity;
            if outside(&layer, &result.frame) {
                result.excluded.push(json!({"widget":name,
                    "reason":"Placed entirely outside the source HUD's virtual canvas"}));
                continue;
            }
            result.layers.push(layer);
            if !settled.is_empty() {
                result.settled.push(json!({"widget":name,"animations":settled,
                    "reason":"The time animation is not reproduced. The layer is drawn at its final frame"}));
            }
        }
    }
    if !result.layers.iter().any(|layer| layer.flags & 8 != 0) {
        // Name each border the zoomed view left out, with the HUD states that show it.
        let mut borders = Vec::new();
        for parent in c.block(result.hud.address()?, 0xdc)? {
            for widget in c.block(parent + 0x98, 0x9c)? {
                if c.u32(widget + 0x7c)? & 8 == 0 {
                    continue;
                }
                let name = c.string_id(c.u32(widget)?)?;
                borders.push(if result.excluded.iter().any(|row| row["widget"] == name) {
                    format!("{name} is dynamic")
                } else {
                    let mut shown = states(c, parent)?;
                    shown.extend(states(c, widget)?);
                    format!("{name} is shown in HUD states {shown:?}")
                });
            }
        }
        anyhow::bail!(
            "Scoped HUD has no supported border mask in the zoomed view at rest{}",
            if borders.is_empty() {
                String::new()
            } else {
                format!(", and {}", borders.join(", "))
            }
        );
    }
    Ok(Some(result))
}
