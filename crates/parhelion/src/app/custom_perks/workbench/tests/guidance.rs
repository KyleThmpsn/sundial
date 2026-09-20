use super::super::guidance::{self, EditingFilter, EffectOrder, Purpose};
use super::*;
use sundial::package_authoring::sandbox_perk::{
    dependencies::Behavior,
    nodes::Support,
    program::{Action, Asset, Position, Program, Trigger},
};

#[test]
fn copying_one_effect_keeps_its_identity_separate_from_the_template_plug() {
    let mut workbench = Workbench::default();
    let choice = WeaponSandboxPerkChoice {
        perk_index: 453,
        representative_hash: 0xF3DB_4BB3,
        representative_name: "Thorn Catalyst".into(),
        representative_type_name: String::new(),
    };
    workbench.copy_behavior(&choice);
    let recipe = &workbench.documents[workbench.selected].recipe;
    assert_eq!(recipe.name, "Custom Effect 453");
    assert!(
        recipe.description.is_empty(),
        "No decoded summary is available"
    );
    assert_eq!(recipe.template_plug, choice.representative_hash.into());
    assert_eq!(recipe.effects, [PerkRecipe::effect(453)]);
}

#[test]
fn discovery_matches_displayed_behavior_and_operation_names() {
    let behavior = Behavior {
        headline: "Spawns a projectile on a precision kill".into(),
        support: Support::Authorable,
        editable: true,
        program: None,
        condition_kinds: vec![2],
        effect_kinds: vec![3],
        details: Vec::new(),
        notes: Vec::new(),
    };
    let searchable = guidance::behavior_search(&behavior);
    assert!(pickers::matches("precision projectile", &searchable));
    assert!(pickers::matches("selected transform", &searchable));
    assert!(Purpose::Projectiles.allows(Some(&behavior)));
    assert!(!Purpose::Patterns.allows(Some(&behavior)));
    assert!(EditingFilter::Programs.allows(Some(&behavior)));
    assert!(!EditingFilter::Stock.allows(Some(&behavior)));
    assert!(!EditingFilter::Programs.allows(None));
    assert!(EditingFilter::All.allows(None));
}

#[test]
fn summary_distinguishes_spawn_lifetime_from_retained_duration_and_rearming() {
    let mut program = Program {
        trigger: Trigger::PrecisionKill,
        cooldown_ms: 5_000,
        duration_ms: 2_000,
        chance_permyriad: 5_000,
        actions: vec![Action::Spawn {
            asset: Asset::default(),
            position: Position::Event,
        }],
        ..Default::default()
    };
    let summary = guidance::summary(&program, None);
    assert!(summary.contains("precision kill with this weapon"));
    assert!(summary.contains("50%"));
    assert!(summary.contains("Cooldown: 5 seconds"));
    assert!(
        !summary.contains("After 2 s"),
        "a spawn has its own lifetime"
    );
    assert!(!summary.contains("Retained effects end"));
    program.actions.push(Action::attach(Asset::default()));
    assert!(guidance::summary(&program, None).contains("Retained effects end: After 2 s"));
    program.actions.push(Action::property(0x1234));
    assert!(guidance::summary(&program, None).contains("0x00001234"));
}

#[test]
fn effect_orders_keep_the_same_results_and_only_change_their_order() {
    let rows = [
        ("rampage", "Weapon Perk", false),
        ("absolution", "Armor Mod", false),
        ("outlaw", "Weapon Perk", true),
    ];
    let order_by = |order| {
        let mut sorted = rows.to_vec();
        sorted.sort_by_cached_key(|(name, kind, direct)| {
            guidance::effect_sort_key(order, kind, name, *direct)
        });
        sorted.iter().map(|(name, _, _)| *name).collect::<Vec<_>>()
    };
    // Best Match puts the effect whose own name matched the search first.
    assert_eq!(
        order_by(EffectOrder::BestMatch),
        ["outlaw", "absolution", "rampage"]
    );
    assert_eq!(
        order_by(EffectOrder::Name),
        ["absolution", "outlaw", "rampage"]
    );
    assert_eq!(
        order_by(EffectOrder::Kind),
        ["absolution", "outlaw", "rampage"]
    );
}
