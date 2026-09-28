//! The assets Suggested lists first: the ones a player knows by name.
//!
//! An entry's first name is what the game calls it. Any later name is the native name the
//! packages give the same thing, which is how most of these assets are labelled, such as
//! Thermal Hammer for Hammer of Sol. A label belongs to an entry when it begins with one of
//! its names, so a raid's Sentinel Void Shield is not taken for the Sentinel's super and a
//! shared asset named after every super it serves leads none of them.

/// Supers first, then exotic weapons and famous weapon perks, then grenades, each in the order
/// Suggested lists them. A native name whose super is uncertain is listed under its own name.
pub(in crate::app::custom_perks::workbench) const LEAD: &[&[&str]] = &[
    &["nova bomb"],
    &["hammer of sol", "thermal hammer"],
    &["golden gun"],
    &["blade barrage", "thermal knives"],
    &["fist of havoc"],
    &["chaos reach", "arc beam"],
    &["stormtrance", "arc lightning"],
    &["daybreak", "thermal sword"],
    &["burning maul", "thermal maul"],
    &["shadowshot", "void bow"],
    &["spectral blades", "void blade"],
    &["sentinel shield", "void shield"],
    &["arc staff"],
    &["ward of dawn"],
    &["nova pulse"],
    &["sleeper simulant"],
    &["wardcliff coil"],
    &["thunderlord"],
    &["gjallarhorn"],
    &["outbreak perfected", "outbreak"],
    &["tractor cannon"],
    &["truth"],
    &["two-tailed fox"],
    &["deathbringer"],
    &["graviton lance"],
    &["riskrunner"],
    &["coldheart"],
    &["telesto"],
    &["anarchy"],
    &["whisper of the worm"],
    &["lord of wolves"],
    &["jötunn", "jotunn"],
    &["le monarque"],
    &["trinity ghoul"],
    &["wish-ender"],
    &["ace of spades"],
    &["hawkmoon"],
    &["the last word"],
    &["thorn"],
    &["sunshot"],
    &["witherhoard"],
    &["xenophage"],
    &["divinity"],
    &["arbalest"],
    &["merciless"],
    &["polaris lance"],
    &["prometheus lens"],
    &["legend of acrius"],
    &["black talon"],
    &["one thousand voices"],
    &["ruinous effigy"],
    &["eriana's vow"],
    &["izanagi's burden"],
    &["bad juju"],
    &["red death"],
    &["hard light"],
    &["mida multi-tool"],
    &["sweet business"],
    &["skyburner's oath"],
    &["leviathan's breath"],
    &["firefly"],
    &["dragonfly"],
    &["axion bolt"],
    &["pulse grenade"],
    &["flux grenade"],
    &["vortex grenade"],
    &["arcbolt grenade"],
    &["skip grenade"],
    &["storm grenade"],
    &["swarm grenade"],
    &["scatter grenade"],
    &["suppressor grenade"],
    &["magnetic grenade"],
    &["spike grenade"],
    &["voidwall grenade"],
    &["firebolt grenade"],
    &["fusion grenade"],
    &["incendiary grenade"],
    &["lightning grenade"],
    &["thermite grenade"],
    &["tripmine grenade"],
    &["solar grenade"],
    &["thermal flux"],
    &["thermal flare"],
    &["thermal prox"],
    &["solar flare"],
    &["flashbang"],
];

/// The entry a lowercase label begins with, and the name it began with.
fn lead(label: &str) -> Option<(usize, &'static [&'static str], &'static str)> {
    LEAD.iter().enumerate().find_map(|(rank, names)| {
        names
            .iter()
            .find(|name| {
                label.strip_prefix(**name).is_some_and(|rest| {
                    rest.chars()
                        .next()
                        .is_none_or(|next| !next.is_alphanumeric())
                })
            })
            .map(|name| (rank, *names, *name))
    })
}

/// Where Suggested places an asset by its lowercase label: its entry's place, or after them all.
pub(in crate::app::custom_perks::workbench) fn rank(label: &str) -> usize {
    lead(label).map_or(usize::MAX, |(rank, _, _)| rank)
}

/// The game's name for an asset whose lowercase label uses a native name for it, such as
/// Hammer of Sol for a Thermal Hammer Projectile. Search answers to it and the row shows it.
pub(super) fn game_name(label: &str) -> Option<String> {
    let (_, names, matched) = lead(label)?;
    let words = names[0].split(' ').collect::<Vec<_>>();
    (matched != names[0]).then(|| {
        words
            .iter()
            .enumerate()
            .map(|(index, word)| {
                sundial::package_authoring::sandbox_perk::nodes::title_word(
                    word,
                    index,
                    words.len(),
                )
            })
            .collect::<Vec<_>>()
            .join(" ")
    })
}
