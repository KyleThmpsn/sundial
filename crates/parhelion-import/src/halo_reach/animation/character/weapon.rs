//! Automatic character actions for a source weapon, selected through the source class's own
//! action routes and the native states its donor's attachment profile resolves.
use super::super::route::{Kind, Route};
use super::*;
use crate::halo_reach::Scene;
use std::collections::BTreeSet;

const GRAPH: &str = "objects\\characters\\spartans\\spartans";
/// The attachment category of native sword-controller weapons.
const ARTIFACT: u32 = 0x19EC2C8B;

/// Native human states and the source state each takes, for every weapon class.
const ACTIONS: [(&str, &str); 7] = [
    ("ready", "ready"),
    ("put_away", "put_away"),
    ("fire_1", "fire_1"),
    ("reload_1", "reload_1"),
    ("throw_grenade", "throw_grenade"),
    ("airborne", "airborne"),
    ("land_hard", "land_hard"),
];

/// The native sword's aerial and grounded profile lists name these states. Their human bank
/// routes corroborate the first air and ground light roles. 35F9730A is the
/// ammunition-gated heavy strike of the grounded powered list, and 7843564F is the aerial
/// slam a hammer's sword rows select in place of 054A16BE.
const MELEE: [(&str, &str); 10] = [
    ("0x35F9730A", "melee_strike_1"),
    ("0x054A16BE", "melee_lunge_airborne"),
    ("0x7843564F", "melee_lunge_airborne"),
    ("0x06B623AD", "melee_lunge_airborne"),
    ("0xC5C3F7A0", "melee_strike_1"),
    ("0xC5C3F7A3", "melee_strike_2"),
    ("0xCE9A511C", "melee_strike_2"),
    ("0xCE9A511D", "melee_strike_1"),
    ("0xCE9A511F", "melee_strike_1"),
    ("0xE9D650B6", "melee_lunge"),
];

/// Source movement cycles offered to each native direction's speed tiers.
const MOVEMENT: [(&str, [&str; 7]); 4] = [
    (
        "front",
        [
            "walk_front_slow",
            "walk_front",
            "walk_front_fast",
            "jog_front",
            "move_front_slow",
            "move_front",
            "move_front_fast",
        ],
    ),
    (
        "back",
        [
            "walk_back_slow",
            "walk_back",
            "walk_back_fast",
            "jog_back",
            "move_back_slow",
            "move_back",
            "move_back_fast",
        ],
    ),
    (
        "left",
        [
            "walk_left_slow",
            "walk_left",
            "walk_left_fast",
            "jog_left",
            "move_left_slow",
            "move_left",
            "move_left_fast",
        ],
    ),
    (
        "right",
        [
            "walk_right_slow",
            "walk_right",
            "walk_right_fast",
            "jog_right",
            "move_right_slow",
            "move_right",
            "move_right_fast",
        ],
    ),
];

/// A resolved source action with its clip's animation and movement kinds.
struct Resolved {
    action: Action,
    kind: u64,
    movement: u64,
}

