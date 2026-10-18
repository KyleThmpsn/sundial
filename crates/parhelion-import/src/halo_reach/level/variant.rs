//! A Halo: Reach map variant (`.mvar`): a Forge layout of a multiplayer level. Its objects stand in
//! for the scenario's placements of every object the scenario's map variant palette offers, and add
//! the pieces a Forge author placed. The file is a BLF chunk set whose `mvar` chunk holds a 20-byte
//! hash, a 32-bit length and a bitstream read most significant bit first: a content header, the
//! variant's world bounds and Forge labels, 651 object slots and the palette's quotas. An object
//! names its palette entry and the entry's variant, and keeps its position quantized to the world
//! bounds, its up axis as a cube-face code and its forward axis as an angle about the up axis.
use super::{
    super::cache::{Cache, Tag},
    METRES_PER_UNIT,
};
use anyhow::{Context, Result, bail, ensure};
use std::{collections::BTreeSet, io::Read, path::Path};

/// The `mvar` chunk and content versions this reader knows.
const CHUNK: &[u8; 4] = b"mvar";
const VERSION: u64 = 31;
const CHUNK_HEADER: usize = 12;
/// The hash and the 32-bit length that precede the bitstream.
const HASH: usize = 20;
/// Object slots a map variant keeps, and the most palette quotas it can.
const OBJECT_SLOTS: usize = 651;
const QUOTA_SLOTS: u64 = 256;
/// Content header strings: a creator's name in ASCII, a title and description in UTF-16.
const AUTHOR_LENGTH: usize = 16;
const TEXT_LENGTH: usize = 128;
/// The bit count a position starts from before the world bounds narrow each axis, the step that
/// base sets, the smallest step that narrowing uses, and the most bits an axis takes.
const POSITION_BITS: i32 = 21;
const POSITION_STEP: f64 = 0.008_333_333_33;
const POSITION_LIMIT: f64 = 0.0001;
const POSITION_MAX_BITS: u32 = 26;
const POSITION_MAX_STEPS: f64 = 8_388_608.;
/// An up axis code: codes per cube face, then per row within a face.
const FACE_CODES: u64 = 0x2AAAA;
const ROW_CODES: u64 = 0x1A1;
/// The forward angle's step and offset, in radians.
const ANGLE_STEP: f64 = 0.000_383_495_2;
const ANGLE_OFFSET: f64 = 0.000_191_747_6;
/// A shape dimension's 11-bit code: step, half step, and the code of the largest dimension.
const DIMENSION_STEP: f32 = 0.097_751_714;
const DIMENSION_HALF: f32 = 0.048_875_857;
const DIMENSION_MAX: u64 = 0x7FF;
const DIMENSION_LIMIT: f32 = 200.;
/// The scenario's map ID, and its map variant palettes: `0x14`-byte rows whose entries (`+0x08`,
/// `0x1C`-byte rows) each list variants (`+0x04`, `0x18`-byte rows) naming an object at `+0x04` and
/// a variant name at `+0x14`.
const SCENARIO_MAP_ID: usize = 0x10;
const PALETTES: usize = 0x228;
const PALETTE_SIZE: usize = 0x14;
const PALETTE_ENTRIES: usize = 0x08;
const ENTRY_SIZE: usize = 0x1C;
const ENTRY_VARIANTS: usize = 0x04;
const VARIANT_SIZE: usize = 0x18;
const VARIANT_OBJECT: usize = 0x04;
const VARIANT_NAME: usize = 0x14;

/// An object's boundary shape, in meters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    None,
    Sphere {
        radius: f32,
    },
    Cylinder {
        radius: f32,
        top: f32,
        bottom: f32,
    },
    Box {
        width: f32,
        length: f32,
        top: f32,
        bottom: f32,
    },
}

/// One object of a map variant, resolved against the scenario's palette.
#[derive(Clone, Debug)]
pub struct Placement {
    pub tag: Tag,
    /// The palette variant's model variant name, empty for the object's default.
    pub variant: String,
    /// Position in meters.
    pub position: [f32; 3],
    /// Forward, left and up axes.
    pub axes: [[f32; 3]; 3],
    pub shape: Shape,
    /// The object's Forge label, when it has one.
    pub label: Option<String>,
    /// Reach's multiplayer object type: 1 weapon, 2 grenade, 15 spawn point, 16 respawn zone, 26
    /// soft kill boundary and 27 cinematic camera among those the survey met.
    pub kind: u8,
    pub team: Option<u8>,
    pub spawn_sequence: u8,
}

