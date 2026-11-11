//! The map's light collection: Destiny's deferred lights, which light players, weapons and every
//! other surface within their range. A converted level's lamps live only in its baked lightmap, so
//! its floors draw lit while a player beside a lamp draws unlit. The map places a light of its own
//! for each of the level's lights instead.
//!
//! A placement row of a light collection (resource `80806F5A`) names its record (`8080713A`) at
//! resource `+0x10`. The record holds its bounds at `+0x10`, its lights at `+0x30` (`0xA0` byte
//! rows), each light's rotation and position at `+0x40` (`0x20` byte rows: a quaternion, then a
//! position with w 1), its light count at `+0x50`, at `+0x54` how many of the first lights name a
//! second technique, and its occlusion bounds record (`80809671`) at `+0x58`. A light row holds the
//! volume its pass draws as a matrix at `+0x20`, a point light's scaled by its range with each axis
//! negated, two bounding spheres at `+0x60` (a point light's at its middle, the range and the
//! range times the square root of three), its shading technique at `+0x80`, a second technique or
//! none at `+0x84`, and two light programs (`808071DC`) at `+0x88` and `+0x8C`. The first
//! program's value rows at `+0x60` hold `(start, range, -1, -1)` for a point light in row one and
//! its color times its intensity in row two. The second's first row repeats the color. The occlusion
//! bounds record keeps a `0x30` byte row per light at `+0x08`: its box's corners with w 1 and, at
//! `+0x24`, an identifier unique to the light.
//!
//! The light's shading reads its color and range from its techniques' pixel stage initial rows
//! (stage `+0x2C8`, rows at stage `+0x50`), not from the programs. Pixel program `80FEB406` lights
//! with `lerp(row 15, row 13 * row 14.x, edge)`: row 15 the color times the intensity, row 13 the
//! normalized color at the light's edge and row 14's x the intensity, and fades by
//! `saturate(distance * row16.x + row16.y)^2` with row 16 `(-k, range * k, k, k)` and
//! `k = 1 / (range - start)`, as every inspected stock light holds it. The second technique's
//! program `80C6BF41` takes the normalized color in rows 11 and 20, the edge color less it in row
//! 12, the color times the intensity with the intensity in w in row 38 and the range terms in row
//! 39. Copied with Midtown's first point light, every converted light drew with its 2 m reddish
//! lamp in place of its own.
//!
//! A projected light's volume is an inverse perspective along its x axis: rows
//! `(0, -t, 0, 0)`, `(0, 0, t, 0)`, `(0, 0, 0, 1 / range - 1 / near)` and `(1, 0, 0, 1 / near)`,
//! with `t` the tangent of its cone's half angle and its near plane at its range over 50.5, as
//! every inspected stock projected light holds them. Its first sphere lies on its axis with radius
//! `range / (2 cos(half angle / 2))`, and its second bounds the volume's corners, which stock
//! lights from 35 degrees hold exactly and narrower stock cones hold within a larger, approximate
//! sphere. Its program's row one holds `(start, range, 0, half angle)`. Pixel program `80FE7A03`
//! takes its cone in row 11 as `saturate(cos * x + y)^z` of the angle from its axis, its color in
//! row 13 and its range terms in row 14. Program `80FE7A13` adds a gel at texture slot 3, sampled
//! through the light's projection, and takes them in rows 15, 17 and 18. The renderer fills both
//! techniques' remaining rows, which stock packages leave zero. Neither names a second technique.
//!
//! So no light value comes from a donor. The table, record and occlusion bounds copy Distant
//! Shore's light collection (`80EB02EB`), whose structure alone the map keeps, and every point light
//! copies its white 20 m point light that draws with techniques `80FE7A1F` and `80FE7A1E`, checked
//! by their pixel programs. Spot lights copy a stock collection's (`80C5DE6E`) first plain and first
//! gel projected light, checked likewise, and without them become point lights. Each light's values
//! take their own copies of its techniques and programs with every one of those rows written from
//! the converted light, its edge color its own. Whether Destiny's occlusion culling consults the
//! identifiers, which the map's spatial tome does not know, was not traced.
use sundial::package_authoring::PackageManager;
use tiger_pkg::TagHash;

use crate::{
    AuthoringResult,
    error::invalid,
    tag_payload::{
        append_native_array, array_at, read_i64, read_tag, read_u32, read_u64,
        synchronize_payload_size, write_bytes, write_relative_pointer, write_u32,
    },
};

use super::{ROW_POINTERS, ROW_RESOURCE, ROW_SIZE, TABLE_ROW_CLASS, TABLE_ROWS, class};

