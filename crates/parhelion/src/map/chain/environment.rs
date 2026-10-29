//! The map's environment: Destiny's global lighting channels, which a stock map sets through
//! environment volumes. The client gathers the volumes around the camera every frame, and each
//! volume's type 3 rows name channel modifier records whose values the frame's channel bank lerps
//! toward. Channel 8 is the sun's intensity and channel 9 its color, whose product the native
//! global lighting holds as its sun. The chain drops the donor's volumes, whose channels describe
//! the donor's lighting, so without volumes of its own a converted map kept channel 8's default of
//! 10 and lit Nightfall's weapons with a daylight sun.
//!
//! A placement row of an environment volume (resource `80806FAD`) holds, inline, two zero words,
//! its volume set (`80806FAF`) and the volume's index in it. A volume set names its Havok packfile
//! of volume shapes at `+0x08` and holds its volumes at `+0x10` (`0xF0` byte `8080738B` rows). A
//! volume holds its box at `+0x40` and `+0x50`, its footprint's corners at `+0x60` (float4 points
//! at the bottom), their triangles at `+0x70` (byte triples), its height at `+0x80`, the identity
//! its placement row carries at `+0xA0`, its index at `+0xAC`, its rows at `+0xB8` (8 byte
//! `80807390` pairs of a type and a record), a fade distance at `+0xC8` and its priority at
//! `+0xD0`. A type 2 row names no record and scales the sun down by the volume's weight, as stock
//! interiors use it. A channel modifier record (`80808586`) holds its rows at `+0x10` (`0x18` byte
//! `80808589` rows: a channel, a time window's start and length and an expression index) and its
//! expressions at `+0x20` (`0x30` byte `80809A32` rows naming a bytecode array and a constants
//! array). The constant expression `34 00 3E 00` pushes its first constant.
//!
//! So no environment value comes from a donor. The table, set, records and packfile copy Distant
//! Shore's (`80EB047C`, `80EB047B`, `80EB0479`, `80B8C78B`) for their structure, whatever the
//! donor, and every value is the map's own.
use parhelion_import::halo_reach::level::volumes::{Kind, Volume};
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

/// The placement resource class of an environment volume.
const ENVIRONMENT: u32 = 0x8080_6FAD;
/// An environment resource's inline body: two zero words, the volume set and the volume's index.
const RESOURCE_SIZE: usize = 0x18;
const RESOURCE_SET: usize = 0x10;
const RESOURCE_INDEX: usize = 0x14;
const SET_CLASS: u32 = 0x8080_6FAF;
const SET_PACKFILE: usize = 0x08;
const SET_VOLUMES: usize = 0x10;
/// The set's fixed fields end where its volumes' array begins.
const SET_HEADER: usize = 0x20;
const VOLUME_SIZE: usize = 0xF0;
const VOLUME_LOW: usize = 0x40;
const VOLUME_HIGH: usize = 0x50;
const VOLUME_CORNERS: usize = 0x60;
const VOLUME_TRIANGLES: usize = 0x70;
const VOLUME_HEIGHT: usize = 0x80;
const VOLUME_IDENTITY: usize = 0xA0;
const VOLUME_HASH: usize = 0xA8;
const VOLUME_INDEX: usize = 0xAC;
const VOLUME_ROWS: usize = 0xB8;
const VOLUME_FADE: usize = 0xC8;
const VOLUME_PRIORITY: usize = 0xD0;
const CORNER_CLASS: u32 = 0x8080_0094;
const TRIANGLE_CLASS: u32 = 0x8080_9B92;
const VOLUME_ROW_CLASS: u32 = 0x8080_7390;
/// A volume row's type: the sun scaled down by the volume's weight, or a channel modifier record.
const SUN_SCALE: u32 = 2;
const MODIFIER: u32 = 3;
const MODIFIER_CLASS: u32 = 0x8080_8586;
const MODIFIER_ROWS: usize = 0x10;
const MODIFIER_EXPRESSIONS: usize = 0x20;
/// The record's fixed fields end where its rows' array begins.
const MODIFIER_HEADER: usize = 0x30;
const CHANNEL_ROW_SIZE: usize = 0x18;
const EXPRESSION_SIZE: usize = 0x30;
const EXPRESSION_BYTECODE: usize = 0x00;
const EXPRESSION_CONSTANTS: usize = 0x10;
const BYTECODE_CLASS: u32 = 0x8080_0009;
const CONSTANT_CLASS: u32 = 0x8080_0090;
/// Push the first constant, then end.
const CONSTANT_EXPRESSION: [u8; 4] = [0x34, 0x00, 0x3E, 0x00];
/// The sun's intensity and color channels.
const SUN_INTENSITY: u32 = 8;
const SUN_COLOR: u32 = 9;
/// How far the map's volume reaches past its pieces, sideways, below and above, in meters.
const SIDE_MARGIN: f32 = 512.;
const BELOW_MARGIN: f32 = 256.;
const ABOVE_MARGIN: f32 = 1024.;
/// An interior's fade distance and priority, above the map volume's, which takes neither.
const INTERIOR_FADE: f32 = 2.;
const INTERIOR_PRIORITY: u32 = 1;

