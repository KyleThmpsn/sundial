//! Select compatible source actions for browser imports without an explicit motion plan.
use super::*;

/// The source overheat sequence: entering the lock, holding it and leaving it.
const VENT: [&str; 3] = ["overheating", "overheated", "o_h_exit"];

/// `vents` is true when the weapon's heat lock stands in for its magazine, so the native reload is
/// its vent.
pub(in crate::halo_reach) fn select(
    cache: &Cache,
    scene: &Scene,
    native: &mut Reader,
    donor: u32,
    root: &Path,
    vents: bool,
) -> Result<Option<Options>> {
    const ARMS: &str = "objects\\characters\\spartans\\fp\\fp";
    if scene.object.tag.group != "weap"
        // A sword donor's first-person entity has no single 80808546 component, which the
        // carrier reader requires, so Reach swords and hammers keep native first-person arms.
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
    // Every descriptor of a native state plays absolute motion, which a complete source pose needs.
    let absolute = |state: &str| -> Result<bool> {
        let mut absolute = true;
        for descriptor in animation::state::descriptors(&carrier.states, hash(state))? {
            let row = *carrier
                .descriptors
                .get(descriptor)
                .context("Native action descriptor outside bank")?;
            absolute &= carrier.bank.u16(row + 26)? == 1;
        }
        Ok(absolute)
    };
    for (state, names) in &candidates {
        if !states.contains(&hash(state)) {
            continue;
        }
        ensure!(
            names.len() == 1,
            "Source action {state} has ambiguous unconditional routes"
        );
        if !absolute(state)? {
            continue;
        }
        for name in names {
            if source.supports(root, name)? {
                actions.insert(state.clone(), Action::Clip(name.clone()));
                break;
            }
        }
    }
    // Reach plays a heat-locked weapon's overheat sequence while it cools. Where that lock is the
    // native magazine, the native reload is the vent, so a source without reloads of its own plays
    // the sequence there, fitted to the reload's duration.
    let vent = VENT
        .iter()
        .map(|state| match candidates.get(*state) {
            Some(names) if names.len() == 1 => names.first().cloned(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>();
    if vents && let Some(vent) = vent {
        let mut supported = true;
        for name in &vent {
            supported &= source.supports(root, name)?;
        }
        for state in ["reload_full", "reload_empty"] {
            if supported
                && !actions.contains_key(state)
                && states.contains(&hash(state))
                && absolute(state)?
            {
                actions.insert(state.to_owned(), Action::Sequence(vent.clone()));
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
