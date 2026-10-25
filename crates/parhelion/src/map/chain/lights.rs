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
//! program's value rows at `+0x60` hold `(0, range, -1, -1)` for a point light in row one and its
//! color times its intensity in row two. The second's first row repeats the color. The occlusion
//! bounds record keeps a `0x30` byte row per light at `+0x08`: its box's corners with w 1 and, at
//! `+0x24`, an identifier unique to the light.
//!
//! The light's shading reads its color and range from its techniques' pixel stage initial rows
//! (stage `+0x2C8`, rows at stage `+0x50`), not from the programs. Pixel program `80FEB406` lights
//! with `lerp(row 15, row 13 * row 14.x, edge)`: row 15 the color times the intensity, row 13 the
//! normalized color at the light's edge and row 14's x the intensity, and fades by
//! `saturate(distance * row16.x + row16.y)` with row 16 `(-1 / range, 1, 1 / range, 1 / range)`.
//! The second technique's program `80C6BF41` takes the normalized color in rows 11 and 20, the edge
//! color less it in row 12, the color times the intensity with the intensity in w in row 38 and the
//! range terms in row 39. Copied with Midtown's first point light, every converted light drew with
//! its 2 m reddish lamp in place of its own.
//!
//! So no light value comes from a donor. The table, record and occlusion bounds copy Distant
//! Shore's light collection (`80EB02EB`), whose structure alone the map keeps, and every map light
//! copies its white 20 m point light that draws with techniques `80FE7A1F` and `80FE7A1E`, checked
//! by their pixel programs. Each range and color takes its own copies of both techniques and both
//! programs with every one of those rows written from the converted light, its edge color its own.
//! Whether Destiny's occlusion culling consults the identifiers, which the map's spatial tome does
//! not know, was not traced.
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
/// A technique's pixel stage, its program at `+0x00` and its initial rows at `+0x50`.
const PIXEL_STAGE: usize = 0x2C8;
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

/// One of the map's lights: its position in the map's coordinates, its range in meters, its color
/// times its intensity in Destiny's units and its occlusion identifier.
#[derive(Clone, Debug)]
pub(in crate::map) struct Light {
    pub position: [f32; 3],
    pub range: f32,
    pub color: [f32; 3],
    pub identity: u32,
}

/// The table, record and point light the map's lights copy, with the light's techniques and
/// programs.
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

/// The point light range of light row `row` whose program is `program`: its volume a uniform
/// negated scale, its spheres the range and the range times the square root of three, and its
/// program's first value row `(0, range, -1, -1)`.
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
        }));
    }
    Ok(None)
}

/// The distinct ranges and colors of `lights`, and the group of each light.
fn groups(lights: &[Light]) -> (Vec<(f32, [f32; 3])>, Vec<usize>) {
    let key = |light: &Light| {
        (
            light.range.to_bits(),
            light.color.map(|channel| channel.to_bits()),
        )
    };
    let mut distinct = Vec::<(f32, [f32; 3])>::new();
    let mut keys = Vec::new();
    let mut members = Vec::with_capacity(lights.len());
    for light in lights {
        let k = key(light);
        let group = match keys.iter().position(|other| *other == k) {
            Some(group) => group,
            None => {
                keys.push(k);
                distinct.push((light.range, light.color));
                distinct.len() - 1
            }
        };
        members.push(group);
    }
    (distinct, members)
}

/// Tags each group of lights takes: both techniques and both programs.
const GROUP_TAGS: usize = 4;