/// What the map's environment sets: the native sun's intensity and color at unit luminance, and
/// the interior boxes where the sun is off.
#[derive(Clone, Debug)]
pub(in crate::map) struct Environment {
    pub sun_intensity: f32,
    pub sun_color: [f32; 3],
    pub interiors: Vec<([f32; 3], [f32; 3])>,
}

/// The table, volume set, channel record, packfile and entity the map's environment copies.
#[derive(Clone)]
pub(super) struct Template {
    table: u32,
    table_payload: Vec<u8>,
    row: Vec<u8>,
    set: TagHash,
    set_payload: Vec<u8>,
    volume: Vec<u8>,
    record: TagHash,
    record_payload: Vec<u8>,
    packfile: TagHash,
    packfile_payload: Vec<u8>,
    entity: TagHash,
}

fn put_floats(data: &mut [u8], offset: usize, values: &[f32]) -> AuthoringResult<()> {
    let bytes = values
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect::<Vec<_>>();
    write_bytes(data, offset, &bytes)
}

/// The target of the self-relative pointer at `field`, when it points inside `payload`.
fn target(payload: &[u8], field: usize) -> AuthoringResult<Option<usize>> {
    let relative = read_i64(payload, field)?;
    Ok(field
        .checked_add_signed(relative as isize)
        .filter(|&at| relative != 0 && at < payload.len()))
}

/// The environment template of placement table `table`: its first row that places a volume
/// whose rows name a channel modifier record, with the volume set, record, packfile and entity.
pub(super) fn template(manager: &PackageManager, table: u32) -> AuthoringResult<Option<Template>> {
    if class(manager, TagHash(table)) != Some(super::TABLE_CLASS) {
        return Ok(None);
    }
    let table_payload = read_tag(manager, TagHash(table), "environment placement table")?;
    if read_u64(&table_payload, TABLE_ROWS)? == 0 {
        return Ok(None);
    }
    let (count, _, rows, row_class) = array_at(&table_payload, TABLE_ROWS)?;
    if row_class != TABLE_ROW_CLASS || rows + count * ROW_SIZE > table_payload.len() {
        return Ok(None);
    }
    for row in (0..count).map(|index| rows + index * ROW_SIZE) {
        let Some(resource) = target(&table_payload, row + ROW_RESOURCE)? else {
            continue;
        };
        if resource < 4
            || resource + RESOURCE_SIZE > table_payload.len()
            || read_u32(&table_payload, resource - 4)? != ENVIRONMENT
        {
            continue;
        }
        let set = TagHash(read_u32(&table_payload, resource + RESOURCE_SET)?);
        if class(manager, set) != Some(SET_CLASS) {
            continue;
        }
        let set_payload = read_tag(manager, set, "environment volume set")?;
        let packfile = TagHash(read_u32(&set_payload, SET_PACKFILE)?);
        if manager.get_entry(packfile).is_none() || read_u64(&set_payload, SET_VOLUMES)? == 0 {
            continue;
        }
        let (volumes, _, first, _) = array_at(&set_payload, SET_VOLUMES)?;
        if first + volumes * VOLUME_SIZE > set_payload.len() {
            continue;
        }
        for volume in (0..volumes).map(|index| first + index * VOLUME_SIZE) {
            if read_u64(&set_payload, volume + VOLUME_ROWS)? == 0 {
                continue;
            }
            let (pairs, _, pair_rows, _) = array_at(&set_payload, volume + VOLUME_ROWS)?;
            for pair in (0..pairs).map(|index| pair_rows + index * 8) {
                let record = TagHash(read_u32(&set_payload, pair + 4)?);
                if read_u32(&set_payload, pair)? != MODIFIER
                    || class(manager, record) != Some(MODIFIER_CLASS)
                {
                    continue;
                }
                let record_payload = read_tag(manager, record, "channel modifier record")?;
                if read_u64(&record_payload, MODIFIER_EXPRESSIONS)? == 0 {
                    continue;
                }
                return Ok(Some(Template {
                    table,
                    table_payload: table_payload.clone(),
                    row: table_payload[row..row + ROW_SIZE].to_vec(),
                    set,
                    volume: set_payload[volume..volume + VOLUME_SIZE].to_vec(),
                    set_payload,
                    record,
                    record_payload,
                    packfile,
                    packfile_payload: read_tag(manager, packfile, "environment volume shapes")?,
                    entity: TagHash(read_u32(&table_payload, row)?),
                }));
            }
        }
    }
    Ok(None)
}

