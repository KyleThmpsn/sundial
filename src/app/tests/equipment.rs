use crate::app::equipment::selected_attunement_index;
use crate::catalog;
use crate::catalog::AbilityChoice;

#[test]
fn distinctive_super_selection_wins_when_old_attunements_are_mixed() {
    let choice = |entry, name: &str| AbilityChoice {
        entry,
        name: name.into(),
    };
    let abilities = catalog::AbilityOptions {
        attunements: vec![
            catalog::AttunementChoice {
                name: "Top".into(),
                super_abilities: vec![choice(10, "Base super")],
                melee: choice(11, "Top melee"),
                perks: vec![choice(13, "Former top selector")],
            },
            catalog::AttunementChoice {
                name: "Bottom".into(),
                super_abilities: vec![choice(10, "Base super")],
                melee: choice(15, "Bottom melee"),
                perks: vec![choice(18, "Former bottom selector")],
            },
            catalog::AttunementChoice {
                name: "Middle".into(),
                super_abilities: vec![choice(20, "Middle super")],
                melee: choice(21, "Middle melee"),
                perks: Vec::new(),
            },
        ],
        ..Default::default()
    };
    assert_eq!(selected_attunement_index(&abilities, 10, 15), 1);
    assert_eq!(selected_attunement_index(&abilities, 20, 15), 2);
    assert_eq!(selected_attunement_index(&abilities, 18, 21), 1);
}