/// A map variant's layout of a multiplayer level.
pub struct Variant {
    pub title: String,
    pub map_id: u32,
    pub placements: Vec<Placement>,
    /// The datum of every object the scenario's palette offers. The scenario's own placements of
    /// these give way to the variant's.
    pub offered: BTreeSet<u32>,
}

impl Variant {
    /// Whether the variant stands in for the scenario's placements of `tag`.
    pub fn replaces(&self, tag: &Tag) -> bool {
        self.offered.contains(&tag.datum)
    }
}

/// A bitstream read most significant bit first.
struct Bits<'a> {
    data: &'a [u8],
    at: usize,
}

impl Bits<'_> {
    /// Steps over `count` bits the converter does not keep.
    fn skip(&mut self, count: usize) -> Result<()> {
        ensure!(
            self.at + count <= self.data.len() * 8,
            "The map variant ends early"
        );
        self.at += count;
        Ok(())
    }

    fn read(&mut self, count: u32) -> Result<u64> {
        ensure!(
            count <= u64::BITS,
            "A map variant field is wider than 64 bits"
        );
        let mut value = 0u64;
        for _ in 0..count {
            let byte = *self
                .data
                .get(self.at >> 3)
                .context("The map variant ends early")?;
            value = (value << 1) | u64::from((byte >> (7 - (self.at & 7))) & 1);
            self.at += 1;
        }
        Ok(value)
    }

    fn flag(&mut self) -> Result<bool> {
        Ok(self.read(1)? != 0)
    }

    fn byte(&mut self) -> Result<u8> {
        Ok(u8::try_from(self.read(8)?)?)
    }

    fn float(&mut self) -> Result<f32> {
        Ok(f32::from_bits(u32::try_from(self.read(32)?)?))
    }

    /// Characters of `width` bits up to a zero or `limit` characters, the zero consumed.
    fn text(&mut self, width: u32, limit: usize) -> Result<Vec<u16>> {
        let mut out = Vec::new();
        for _ in 0..limit {
            match u16::try_from(self.read(width)?)? {
                0 => break,
                unit => out.push(unit),
            }
        }
        Ok(out)
    }

    fn ascii(&mut self, limit: usize) -> Result<String> {
        Ok(self
            .text(8, limit)?
            .into_iter()
            .map(|unit| char::from(u8::try_from(unit).unwrap_or(b'?')))
            .collect())
    }

    fn wide(&mut self, limit: usize) -> Result<String> {
        Ok(char::decode_utf16(self.text(16, limit)?)
            .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect())
    }
}

/// The bits each position axis takes over world `bounds`, in world units.
fn position_bits(bounds: [[f32; 2]; 3]) -> [u32; 3] {
    let mut step = POSITION_STEP / 2f64.powi(POSITION_BITS - 16);
    if step < POSITION_LIMIT {
        return [POSITION_MAX_BITS; 3];
    }
    step *= 2.;
    bounds.map(|[low, high]| {
        let steps = ((f64::from(high) - f64::from(low)) / step)
            .ceil()
            .clamp(0., POSITION_MAX_STEPS);
        // A whole number of steps no larger than 2^23.
        let steps = steps as u64;
        match steps {
            0 => 0,
            _ => (u64::BITS - (steps - 1).leading_zeros()).min(POSITION_MAX_BITS),
        }
    })
}

/// A unit vector's components, each operation in single precision as Reach's decoder computes
/// them, since which reference axis a nearly vertical up chooses depends on their last bits.
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if length.abs() < 0.0001 {
        return v;
    }
    v.map(|c| c / length)
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The up axis an up code names: a cube face whose axis is +-1, and the other two components from
/// the code's row and column.
fn up_axis(code: u64) -> Result<[f32; 3]> {
    let face = code / FACE_CODES;
    let rest = code % FACE_CODES;
    let step = 2f32 / (ROW_CODES - 1) as f32;
    let coordinate = |n: u64| n as f32 * step - 1. + 0.5 * step;
    let (u, v) = (coordinate(rest / ROW_CODES), coordinate(rest % ROW_CODES));
    Ok(normalize(match face {
        0 => [1., u, v],
        1 => [u, 1., v],
        2 => [u, v, 1.],
        3 => [-1., u, v],
        4 => [u, -1., v],
        5 => [u, v, -1.],
        _ => bail!("A map variant up axis names cube face {face}"),
    }))
}