/// Entries the map's environment takes: the table, the volume set, its packfile and the map's
/// channel record, and an interior record when it has interiors.
pub(super) fn entries(environment: Option<&Environment>) -> usize {
    match environment {
        Some(environment) if environment.interiors.is_empty() => 4,
        Some(_) => 5,
        None => 0,
    }
}

/// One volume of the map's environment: its box, its rows and how it blends.
struct Planned {
    low: [f32; 3],
    high: [f32; 3],
    rows: Vec<(u32, u32)>,
    fade: f32,
    priority: u32,
}

impl Template {
    /// The stock entity every environment row places, which the map's container loads.
    pub(super) fn entity(&self) -> TagHash {
        self.entity
    }

    /// The map's environment tags, each a template, its payload and its tag from `at`, in
    /// [`entries`] order, for a map whose pieces span `bounds`.
    pub(super) fn tags(
        &self,
        environment: &Environment,
        bounds: ([f32; 3], [f32; 3]),
        at: &dyn Fn(usize) -> AuthoringResult<TagHash>,
    ) -> AuthoringResult<Vec<(TagHash, Vec<u8>, TagHash)>> {
        let (table, set, packfile, record) = (at(0)?, at(1)?, at(2)?, at(3)?);
        let interior = (!environment.interiors.is_empty())
            .then(|| at(4))
            .transpose()?;
        let (low, high) = bounds;
        if (0..3).any(|k| !low[k].is_finite() || !high[k].is_finite() || low[k] > high[k]) {
            return Err(invalid("The map's environment has no finite bounds"));
        }
        let mut planned = vec![Planned {
            low: [
                low[0] - SIDE_MARGIN,
                low[1] - SIDE_MARGIN,
                low[2] - BELOW_MARGIN,
            ],
            high: [
                high[0] + SIDE_MARGIN,
                high[1] + SIDE_MARGIN,
                high[2] + ABOVE_MARGIN,
            ],
            rows: vec![(MODIFIER, record.0)],
            fade: 0.,
            priority: 0,
        }];
        if let Some(interior) = interior {
            planned.extend(environment.interiors.iter().map(|&(low, high)| Planned {
                low,
                high,
                rows: vec![(SUN_SCALE, u32::MAX), (MODIFIER, interior.0)],
                fade: INTERIOR_FADE,
                priority: INTERIOR_PRIORITY,
            }));
        }
        let identity = |index: usize| {
            format!("parhelion/environment/{:08X}/{index}", table.0)
                .bytes()
                .fold(0xCBF2_9CE4_8422_2325u64, |hash, byte| {
                    (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01B3)
                })
                | 1
        };
        let volumes = planned
            .iter()
            .enumerate()
            .map(|(index, volume)| Volume {
                name: format!("environment_{index}"),
                kind: Kind::Kill,
                corners: vec![
                    [volume.high[0], volume.low[1]],
                    [volume.high[0], volume.high[1]],
                    [volume.low[0], volume.high[1]],
                    [volume.low[0], volume.low[1]],
                ],
                z: [volume.low[2], volume.high[2]],
            })
            .collect::<Vec<_>>();
        let mut tags = Vec::with_capacity(entries(Some(environment)));
        tags.push((
            TagHash(self.table),
            self.table_payload(set, &planned, &identity)?,
            table,
        ));
        tags.push((
            self.set,
            self.set_payload(packfile, &planned, &volumes, &identity)?,
            set,
        ));
        let shapes = parhelion_import::halo_reach::level::volumes::packfile(
            &self.packfile_payload,
            &volumes,
        )
        .map_err(|error| invalid(format!("Environment volume shapes: {error:#}")))?;
        tags.push((self.packfile, shapes, packfile));
        let color = environment.sun_color;
        tags.push((
            self.record,
            self.modifier(&[
                (SUN_INTENSITY, [environment.sun_intensity; 4]),
                (SUN_COLOR, [color[0], color[1], color[2], 1.]),
            ])?,
            record,
        ));
        if let Some(interior) = interior {
            tags.push((
                self.record,
                self.modifier(&[(SUN_INTENSITY, [0.; 4])])?,
                interior,
            ));
        }
        Ok(tags)
    }

