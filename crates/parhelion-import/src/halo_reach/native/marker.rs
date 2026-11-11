//! Source attachment markers in the native object-space marker set.
//!
//! The native marker resolver (`A1E8A0`) reads a row's bone index at `+0x04`, samples that bone's
//! frame and composes the row's transform with it, so a row is local to its bone. Each source marker
//! therefore binds the native bone its source node drives, with its bind pose expressed in that
//! bone's frame. A row carrying an object-space point with another bone's index draws displaced by
//! that bone's bind transform, which is how every firing effect anchored at `primary_trigger` once
//! floated ahead of and beside the barrel.
use super::seat;
use crate::halo_reach::model;
use crate::{
    presentation::{Graph, put},
    tiger::{entity, markers, payload::Payload, reader::Reader},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

fn hash(name: &str) -> u32 {
    name.bytes()
        .fold(0x811c9dc5, |h, b| h.wrapping_mul(0x01000193) ^ u32::from(b))
}

/// The source muzzle the primary barrel fires from: its optional marker name, or the default
/// `primary_trigger`. That a named barrel marker also replaces the firing effect's
/// `primary_trigger` location is inferred from the field's name.
pub(super) fn muzzle(gameplay: &Value) -> String {
    let barrel = gameplay["triggers"]
        .as_array()
        .and_then(|triggers| triggers.first())
        .and_then(|trigger| trigger["primary_barrel"].as_u64())
        .and_then(|index| gameplay["barrels"].get(usize::try_from(index).ok()?));
    barrel
        .and_then(|barrel| barrel["marker"].as_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("primary_trigger")
        .to_owned()
}

/// The native names a source marker also answers to. The muzzle answers to the donor's firing
/// marker names, so its presentation and trace origins follow the source barrel.
fn aliases(name: &str, muzzle: &str) -> Vec<&'static str> {
    if name == muzzle {
        let mut names = vec!["fire", "primary_fire"];
        if muzzle != "primary_trigger" {
            names.push("primary_trigger");
        }
        return names;
    }
    match name {
        "right_hand" => vec!["grip"],
        "left_hand" => vec!["support"],
        "primary_ejection" => vec!["eject"],
        _ => Vec::new(),
    }
}

/// The marker the ejection locators of the stock firing flows name, both point kinds of every
/// inspected flow with casing events. Its preimage is not recovered.
const NATIVE_EJECTION: u32 = 0x9216_eea9;

/// Native marker names without a recovered preimage that a source marker also answers to.
fn hashed_aliases(name: &str) -> &'static [u32] {
    match name {
        "primary_ejection" => &[NATIVE_EJECTION],
        _ => &[],
    }
}

/// Every native name the converted set gives source marker `name`: its own unless the muzzle owns
/// `primary_trigger`, then its aliases.
pub(super) fn names(name: &str, muzzle: &str) -> Vec<u32> {
    let own = (muzzle == "primary_trigger" || name != "primary_trigger").then_some(name);
    own.into_iter()
        .map(hash)
        .chain(aliases(name, muzzle).into_iter().map(hash))
        .chain(hashed_aliases(name).iter().copied())
        .collect()
}

/// The bind transform `pose` takes in native bone `bone`'s frame. A static carrier skeleton has
/// no bind transforms, and its single root is the object frame.
fn local(bones: &Value, bone: usize, pose: ([f32; 3], [f32; 4])) -> Result<([f32; 3], [f32; 4])> {
    let count = bones["bones"]
        .as_array()
        .context("Native marker skeleton")?
        .len();
    ensure!(
        bone < count,
        "Source marker bone is outside the native skeleton"
    );
    if bones["bones"][bone]["inverse_object_transform"].is_null() {
        ensure!(
            bone == 0 && bones["static"] == true,
            "Native marker bone has no bind transform"
        );
        return Ok(pose);
    }
    seat::frame(bones, bone, pose)
}

/// The weapon runtime's rear aim point. A native runtime marker set can carry its own row of this
/// name, the donor's eye behind its sights, and aimed fire then follows the donor's eye together
/// with the converted optic. Scoped Reach imports on scout and auto rifle runtimes fired 2.8 and
/// 3.0 degrees above the reticle, while one on a sidearm runtime, whose set has no such row, did not.
const REAR_AIM: u32 = 0x8d0d_d3cd;

/// Rows the build writes over the weapon runtime's own marker rows of the same name: with a
/// converted optic, the rear aim point at the optic's eye, so both marker layers hold one level
/// sight line whichever one aimed fire reads. Other converted markers stay in the part set alone.
/// Writing them into the runtime set as well would move rows that already resolve correctly, such
/// as a sidearm runtime's muzzle and the third-person hand targets.
pub(super) fn runtime_rows(optic: Option<&Value>) -> Result<Value> {
    let mut rows = Vec::new();
    if let Some(rear) = optic.map(|optic| &optic["rear"]) {
        let point = rear
            .as_array()
            .filter(|values| values.len() == 3)
            .context("Converted optic has no rear aim point")?
            .iter()
            .map(|value| value.as_f64().map(|v| v as f32))
            .collect::<Option<Vec<_>>>()
            .context("Converted optic rear aim point is not numeric")?;
        rows.push(
            json!({"name":format!("{REAR_AIM:08X}"),"orientation":[0., 0., 0., 1.],
            "position":[point[0], point[1], point[2], 1.]}),
        );
    }
    Ok(json!({"rows":rows}))
}

