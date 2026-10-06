//! The native hip-fire crosshair table.
//!
//! Rows are keyed by inventory bucket and type key. Each names a crosshair pair, the crosshair
//! entity and an entity every crosshair shares, and carries style sub-rows. Two indexes point at
//! the rows, one sorted by row hash and one by bucket and key. Equal keys keep the stock order,
//! which is not a stable sort, so a rebuild inserts new rows into the stock order rather than
//! sorting again.
//!
//! A weapon chooses its row through its runtime content: every property block holds a type key
//! at +0x30, an FNV-1 name of the weapon type such as `sidearm` or `glaive`. The block's style key
//! at +0x18 names the variant's archetype. The client looks it up among the row's style sub-rows,
//! and stock rows hold none of the styles their weapons name, so it falls back there. Other
//! systems match the style too, so a weapon keeps its base's styles.
//!
//! The type key also names a first-person animation parameter. The client activates that name,
//! with its ancestors, in the weapon's first-person parameter dictionary, and the base's state
//! selectors test the base's type. A weapon whose dictionary does not activate the base's type
//! through the new key keeps the base's key, or those selectors stop matching.
// Only imported weapons add crosshair rows. Without the importer the build only keeps the
// table's own package apart, through `TABLE`.
#![cfg_attr(not(feature = "d2-model-importer"), allow(dead_code))]
use crate::{
    AuthoringResult,
    error::{invalid, validation},
    item::WeaponRuntimeResourcePatch,
    tag_payload::{read_u32, read_u64},
};
use sundial::package_authoring::PackageManager;
use tiger_pkg::TagHash;

pub(crate) const TABLE: TagHash = TagHash(0x80B4_79A9);
const MARK: u32 = 0x8080_9FBD;
const ROW_CLASS: u32 = 0x8080_47A3;
const SUB_ROW_CLASS: u32 = 0x8080_47A8;
const INDEX_CLASS: u32 = 0x8080_47A1;
const ROW_SIZE: usize = 0x28;
pub(crate) const SUB_ROW_SIZE: usize = 0x14;
/// Root descriptors: the rows, the index by row hash and the index by bucket and key.
const DESCRIPTORS: [usize; 3] = [0x08, 0x18, 0x28];
const ROOT_SIZE: usize = 0x3C;
/// The weapon content property field that holds the type key selecting the row (accessor at
/// exe+0xC9D3A0). The style key at +0x18 (accessor at exe+0xC9F420) is left as the base has it,
/// since exe+0xCB1AC1 matches it against other records as well as the crosshair's style rows.
const TYPE_FIELD: usize = 0x30;

/// Point a weapon's runtime content at a crosshair row: every property block, the default and
/// each variant, takes the row's type key.
pub(crate) fn content_patches(
    manager: &PackageManager,
    entity: &[u8],
    type_key: u32,
) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
    crate::hud_icon::runtime::field_patches(manager, entity, &[(TYPE_FIELD, type_key)])
}

/// The type keys the weapon content's property blocks hold now.
pub(crate) fn type_keys(manager: &PackageManager, entity: &[u8]) -> AuthoringResult<Vec<u32>> {
    crate::hud_icon::runtime::field_values(manager, entity, TYPE_FIELD)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    pub hash: u32,
    pub bucket: u32,
    pub key: u32,
    pub pair: u32,
    pub subs: Vec<[u8; SUB_ROW_SIZE]>,
}

pub(crate) struct Table {
    rows: Vec<Row>,
    by_hash: Vec<usize>,
    by_key: Vec<usize>,
}

fn block(data: &[u8], descriptor: usize) -> AuthoringResult<(usize, u32, usize)> {
    let count = usize::try_from(read_u64(data, descriptor)?)
        .map_err(|_| invalid("Crosshair table count overflow"))?;
    let header = crate::tag_payload::relative_target(data, descriptor + 8)?;
    if header < 4
        || read_u32(data, header - 4)? != MARK
        || usize::try_from(read_u64(data, header)?).ok() != Some(count)
    {
        return Err(invalid("Unsupported crosshair table array"));
    }
    Ok((count, read_u32(data, header + 8)?, header + 16))
}