    /// A copy of the template table placing each of `planned` from volume set `set`.
    fn table_payload(
        &self,
        set: TagHash,
        planned: &[Planned],
        identity: &dyn Fn(usize) -> u64,
    ) -> AuthoringResult<Vec<u8>> {
        let mut payload = self.table_payload.clone();
        let rows = (0..planned.len())
            .flat_map(|_| self.row.iter().copied())
            .collect::<Vec<_>>();
        append_native_array(
            &mut payload,
            TABLE_ROWS,
            TABLE_ROW_CLASS,
            planned.len(),
            &rows,
        )?;
        let first = array_at(&payload, TABLE_ROWS)?.2;
        for index in 0..planned.len() {
            let row = first + index * ROW_SIZE;
            write_bytes(&mut payload, row + 0x70, &identity(index).to_le_bytes())?;
            for field in ROW_POINTERS {
                write_bytes(&mut payload, row + field, &0i64.to_le_bytes())?;
            }
            // Each resource sits 8-aligned behind its class word, after the rows.
            let resource = (payload.len() + 4).div_ceil(8) * 8;
            payload.resize(resource - 4, 0);
            payload.extend_from_slice(&ENVIRONMENT.to_le_bytes());
            let mut body = vec![0u8; RESOURCE_SIZE];
            write_u32(&mut body, RESOURCE_SET, set.0)?;
            write_u32(&mut body, RESOURCE_INDEX, index as u32)?;
            payload.extend_from_slice(&body);
            write_relative_pointer(&mut payload, row + ROW_RESOURCE, resource)?;
        }
        synchronize_payload_size(payload)
    }

    /// The map's volume set: `planned`'s volumes, shaped as `volumes`, with packfile `packfile`.
    fn set_payload(
        &self,
        packfile: TagHash,
        planned: &[Planned],
        volumes: &[Volume],
        identity: &dyn Fn(usize) -> u64,
    ) -> AuthoringResult<Vec<u8>> {
        let (_, _, _, volume_class) = array_at(&self.set_payload, SET_VOLUMES)?;
        let mut payload = self.set_payload[..SET_HEADER].to_vec();
        write_u32(&mut payload, SET_PACKFILE, packfile.0)?;
        let mut rows = Vec::with_capacity(planned.len() * VOLUME_SIZE);
        for (index, volume) in planned.iter().enumerate() {
            let mut row = self.volume.clone();
            put_floats(
                &mut row,
                VOLUME_LOW,
                &[volume.low[0], volume.low[1], volume.low[2], 1.],
            )?;
            put_floats(
                &mut row,
                VOLUME_HIGH,
                &[volume.high[0], volume.high[1], volume.high[2], 1.],
            )?;
            put_floats(&mut row, VOLUME_HEIGHT, &[volume.high[2] - volume.low[2]])?;
            let identity = identity(index);
            write_bytes(&mut row, VOLUME_IDENTITY, &identity.to_le_bytes())?;
            write_u32(&mut row, VOLUME_HASH, (identity >> 32) as u32)?;
            write_u32(&mut row, VOLUME_INDEX, index as u32)?;
            put_floats(&mut row, VOLUME_FADE, &[volume.fade])?;
            write_u32(&mut row, VOLUME_PRIORITY, volume.priority)?;
            for field in [VOLUME_CORNERS, VOLUME_TRIANGLES, VOLUME_ROWS] {
                write_bytes(&mut row, field, &[0u8; 16])?;
            }
            rows.extend_from_slice(&row);
        }
        append_native_array(
            &mut payload,
            SET_VOLUMES,
            volume_class,
            planned.len(),
            &rows,
        )?;
        let first = array_at(&payload, SET_VOLUMES)?.2;
        for (index, (volume, shape)) in planned.iter().zip(volumes).enumerate() {
            let row = first + index * VOLUME_SIZE;
            let corners = shape
                .corners
                .iter()
                .flat_map(|&[x, y]| [x, y, volume.low[2], 1.])
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>();
            append_native_array(
                &mut payload,
                row + VOLUME_CORNERS,
                CORNER_CLASS,
                shape.corners.len(),
                &corners,
            )?;
            let triangles = shape
                .triangles()
                .map_err(|error| invalid(format!("Environment volume: {error:#}")))?;
            let bytes = triangles.iter().flatten().copied().collect::<Vec<_>>();
            append_native_array(
                &mut payload,
                row + VOLUME_TRIANGLES,
                TRIANGLE_CLASS,
                triangles.len(),
                &bytes,
            )?;
            let pairs = volume
                .rows
                .iter()
                .flat_map(|&(kind, record)| {
                    kind.to_le_bytes().into_iter().chain(record.to_le_bytes())
                })
                .collect::<Vec<_>>();
            append_native_array(
                &mut payload,
                row + VOLUME_ROWS,
                VOLUME_ROW_CLASS,
                volume.rows.len(),
                &pairs,
            )?;
        }
        synchronize_payload_size(payload)
    }

