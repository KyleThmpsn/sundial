//! Fit qualified source contact poses around the retained native attack event.
use super::Action;
use crate::tiger::payload::Payload;
use anyhow::{Context, Result};
use serde_json::{Value, json};

pub(super) struct Fit {
    pub contact: Option<(usize, usize)>,
    pub receipt: Value,
}

impl Fit {
    pub fn linear(reason: &str) -> Self {
        Self {
            contact: None,
            receipt: json!({"mode":"linear","reason":reason}),
        }
    }
}

pub(super) fn contact(graph: &Value, action: &Action, native: &Payload) -> Result<Fit> {
    if matches!(action, Action::Sequence(_)) {
        return Ok(Fit::linear(
            "An ordered source sequence has no qualified contact",
        ));
    }
    let names = action.names(graph)?;
    let [name] = names.as_slice() else {
        return Ok(Fit::linear("Source action does not select one clip"));
    };
    let parts = name.split(':').collect::<Vec<_>>();
    if !matches!(
        parts.as_slice(),
        [
            _,
            "sword" | "hammer",
            "melee_strike_1"
                | "melee_strike_2"
                | "melee_lunge"
                | "melee_lunge_airborne"
                | "melee_lunge_unaimed"
                | "melee_lunge_unaimed_airborne"
        ]
    ) {
        return Ok(Fit::linear(
            "Source action has no qualified melee contact role",
        ));
    }
    let clip = graph["clips"]
        .as_array()
        .context("Character source clips")?
        .iter()
        .find(|clip| clip["name"].as_str() == Some(*name))
        .context("Source contact action is absent")?;
    let shared = &clip["shared"][0];
    let source_frames = usize::try_from(
        shared["frames"]
            .as_u64()
            .context("Source contact frame count")?,
    )?;
    let primary = shared["frame_events"]
        .as_array()
        .context("Source contact frame events")?
        .iter()
        .filter(|event| event["kind"] == 1)
        .collect::<Vec<_>>();
    let [primary] = primary.as_slice() else {
        return Ok(Fit::linear(
            "Source action has no unique primary frame event",
        ));
    };
    let source_frame = usize::try_from(primary["frame"].as_u64().context("Source primary frame")?)?;
    if source_frame == 0 || source_frame >= source_frames.saturating_sub(1) {
        return Ok(Fit::linear(
            "Source primary frame is not inside its motion interval",
        ));
    }
    let mut contacts = Vec::new();
    for row in native.array(0x160, 8, Some(0x8080902B))? {
        let event = native.pointer(row)?;
        let class = native.u32(event.checked_sub(4).context("Native contact event class")?)?;
        if class == 0x8080903A && native.u16(event + 2)? == 2 && native.u32(event + 4)? == 1 {
            contacts.push(usize::from(native.u16(event)?));
        }
    }
    let [native_frame] = contacts.as_slice() else {
        return Ok(Fit::linear(
            "Native action has no unique contact sequence event",
        ));
    };
    let native_frames = usize::from(native.u16(0x13C)?);
    if *native_frame == 0 || *native_frame >= native_frames.saturating_sub(1) {
        return Ok(Fit::linear(
            "Native contact frame is not inside its motion interval",
        ));
    }
    Ok(Fit {
        contact: Some((source_frame, *native_frame)),
        receipt: json!({"mode":"contact","source_frame":source_frame,
            "source_frames":source_frames,"native_frame":native_frame,"native_frames":native_frames,
            "retained":"Native duration, event frames and resource references"}),
    })
}
