//! Native aiming states reuse source actions when Reach has no separate aimed pose.
use super::*;

pub(super) fn complete(
    requested: &BTreeMap<String, Action>,
    states: &Payload,
    bank: &Payload,
) -> Result<BTreeMap<String, Action>> {
    let names = states
        .array(8, 8, Some(0x8080342E))?
        .into_iter()
        .map(|at| states.u32(at))
        .collect::<Result<BTreeSet<_>>>()?;
    let descriptors = bank.array(0x68, 32, Some(0x80809002))?;
    let mut actions = requested.clone();
    // Aiming placement is applied by the native sight consumer. Its absolute
    // animation still needs the source weapon and hand poses before that pass.
    // Explicit per-state selections win, including separate source aimed fire.
    for (target, sources) in [
        ("iron_sight_idle", &["idle"][..]),
        ("fire_auto", &["fire_1", "fire"][..]),
        ("iron_sight_fire", &["fire_1", "fire"][..]),
        (
            "iron_sight_fire_auto",
            &["iron_sight_fire", "fire_auto", "fire_1", "fire"][..],
        ),
        ("iron_sight_reload_full", &["reload_full"][..]),
        ("iron_sight_reload_empty", &["reload_empty"][..]),
    ] {
        if actions.contains_key(target) || !names.contains(&hash(target)) {
            continue;
        }
        let Some(source) = sources
            .iter()
            .find_map(|source| actions.get(*source))
            .cloned()
        else {
            continue;
        };
        let mut absolute = true;
        for descriptor in animation::state::descriptors(states, hash(target))? {
            let at = *descriptors
                .get(descriptor)
                .context("Aiming descriptor outside bank")?;
            absolute &= bank.u16(at + 26)? == 1;
        }
        // Never reinterpret an additive state as a complete source pose.
        if absolute {
            actions.insert(target.to_owned(), source);
        }
    }
    Ok(actions)
}
