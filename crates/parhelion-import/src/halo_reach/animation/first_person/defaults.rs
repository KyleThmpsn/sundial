//! Select compatible source actions for browser imports without an explicit motion plan.
use super::*;

pub(in crate::halo_reach) fn select(
    cache: &Cache,
    scene: &Scene,
    native: &mut Reader,
    donor: u32,
    root: &Path,
) -> Result<Option<Options>> {
    const ARMS: &str = "objects\\characters\\spartans\\fp\\fp";
    if scene.object.tag.group != "weap"
        || matches!(
            scene.object.gameplay["weapon_class"].as_str(),
            Some("sword" | "hammer")
        )
        || !scene
            .object
            .first_person
            .iter()
            .any(|branch| branch.model.is_some() && branch.animation.is_some())
        || !cache
            .tags
            .iter()
            .flatten()
            .any(|tag| tag.group == "mode" && tag.path == ARMS)
    {
        return Ok(None);
    }
    let Some(source) = source::Source::select(cache, scene, ARMS, root)? else {
        return Ok(None);
    };
    let carrier = carrier::Carrier::read(native, donor)?;
    let states = carrier
        .states
        .array(8, 8, Some(0x8080342E))?
        .into_iter()
        .map(|row| carrier.states.u32(row))
        .collect::<Result<BTreeSet<_>>>()?;
    let mut actions = BTreeMap::new();
    // Routes identify the action independently of a clip's authored spelling.
    // Only the unconditional local action domain is safe to select here.
    // Mode-specific actions and overlays need their runtime selector translated.
    let mut candidates = BTreeMap::<String, BTreeSet<String>>::new();
    for row in source.graph["routes"]
        .as_array()
        .context("Source arm routes")?
    {
        if row["kind"] != "action"
            || row["graph_index"] != -1
            || ["mode", "weapon_class", "weapon_type", "set"]
                .iter()
                .any(|key| row[*key] != "any")
        {
            continue;
        }
        let state = row["state"].as_str().context("Source action name")?;
        let clip = row["clip"].as_str().context("Source action clip")?;
        candidates
            .entry(state.to_owned())
            .or_default()
            .insert(clip.to_owned());
        if state == "fire_1" {
            candidates
                .entry("fire".to_owned())
                .or_default()
                .insert(clip.to_owned());
        }
    }
    for (state, names) in candidates {
        if !states.contains(&hash(&state)) {
            continue;
        }
        ensure!(
            names.len() == 1,
            "Source action {state} has ambiguous unconditional routes"
        );
        let descriptors = animation::state::descriptors(&carrier.states, hash(&state))?;
        let mut absolute = true;
        for descriptor in descriptors {
            let row = *carrier
                .descriptors
                .get(descriptor)
                .context("Native action descriptor outside bank")?;
            absolute &= carrier.bank.u16(row + 26)? == 1;
        }
        if !absolute {
            continue;
        }
        for name in names {
            if source.supports(root, &name)? {
                actions.insert(state.clone(), Action::Clip(name));
                break;
            }
        }
    }
    ensure!(
        actions.contains_key("idle"),
        "Source arms require a compatible absolute idle action"
    );
    let melee = melee::defaults(&source, root, native, &carrier)?;
    Ok(Some(Options {
        arms_model: ARMS.to_owned(),
        actions,
        melee,
        held_offset: [0.; 3],
        framing: Framing::SourceCentered,
        // Preparation enables source cues when source audio is available. Explicit
        // saved handling choices retain their value.
        handling_audio: false,
    }))
}
