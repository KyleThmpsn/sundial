//! Read emitted hip framing and the weapon and hand poses retained while aiming.
use super::*;
use parhelion_import::tiger::animation::control::Rig;

fn first_frame(clip: &Payload, defaults: &[[f32; 8]]) -> Result<Vec<[f32; 8]>> {
    let mut poses = defaults.to_vec();
    for (field, maps, frames) in [(16, 0xA8, 1), (24, 0xD8, usize::from(clip.u16(0x13C)?))] {
        if clip.u64(field)? == 0 {
            continue;
        }
        let codec = clip.pointer(field)?;
        ensure!(
            clip.u16(codec)? == 0 && frames > 0,
            "Expected emitted float channels"
        );
        for (kind, (start, width, stride)) in
            [(7, 1, 4), (0, 4, 16), (4, 3, 16)].into_iter().enumerate()
        {
            let slots = clip.array(maps + kind * 16, 2, Some(0x8080000A))?;
            let values = clip.array(codec + 24 + kind * 16, stride, None)?;
            ensure!(
                values.len() == slots.len() * frames,
                "Incomplete emitted pose channels"
            );
            for (track, slot) in slots.into_iter().enumerate() {
                let pose = poses
                    .get_mut(usize::from(clip.u16(slot)?))
                    .context("Pose slot")?;
                for axis in 0..width {
                    pose[start + axis] = clip.f32(values[track * frames] + axis * 4)?;
                }
            }
        }
    }
    Ok(poses)
}

pub(super) fn verify(
    staged: &mut Reader,
    components: &[Value],
    entity: &str,
    graph: &Value,
    states: &Payload,
    bank: &Payload,
    required: &Value,
) -> Result<Value> {
    let mode = graph["import_settings"]["first_person"]["framing"]
        .as_str()
        .unwrap_or("native");
    if mode == "native" {
        ensure!(
            required.is_null(),
            "Required source hip framing was omitted"
        );
        return Ok(Value::Null);
    }
    ensure!(
        required["mode"] == mode,
        "Source framing requires an independent mode witness"
    );
    let expected = required["gun_relative"]
        .as_array()
        .context("Source gun position witness")?;
    ensure!(expected.len() == 3, "Invalid source gun position witness");
    let component = |class: &str| -> Result<u32> {
        let rows = components
            .iter()
            .filter(|row| row["entity"] == entity && row["class"] == class)
            .collect::<Vec<_>>();
        let [row] = rows.as_slice() else {
            anyhow::bail!("Ambiguous arm component {class}");
        };
        Ok(u32::from_str_radix(
            row["owner"].as_str().context("Arm owner")?,
            16,
        )?)
    };
    let skeleton = staged.tag(component("80808546")?, Some(0x80809C36))?;
    let controls = staged.tag(component("80808F8F")?, Some(0x80809C36))?;
    let rig = Rig::read(&skeleton, &controls)?;
    let bone = |name: &str| -> Result<usize> {
        rig.names
            .iter()
            .position(|&hash| hash == sundial::package_authoring::fnv1_name_hash(name))
            .context("Framing bone")
    };
    let reference = bone("b_utility")?;
    let gun = bone("b_handle")?;
    let mut receipts = Vec::new();
    for (descriptor, tag) in arm_routes(states, bank, "idle")? {
        let clip = staged.tag(tag, Some(0x80808F49))?;
        let objects = rig.objects(&first_frame(&clip, &rig.defaults)?)?;
        let relative = objects[reference].inverse().then(objects[gun]);
        for (axis, value) in expected.iter().enumerate() {
            let expected = value.as_f64().context("Source framing coordinate")?;
            ensure!(
                expected.is_finite() && (f64::from(relative.0[4 + axis]) - expected).abs() < 0.0001,
                "Emitted hip framing differs from the source witness on axis {axis}"
            );
        }
        receipts.push(json!({"descriptor":descriptor,"clip":format!("{tag:08X}"),
            "gun_relative":&relative.0[4..7],"expected":expected}));
    }
    ensure!(!receipts.is_empty(), "Empty required hip framing routes");
    let actions = &graph["import_settings"]["first_person"]["actions"];
    let names = states.array(8, 8, Some(0x8080342E))?;
    let descriptors = bank.array(0x68, 32, Some(0x80809002))?;
    let mut aiming = Vec::new();
    let mut excluded_aiming = Vec::new();
    for (base, aim) in [("idle", "iron_sight_idle"), ("fire", "iron_sight_fire")] {
        let base = if base == "fire" && actions.get("fire_1").is_some() {
            "fire_1"
        } else {
            base
        };
        if actions.get(base).is_none() || actions.get(aim).is_some() {
            continue;
        }
        let hash = sundial::package_authoring::fnv1_name_hash(aim);
        if !names.iter().any(|&at| states.u32(at).ok() == Some(hash)) {
            continue;
        }
        let routes = arm_routes(states, bank, aim)?;
        ensure!(!routes.is_empty(), "Empty required aiming routes");
        let mut absolute = true;
        for &(descriptor, _) in &routes {
            let at = *descriptors
                .get(descriptor as usize)
                .context("Aiming descriptor outside bank")?;
            absolute &= bank.u16(at + 26)? == 1;
        }
        if !absolute {
            excluded_aiming
                .push(json!({"state":aim,"reason":"Native state includes additive playback"}));
            continue;
        }
        let reference_tag = arm_routes(states, bank, base)?
            .first()
            .context("Base action has no clip")?
            .1;
        let reference_clip = staged.tag(reference_tag, Some(0x80808F49))?;
        let reference_pose = rig.objects(&first_frame(&reference_clip, &rig.defaults)?)?;
        let tracked = [gun, bone("b_l_hand")?, bone("b_r_hand")?];
        for (descriptor, tag) in routes {
            let clip = staged.tag(tag, Some(0x80808F49))?;
            let objects = rig.objects(&first_frame(&clip, &rig.defaults)?)?;
            for &bone in &tracked {
                let expected = reference_pose[reference]
                    .inverse()
                    .then(reference_pose[bone]);
                let actual = objects[reference].inverse().then(objects[bone]);
                let distance = (4..7)
                    .map(|axis| (expected.0[axis] - actual.0[axis]).powi(2))
                    .sum::<f32>()
                    .sqrt();
                let rotation = (0..4)
                    .map(|axis| expected.0[axis] * actual.0[axis])
                    .sum::<f32>()
                    .abs();
                ensure!(
                    distance < 0.0001 && (1. - rotation).abs() < 0.0001,
                    "Aiming action {aim} loses translated weapon or hand alignment"
                );
            }
            aiming.push(json!({"state":aim,"descriptor":descriptor,"clip":format!("{tag:08X}"),"base_clip":format!("{reference_tag:08X}")}));
        }
    }
    Ok(
        json!({"mode":mode,"native_reference":"b_utility","idle":receipts,"aiming":aiming,"excluded_aiming":excluded_aiming,
        "limit":"Emitted control-space placement, not final camera projection or gameplay."}),
    )
}
