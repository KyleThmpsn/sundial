//! Gameplay roles corroborated by independent native perk programs and item descriptions.

/// Kind 8's ability targets. The selector alone picks the ability; the flag and option bytes
/// are a separate axis and do not change which ability is adjusted.
///
/// Innervation (945) and Bomber (981, 982) independently identify slot 0 as grenade.
/// Invigoration (946) and Outreach (980) identify slot 2 as melee.
/// Insulation (947) and Perpetuation (979) identify slot 7 as class ability.
/// Absolution (948) combines exactly these three.
///
/// A census of every kind 8 node in the installed stock perks found 11 distinct
/// selector/flag/option combinations, and the ability each one adjusts follows the selector
/// in all of them. That establishes slot 1 as super, from 24 perks across three flag values:
/// Ashes to Assets, Hands-On, Heavy Lifting, Remote Connection, Pump Action and Light
/// Reactor all read "gain bonus Super energy", Dynamo reduces Super cooldown, and Roving
/// Assassin gives "more Super energy". It also shows the flag and option bytes leave the
/// ability alone: Momentum Transfer sets flag 1 on slot 2 and reduces a melee cooldown,
/// Empowering Largesse sets option 1 on slot 0 and recharges a grenade, and Insatiable sets
/// option 1 on slot 7 and grants class energy.
///
/// These are source-derived roles, not player observations or a claim about the units of the
/// adjustment scale.
#[must_use]
pub fn component_target(selector: u8, flag: u8, option: u8) -> Option<&'static str> {
    // The flag and option bytes select something else about the adjustment. No stock node
    // contradicts the selector's ability, so they are not part of this reading.
    let _ = (flag, option);
    match selector {
        0 => Some("Grenade Energy"),
        1 => Some("Super Energy"),
        2 => Some("Melee Energy"),
        7 => Some("Class Ability Energy"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_selector_names_the_ability_whatever_the_flag_and_option_bytes_hold() {
        // The flag and option gate was dropped after a census of every kind 8 node in the
        // stock perks: all 11 selector, flag and option combinations adjust the ability the
        // selector names. Momentum Transfer sets flag 1 on slot 2 and reduces a melee
        // cooldown, Empowering Largesse sets option 1 on slot 0 and recharges a grenade.
        for (selector, name) in [
            (0, "Grenade Energy"),
            (1, "Super Energy"),
            (2, "Melee Energy"),
            (7, "Class Ability Energy"),
        ] {
            for (flag, option) in [(0, 0), (1, 0), (2, 0), (0, 1)] {
                assert_eq!(component_target(selector, flag, option), Some(name));
            }
        }
        // A selector no stock perk uses stays a number rather than borrowing a neighbour.
        for selector in [3, 4, 5, 6, 8, 255] {
            assert_eq!(component_target(selector, 0, 0), None);
        }
    }
}
