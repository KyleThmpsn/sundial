//! Follow staged action routes to the hand targets consumed by native arm solving.
use super::*;
use std::collections::BTreeSet;

fn definition(p: &Payload, class: u32) -> Result<usize> {
    let instance = p.pointer(16)?;
    let definition = usize::try_from(p.u64(instance + 8)?)?;
    ensure!(
        p.u32(instance + 4)? == class
            && p.u32(definition.checked_sub(4).context("Arm definition")?)? == class
            && p.u64(definition + 8)? == instance as u64,
        "Staged arm component has no reciprocal runtime definition"
    );
    Ok(definition)
}

pub(super) fn verify(
    staged: &mut Reader,
    components: &[Value],
    entity: &str,
    graph: &Value,
    states: &Payload,
    bank: &Payload,
) -> Result<Value> {
    let first_person = &graph["animation"]["first_person"];
    let mut routes = Vec::new();
    for action in first_person["arm_motion"]["actions"]
        .as_array()
        .context("Translated arm actions")?
    {
        let state = action["state"].as_str().context("Arm action state")?;
        let selected = arm_routes(states, bank, state)?;
        ensure!(!selected.is_empty(), "Source action has no staged route");
        for (descriptor, clip) in selected {
            routes.push((state.to_owned(), descriptor, clip));
        }
    }
    if !first_person["melee"]["strikes"]
        .as_array()
        .context("Translated weapon strikes")?
        .is_empty()
    {
        for state in [0xFDCA54D9, 0xFDCA54DB] {
            let selected = arm_routes_hash(states, bank, state)?;
            ensure!(!selected.is_empty(), "Source strike has no staged route");
            for (descriptor, clip) in selected {
                routes.push((format!("{state:08X}"), descriptor, clip));
            }
        }
    }
    if routes.is_empty() {
        return Ok(Value::Null);
    }
    let owner = |class: &str| -> Result<u32> {
        let rows = components
            .iter()
            .filter(|row| row["entity"] == entity && row["class"] == class)
            .collect::<Vec<_>>();
        let [row] = rows.as_slice() else {
            anyhow::bail!("Ambiguous staged arm component {class}");
        };
        Ok(u32::from_str_radix(
            row["owner"].as_str().context("Arm component owner")?,
            16,
        )?)
    };
    let skeleton_tag = owner("80808546")?;
    let controls_tag = owner("80808F8F")?;
    let skeleton = staged.tag(skeleton_tag, Some(0x80809C36))?;
    let controls = staged.tag(controls_tag, Some(0x80809C36))?;
    let bones = skeleton.array(
        definition(&skeleton, 0x80808546)? + 0x80,
        16,
        Some(0x80808A08),
    )?;
    let rows = controls.array(definition(&controls, 0x80808F8F)? + 0xA0, 52, None)?;
    let mut hands = BTreeSet::new();
    let mut solvers = Vec::new();
    for side in ["l", "r"] {
        let bone = |role: &str| -> Result<usize> {
            let name = sundial::package_authoring::fnv1_name_hash(&format!("b_{side}_{role}"));
            let matches = bones
                .iter()
                .enumerate()
                .filter_map(|(index, &row)| (skeleton.u32(row).ok() == Some(name)).then_some(index))
                .collect::<Vec<_>>();
            let [index] = matches.as_slice() else {
                anyhow::bail!("Missing or ambiguous staged arm bone");
            };
            Ok(*index)
        };
        let upper = bone("upperarm")?;
        let hand = bone("hand")?;
        let slots = rows
            .iter()
            .enumerate()
            .filter_map(|(slot, &row)| {
                (controls.u16(row + 44).ok().map(usize::from) == Some(hand)).then_some((slot, row))
            })
            .collect::<Vec<_>>();
        let [(slot, row)] = slots.as_slice() else {
            anyhow::bail!("Missing or ambiguous staged hand control");
        };
        ensure!(
            usize::from(controls.u16(row + 46)?) == upper
                && controls.u8(row + 49)? == 0
                && controls.u8(row + 50)? == 3,
            "Translated hands lost their native guided arm solver"
        );
        hands.insert(u16::try_from(*slot)?);
        solvers.push(json!({"side":side,"hand_control":slot,"hand_bone":hand,"upper_bone":upper}));
    }
    let mut receipts = Vec::new();
    for (state, descriptor, tag) in routes {
        let clip = staged.tag(tag, Some(0x80808F49))?;
        let mut bindings = Vec::new();
        if clip.u64(0x30)? != 0 {
            let resource = clip.pointer(0x30)?;
            ensure!(
                clip.u32(resource.checked_sub(4).context("Constraint resource")?)? == 0x80808F59,
                "Unknown staged constraint resource"
            );
            for row in clip.array(resource, 64, Some(0x80808F5C))? {
                let end = clip.u16(row + 2)?;
                let mode = clip.u8(row + 0x12)?;
                ensure!(
                    mode != 3 || !hands.contains(&end),
                    "Source action {state} still redirects its hand to a donor target"
                );
                bindings.push(json!({"start":clip.u16(row)?,"end":end,"mode":mode}));
            }
        }
        receipts.push(json!({"state":state,"descriptor":descriptor,"clip":format!("{tag:08X}"),"bindings":bindings}));
    }
    Ok(
        json!({"skeleton":format!("{skeleton_tag:08X}"),"controls":format!("{controls_tag:08X}"),
        "solvers":solvers,"actions":receipts,
        "limit":"Staged target ownership and solver selection. Final poses and transitions need gameplay verification."}),
    )
}