/// Entries the map's lights take: the table, the record, its occlusion bounds and
/// [`GROUP_TAGS`] per group, or none without lights.
pub(super) fn entries(lights: &[Light]) -> usize {
    if lights.is_empty() {
        0
    } else {
        3 + GROUP_TAGS * groups(lights).0.len()
    }
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

impl Template {
    /// The stock techniques the map's copies draw with, which the map's container loads.
    pub(super) fn techniques(&self) -> Vec<TagHash> {
        vec![self.shading.0, self.second.0]
    }

    /// The map's light tags, each a template, its payload and its tag from `at`: the table, the
    /// record, its occlusion bounds and [`GROUP_TAGS`] per group, in [`entries`] order.
    pub(super) fn tags(
        &self,
        lights: &[Light],
        at: &dyn Fn(usize) -> AuthoringResult<TagHash>,
    ) -> AuthoringResult<Vec<(TagHash, Vec<u8>, TagHash)>> {
        let (distinct, members) = groups(lights);
        let (table, record, occlusion) = (at(0)?, at(1)?, at(2)?);
        let own = (0..distinct.len())
            .map(|group| {
                let base = 3 + GROUP_TAGS * group;
                Ok([at(base)?, at(base + 1)?, at(base + 2)?, at(base + 3)?])
            })
            .collect::<AuthoringResult<Vec<_>>>()?;
        let mut tags = Vec::with_capacity(3 + GROUP_TAGS * distinct.len());
        let mut table_payload = self.table_payload.clone();
        write_u32(&mut table_payload, self.field, record.0)?;
        tags.push((TagHash(self.table), table_payload, table));
        tags.push((
            self.record,
            self.collection(lights, &members, &own, occlusion)?,
            record,
        ));
        tags.push((self.occlusion, self.bounds(lights)?, occlusion));
        for (&(range, color), [shading, second, program, supplemental]) in distinct.iter().zip(&own)
        {
            tags.push((self.shading.0, self.shading_copy(range, color)?, *shading));
            tags.push((self.second.0, self.second_copy(range, color)?, *second));
            tags.push((self.program.0, self.light_program(range, color)?, *program));
            tags.push((
                self.supplemental.0,
                self.light_supplemental(color)?,
                *supplemental,
            ));
        }
        Ok(tags)
    }

    fn collection(
        &self,
        lights: &[Light],
        members: &[usize],
        own: &[[TagHash; GROUP_TAGS]],
        occlusion: TagHash,
    ) -> AuthoringResult<Vec<u8>> {
        let (_, _, _, light_class) = array_at(&self.record_payload, LIGHTS)?;
        let (_, _, _, transform_class) = array_at(&self.record_payload, TRANSFORMS)?;
        let mut payload = self.record_payload[..HEADER].to_vec();
        let mut rows = Vec::with_capacity(lights.len() * LIGHT_SIZE);
        let mut transforms = Vec::with_capacity(lights.len() * TRANSFORM_SIZE);
        let mut low = [f32::INFINITY; 3];
        let mut high = [f32::NEG_INFINITY; 3];
        for (light, &group) in lights.iter().zip(members) {
            let r = light.range;
            let [shading, second, program, supplemental] = own[group];
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
            write_u32(&mut row, TECHNIQUE, shading.0)?;
            write_u32(&mut row, SECOND_TECHNIQUE, second.0)?;
            write_u32(&mut row, PROGRAM, program.0)?;
            write_u32(&mut row, SUPPLEMENTAL, supplemental.0)?;
            rows.extend_from_slice(&row);
            let mut transform = self.transform.clone();
            put_floats(&mut transform, 0, &[0., 0., 0., 1.])?;
            put_floats(
                &mut transform,
                16,
                &[light.position[0], light.position[1], light.position[2], 1.],
            )?;
            transforms.extend_from_slice(&transform);
            for k in 0..3 {
                low[k] = low[k].min(light.position[k] - r);
                high[k] = high[k].max(light.position[k] + r);
            }
        }
        put_floats(
            &mut payload,
            BOUNDS,
            &[low[0], low[1], low[2], 1., high[0], high[1], high[2], 1.],
        )?;
        let count = u32::try_from(lights.len()).map_err(|_| invalid("Too many map lights"))?;
        write_u32(&mut payload, COUNT, count)?;
        // Every light names a second technique.
        write_u32(&mut payload, SECOND_COUNT, count)?;
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

    fn bounds(&self, lights: &[Light]) -> AuthoringResult<Vec<u8>> {
        let (count, _, rows, box_class) = array_at(&self.occlusion_payload, BOXES)?;
        if count == 0 || rows + BOX_SIZE > self.occlusion_payload.len() {
            return Err(invalid("The light occlusion bounds hold no box"));
        }
        let template = self.occlusion_payload[rows..rows + BOX_SIZE].to_vec();
        let mut payload = self.occlusion_payload[..BOXES + 16].to_vec();
        let mut boxes = Vec::with_capacity(lights.len() * BOX_SIZE);
        for light in lights {
            let [x, y, z] = light.position;
            let r = light.range;
            let mut row = template.clone();
            put_floats(
                &mut row,
                0,
                &[x - r, y - r, z - r, 1., x + r, y + r, z + r, 1.],
            )?;
            write_u32(&mut row, BOX_IDENTITY, light.identity)?;
            boxes.extend_from_slice(&row);
        }
        append_native_array(&mut payload, BOXES, box_class, lights.len(), &boxes)?;
        synchronize_payload_size(payload)
    }

    fn shading_copy(&self, range: f32, color: [f32; 3]) -> AuthoringResult<Vec<u8>> {
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
        put_floats(
            &mut payload,
            row(SHADING_RANGE),
            &[-1. / range, 1., 1. / range, 1. / range],
        )?;
        Ok(payload)
    }

    fn second_copy(&self, range: f32, color: [f32; 3]) -> AuthoringResult<Vec<u8>> {
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
        put_floats(
            &mut payload,
            row(SECOND_RANGE),
            &[-1. / range, 1., 1. / range, 1. / range],
        )?;
        Ok(payload)
    }

    fn light_program(&self, range: f32, color: [f32; 3]) -> AuthoringResult<Vec<u8>> {
        let mut payload = self.program.1.clone();
        let (_, rows) = values(&payload)?;
        put_floats(&mut payload, rows + VALUE_SIZE + 4, &[range])?;
        put_floats(&mut payload, rows + 2 * VALUE_SIZE, &color)?;
        Ok(payload)
    }

    fn light_supplemental(&self, color: [f32; 3]) -> AuthoringResult<Vec<u8>> {
        let mut payload = self.supplemental.1.clone();
        let (_, rows) = values(&payload)?;
        put_floats(&mut payload, rows, &color)?;
        Ok(payload)
    }
}
