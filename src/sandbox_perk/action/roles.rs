//! Gameplay roles corroborated by independent native perk programs and item descriptions.

/// Kind 8's ordinary ability targets. Other flag combinations retain numeric selectors.
/// Innervation (945) and Bomber (981, 982) independently identify slot 0 as grenade.
/// Invigoration (946) and Outreach (980) identify slot 2 as melee.
/// Insulation (947) and Perpetuation (979) identify slot 7 as class ability.
/// Absolution (948) combines exactly these three. These are source-derived roles,
/// not player observations or a claim about the units of the adjustment scale.
#[must_use]
pub fn component_target(selector: u8, flag: u8, option: u8) -> Option<&'static str> {
    if flag != 0 || option != 0 {
        return None;
    }
    match selector {
        0 => Some("Grenade Energy"),
        2 => Some("Melee Energy"),
        7 => Some("Class Ability Energy"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ability_roles_do_not_spread_to_other_selectors_or_flag_modes() {
        for (selector, name) in [
            (0, "Grenade Energy"),
            (2, "Melee Energy"),
            (7, "Class Ability Energy"),
        ] {
            assert_eq!(component_target(selector, 0, 0), Some(name));
            assert_eq!(component_target(selector, 1, 0), None);
            assert_eq!(component_target(selector, 0, 1), None);
        }
        for selector in [1, 3, 6, 8, 255] {
            assert_eq!(component_target(selector, 0, 0), None);
        }
    }
}
