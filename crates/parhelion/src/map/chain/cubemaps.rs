//! The map's cubemap volume table: a copy of the donor's table of cubemap volumes (`80806B7F`) that
//! places the map's own volumes instead. Each donor row places an inline resource of `0x1CC` bytes
//! after the class marker, holding the volume's box, capture point, intensity, a 64-bit identifier,
//! the same box as three scaled axes and a middle, its transform, a relative pointer to its name and
//! its cube texture. The copy keeps the donor's rows and resources in place, appends one resource
//! per volume built from the first, and names them from a new row array. Every copy's name pointer
//! still names the donor's first volume's name.
use crate::{
    AuthoringResult,
    error::invalid,
    tag_payload::{
        append_native_array, array_at, read_i64, read_u32, synchronize_payload_size,
        write_relative_pointer,
    },
};
use tiger_pkg::TagHash;

use super::{
    CUBEMAP_VOLUME, ROW_POINTERS, ROW_RESOURCE, ROW_ROTATION, ROW_SCALE, ROW_SIZE, ROW_TRANSLATION,
    TABLE_ROW_CLASS, TABLE_ROWS, set_row_identity,
};

/// A volume resource's size after its class marker.
const VOLUME_SIZE: usize = 0x1CC;
const EXTENTS: usize = 0x20;
const CAPTURE: usize = 0x30;
/// Shadowkeep copies this scalar to pixel CB11[2].w, scaled by texture header +0x1B / 255.
/// Imported cube texels already contain their illumination and camera radiance scale.
const INTENSITY: usize = 0x40;
/// Twice the inverse of the box's X and Y half sizes, at two places. Distant Shore's first volume
/// holds 0.2053 and 0.1646 for half sizes of 9.743 and 12.15.
const INVERSE_EXTENTS: [usize; 2] = [0x50, 0x60];
const IDENTITY: usize = 0xB0;
/// The box's three axes scaled by its half sizes, then its middle with w 1.
const AXES: usize = 0xC0;
const TRANSFORM: usize = 0x100;
/// CB11[17].w selects linear sampling for the imported floating-point cube texels.
const LINEAR_COLOUR: usize = 0x16C;
const NAME: usize = 0x190;
const TEXTURE: usize = 0x198;
const IDENTITY_LOW: usize = 0x1B4;

/// One of the map's cubemap volumes: its cube texture, its box's middle and half sizes, where its
/// cube was captured and its identifier.
pub(in crate::map) struct Cubemap {
    pub texture: TagHash,
    pub center: [f32; 3],
    pub extents: [f32; 3],
    pub capture: [f32; 3],
    pub identity: u64,
}

fn put_floats(payload: &mut [u8], at: usize, values: &[f32]) {
    for (k, value) in values.iter().enumerate() {
        payload[at + k * 4..at + k * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
}

/// A copy of donor cubemap volume table `donor` placing `volumes`.
pub(super) fn table(donor: &[u8], volumes: &[Cubemap]) -> AuthoringResult<Vec<u8>> {
    let (count, _, rows, class) = array_at(donor, TABLE_ROWS)?;
    if count == 0 || class != TABLE_ROW_CLASS {
        return Err(invalid("The donor cubemap volume table has no rows"));
    }
    let first_row = donor[rows..rows + ROW_SIZE].to_vec();
    let resource = (rows + ROW_RESOURCE)
        .checked_add_signed(read_i64(donor, rows + ROW_RESOURCE)? as isize)
        .filter(|&at| at >= 4 && at + VOLUME_SIZE <= donor.len())
        .ok_or_else(|| invalid("The donor cubemap volume's resource is outside its table"))?;
    if read_u32(donor, resource - 4)? != CUBEMAP_VOLUME {
        return Err(invalid(
            "The donor cubemap volume's resource has another class",
        ));
    }
    let template = donor[resource..resource + VOLUME_SIZE].to_vec();
    let name = (resource + NAME)
        .checked_add_signed(read_i64(donor, resource + NAME)? as isize)
        .filter(|&at| at < donor.len())
        .ok_or_else(|| invalid("The donor cubemap volume's name is outside its table"))?;
    let mut payload = donor.to_vec();
    let mut resources = Vec::with_capacity(volumes.len());
    for volume in volumes {
        while !(payload.len() + 4).is_multiple_of(16) {
            payload.push(0);
        }
        payload.extend_from_slice(&CUBEMAP_VOLUME.to_le_bytes());
        let at = payload.len();
        payload.extend_from_slice(&template);
        let [ex, ey, ez] = volume.extents;
        let [cx, cy, cz] = volume.center;
        put_floats(&mut payload, at + EXTENTS, &[ex, ey, ez, 0.]);
        put_floats(
            &mut payload,
            at + CAPTURE,
            &[volume.capture[0], volume.capture[1], volume.capture[2], 1.],
        );
        put_floats(&mut payload, at + INTENSITY, &[1.]);
        put_floats(&mut payload, at + LINEAR_COLOUR, &[1.]);
        for field in INVERSE_EXTENTS {
            put_floats(&mut payload, at + field, &[2. / ex, 2. / ey, 0., 0.]);
        }
        payload[at + IDENTITY..at + IDENTITY + 8].copy_from_slice(&volume.identity.to_le_bytes());
        put_floats(
            &mut payload,
            at + AXES,
            &[
                ex, 0., 0., 0., 0., ey, 0., 0., 0., 0., ez, 0., cx, cy, cz, 1.,
            ],
        );
        put_floats(
            &mut payload,
            at + TRANSFORM,
            &[
                1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., cx, cy, cz, 1.,
            ],
        );
        write_relative_pointer(&mut payload, at + NAME, name)?;
        payload[at + TEXTURE..at + TEXTURE + 4].copy_from_slice(&volume.texture.0.to_le_bytes());
        payload[at + IDENTITY_LOW..at + IDENTITY_LOW + 4]
            .copy_from_slice(&(volume.identity as u32).to_le_bytes());
        resources.push(at);
    }
    let mut new_rows = Vec::with_capacity(volumes.len() * ROW_SIZE);
    for volume in volumes {
        let mut row = first_row.clone();
        put_floats(&mut row, ROW_ROTATION, &[0., 0., 0., 1.]);
        put_floats(&mut row, ROW_TRANSLATION, &volume.center);
        put_floats(&mut row, ROW_SCALE, &[1.]);
        // Stock placements and their inline resources carry the same distinct identity.
        set_row_identity(&mut row, 0, volume.identity)?;
        for pointer in ROW_POINTERS {
            row[pointer..pointer + 8].copy_from_slice(&0i64.to_le_bytes());
        }
        new_rows.extend_from_slice(&row);
    }
    append_native_array(
        &mut payload,
        TABLE_ROWS,
        TABLE_ROW_CLASS,
        volumes.len(),
        &new_rows,
    )?;
    if !volumes.is_empty() {
        let rows = array_at(&payload, TABLE_ROWS)?.2;
        for (index, &at) in resources.iter().enumerate() {
            write_relative_pointer(&mut payload, rows + index * ROW_SIZE + ROW_RESOURCE, at)?;
        }
    }
    synchronize_payload_size(payload)
}