/// Forward, left and up axes from `up` and the forward angle code `angle`. The angle turns forward
/// about up from a reference: up crossed into +x when |up.y| exceeds |up.x|, else +y crossed into
/// up, which for a straight up axis is +x.
fn axes(up: [f32; 3], angle: u64) -> [[f32; 3]; 3] {
    let reference = if up[0].abs() >= up[1].abs() {
        normalize([up[2], 0., -up[0]])
    } else {
        normalize([0., up[2], -up[1]])
    };
    let left = normalize(cross(up, reference));
    let angle = angle as f64 * ANGLE_STEP - std::f64::consts::PI + ANGLE_OFFSET;
    let (sine, cosine) = (angle as f32).sin_cos();
    let forward = normalize(std::array::from_fn(|k| {
        reference[k] * cosine + left[k] * sine
    }));
    [forward, normalize(cross(up, forward)), up]
}

/// A shape dimension from its 11-bit code, in world units.
fn dimension(code: u64) -> f32 {
    match code {
        0 => 0.,
        DIMENSION_MAX => DIMENSION_LIMIT,
        _ => (code - 1) as f32 * DIMENSION_STEP + DIMENSION_HALF,
    }
}

/// One object as stored: its palette entry and variant, position in world units, axes, shape in
/// world units, Forge label index and the multiplayer fields the converter keeps.
struct Stored {
    entry: Option<u64>,
    variant: Option<u64>,
    position: [f32; 3],
    axes: [[f32; 3]; 3],
    shape: Shape,
    label: Option<u64>,
    kind: u8,
    team: Option<u8>,
    spawn_sequence: u8,
}

fn object(bits: &mut Bits, bounds: [[f32; 2]; 3], counts: [u32; 3]) -> Result<Option<Stored>> {
    if !bits.flag()? {
        return Ok(None);
    }
    bits.skip(2)?;
    let entry = if bits.flag()? {
        None
    } else {
        Some(bits.read(8)?)
    };
    let variant = if bits.flag()? {
        None
    } else {
        Some(bits.read(5)?)
    };
    // A flag precedes the position. Both of its values quantize to the world bounds.
    bits.skip(1)?;
    let mut position = [0f32; 3];
    for (axis, value) in position.iter_mut().enumerate() {
        let [low, high] = bounds[axis];
        let steps = bits.read(counts[axis])?;
        let size = (f64::from(high) - f64::from(low)) / 2f64.powi(i32::try_from(counts[axis])?);
        *value = ((0.5 + steps as f64) * size + f64::from(low)) as f32;
    }
    let up = if bits.flag()? {
        [0., 0., 1.]
    } else {
        up_axis(bits.read(20)?)?
    };
    let axes = axes(up, bits.read(14)?);
    // The object it spawns relative to.
    bits.skip(10)?;
    let shape = match bits.read(2)? {
        0 => Shape::None,
        1 => Shape::Sphere {
            radius: dimension(bits.read(11)?),
        },
        2 => Shape::Cylinder {
            radius: dimension(bits.read(11)?),
            top: dimension(bits.read(11)?),
            bottom: dimension(bits.read(11)?),
        },
        _ => Shape::Box {
            width: dimension(bits.read(11)?),
            length: dimension(bits.read(11)?),
            top: dimension(bits.read(11)?),
            bottom: dimension(bits.read(11)?),
        },
    };
    let spawn_sequence = bits.byte()?;
    // Respawn time.
    bits.skip(8)?;
    let kind = u8::try_from(bits.read(5)?)?;
    let label = if bits.flag()? {
        None
    } else {
        Some(bits.read(8)?)
    };
    // Placement flags, then a color.
    bits.skip(8 + 4)?;
    let team = if bits.flag()? {
        None
    } else {
        Some(u8::try_from(bits.read(3)?)?)
    };
    // Type-specific fields: spare clips or a location name, or a teleporter's channel and
    // passability.
    match kind {
        1 | 19 => bits.skip(8)?,
        12..=14 => bits.skip(10)?,
        _ => {}
    }
    Ok(Some(Stored {
        entry,
        variant,
        position,
        axes,
        shape,
        label,
        kind,
        team,
        spawn_sequence,
    }))
}

