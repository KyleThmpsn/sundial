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
    // The flag gates ability activity and the option selects its current/base version.
    // Neither changes the ability slot named here.
    let _ = (flag, option);
    match selector {
        0 => Some("Grenade Energy"),
        1 => Some("Super Energy"),
        2 => Some("Melee Energy"),
        7 => Some("Class Ability Energy"),
        _ => None,
    }
}

/// Ability Property's target slots, which kind 7 witnesses on its own. Every stock perk that
/// sets each value names that ability in its own text: slot 0 And Another Thing ("an
/// additional grenade charge") and Bring the Heat ("Fusion Grenades"), slot 1 Actual Grandeur
/// ("during Nova Bomb") and Beacons of Empowerment ("Sun Warrior"), slot 2 Biotic Enhancements
/// ("melee lunge range") and Cobra Totemic ("Melee range is extended"), slot 7 Double Dodge
/// ("a second dodge charge") and Alchemical Etchings ("Your Rifts"). Those four agree with
/// the independent kind 8 census in `component_target`.
///
/// Slot 3 is the jump, and all three perks that set it say so: Jump Jets ("aerial
/// maneuverability"), Hydraulic Boosters ("Improves High Jump") and Move to Survive ("Blink
/// further"). Kind 8 never uses it, so it stays out of `component_target`.
///
/// Slot 4 addresses the movement bank. Its property records separately change sprint speed,
/// slide mode and turning. The native bank handler 105D680 writes those distinct movement
/// fields. Last Stand's weapon and recovery bonuses are separate actions, not alternative
/// meanings of this slot.
#[must_use]
pub fn ability_slot(selector: u8) -> Option<&'static str> {
    match selector {
        0 => Some("Grenade"),
        1 => Some("Super"),
        2 => Some("Melee"),
        3 => Some("Jump"),
        4 => Some("Movement"),
        7 => Some("Class Ability"),
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