pub(super) fn build(
    native: &mut Reader,
    g: &mut Graph,
    model: &model::Model,
    (bones, palette): (&Value, &[u8]),
    muzzle: &str,
    entity_bytes: &mut [u8],
    entity_patches: &mut Vec<Value>,
) -> Result<Vec<Value>> {
    let original_entity = Payload(entity_bytes.to_vec());
    let mut sets = Vec::new();
    for row in original_entity.array(16, 12, None)? {
        let tag = original_entity.u32(row)?;
        // The render owner has already become a graph relocation.
        if tag == u32::MAX {
            continue;
        }
        let owner = native.tag(tag, Some(0x80809c36))?;
        let header = owner.pointer(16)?;
        if header >= 4 && owner.u32(header - 4)? == markers::NATIVE_COMPONENT {
            sets.push((tag, owner));
        }
    }
    ensure!(
        sets.len() <= 1,
        "Native render entity has ambiguous marker sets"
    );
    let Some((tag, owner)) = sets.pop() else {
        return Ok(vec![json!({"status":"no_native_marker_set"})]);
    };
    let muzzle = if model.markers.iter().any(|marker| marker.name == muzzle) {
        muzzle
    } else {
        "primary_trigger"
    };
    let mut converted = Vec::new();
    let mut bound = Vec::new();
    let mut report = Vec::new();
    for marker in &model.markers {
        if !model.marker_selected(marker)? {
            continue;
        }
        let pose = seat::marker_pose(model, marker)?;
        // A marker without a source node keeps its object-space pose in the native root's frame.
        let bone = match marker.bone {
            Some(index) => usize::from(
                *palette
                    .get(index)
                    .context("Source marker node is outside the joint palette")?,
            ),
            None => 0,
        };
        let (position, orientation) = local(bones, bone, pose)?;
        let roles = aliases(&marker.name, muzzle);
        // The muzzle's alias owns the native firing name when a barrel names another marker.
        let own = muzzle == "primary_trigger" || marker.name != "primary_trigger";
        for name in names(&marker.name, muzzle) {
            let row = markers::Marker {
                name,
                binding: [0, 0],
                position: [position[0], position[1], position[2], 1.],
                orientation,
                row: 0,
            };
            converted.push(row);
            bound.push((row, u32::try_from(bone)?));
        }
        let hashed = hashed_aliases(&marker.name)
            .iter()
            .map(|name| format!("{name:08X}"))
            .collect::<Vec<_>>();
        report.push(
            json!({"source":marker.name,"native_role":roles.first(),"native_aliases":roles,
            "native_hashed_aliases":hashed,
            "own_name":own,"position":pose.0,"orientation":pose.1,"bone":bone,
            "local_position":position,"local_orientation":orientation,"binding":"bone_local"}),
        );
    }
    let (mut rewritten, _) = markers::rewrite(&owner, &converted)?;
    // The shared writer keeps root pairs. Each converted row then takes its bone index, found by
    // its name and exact transform, which the writer copies bit for bit.
    for row in markers::read_native(&rewritten)? {
        let mut matches = bound
            .iter()
            .filter(|(marker, _)| {
                marker.name == row.name
                    && marker.position.map(f32::to_bits) == row.position.map(f32::to_bits)
                    && marker.orientation.map(f32::to_bits) == row.orientation.map(f32::to_bits)
            })
            .map(|(_, bone)| *bone);
        let Some(bone) = matches.next() else {
            continue;
        };
        ensure!(
            matches.all(|other| other == bone),
            "Source markers of one name and transform bind different bones"
        );
        put(&mut rewritten.0, row.row + 4, &bone.to_le_bytes())?;
    }
    let mut patches = Vec::new();
    for at in (0..rewritten.0.len().saturating_sub(3)).step_by(4) {
        if rewritten.u32(at)? == tag {
            ensure!(
                rewritten.u32(at + 4)? & 0xffff0000 == 0x80800000
                    && rewritten.u64(at + 8)? < rewritten.0.len() as u64,
                "Untyped marker owner self-reference"
            );
            put(&mut rewritten.0, at, &u32::MAX.to_le_bytes())?;
            patches.push(json!({"offset":at,"symbol":"markers"}));
        }
    }
    g.add("markers", tag, &rewritten.0, None, patches)?;
    for at in entity::owner_slots(&original_entity, &owner, tag)? {
        put(entity_bytes, at, &u32::MAX.to_le_bytes())?;
        entity_patches.push(json!({"offset":at,"symbol":"markers"}));
    }
    entity::reject_stale_owner(&Payload(entity_bytes.to_vec()), tag)?;
    Ok(report)
}
