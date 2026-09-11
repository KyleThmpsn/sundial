use super::*;

#[test]
fn labels_distinguish_native_identity_from_parent_and_perk_context() {
    let mut entry = Entry {
        graph: 7,
        kind: Kind::Projectile,
        owners: Vec::new(),
        package: "test".into(),
        native_name: None,
        native_paths: Vec::new(),
        contexts: Vec::new(),
        perk_indices: Vec::new(),
    };
    assert_eq!(entry.label(), "Unidentified Projectile");
    assert_eq!(entry.label_rank(), 3);
    entry.perk_indices = vec![1178];
    assert_eq!(
        entry.label_with_perks(|_| Some("Micro-Missile".into())),
        "Projectile · Used by Micro-Missile"
    );
    assert_eq!(entry.label(), "Projectile · Used by Effect 1178");
    entry.contexts.push(Context {
        graph: 10,
        owner: 11,
        offset: 12,
        path: "content/solar_strike.pattern.tft".into(),
    });
    assert_eq!(
        entry.label(),
        "Projectile · Referenced by solar_strike.pattern.tft"
    );
    assert_eq!(entry.label_rank(), 1);
    // Repeated native references do not imply multiple distinct identities.
    entry.contexts.push(entry.contexts[0].clone());
    assert!(!entry.label().contains("(+"));
    entry.native_name = Some("Engine Asset".into());
    assert_eq!(entry.label(), "Engine Asset");
    entry.native_paths = vec!["content/solar_strike_projectile.pattern.tft".into()];
    assert_eq!(entry.label(), "solar_strike_projectile.pattern.tft");
    assert_eq!(entry.label_rank(), 0);
}