/// The world whose light collection the map's lights copy.
pub(super) const LIGHT_WORLD: &str = "pvp_greenhouse_2";
/// The stock light collection whose projected lights the map's spot lights copy.
const SPOT_COLLECTION: u32 = 0x80C5_DE6E;
/// The placement resource class of a light collection.
const LIGHT_COLLECTION: u32 = 0x8080_6F5A;
const RECORD_FIELD: usize = 0x10;
const RECORD_CLASS: u32 = 0x8080_713A;
const BOUNDS: usize = 0x10;
const LIGHTS: usize = 0x30;
const TRANSFORMS: usize = 0x40;
const COUNT: usize = 0x50;
const SECOND_COUNT: usize = 0x54;
const OCCLUSION: usize = 0x58;
/// The record's fixed fields end where its first array begins.
const HEADER: usize = 0x60;
const LIGHT_SIZE: usize = 0xA0;
const TRANSFORM_SIZE: usize = 0x20;
const VOLUME: usize = 0x20;
const SPHERES: usize = 0x60;
const TECHNIQUE: usize = 0x80;
const SECOND_TECHNIQUE: usize = 0x84;
const PROGRAM: usize = 0x88;
const SUPPLEMENTAL: usize = 0x8C;
const NO_TECHNIQUE: u32 = u32::MAX;
const OCCLUSION_CLASS: u32 = 0x8080_9671;
const BOXES: usize = 0x08;
const BOX_SIZE: usize = 0x30;
const BOX_IDENTITY: usize = 0x24;
const PROGRAM_CLASS: u32 = 0x8080_71DC;
const VALUES: usize = 0x60;
const VALUE_SIZE: usize = 0x10;
/// A point light's first value row: no cone.
const POINT_SHAPE: [f32; 2] = [-1., -1.];
const TECHNIQUE_CLASS: u32 = 0x8080_71E8;
/// A technique's pixel stage, its program at `+0x00`, its textures at `+0x08` (a slot and a tag
/// each) and its initial rows at `+0x50`.
const PIXEL_STAGE: usize = 0x2C8;
const STAGE_TEXTURES: usize = 0x08;
const TEXTURE_ROW: usize = 8;
const STAGE_INITIAL: usize = 0x50;
/// The pixel programs of the shading and second techniques, and the rows each reads per light.
const SHADING_PROGRAM: u32 = 0x80FE_B406;
const SECOND_PROGRAM: u32 = 0x80C6_BF41;
const SHADING_EDGE: usize = 13;
const SHADING_INTENSITY: usize = 14;
const SHADING_COLOR: usize = 15;
const SHADING_RANGE: usize = 16;
const SECOND_NORMALIZED: [usize; 2] = [11, 20];
const SECOND_EDGE: usize = 12;
const SECOND_COLOR: usize = 38;
const SECOND_RANGE: usize = 39;
/// A projected light's near plane is its range over this.
const NEAR_DIVISOR: f32 = 50.5;
/// The texture slot a gel projected light samples its gel from.
const GEL_SLOT: u32 = 3;
/// Rec. 601 luma, which every inspected stock light's color row holds in w.
const LUMA: [f32; 3] = [0.3, 0.59, 0.11];

/// The pixel program and the cone, color and range rows of a projected light's technique.
#[derive(Clone, Copy)]
struct Shape {
    program: u32,
    cone: usize,
    color: usize,
    range: usize,
    gel: bool,
}
const SPOT: Shape = Shape {
    program: 0x80FE_7A03,
    cone: 11,
    color: 13,
    range: 14,
    gel: false,
};
const GEL: Shape = Shape {
    program: 0x80FE_7A13,
    cone: 15,
    color: 17,
    range: 18,
    gel: true,
};

/// A spot light's cone: its unit forward and up directions, the half angles in radians within
/// which it shines fully and beyond which it is dark, its falloff's power between them, and its
/// gel's index among the gels the map's light tags take.
#[derive(Clone, Debug)]
pub(in crate::map) struct Cone {
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub inner: f32,
    pub outer: f32,
    pub exponent: f32,
    pub gel: Option<usize>,
}

/// One of the map's lights: its position in the map's coordinates, where its falloff starts and
/// its range in meters, its color times its intensity in Destiny's units, a spot's cone and its
/// occlusion identifier.
#[derive(Clone, Debug)]
pub(in crate::map) struct Light {
    pub position: [f32; 3],
    pub start: f32,
    pub range: f32,
    pub color: [f32; 3],
    pub cone: Option<Cone>,
    pub identity: u32,
}

/// A stock projected light the map's spot lights copy: its row, technique and programs.
#[derive(Clone)]
struct Projected {
    shape: Shape,
    row: Vec<u8>,
    technique: (TagHash, Vec<u8>),
    program: (TagHash, Vec<u8>),
    supplemental: (TagHash, Vec<u8>),
}

/// The table, record and point light the map's lights copy, with the light's techniques and
/// programs, and the projected lights its spot lights copy.
#[derive(Clone)]
pub(super) struct Template {
    table: u32,
    table_payload: Vec<u8>,
    field: usize,
    record: TagHash,
    record_payload: Vec<u8>,
    occlusion: TagHash,
    occlusion_payload: Vec<u8>,
    row: Vec<u8>,
    transform: Vec<u8>,
    shading: (TagHash, Vec<u8>),
    second: (TagHash, Vec<u8>),
    program: (TagHash, Vec<u8>),
    supplemental: (TagHash, Vec<u8>),
    spot: Option<Projected>,
    gel: Option<Projected>,
}

fn f32_at(data: &[u8], offset: usize) -> AuthoringResult<f32> {
    Ok(f32::from_bits(read_u32(data, offset)?))
}

fn put_floats(data: &mut [u8], offset: usize, values: &[f32]) -> AuthoringResult<()> {
    let bytes = values
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect::<Vec<_>>();
    write_bytes(data, offset, &bytes)
}