    /// A channel modifier record setting each of `channels` to its constant over the whole day.
    fn modifier(&self, channels: &[(u32, [f32; 4])]) -> AuthoringResult<Vec<u8>> {
        if channels.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
            return Err(invalid("Environment channels must ascend"));
        }
        let template = &self.record_payload;
        let (_, _, template_rows, row_class) = array_at(template, MODIFIER_ROWS)?;
        let (_, _, template_expressions, expression_class) =
            array_at(template, MODIFIER_EXPRESSIONS)?;
        if template_rows + CHANNEL_ROW_SIZE > template.len()
            || template_expressions + EXPRESSION_SIZE > template.len()
        {
            return Err(invalid("The channel modifier template has no rows"));
        }
        let mut payload = template[..MODIFIER_HEADER].to_vec();
        let mut rows = Vec::with_capacity(channels.len() * CHANNEL_ROW_SIZE);
        let mut expressions = Vec::with_capacity(channels.len() * EXPRESSION_SIZE);
        for (index, &(channel, _)) in channels.iter().enumerate() {
            let mut row = template[template_rows..template_rows + CHANNEL_ROW_SIZE].to_vec();
            write_u32(&mut row, 0, channel)?;
            put_floats(&mut row, 4, &[0., 1.])?;
            write_u32(&mut row, 0x0C, 0)?;
            write_bytes(&mut row, 0x10, &(index as u64).to_le_bytes())?;
            rows.extend_from_slice(&row);
            let mut expression =
                template[template_expressions..template_expressions + EXPRESSION_SIZE].to_vec();
            write_bytes(&mut expression, EXPRESSION_BYTECODE, &[0u8; 16])?;
            write_bytes(&mut expression, EXPRESSION_CONSTANTS, &[0u8; 16])?;
            expressions.extend_from_slice(&expression);
        }
        append_native_array(
            &mut payload,
            MODIFIER_ROWS,
            row_class,
            channels.len(),
            &rows,
        )?;
        append_native_array(
            &mut payload,
            MODIFIER_EXPRESSIONS,
            expression_class,
            channels.len(),
            &expressions,
        )?;
        let first = array_at(&payload, MODIFIER_EXPRESSIONS)?.2;
        for (index, (_, value)) in channels.iter().enumerate() {
            let expression = first + index * EXPRESSION_SIZE;
            append_native_array(
                &mut payload,
                expression + EXPRESSION_BYTECODE,
                BYTECODE_CLASS,
                CONSTANT_EXPRESSION.len(),
                &CONSTANT_EXPRESSION,
            )?;
            let constant = value
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>();
            append_native_array(
                &mut payload,
                expression + EXPRESSION_CONSTANTS,
                CONSTANT_CLASS,
                1,
                &constant,
            )?;
        }
        synchronize_payload_size(payload)
    }
}