/// The `mvar` chunk of a BLF file: its version and payload.
fn chunk(data: &[u8]) -> Result<(u16, &[u8])> {
    let mut at = 0;
    while at + CHUNK_HEADER <= data.len() {
        let signature = &data[at..at + 4];
        let size = usize::try_from(u32::from_be_bytes(data[at + 4..at + 8].try_into()?))?;
        ensure!(
            size >= CHUNK_HEADER && at + size <= data.len(),
            "A map variant chunk overruns the file"
        );
        if signature == CHUNK {
            let version = u16::from_be_bytes(data[at + 8..at + 10].try_into()?);
            return Ok((version, &data[at + CHUNK_HEADER..at + size]));
        }
        if signature == b"_eof" {
            break;
        }
        at += size;
    }
    bail!("The file holds no map variant chunk")
}

/// One variant of a map variant palette entry: the object it places and its model variant name.
/// A variant that names no object places nothing.
type PaletteVariant = Option<(Tag, String)>;

/// The scenario's map variant palette: each entry, in order across its palettes, with its
/// variants.
fn palette(cache: &Cache) -> Result<Vec<Vec<PaletteVariant>>> {
    let scenario = cache.only("scnr")?.address()?;
    let mut entries = Vec::new();
    for row in cache.block(scenario + PALETTES, PALETTE_SIZE)? {
        for entry in cache.block(row + PALETTE_ENTRIES, ENTRY_SIZE)? {
            let variants = cache
                .block(entry + ENTRY_VARIANTS, VARIANT_SIZE)?
                .into_iter()
                .map(|variant| {
                    let Some(tag) = cache.reference(variant + VARIANT_OBJECT, None)? else {
                        return Ok(None);
                    };
                    Ok(Some((
                        tag,
                        cache.string_id(cache.u32(variant + VARIANT_NAME)?)?,
                    )))
                })
                .collect::<Result<Vec<_>>>()?;
            entries.push(variants);
        }
    }
    Ok(entries)
}