/// The value rows of light program `payload`.
fn values(payload: &[u8]) -> AuthoringResult<(usize, usize)> {
    let (count, _, rows, _) = array_at(payload, VALUES)?;
    Ok((count, rows))
}

/// The initial rows of technique `payload`'s pixel stage, when its pixel program is `program` and
/// it holds more than `last` rows.
fn initial_rows(payload: &[u8], program: u32, last: usize) -> AuthoringResult<Option<usize>> {
    if read_u32(payload, PIXEL_STAGE)? != program {
        return Ok(None);
    }
    let (count, _, rows, _) = array_at(payload, PIXEL_STAGE + STAGE_INITIAL)?;
    Ok((count > last && rows + count * VALUE_SIZE <= payload.len()).then_some(rows))
}

/// The texture row of technique `payload`'s pixel stage that binds `slot`.
fn texture_row(payload: &[u8], slot: u32) -> AuthoringResult<Option<usize>> {
    if read_u64(payload, PIXEL_STAGE + STAGE_TEXTURES)? == 0 {
        return Ok(None);
    }
    let (count, _, rows, _) = array_at(payload, PIXEL_STAGE + STAGE_TEXTURES)?;
    if rows + count * TEXTURE_ROW > payload.len() {
        return Ok(None);
    }
    for row in (0..count).map(|index| rows + index * TEXTURE_ROW) {
        if read_u32(payload, row)? == slot {
            return Ok(Some(row));
        }
    }
    Ok(None)
}

/// The point light range of light row `row` whose program is `program`: its volume a uniform
/// negated scale, its spheres the range and the range times the square root of three, and its
/// program's first value row `(start, range, -1, -1)`.
fn point_range(row: &[u8], program: &[u8]) -> AuthoringResult<Option<f32>> {
    let volume = (0..16)
        .map(|k| f32_at(row, VOLUME + k * 4))
        .collect::<AuthoringResult<Vec<_>>>()?;
    let range = -volume[0];
    let diagonal = [0, 5, 10];
    let uniform = range > 0.
        && diagonal.iter().all(|&k| volume[k] == -range)
        && (0..15).all(|k| diagonal.contains(&k) || volume[k] == 0.)
        && volume[15] == 1.
        && f32_at(row, SPHERES + 12)? == range;
    if !uniform {
        return Ok(None);
    }
    let (count, rows) = values(program)?;
    if count < 3 {
        return Ok(None);
    }
    let shape = [
        f32_at(program, rows + VALUE_SIZE + 4)?,
        f32_at(program, rows + VALUE_SIZE + 8)?,
        f32_at(program, rows + VALUE_SIZE + 12)?,
    ];
    Ok((shape == [range, POINT_SHAPE[0], POINT_SHAPE[1]]).then_some(range))
}

/// A projected light's volume for `range` and cone half angle `outer`, and its two spheres.
fn projection(range: f32, outer: f32) -> ([f32; 16], [f32; 8]) {
    let t = outer.tan();
    let near = range / NEAR_DIVISOR;
    let volume = [
        0.,
        -t,
        0.,
        0.,
        0.,
        0.,
        t,
        0.,
        0.,
        0.,
        0.,
        1. / range - 1. / near,
        1.,
        0.,
        0.,
        1. / near,
    ];
    let first = range / (2. * (outer * 0.5).cos());
    // The smallest sphere about the axis holding the volume's near and far corners.
    let spread = 2. * t * t;
    let (middle, radius) =
        if (range - near).powi(2) + spread * near * near <= spread * range * range {
            (range, range * spread.sqrt())
        } else {
            let middle = (range + near) * (1. + spread) * 0.5;
            (
                middle,
                ((range - middle).powi(2) + spread * range * range).sqrt(),
            )
        };
    (volume, [first, 0., 0., first, middle, 0., 0., radius])
}