impl Table {
    /// Parse the table and require that a rebuild reproduces it, so the layout is the audited one.
    pub(crate) fn parse(data: &[u8]) -> AuthoringResult<Self> {
        let (count, class, start) = block(data, DESCRIPTORS[0])?;
        if class != ROW_CLASS || count == 0 || count > 4096 {
            return Err(invalid("Unsupported crosshair table rows"));
        }
        let mut rows = Vec::with_capacity(count);
        for index in 0..count {
            let at = start + index * ROW_SIZE;
            if data.get(at + 0x20..at + ROW_SIZE) != Some(&[0; 8][..]) {
                return Err(invalid("Unsupported crosshair table row tail"));
            }
            let subs = if read_u64(data, at + 0x10)? == 0 {
                if data.get(at + 0x10..at + 0x20) != Some(&[0; 16][..]) {
                    return Err(invalid("Unsupported empty crosshair style array"));
                }
                Vec::new()
            } else {
                let (count, class, sub) = block(data, at + 0x10)?;
                if class != SUB_ROW_CLASS {
                    return Err(invalid("Unsupported crosshair style rows"));
                }
                (0..count)
                    .map(|i| {
                        data.get(sub + i * SUB_ROW_SIZE..sub + (i + 1) * SUB_ROW_SIZE)
                            .and_then(|row| row.try_into().ok())
                            .ok_or_else(|| invalid("Crosshair style row is truncated"))
                    })
                    .collect::<AuthoringResult<Vec<_>>>()?
            };
            rows.push(Row {
                hash: read_u32(data, at)?,
                bucket: read_u32(data, at + 4)?,
                key: read_u32(data, at + 8)?,
                pair: read_u32(data, at + 0xC)?,
                subs,
            });
        }
        let mut orders = Vec::new();
        for descriptor in &DESCRIPTORS[1..] {
            let (n, class, entries) = block(data, *descriptor)?;
            if class != INDEX_CLASS || n != count {
                return Err(invalid("Unsupported crosshair table index"));
            }
            let mut order = Vec::with_capacity(n);
            for i in 0..n {
                let row = crate::tag_payload::relative_target(data, entries + i * 8)?;
                let index = row
                    .checked_sub(start)
                    .filter(|offset| offset % ROW_SIZE == 0)
                    .map(|offset| offset / ROW_SIZE)
                    .filter(|index| *index < count)
                    .ok_or_else(|| invalid("Crosshair table index names no row"))?;
                order.push(index);
            }
            orders.push(order);
        }
        let by_key = orders
            .pop()
            .ok_or_else(|| invalid("Crosshair key index missing"))?;
        let by_hash = orders
            .pop()
            .ok_or_else(|| invalid("Crosshair hash index missing"))?;
        let table = Self {
            rows,
            by_hash,
            by_key,
        };
        if table.serialize()? != data {
            return Err(validation(
                "The crosshair table rebuild differs from its stock layout",
            ));
        }
        Ok(table)
    }

    pub(crate) fn has(&self, bucket: u32, key: u32) -> bool {
        self.rows
            .iter()
            .any(|row| row.bucket == bucket && row.key == key)
    }

    /// Add a row unless the bucket and key already have one. A row hash may not repeat.
    pub(crate) fn insert(&mut self, row: Row) -> AuthoringResult<bool> {
        if self.has(row.bucket, row.key) {
            return Ok(false);
        }
        if self.rows.iter().any(|existing| existing.hash == row.hash) {
            return Err(invalid(format!(
                "Crosshair row hash {:08X} already names another row",
                row.hash
            )));
        }
        self.rows.push(row);
        let index = self.rows.len() - 1;
        let rows = &self.rows;
        let place = |order: &mut Vec<usize>, key: &dyn Fn(&Row) -> (u32, u32)| {
            let at = order.partition_point(|&i| key(&rows[i]) <= key(&rows[index]));
            order.insert(at, index);
        };
        place(&mut self.by_hash, &|row| (row.hash, 0));
        place(&mut self.by_key, &|row| (row.bucket, row.key));
        Ok(true)
    }

    pub(crate) fn serialize(&self) -> AuthoringResult<Vec<u8>> {
        let mut data = vec![0; ROOT_SIZE];
        let marked = |data: &mut Vec<u8>, count: usize, class: u32| {
            let header = (data.len() + 4).div_ceil(16) * 16;
            data.resize(header - 4, 0);
            data.extend_from_slice(&MARK.to_le_bytes());
            data.extend_from_slice(&(count as u64).to_le_bytes());
            data.extend_from_slice(&class.to_le_bytes());
            data.extend_from_slice(&0u32.to_le_bytes());
            header
        };
        let rows_header = marked(&mut data, self.rows.len(), ROW_CLASS);
        let rows_start = data.len();
        data.resize(rows_start + ROW_SIZE * self.rows.len(), 0);
        let mut subs = Vec::with_capacity(self.rows.len());
        for row in &self.rows {
            if row.subs.is_empty() {
                subs.push(None);
                continue;
            }
            let header = marked(&mut data, row.subs.len(), SUB_ROW_CLASS);
            for sub in &row.subs {
                data.extend_from_slice(sub);
            }
            subs.push(Some(header));
        }
        let mut indexes = Vec::new();
        for order in [&self.by_hash, &self.by_key] {
            let header = marked(&mut data, self.rows.len(), INDEX_CLASS);
            for &index in order {
                let at = data.len() as i64;
                data.extend_from_slice(
                    &((rows_start + index * ROW_SIZE) as i64 - at).to_le_bytes(),
                );
            }
            indexes.push(header);
        }
        let put = |data: &mut Vec<u8>, at: usize, bytes: &[u8]| {
            data[at..at + bytes.len()].copy_from_slice(bytes);
        };
        for (index, row) in self.rows.iter().enumerate() {
            let at = rows_start + index * ROW_SIZE;
            for (offset, value) in [
                (0, row.hash),
                (4, row.bucket),
                (8, row.key),
                (0xC, row.pair),
            ] {
                put(&mut data, at + offset, &value.to_le_bytes());
            }
            if let Some(header) = subs[index] {
                put(&mut data, at + 0x10, &(row.subs.len() as u64).to_le_bytes());
                put(
                    &mut data,
                    at + 0x18,
                    &(header as i64 - (at + 0x18) as i64).to_le_bytes(),
                );
            }
        }
        let size = data.len() as u64;
        put(&mut data, 0, &size.to_le_bytes());
        for (descriptor, header) in
            DESCRIPTORS
                .into_iter()
                .zip([rows_header, indexes[0], indexes[1]])
        {
            put(
                &mut data,
                descriptor,
                &(self.rows.len() as u64).to_le_bytes(),
            );
            put(
                &mut data,
                descriptor + 8,
                &(header as i64 - (descriptor + 8) as i64).to_le_bytes(),
            );
        }
        Ok(data)
    }
}