/// Reads the map variant at `path` for the scenario of `cache`, whose map it must name, and
/// resolves its objects against the scenario's palette. The bitstream must end within the last
/// byte of the length the chunk declares, so a reading that drifts out of step fails.
pub fn read(cache: &Cache, path: &Path) -> Result<Variant> {
    let data = std::fs::read(path)
        .with_context(|| format!("Could not read the map variant {}", path.display()))?;
    let (version, payload) = chunk(&data)?;
    ensure!(
        u64::from(version) == VERSION,
        "The map variant {} has chunk version {version}, expected {VERSION}",
        path.display()
    );
    ensure!(payload.len() > HASH + 4, "The map variant chunk is empty");
    let declared = usize::try_from(u32::from_be_bytes(payload[HASH..HASH + 4].try_into()?))?;
    let stream = &payload[HASH + 4..];
    ensure!(
        declared <= stream.len(),
        "The map variant declares {declared} bytes of {}",
        stream.len()
    );
    let mut bits = Bits {
        data: &stream[..declared],
        at: 0,
    };
    // Content header: type (4 bits), file length (32), four 64-bit identifiers, then activity,
    // game mode and engine (3 bits each), the map ID and an engine category (8).
    bits.skip(4 + 32 + 4 * 64)?;
    let activity = bits.read(3)?;
    bits.skip(6)?;
    let map_id = u32::try_from(bits.read(32)?)?;
    bits.skip(8)?;
    for _ in 0..2 {
        // A timestamp and an identity of 64 bits each, the name, and whether the identity is
        // online.
        bits.skip(128)?;
        bits.ascii(AUTHOR_LENGTH)?;
        bits.skip(1)?;
    }
    let title = bits.wide(TEXT_LENGTH)?;
    bits.wide(TEXT_LENGTH)?;
    // The activity is stored plus one, and activity 2 names its hopper.
    if activity == 3 {
        bits.skip(16)?;
    }
    let content = bits.read(8)?;
    ensure!(
        content == VERSION,
        "The map variant has version {content}, expected {VERSION}"
    );
    // Two 32-bit fields, then the quota count.
    bits.skip(64)?;
    let quotas = bits.read(9)?;
    ensure!(
        quotas <= QUOTA_SLOTS,
        "The map variant keeps {quotas} quotas"
    );
    let variant_map = u32::try_from(bits.read(32)?)?;
    bits.skip(2)?;
    let mut bounds = [[0f32; 2]; 3];
    for axis in &mut bounds {
        *axis = [bits.float()?, bits.float()?];
        ensure!(
            axis[0].is_finite() && axis[1].is_finite() && axis[0] <= axis[1],
            "The map variant's world bounds are not ordered"
        );
    }
    // Two budgets.
    bits.skip(64)?;
    let label_count = usize::try_from(bits.read(9)?)?;
    let offsets = (0..label_count)
        .map(|_| {
            Ok(if bits.flag()? {
                Some(usize::try_from(bits.read(12)?)?)
            } else {
                None
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let mut labels = Vec::with_capacity(label_count);
    if label_count > 0 {
        let length = usize::try_from(bits.read(13)?)?;
        let text = if bits.flag()? {
            let size = usize::try_from(bits.read(13)?)?;
            let packed = (0..size).map(|_| bits.byte()).collect::<Result<Vec<_>>>()?;
            ensure!(packed.len() >= 4, "The map variant's labels are truncated");
            let mut text = Vec::new();
            flate2::read::ZlibDecoder::new(&packed[4..])
                .read_to_end(&mut text)
                .context("The map variant's labels do not inflate")?;
            text
        } else {
            (0..length)
                .map(|_| bits.byte())
                .collect::<Result<Vec<_>>>()?
        };
        for offset in offsets {
            labels.push(match offset {
                Some(start) => {
                    let end = text
                        .get(start..)
                        .and_then(|rest| rest.iter().position(|&c| c == 0))
                        .context("A map variant label overruns its table")?;
                    Some(
                        text[start..start + end]
                            .iter()
                            .map(|&c| char::from(c))
                            .collect(),
                    )
                }
                None => None,
            });
        }
    }
    let counts = position_bits(bounds);
    let mut stored = Vec::new();
    for _ in 0..OBJECT_SLOTS {
        if let Some(object) = object(&mut bits, bounds, counts)? {
            stored.push(object);
        }
    }
    // Each quota's three bytes.
    bits.skip(usize::try_from(quotas)? * 24)?;
    ensure!(
        bits.at + 8 > declared * 8,
        "The map variant {} leaves {} of its {} bits unread",
        path.display(),
        declared * 8 - bits.at,
        declared * 8
    );
    let scenario_map = cache.u32(cache.only("scnr")?.address()? + SCENARIO_MAP_ID)?;
    ensure!(
        map_id == scenario_map && variant_map == scenario_map,
        "The map variant {} is for map {map_id}, the cache is map {scenario_map}",
        path.display()
    );
    let entries = palette(cache)?;
    let offered = entries
        .iter()
        .flatten()
        .flatten()
        .map(|(tag, _)| tag.datum)
        .collect();
    let mut placements = Vec::with_capacity(stored.len());
    for object in stored {
        let (Some(entry), Some(variant)) = (object.entry, object.variant) else {
            continue;
        };
        let Some((tag, name)) = entries
            .get(usize::try_from(entry)?)
            .and_then(|variants| variants.get(usize::try_from(variant).ok()?))
            .cloned()
            .flatten()
        else {
            bail!(
                "A map variant object names palette entry {entry} variant {variant}, which the scenario lacks"
            );
        };
        let label = match object.label {
            Some(index) => labels.get(usize::try_from(index)?).cloned().flatten(),
            None => None,
        };
        let meters = |v: f32| v * METRES_PER_UNIT;
        placements.push(Placement {
            tag,
            variant: name,
            position: object.position.map(meters),
            axes: object.axes,
            shape: match object.shape {
                Shape::None => Shape::None,
                Shape::Sphere { radius } => Shape::Sphere {
                    radius: meters(radius),
                },
                Shape::Cylinder {
                    radius,
                    top,
                    bottom,
                } => Shape::Cylinder {
                    radius: meters(radius),
                    top: meters(top),
                    bottom: meters(bottom),
                },
                Shape::Box {
                    width,
                    length,
                    top,
                    bottom,
                } => Shape::Box {
                    width: meters(width),
                    length: meters(length),
                    top: meters(top),
                    bottom: meters(bottom),
                },
            },
            label,
            kind: object.kind,
            team: object.team,
            spawn_sequence: object.spawn_sequence,
        });
    }
    Ok(Variant {
        title,
        map_id,
        placements,
        offered,
    })
}
