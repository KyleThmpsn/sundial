//! Checked Havok cloth graph translation into Shadowkeep's native packfile.
//!
//! Source layout support is structural and scoped to SDKV 20180100 with the
//! observed zero TCRF. Full graph and pointer coverage is required. A translated
//! solver is a separate contract from its game component and render bindings.
mod lower;
mod pack;
mod schema;
mod source;
mod validate;

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// A serialized native solver and explicit evidence about its translation.
pub struct Translated {
    pub bytes: Vec<u8>,
    pub report: Value,
}

/// Translate a complete source solver, optionally adapting its bone palette.
///
/// The map addresses source transform indexes and supplies native indexes.
/// Unsupported source extensions fail before any native bytes are returned.
/// This does not attach the solver to a game model or establish gameplay.
pub fn translate(bytes: &[u8], bone_map: Option<&[u16]>) -> Result<Translated> {
    let schema = schema::Schema::read()?;
    let source = source::read(bytes, &schema).context("Read source cloth solver")?;
    let source_items = source.len();
    let (mut objects, limits) = lower::translate(source, &schema)?;
    if let Some(mapping) = bone_map {
        lower::remap(&mut objects, mapping)?;
    }
    let bindings = validate::graph(&objects).context("Validate native cloth topology")?;
    let bytes = pack::write(&objects, &schema).context("Write native cloth solver")?;
    Ok(Translated {
        report: json!({"source_sdk":"20180100","target_sdk":"hk_2012.2.0-r1",
            "source_items":source_items,"native_objects":objects.len(),"limits":limits,
            "bone_mapping_applied":bone_map.is_some(),"bindings":bindings,"gameplay_verified":false}),
        bytes,
    })
}

/// Include solver anchors and colliders when choosing a compatible native rig.
pub(crate) fn used_bones(bytes: &[u8]) -> Result<BTreeSet<usize>> {
    let schema = schema::Schema::read()?;
    let source = source::read(bytes, &schema)?;
    let (objects, _) = lower::translate(source, &schema)?;
    validate::graph(&objects)?;
    let mut bones = BTreeSet::new();
    for object in objects.values() {
        let values = match object.name.as_str() {
            "hclObjectSpaceSkinPNTOperator" => Some(&object.value["transformSubset"]),
            "hclSimClothData" => Some(&object.value["collidableTransformMap"]["transformIndices"]),
            _ => None,
        };
        if let Some(values) = values {
            bones.extend(
                array(values)?
                    .iter()
                    .map(integer)
                    .collect::<Result<Vec<_>>>()?,
            );
        }
    }
    Ok(bones)
}

#[derive(Clone)]
struct Object {
    name: String,
    value: Value,
}

fn integer(v: &Value) -> Result<usize> {
    usize::try_from(v.as_u64().context("Expected a nonnegative cloth index")?)
        .context("Cloth index exceeds address space")
}

fn array(v: &Value) -> Result<&[Value]> {
    if v.is_null() {
        Ok(&[])
    } else {
        v.as_array()
            .map(Vec::as_slice)
            .context("Expected a cloth array")
    }
}

fn zeros(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Number(n) => n.as_f64() == Some(0.),
        Value::Array(rows) => rows.iter().all(zeros),
        Value::Object(fields) => fields.values().all(zeros),
        _ => false,
    }
}
