//! Select complete source driver actions from the seat's animation role.
use super::*;
use crate::halo_reach::Scene;

pub(super) fn defaults(cache: &Cache, scene: &Scene) -> Result<Option<Options>> {
    const CHARACTER: &str = "objects\\characters\\spartans\\spartans";
    let Some(seats) = scene.object.gameplay["seats"].as_array() else {
        return Ok(None);
    };
    let drivers = seats
        .iter()
        .filter(|seat| seat["flags"].as_u64().is_some_and(|f| f & 4 != 0))
        .collect::<Vec<_>>();
    if drivers.is_empty() {
        return Ok(None);
    }
    ensure!(drivers.len() == 1, "Source rider has multiple driver roles");
    let role = drivers[0]["animation"]
        .as_str()
        .filter(|v| !v.is_empty())
        .context("Source driver animation role")?;
    if !["mode", "jmad"].iter().all(|group| {
        cache
            .tags
            .iter()
            .flatten()
            .any(|tag| tag.group == *group && tag.path == CHARACTER)
    }) {
        return Ok(None);
    }
    let graph = crate::halo_reach::behavior::animation_data(cache, cache.find("jmad", CHARACTER)?)?;
    let names = graph["clips"]
        .as_array()
        .context("Source rider actions")?
        .iter()
        .filter_map(|clip| clip["name"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let select = |state: &str| -> Result<String> {
        let candidates = [format!("{role}:{state}"), format!("{role}:any:{state}")]
            .into_iter()
            .filter(|name| names.contains(name.as_str()))
            .collect::<Vec<_>>();
        ensure!(
            candidates.len() == 1,
            "Source driver role {role} has no unique {state} action"
        );
        Ok(candidates.into_iter().next().unwrap())
    };
    let idle = select("idle")?;
    Ok(Some(Options {
        graph: CHARACTER.into(),
        model: CHARACTER.into(),
        reference: idle.clone(),
        actions: BTreeMap::new(),
        stance: BTreeMap::new(),
        rider: Some(Rider {
            enter: Action::Clip(select("enter")?),
            idle: Action::Clip(idle),
            exit: Action::Clip(select("exit")?),
        }),
        bone_aliases: BTreeMap::new(),
        native: Controller {
            skeleton: 0x80C22410,
            controls: 0x80BFCB38,
            lookup: 0x80EF62E5,
            pose: 0x8161D5F7,
        },
    }))
}
