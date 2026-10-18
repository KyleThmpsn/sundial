//! The native weapon type a source weapon is built on, read from the source's own names.
//! Every source weapon with the same words gets the same type, so no weapon is special-cased.

/// Phrases in priority order: a narrower phrase precedes any broader one it contains, so
/// "submachine" wins over "machine gun" and "grenade launcher" over "launcher".
const PHRASES: [(&str, &str); 30] = [
    ("submachine", "Submachine Gun"),
    ("shotgun", "Shotgun"),
    ("sniper", "Sniper Rifle"),
    ("grenade launcher", "Grenade Launcher"),
    ("rocket", "Rocket Launcher"),
    ("launcher", "Rocket Launcher"),
    ("machine gun", "Machine Gun"),
    ("machinegun", "Machine Gun"),
    ("chaingun", "Machine Gun"),
    ("linear fusion", "Linear Fusion Rifle"),
    ("railgun", "Linear Fusion Rifle"),
    ("laser", "Linear Fusion Rifle"),
    ("fusion", "Fusion Rifle"),
    ("precision rifle", "Scout Rifle"),
    ("trace", "Trace Rifle"),
    ("beam", "Trace Rifle"),
    ("pulse", "Pulse Rifle"),
    ("scout", "Scout Rifle"),
    ("marksman", "Scout Rifle"),
    ("battle rifle", "Scout Rifle"),
    ("hand cannon", "Hand Cannon"),
    ("magnum", "Hand Cannon"),
    ("revolver", "Hand Cannon"),
    ("sidearm", "Sidearm"),
    ("pistol", "Sidearm"),
    ("sword", "Sword"),
    ("blade", "Sword"),
    ("assault rifle", "Auto Rifle"),
    ("auto rifle", "Auto Rifle"),
    ("rifle", "Auto Rifle"),
];

/// Abbreviations that only count as whole words.
const WORDS: [(&str, &str); 7] = [
    ("smg", "Submachine Gun"),
    ("lmg", "Machine Gun"),
    ("hmg", "Machine Gun"),
    ("dmr", "Scout Rifle"),
    ("ar", "Auto Rifle"),
    ("gl", "Grenade Launcher"),
    ("bow", "Combat Bow"),
];

/// The native type named by source text such as a localized type, an item name or a tag path
/// leaf. Underscores and hyphens separate words.
pub fn from_text(text: &str) -> Option<&'static str> {
    let text = text.to_lowercase().replace(['_', '-'], " ");
    let words: Vec<&str> = text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let spaced = words.join(" ");
    PHRASES
        .iter()
        .find(|(phrase, _)| spaced.contains(phrase))
        .or_else(|| WORDS.iter().find(|(word, _)| words.contains(word)))
        .map(|(_, kind)| *kind)
}

/// The native type spelled exactly as `name`, such as a stored result of `from_text`.
pub fn canonical(name: &str) -> Option<&'static str> {
    PHRASES
        .iter()
        .chain(WORDS.iter())
        .map(|(_, kind)| *kind)
        .find(|kind| *kind == name)
}

/// The type most of `texts` name, when at least three do and they are a majority of those
/// naming any type. A source that groups strings by weapon category names its category this
/// way, while a mixed group names none.
pub fn by_majority<'a>(texts: impl IntoIterator<Item = &'a str>) -> Option<&'static str> {
    let mut votes = std::collections::BTreeMap::<&'static str, usize>::new();
    for kind in texts.into_iter().filter_map(from_text) {
        *votes.entry(kind).or_default() += 1;
    }
    let total: usize = votes.values().sum();
    votes
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .filter(|(_, count)| *count >= 3 && *count * 2 > total)
        .map(|(kind, _)| kind)
}
