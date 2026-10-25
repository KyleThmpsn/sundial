//! The crosshair type key an imported weapon's runtime content takes from its converted crosshair.
use super::imports::Inputs;
use super::*;
use crate::tag_payload::{array_at, read_u32};

/// The converted crosshair row's type key, with the weapon's converted first-person parameter
/// dictionary when the import has one.
pub(in crate::item) struct Crosshair {
    pub type_key: u32,
    parameters: Option<Vec<u8>>,
}

/// The type key of the converted crosshair row for the item's own bucket. Only a converted
/// crosshair has rows the build adds. A source key Shadowkeep already has keeps the base
/// weapon's runtime keys.
pub(in crate::item) fn load(graph: &Inputs) -> AuthoringResult<Option<Crosshair>> {
    let value = graph.value();
    let record = &value["crosshair"];
    if record["status"] != "converted" {
        return Ok(None);
    }
    let hex = |field: &serde_json::Value| {
        field
            .as_str()
            .and_then(|text| u32::from_str_radix(text, 16).ok())
            .ok_or_else(|| invalid("Imported crosshair key is not hexadecimal"))
    };
    let bucket = hex(&record["bucket"])?;
    let keys = [hex(&record["first"])?, hex(&record["second"])?];
    let rows = record["rows"]
        .as_array()
        .ok_or_else(|| invalid("Imported crosshair rows are missing"))?;
    let mut selected = None;
    for row in rows {
        let key = hex(&row["key"])?;
        if hex(&row["bucket"])? != bucket || !keys.contains(&key) {
            continue;
        }
        if selected.replace(key).is_some() {
            return Err(invalid(
                "Imported crosshair has more than one row for the item's bucket",
            ));
        }
    }
    let Some(type_key) = selected else {
        return Ok(None);
    };
    let parameters = value["animation"]["first_person"]["files"]["parameters"]
        .as_str()
        .map(|path| {
            graph
                .read(path)
                .map_err(|error| invalid(format!("Imported first-person parameters: {error}")))
        })
        .transpose()?;
    Ok(Some(Crosshair {
        type_key,
        parameters,
    }))
}

impl Crosshair {
    /// Whether the content can take the row's type key in place of `base`, its current keys. The
    /// client activates the type key's name and its ancestors in the first-person parameter
    /// dictionary, so every base key a dictionary group names must be among the bits the new
    /// key's lookup row sets. A weapon without a converted dictionary keeps the base's states.
    pub(in crate::item) fn replaces(&self, base: &[u32]) -> AuthoringResult<bool> {
        let Some(parameters) = &self.parameters else {
            return Ok(true);
        };
        for &key in base {
            if key != self.type_key && !activates(parameters, self.type_key, key)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// Whether activating `name` in a first-person parameter dictionary also sets `other`'s bit, in
/// every group that names `other`. A group is a 32-byte row with its names at `+0` (8 bytes:
/// name, parent ordinal) and its lookup at `+0x10` (24 bytes: a 16-byte mask of the name and its
/// ancestors, the name at `+0x10` and its source index at `+0x14`). A name's bit is its ordinal
/// in the group's names.
fn activates(parameters: &[u8], name: u32, other: u32) -> AuthoringResult<bool> {
    let (groups, _, group_rows, _) = array_at(parameters, 8)?;
    for group in 0..groups {
        let row = group_rows + group * 32;
        let (names, _, name_rows, _) = array_at(parameters, row)?;
        let Some(bit) = (0..names)
            .map(|index| read_u32(parameters, name_rows + index * 8))
            .collect::<AuthoringResult<Vec<_>>>()?
            .iter()
            .position(|named| *named == other)
        else {
            continue;
        };
        let (lookups, _, lookup_rows, _) = array_at(parameters, row + 16)?;
        let mut set = false;
        for lookup in 0..lookups {
            let at = lookup_rows + lookup * 24;
            if read_u32(parameters, at + 16)? == name {
                set = parameters
                    .get(at + bit / 8)
                    .is_some_and(|byte| byte & (1 << (bit % 8)) != 0);
            }
        }
        if !set {
            return Ok(false);
        }
    }
    Ok(true)
}