pub(in crate::halo_reach) fn defaults(
    cache: &Cache,
    scene: &Scene,
    native: &mut Reader,
    donor: u32,
) -> Result<Option<Options>> {
    if scene.object.tag.group != "weap" {
        return Ok(None);
    }
    let Some(class) = scene.object.gameplay["weapon_class"]
        .as_str()
        .filter(|class| !class.is_empty())
    else {
        return Ok(None);
    };
    let Ok(graph) = cache.find("jmad", GRAPH) else {
        return Ok(None);
    };
    let routes = json!({"routes":super::super::route::read(cache, graph.address()?)?});
    let data = crate::halo_reach::behavior::animation_data(cache, graph)?;
    let mut metadata = BTreeMap::new();
    for clip in data["clips"].as_array().context("Source character clips")? {
        let shared = &clip["shared"][0];
        if let (Some(name), Some(kind), Some(movement)) = (
            clip["name"].as_str(),
            shared["animation_type"].as_u64(),
            shared["movement_type"].as_u64(),
        ) {
            metadata.insert(name.to_owned(), (kind, movement));
        }
    }
    // Reach's route type is the weapon's name, such as `ne` for the Needler, so a weapon's own
    // set comes before its class's `any` set. Draw, holster, fire, reload and throw are overlay
    // routes, while stances and movement are action routes.
    let weapon_type = scene.object.gameplay["weapon_name"]
        .as_str()
        .filter(|name| !name.is_empty());
    let route = |mode: &str, state: &str| -> Option<(Route, String)> {
        for weapon_type in weapon_type.into_iter().chain(["any"]) {
            for kind in [Kind::Action, Kind::Overlay] {
                let route = Route {
                    mode: mode.into(),
                    weapon_class: class.into(),
                    weapon_type: weapon_type.into(),
                    set: "any".into(),
                    state: state.into(),
                    kind,
                    destination: None,
                };
                if let Ok(clip) = route.clip(&routes) {
                    let clip = clip.to_owned();
                    return Some((route, clip));
                }
            }
        }
        None
    };
    // A crouching action the class leaves unrouted takes the crouching clip of the family its
    // standing route plays, which is an inference about Reach's fallback, recorded by the clip
    // selection itself.
    let resolve = |mode: &str, state: &str| -> Option<Resolved> {
        if let Some((route, clip)) = route(mode, state) {
            let &(kind, movement) = metadata.get(&clip)?;
            return Some(Resolved {
                action: Action::Route(route),
                kind,
                movement,
            });
        }
        if mode != "crouch" {
            return None;
        }
        let (_, clip) = route("combat", state)?;
        let name = format!("{mode}:{}", clip.split_once(':')?.1);
        let &(kind, movement) = metadata.get(&name)?;
        Some(Resolved {
            action: Action::Clip(name),
            kind,
            movement,
        })
    };
    let Some(reference) = resolve("combat", "idle").filter(|r| r.kind == 1 && r.movement == 0)
    else {
        return Ok(None);
    };
    let reference_name = reference.action.names(&routes)?[0].to_owned();
    let item = crate::tiger::shadowkeep::extract(native, donor)?;
    let item_tag = u32::from_str_radix(item["item_tag"].as_str().context("Native item tag")?, 16)?;
    let report = crate::tiger::rig::inspect(native, item_tag, false)?;
    let runtime = u32::from_str_radix(
        report["runtime_entity"]
            .as_str()
            .context("Weapon runtime")?,
        16,
    )?;
    let content = u32::from_str_radix(
        report["content_key"].as_str().context("Weapon content")?,
        16,
    )?;
    let entity = native.tag(runtime, Some(0x80809C0F))?;
    let bindings =
        sundial::package_authoring::entity::weapon_component_bindings(&entity.0, 0xD3A5500E)
            .map_err(anyhow::Error::msg)?;
    let [binding] = bindings.as_slice() else {
        return Ok(None);
    };
    let owner = native.tag(binding.owner_tag, Some(0x80809C36))?;
    let definition = owner.pointer(24)?;
    if owner.u32(definition - 4)? != 0x80804221 {
        return Ok(None);
    }
    let rows = crate::tiger::rig::attachment_rows(&owner, definition, content, false)?;
    let mut profiles = BTreeSet::new();
    for &row in &rows {
        profiles.insert((owner.u32(row + 0x68)?, owner.u32(row + 0x6C)?));
    }
    let profiles = profiles.into_iter().collect::<Vec<_>>();
    let &[(category, parent)] = profiles.as_slice() else {
        return Ok(None);
    };
    let human = Controller {
        skeleton: 0x80C22410,
        controls: 0x80BFCB38,
        lookup: 0x80EF62E5,
        pose: 0x8161D5F7,
    };
    let lookup = native.tag(human.lookup, Some(0x80809C36))?;
    let at = lookup.pointer(24)?;
    let dictionary = native.tag(lookup.u32(at + 0x94)?, Some(0x80808EE1))?;
    let states = native.tag(lookup.u32(at + 0x9C)?, Some(0x80803465))?;
    let selector = native.tag(lookup.u32(at + 0xB8)?, Some(0x80803478))?;
    // A state is filled only where the donor's profile selects native descriptors for it.
    let available = |state: &str| -> Result<bool> {
        Ok(animation::state::parameter::descriptors(
            &states,
            &dictionary,
            action_name(state)?,
            3,
            parent,
        )
        .is_ok())
    };
    let mut actions = BTreeMap::new();
    let mut requested = ACTIONS.to_vec();
    if category == ARTIFACT {
        requested.extend(MELEE);
    }
    for (state, role) in requested {
        if let Some(source) = resolve("combat", role)
            && matches!(source.kind, 1..=3)
            && matches!(source.movement, 0 | 1)
            && available(state)?
        {
            actions.insert(state.to_owned(), source.action);
        }
    }
    let mut stance = BTreeMap::new();
    let mut locomotion = BTreeMap::new();
    for (name, mode) in [("stand", "combat"), ("crouch", "crouch")] {
        let Some(idle) = resolve(mode, "idle").filter(|r| r.kind == 1 && r.movement == 0) else {
            continue;
        };
        let parameters = BTreeMap::from([(3, parent), (1, action_name(name)?)]);
        if animation::state::parameter::table_choices_for(&selector, &dictionary, &parameters)
            .is_err()
        {
            continue;
        }
        stance.insert(name.to_owned(), idle.action);
        let mut directions = BTreeMap::new();
        for (direction, states) in MOVEMENT {
            // Reach's sprint mode routes a longer forward run, which the fastest native
            // forward tiers can take.
            let sprint = (name == "stand" && direction == "front")
                .then(|| resolve("sprint", "move_front_fast"))
                .flatten();
            let candidates = states
                .iter()
                .filter_map(|state| resolve(mode, state))
                .chain(sprint)
                .filter(|r| r.kind == 1 && r.movement == 1)
                .map(|r| r.action)
                .collect::<Vec<_>>();
            if !candidates.is_empty() {
                directions.insert(direction.to_owned(), candidates);
            }
        }
        if !directions.is_empty() {
            locomotion.insert(name.to_owned(), directions);
        }
    }
    if actions.is_empty() && stance.is_empty() {
        return Ok(None);
    }
    Ok(Some(Options {
        graph: GRAPH.into(),
        model: GRAPH.into(),
        reference: reference_name,
        actions,
        stance,
        rider: None,
        passengers: Vec::new(),
        locomotion,
        bone_aliases: BTreeMap::new(),
        native: human,
    }))
}