/// Whether light row `row` with program `program` is a projected light of the volume
/// [`projection`] writes for its program's range and half angle.
fn projected_shape(row: &[u8], program: &[u8]) -> AuthoringResult<bool> {
    let (count, rows) = values(program)?;
    if count < 3 {
        return Ok(false);
    }
    let shape = (0..4)
        .map(|k| f32_at(program, rows + VALUE_SIZE + k * 4))
        .collect::<AuthoringResult<Vec<_>>>()?;
    let (range, outer) = (shape[1], shape[3]);
    if !(range > shape[0] && shape[2] == 0. && outer > 0. && outer < 1.5) {
        return Ok(false);
    }
    let (volume, _) = projection(range, outer);
    for (k, expected) in volume.into_iter().enumerate() {
        let actual = f32_at(row, VOLUME + k * 4)?;
        if (actual - expected).abs() > 1e-3 * expected.abs().max(1.) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The first light of stock collection [`SPOT_COLLECTION`] drawn with `shape`'s program, without
/// a second technique, holding a projected volume, and, for a gel shape, a texture at
/// [`GEL_SLOT`].
fn projected(manager: &PackageManager, shape: Shape) -> AuthoringResult<Option<Projected>> {
    let record = TagHash(SPOT_COLLECTION);
    if class(manager, record) != Some(RECORD_CLASS) {
        return Ok(None);
    }
    let payload = read_tag(manager, record, "projected light collection")?;
    let (count, _, lights, _) = array_at(&payload, LIGHTS)?;
    if lights + count * LIGHT_SIZE > payload.len() {
        return Ok(None);
    }
    let read = |tag: u32, expected: u32| -> AuthoringResult<Option<(TagHash, Vec<u8>)>> {
        let tag = TagHash(tag);
        if class(manager, tag) != Some(expected) {
            return Ok(None);
        }
        Ok(Some((
            tag,
            read_tag(manager, tag, "projected light resource")?,
        )))
    };
    for index in 0..count {
        let row = &payload[lights + index * LIGHT_SIZE..lights + (index + 1) * LIGHT_SIZE];
        if read_u32(row, SECOND_TECHNIQUE)? != NO_TECHNIQUE {
            continue;
        }
        let (Some(technique), Some(program), Some(supplemental)) = (
            read(read_u32(row, TECHNIQUE)?, TECHNIQUE_CLASS)?,
            read(read_u32(row, PROGRAM)?, PROGRAM_CLASS)?,
            read(read_u32(row, SUPPLEMENTAL)?, PROGRAM_CLASS)?,
        ) else {
            continue;
        };
        let last = shape.cone.max(shape.color).max(shape.range);
        if initial_rows(&technique.1, shape.program, last)?.is_none()
            || !projected_shape(row, &program.1)?
            || values(&supplemental.1)?.0 == 0
            || (shape.gel && texture_row(&technique.1, GEL_SLOT)?.is_none())
        {
            continue;
        }
        return Ok(Some(Projected {
            shape,
            row: row.to_vec(),
            technique,
            program,
            supplemental,
        }));
    }
    Ok(None)
}

/// The light template of placement table `table`: when a row places a light collection holding a
/// point light that draws with the checked shading and second programs, a copy of the table
/// holding that row alone, with the record, light, techniques and programs. Distant Shore's table
/// (`80EB02EC`) also places ten shadowing lights, which the copy leaves out.
pub(super) fn template(manager: &PackageManager, table: u32) -> AuthoringResult<Option<Template>> {
    if class(manager, TagHash(table)) != Some(super::TABLE_CLASS) {
        return Ok(None);
    }
    let mut table_payload = read_tag(manager, TagHash(table), "light placement table")?;
    if read_u64(&table_payload, TABLE_ROWS)? == 0 {
        return Ok(None);
    }
    let (count, _, rows, row_class) = array_at(&table_payload, TABLE_ROWS)?;
    if row_class != TABLE_ROW_CLASS || rows + count * ROW_SIZE > table_payload.len() {
        return Ok(None);
    }
    let target = |payload: &[u8], field: usize| -> AuthoringResult<Option<usize>> {
        let relative = read_i64(payload, field)?;
        Ok(field
            .checked_add_signed(relative as isize)
            .filter(|&at| relative != 0 && at < payload.len()))
    };
    let mut found = None;
    for row in (0..count).map(|index| rows + index * ROW_SIZE) {
        if let Some(resource) = target(&table_payload, row + ROW_RESOURCE)?
            && resource >= 4
            && resource + RECORD_FIELD + 4 <= table_payload.len()
            && read_u32(&table_payload, resource - 4)? == LIGHT_COLLECTION
        {
            found = Some((row, resource));
            break;
        }
    }
    let Some((row, resource)) = found else {
        return Ok(None);
    };
    // The copy's rows hold this row alone, its pointers rebased to the targets they named.
    let targets = ROW_POINTERS
        .iter()
        .map(|&field| target(&table_payload, row + field))
        .collect::<AuthoringResult<Vec<_>>>()?;
    let row_bytes = table_payload[row..row + ROW_SIZE].to_vec();
    append_native_array(
        &mut table_payload,
        TABLE_ROWS,
        TABLE_ROW_CLASS,
        1,
        &row_bytes,
    )?;
    let own_row = array_at(&table_payload, TABLE_ROWS)?.2;
    for (&field, target) in ROW_POINTERS.iter().zip(targets) {
        match target {
            Some(at) => write_relative_pointer(&mut table_payload, own_row + field, at)?,
            None => write_bytes(&mut table_payload, own_row + field, &0i64.to_le_bytes())?,
        }
    }
    let table_payload = synchronize_payload_size(table_payload)?;
    let field = resource + RECORD_FIELD;
    let record = TagHash(read_u32(&table_payload, field)?);
    if class(manager, record) != Some(RECORD_CLASS) {
        return Ok(None);
    }
    let record_payload = read_tag(manager, record, "light collection")?;
    let occlusion = TagHash(read_u32(&record_payload, OCCLUSION)?);
    if class(manager, occlusion) != Some(OCCLUSION_CLASS) {
        return Ok(None);
    }
    let occlusion_payload = read_tag(manager, occlusion, "light occlusion bounds")?;
    let (count, _, lights, _) = array_at(&record_payload, LIGHTS)?;
    let (transforms_count, _, transforms, _) = array_at(&record_payload, TRANSFORMS)?;
    if transforms_count != count
        || lights + count * LIGHT_SIZE > record_payload.len()
        || transforms + count * TRANSFORM_SIZE > record_payload.len()
    {
        return Err(invalid("The light collection has an unexpected layout"));
    }
    let read = |tag: TagHash, expected: u32| -> AuthoringResult<Option<(TagHash, Vec<u8>)>> {
        if class(manager, tag) != Some(expected) {
            return Ok(None);
        }
        Ok(Some((tag, read_tag(manager, tag, "light resource")?)))
    };
    for index in 0..count {
        let row = &record_payload[lights + index * LIGHT_SIZE..lights + (index + 1) * LIGHT_SIZE];
        let (Some(shading), Some(second), Some(program), Some(supplemental)) = (
            read(TagHash(read_u32(row, TECHNIQUE)?), TECHNIQUE_CLASS)?,
            read(TagHash(read_u32(row, SECOND_TECHNIQUE)?), TECHNIQUE_CLASS)?,
            read(TagHash(read_u32(row, PROGRAM)?), PROGRAM_CLASS)?,
            read(TagHash(read_u32(row, SUPPLEMENTAL)?), PROGRAM_CLASS)?,
        ) else {
            continue;
        };
        if point_range(row, &program.1)?.is_none()
            || values(&supplemental.1)?.0 == 0
            || initial_rows(&shading.1, SHADING_PROGRAM, SHADING_RANGE)?.is_none()
            || initial_rows(&second.1, SECOND_PROGRAM, SECOND_RANGE)?.is_none()
        {
            continue;
        }
        // Spot lights are optional: without their stock templates they light all around them.
        let spot = projected(manager, SPOT).ok().flatten();
        let gel = projected(manager, GEL).ok().flatten();
        return Ok(Some(Template {
            table,
            table_payload,
            field,
            record,
            record_payload: record_payload.clone(),
            occlusion,
            occlusion_payload,
            row: row.to_vec(),
            transform: record_payload
                [transforms + index * TRANSFORM_SIZE..transforms + (index + 1) * TRANSFORM_SIZE]
                .to_vec(),
            shading,
            second,
            program,
            supplemental,
            spot,
            gel,
        }));
    }
    Ok(None)
}

/// How a light draws: a point light, or a spot light with or without its gel.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Point,
    Spot,
    Gel,
}

/// The values a light's technique and program copies hold, which lights with equal values share:
/// its kind, start, range and color, and a spot's inner and outer half angles, falloff power and
/// gel.
#[derive(Clone, Copy, PartialEq)]
struct Values {
    kind: Kind,
    start: f32,
    range: f32,
    color: [f32; 3],
    cone: [f32; 3],
    gel: Option<usize>,
}

impl Values {
    fn tags(&self) -> usize {
        match self.kind {
            Kind::Point => 4,
            Kind::Spot | Kind::Gel => 3,
        }
    }
}

impl Template {
    /// How `light` draws with the templates found: a spot light without its template, or a gel
    /// light without the gel template, draws as the next simpler kind.
    fn kind(&self, light: &Light) -> Kind {
        match &light.cone {
            Some(cone) if cone.gel.is_some() && self.gel.is_some() => Kind::Gel,
            Some(_) if self.spot.is_some() => Kind::Spot,
            _ => Kind::Point,
        }
    }

    fn values(&self, light: &Light) -> Values {
        let kind = self.kind(light);
        let cone = light.cone.as_ref().filter(|_| kind != Kind::Point);
        Values {
            kind,
            start: light.start,
            range: light.range,
            color: light.color,
            cone: cone.map_or([0.; 3], |cone| [cone.inner, cone.outer, cone.exponent]),
            gel: cone.and_then(|cone| cone.gel).filter(|_| kind == Kind::Gel),
        }
    }

    /// The order the collection holds `lights` in, those with a second technique first, the
    /// distinct values among them, and the group of each light in that order.
    fn groups(&self, lights: &[Light]) -> (Vec<usize>, Vec<Values>, Vec<usize>) {
        let mut order = (0..lights.len()).collect::<Vec<_>>();
        order.sort_by_key(|&index| self.kind(&lights[index]) != Kind::Point);
        let mut distinct = Vec::<Values>::new();
        let mut members = Vec::with_capacity(lights.len());
        for &index in &order {
            let values = self.values(&lights[index]);
            let group = match distinct.iter().position(|other| *other == values) {
                Some(group) => group,
                None => {
                    distinct.push(values);
                    distinct.len() - 1
                }
            };
            members.push(group);
        }
        (order, distinct, members)
    }

    /// Entries the map's lights take: the table, the record, its occlusion bounds and each
    /// group's techniques and programs, or none without lights.
    pub(super) fn entries(&self, lights: &[Light]) -> usize {
        if lights.is_empty() {
            0
        } else {
            3 + self
                .groups(lights)
                .1
                .iter()
                .map(Values::tags)
                .sum::<usize>()
        }
    }

    /// The stock techniques the map's copies draw with, which the map's container loads.
    pub(super) fn techniques(&self) -> Vec<TagHash> {
        let mut techniques = vec![self.shading.0, self.second.0];
        techniques.extend(
            [&self.spot, &self.gel]
                .into_iter()
                .flatten()
                .map(|projected| projected.technique.0),
        );
        techniques
    }

    /// The map's light tags, each a template, its payload and its tag from `at`: the table, the
    /// record, its occlusion bounds and each group's techniques and programs, in
    /// [`Template::entries`] order. A gel light's technique binds the gel at its index in `gels`.
    pub(super) fn tags(
        &self,
        lights: &[Light],
        gels: &[TagHash],
        at: &dyn Fn(usize) -> AuthoringResult<TagHash>,
    ) -> AuthoringResult<Vec<(TagHash, Vec<u8>, TagHash)>> {
        let (order, distinct, members) = self.groups(lights);
        let (table, record, occlusion) = (at(0)?, at(1)?, at(2)?);
        let mut next = 3;
        let mut own = Vec::with_capacity(distinct.len());
        for values in &distinct {
            let tags = (next..next + values.tags())
                .map(at)
                .collect::<AuthoringResult<Vec<_>>>()?;
            next += values.tags();
            own.push(tags);
        }
        let ordered = order
            .iter()
            .map(|&index| &lights[index])
            .collect::<Vec<_>>();
        let mut tags = Vec::with_capacity(next);
        let mut table_payload = self.table_payload.clone();
        write_u32(&mut table_payload, self.field, record.0)?;
        tags.push((TagHash(self.table), table_payload, table));
        tags.push((
            self.record,
            self.collection(&ordered, &distinct, &members, &own, occlusion)?,
            record,
        ));
        tags.push((self.occlusion, self.bounds(&ordered)?, occlusion));
        for (values, own) in distinct.iter().zip(&own) {
            let (start, range, color) = (values.start, values.range, values.color);
            match self.projected(values.kind) {
                None => {
                    tags.push((
                        self.shading.0,
                        self.shading_copy(start, range, color)?,
                        own[0],
                    ));
                    tags.push((
                        self.second.0,
                        self.second_copy(start, range, color)?,
                        own[1],
                    ));
                    tags.push((
                        self.program.0,
                        light_program(
                            &self.program.1,
                            [start, range, POINT_SHAPE[0], POINT_SHAPE[1]],
                            color,
                        )?,
                        own[2],
                    ));
                    tags.push((
                        self.supplemental.0,
                        light_supplemental(&self.supplemental.1, color)?,
                        own[3],
                    ));
                }
                Some(projected) => {
                    let gel = match values.gel {
                        Some(index) => Some(
                            *gels
                                .get(index)
                                .ok_or_else(|| invalid("A map light names a missing gel"))?,
                        ),
                        None => None,
                    };
                    tags.push((
                        projected.technique.0,
                        projected_copy(projected, values, gel)?,
                        own[0],
                    ));
                    tags.push((
                        projected.program.0,
                        light_program(
                            &projected.program.1,
                            [start, range, 0., values.cone[1]],
                            color,
                        )?,
                        own[1],
                    ));
                    tags.push((
                        projected.supplemental.0,
                        light_supplemental(&projected.supplemental.1, color)?,
                        own[2],
                    ));
                }
            }
        }
        Ok(tags)
    }

    /// The stock projected light a light of `kind` copies, none for a point light.
    fn projected(&self, kind: Kind) -> Option<&Projected> {
        match kind {
            Kind::Point => None,
            Kind::Spot => self.spot.as_ref(),
            Kind::Gel => self.gel.as_ref(),
        }
    }

    fn collection(
        &self,
        lights: &[&Light],
        distinct: &[Values],
        members: &[usize],
        own: &[Vec<TagHash>],
        occlusion: TagHash,
    ) -> AuthoringResult<Vec<u8>> {
        let (_, _, _, light_class) = array_at(&self.record_payload, LIGHTS)?;
        let (_, _, _, transform_class) = array_at(&self.record_payload, TRANSFORMS)?;
        let mut payload = self.record_payload[..HEADER].to_vec();
        let mut rows = Vec::with_capacity(lights.len() * LIGHT_SIZE);
        let mut transforms = Vec::with_capacity(lights.len() * TRANSFORM_SIZE);
        let mut low = [f32::INFINITY; 3];
        let mut high = [f32::NEG_INFINITY; 3];
        let mut seconds = 0u32;
        for (light, &group) in lights.iter().zip(members) {
            let r = light.range;
            let tags = &own[group];
            let values = &distinct[group];
            let (row, rotation) = match self.projected(values.kind) {
                None => {
                    let mut row = self.row.clone();
                    put_floats(
                        &mut row,
                        VOLUME,
                        &[
                            -r, 0., 0., 0., 0., -r, 0., 0., 0., 0., -r, 0., 0., 0., 0., 1.,
                        ],
                    )?;
                    put_floats(
                        &mut row,
                        SPHERES,
                        &[0., 0., 0., r, 0., 0., 0., r * 3f32.sqrt()],
                    )?;
                    write_u32(&mut row, TECHNIQUE, tags[0].0)?;
                    write_u32(&mut row, SECOND_TECHNIQUE, tags[1].0)?;
                    write_u32(&mut row, PROGRAM, tags[2].0)?;
                    write_u32(&mut row, SUPPLEMENTAL, tags[3].0)?;
                    seconds += 1;
                    (row, [0., 0., 0., 1.])
                }
                Some(projected) => {
                    let cone = light
                        .cone
                        .as_ref()
                        .ok_or_else(|| invalid("A spot light has no cone"))?;
                    let mut row = projected.row.clone();
                    let (volume, spheres) = projection(r, values.cone[1]);
                    put_floats(&mut row, VOLUME, &volume)?;
                    put_floats(&mut row, SPHERES, &spheres)?;
                    write_u32(&mut row, TECHNIQUE, tags[0].0)?;
                    write_u32(&mut row, SECOND_TECHNIQUE, NO_TECHNIQUE)?;
                    write_u32(&mut row, PROGRAM, tags[1].0)?;
                    write_u32(&mut row, SUPPLEMENTAL, tags[2].0)?;
                    (row, quaternion(cone.forward, cone.up))
                }
            };
            rows.extend_from_slice(&row);
            let mut transform = self.transform.clone();
            put_floats(&mut transform, 0, &rotation)?;
            put_floats(
                &mut transform,
                16,
                &[light.position[0], light.position[1], light.position[2], 1.],
            )?;
            transforms.extend_from_slice(&transform);
            let (box_low, box_high) = self.extent(light);
            for k in 0..3 {
                low[k] = low[k].min(box_low[k]);
                high[k] = high[k].max(box_high[k]);
            }
        }
        put_floats(
            &mut payload,
            BOUNDS,
            &[low[0], low[1], low[2], 1., high[0], high[1], high[2], 1.],
        )?;
        let count = u32::try_from(lights.len()).map_err(|_| invalid("Too many map lights"))?;
        write_u32(&mut payload, COUNT, count)?;
        // The point lights, which come first, name a second technique.
        write_u32(&mut payload, SECOND_COUNT, seconds)?;
        write_u32(&mut payload, OCCLUSION, occlusion.0)?;
        append_native_array(&mut payload, LIGHTS, light_class, lights.len(), &rows)?;
        append_native_array(
            &mut payload,
            TRANSFORMS,
            transform_class,
            lights.len(),
            &transforms,
        )?;
        synchronize_payload_size(payload)
    }

    /// The box `light` lights within: a point light's range around it, a spot light's apex and the
    /// corners of its volume's far end.
    fn extent(&self, light: &Light) -> ([f32; 3], [f32; 3]) {
        let r = light.range;
        let p = light.position;
        match light
            .cone
            .as_ref()
            .filter(|_| self.kind(light) != Kind::Point)
        {
            None => (p.map(|v| v - r), p.map(|v| v + r)),
            Some(cone) => {
                let t = cone.outer.tan();
                let left = cross(cone.up, cone.forward);
                let mut low = p;
                let mut high = p;
                for (a, b) in [(1., 1.), (1., -1.), (-1., 1.), (-1., -1.)] {
                    for k in 0..3 {
                        let corner =
                            p[k] + r * (cone.forward[k] + a * t * left[k] + b * t * cone.up[k]);
                        low[k] = low[k].min(corner);
                        high[k] = high[k].max(corner);
                    }
                }
                (low, high)
            }
        }
    }

    fn bounds(&self, lights: &[&Light]) -> AuthoringResult<Vec<u8>> {
        let (count, _, rows, box_class) = array_at(&self.occlusion_payload, BOXES)?;
        if count == 0 || rows + BOX_SIZE > self.occlusion_payload.len() {
            return Err(invalid("The light occlusion bounds hold no box"));
        }
        let template = self.occlusion_payload[rows..rows + BOX_SIZE].to_vec();
        let mut payload = self.occlusion_payload[..BOXES + 16].to_vec();
        let mut boxes = Vec::with_capacity(lights.len() * BOX_SIZE);
        for light in lights {
            let (low, high) = self.extent(light);
            let mut row = template.clone();
            put_floats(
                &mut row,
                0,
                &[low[0], low[1], low[2], 1., high[0], high[1], high[2], 1.],
            )?;
            write_u32(&mut row, BOX_IDENTITY, light.identity)?;
            boxes.extend_from_slice(&row);
        }
        append_native_array(&mut payload, BOXES, box_class, lights.len(), &boxes)?;
        synchronize_payload_size(payload)
    }

    fn shading_copy(&self, start: f32, range: f32, color: [f32; 3]) -> AuthoringResult<Vec<u8>> {
        let mut payload = self.shading.1.clone();
        let rows = initial_rows(&payload, SHADING_PROGRAM, SHADING_RANGE)?
            .ok_or_else(|| invalid("The light shading technique changed"))?;
        let ([r, g, b], intensity) = normalized(color);
        let row = |index: usize| rows + index * VALUE_SIZE;
        put_floats(&mut payload, row(SHADING_EDGE), &[r, g, b, 1.])?;
        put_floats(&mut payload, row(SHADING_INTENSITY), &[intensity])?;
        put_floats(
            &mut payload,
            row(SHADING_COLOR),
            &[color[0], color[1], color[2], intensity],
        )?;
        put_floats(&mut payload, row(SHADING_RANGE), &falloff(start, range))?;
        Ok(payload)
    }

    fn second_copy(&self, start: f32, range: f32, color: [f32; 3]) -> AuthoringResult<Vec<u8>> {
        let mut payload = self.second.1.clone();
        let rows = initial_rows(&payload, SECOND_PROGRAM, SECOND_RANGE)?
            .ok_or_else(|| invalid("The light second technique changed"))?;
        let ([r, g, b], intensity) = normalized(color);
        let row = |index: usize| rows + index * VALUE_SIZE;
        for index in SECOND_NORMALIZED {
            put_floats(&mut payload, row(index), &[r, g, b, 1.])?;
        }
        put_floats(&mut payload, row(SECOND_EDGE), &[0.; 4])?;
        put_floats(
            &mut payload,
            row(SECOND_COLOR),
            &[color[0], color[1], color[2], intensity],
        )?;
        put_floats(&mut payload, row(SECOND_RANGE), &falloff(start, range))?;
        Ok(payload)
    }
}

/// A copy of light program `template` whose first value row holds `shape` and whose second holds
/// `color`.
fn light_program(template: &[u8], shape: [f32; 4], color: [f32; 3]) -> AuthoringResult<Vec<u8>> {
    let mut payload = template.to_vec();
    let (_, rows) = values(&payload)?;
    put_floats(&mut payload, rows + VALUE_SIZE, &shape)?;
    put_floats(&mut payload, rows + 2 * VALUE_SIZE, &color)?;
    Ok(payload)
}

/// A copy of supplemental light program `template` whose first value row holds `color`.
fn light_supplemental(template: &[u8], color: [f32; 3]) -> AuthoringResult<Vec<u8>> {
    let mut payload = template.to_vec();
    let (_, rows) = values(&payload)?;
    put_floats(&mut payload, rows, &color)?;
    Ok(payload)
}

/// A copy of `projected`'s technique holding `values`' cone, color and range, binding `gel`.
fn projected_copy(
    projected: &Projected,
    values: &Values,
    gel: Option<TagHash>,
) -> AuthoringResult<Vec<u8>> {
    let shape = projected.shape;
    let mut payload = projected.technique.1.clone();
    let last = shape.cone.max(shape.color).max(shape.range);
    let rows = initial_rows(&payload, shape.program, last)?
        .ok_or_else(|| invalid("The projected light technique changed"))?;
    let row = |index: usize| rows + index * VALUE_SIZE;
    let [inner, outer, exponent] = values.cone;
    let (inner, outer) = (inner.cos(), outer.cos());
    let scale = 1. / (inner - outer).max(1e-4);
    put_floats(
        &mut payload,
        row(shape.cone),
        &[scale, -outer * scale, exponent, 1.],
    )?;
    let color = values.color;
    let luma = (0..3).map(|k| color[k] * LUMA[k]).sum::<f32>();
    put_floats(
        &mut payload,
        row(shape.color),
        &[color[0], color[1], color[2], luma],
    )?;
    put_floats(
        &mut payload,
        row(shape.range),
        &falloff(values.start, values.range),
    )?;
    if shape.gel {
        let at = texture_row(&payload, GEL_SLOT)?
            .ok_or_else(|| invalid("The gel light technique has no gel slot"))?;
        let gel = gel.ok_or_else(|| invalid("A gel light has no gel"))?;
        write_u32(&mut payload, at + 4, gel.0)?;
    }
    Ok(payload)
}

/// A light's falloff terms from `start` to `range` meters: `(-k, range * k, k, k)` with
/// `k = 1 / (range - start)`.
fn falloff(start: f32, range: f32) -> [f32; 4] {
    let k = 1. / (range - start).max(range * 0.01);
    [-k, range * k, k, k]
}

/// A light's color divided by its brightest channel, and that channel.
fn normalized(color: [f32; 3]) -> ([f32; 3], f32) {
    let peak = color.into_iter().fold(0f32, f32::max);
    if peak > 0. {
        (color.map(|c| c / peak), peak)
    } else {
        ([1.; 3], 0.)
    }
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The rotation, a quaternion `(x, y, z, w)`, taking a light's x axis to unit `forward` and its z
/// axis to unit `up`, perpendicular to it.
fn quaternion(forward: [f32; 3], up: [f32; 3]) -> [f32; 4] {
    let left = cross(up, forward);
    // Row r, column c of the rotation whose columns are forward, left and up.
    let m = |r: usize, c: usize| [forward, left, up][c][r];
    let trace = m(0, 0) + m(1, 1) + m(2, 2);
    let q = if trace > 0. {
        let s = (trace + 1.).sqrt() * 2.;
        [
            (m(2, 1) - m(1, 2)) / s,
            (m(0, 2) - m(2, 0)) / s,
            (m(1, 0) - m(0, 1)) / s,
            0.25 * s,
        ]
    } else if m(0, 0) > m(1, 1) && m(0, 0) > m(2, 2) {
        let s = (1. + m(0, 0) - m(1, 1) - m(2, 2)).sqrt() * 2.;
        [
            0.25 * s,
            (m(0, 1) + m(1, 0)) / s,
            (m(0, 2) + m(2, 0)) / s,
            (m(2, 1) - m(1, 2)) / s,
        ]
    } else if m(1, 1) > m(2, 2) {
        let s = (1. + m(1, 1) - m(0, 0) - m(2, 2)).sqrt() * 2.;
        [
            (m(0, 1) + m(1, 0)) / s,
            0.25 * s,
            (m(1, 2) + m(2, 1)) / s,
            (m(0, 2) - m(2, 0)) / s,
        ]
    } else {
        let s = (1. + m(2, 2) - m(0, 0) - m(1, 1)).sqrt() * 2.;
        [
            (m(0, 2) + m(2, 0)) / s,
            (m(1, 2) + m(2, 1)) / s,
            0.25 * s,
            (m(1, 0) - m(0, 1)) / s,
        ]
    };
    let length = q.iter().map(|v| v * v).sum::<f32>().sqrt();
    q.map(|v| v / length)
}
