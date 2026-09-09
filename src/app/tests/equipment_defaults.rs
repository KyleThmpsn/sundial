use crate::app::equipment::default_ability_values;
use crate::app::equipment::default_subclass_name;
use crate::catalog;
use crate::catalog::AbilityChoice;

#[test]
fn stock_classes_use_stock_subclasses_and_movement_defaults() {
    assert_eq!(default_subclass_name(0), "Sunbreaker");
    assert_eq!(default_subclass_name(1), "Nightstalker");
    assert_eq!(default_subclass_name(2), "Dawnblade");

    let abilities = catalog::AbilityOptions {
        movement: vec![
            AbilityChoice {
                entry: 4,
                name: "First".into(),
            },
            AbilityChoice {
                entry: 5,
                name: "Second".into(),
            },
            AbilityChoice {
                entry: 6,
                name: "Third".into(),
            },
        ],
        grenade: vec![AbilityChoice {
            entry: 7,
            name: "Grenade".into(),
        }],
        super_ability: vec![AbilityChoice {
            entry: 10,
            name: "Super".into(),
        }],
        melee: vec![AbilityChoice {
            entry: 11,
            name: "Melee".into(),
        }],
        class_ability: vec![AbilityChoice {
            entry: 2,
            name: "Class".into(),
        }],
        attunements: Vec::new(),
    };
    assert_eq!(
        default_ability_values(0, &abilities, Some(2)),
        (5, 7, 10, 11, 2)
    );
    assert_eq!(
        default_ability_values(0, &abilities, Some(3)),
        (6, 7, 10, 11, 2)
    );
    assert_eq!(
        default_ability_values(1, &abilities, Some(2)),
        (6, 7, 10, 11, 2)
    );
    assert_eq!(
        default_ability_values(2, &abilities, Some(3)),
        (5, 7, 10, 11, 2)
    );
}
