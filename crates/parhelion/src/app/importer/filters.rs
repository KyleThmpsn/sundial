//! Source catalog filters. These values do not certify converted gameplay.
use super::*;

#[derive(Clone, Default, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub(super) struct Filters {
    family: Option<service::Family>,
    bucket: Option<u32>,
    class: Option<u8>,
    rarity: Option<u8>,
    ammo: Option<u16>,
    damage: Option<String>,
}

impl Filters {
    pub fn active(&self) -> bool {
        self.family.is_some()
            || self.bucket.is_some()
            || self.class.is_some()
            || self.rarity.is_some()
            || self.ammo.is_some()
            || self.damage.is_some()
    }

    pub fn matches(&self, item: &Weapon) -> bool {
        self.family.is_none_or(|value| item.family() == value)
            && self
                .bucket
                .is_none_or(|value| item.bucket_hash == Some(value))
            && self
                .class
                .is_none_or(|value| item.class_type == Some(value))
            && self.rarity.is_none_or(|value| item.rarity == Some(value))
            && self.ammo.is_none_or(|value| item.ammo == Some(value))
            && self
                .damage
                .as_ref()
                .is_none_or(|value| item.damage.as_ref() == Some(value))
    }
}

const FAMILIES: [service::Family; 7] = [
    service::Family::Weapon,
    service::Family::Armor,
    service::Family::GhostShell,
    service::Family::Ship,
    service::Family::Sparrow,
    service::Family::Shader,
    service::Family::Emblem,
];

const SLOTS: [(u32, &str); 12] = [
    (1498876634, "Kinetic"),
    (2465295065, "Energy"),
    (953998645, "Power"),
    (3448274439, "Helmet"),
    (3551918588, "Gauntlets"),
    (14239492, "Chest Armor"),
    (20886954, "Leg Armor"),
    (1585787867, "Class Armor"),
    (4023194814, "Ghost Shell"),
    (284967655, "Ship"),
    (2025709351, "Sparrow"),
    (4274335291, "Emblem"),
];

const CLASSES: [(u8, &str); 3] = [(0, "Titan"), (1, "Hunter"), (2, "Warlock")];
const RARITIES: [(u8, &str); 5] = [
    (1, "Common"),
    (2, "Uncommon"),
    (3, "Rare"),
    (4, "Legendary"),
    (5, "Exotic"),
];
const AMMO: [(u16, &str); 3] = [(1, "Primary"), (2, "Special"), (3, "Heavy")];
const DAMAGE: [&str; 6] = ["kinetic", "arc", "solar", "void", "stasis", "strand"];

fn damage_label(value: &str) -> &str {
    match value {
        "kinetic" => "Kinetic",
        "arc" => "Arc",
        "solar" => "Solar",
        "void" => "Void",
        "stasis" => "Stasis",
        "strand" => "Strand",
        _ => value,
    }
}

fn label<T: PartialEq>(options: &[(T, &'static str)], value: Option<T>) -> Option<&'static str> {
    value.and_then(|value| {
        options
            .iter()
            .find(|(key, _)| *key == value)
            .map(|(_, name)| *name)
    })
}

/// Keep the same labelled compact dropdowns as the donor and ornament pickers.
fn choice<T: Clone + PartialEq>(
    ui: &mut egui::Ui,
    salt: &str,
    name: &str,
    title: &str,
    selected: &mut Option<T>,
    options: impl IntoIterator<Item = (T, String)>,
    width: f32,
) -> bool {
    let options: Vec<_> = options.into_iter().collect();
    let value = selected
        .as_ref()
        .and_then(|value| options.iter().find(|(key, _)| key == value))
        .map_or("Any", |(_, text)| text.as_str());
    let mut changed = false;
    let response = egui::ComboBox::from_id_salt(salt)
        .width(width)
        .truncate()
        .selected_text(format!("{title}: {value}"))
        .show_ui(ui, |ui| {
            changed |= ui.selectable_value(selected, None, "Any").changed();
            for (key, text) in options {
                changed |= ui.selectable_value(selected, Some(key), text).changed();
            }
        })
        .response;
    super::super::pickers::name_response(ui, &response, name);
    changed
}

pub(super) fn draw(ui: &mut egui::Ui, browser: &mut browser::Browser, items: &[Weapon]) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        style::compact_controls(ui);
        let filters = &mut browser.filters;
        if choice(
            ui,
            "import-kind",
            "Item Kind",
            "Kind",
            &mut filters.family,
            FAMILIES
                .into_iter()
                .map(|value| (value, value.label().to_owned())),
            142.0,
        ) {
            // Changing families must not leave hidden weapon or armor constraints active.
            browser.kind.clear();
            filters.bucket = None;
            filters.class = None;
            filters.ammo = None;
            filters.damage = None;
            changed = true;
        }
        let mut types = BTreeSet::<&str>::new();
        for item in items
            .iter()
            .filter(|item| filters.family.is_none_or(|family| item.family() == family))
        {
            if !item.weapon_type.is_empty() {
                types.insert(item.weapon_type.as_str());
            }
        }
        let mut kind = (!browser.kind.is_empty()).then(|| browser.kind.clone());
        if choice(
            ui,
            "import-type",
            "Item Type",
            "Type",
            &mut kind,
            types
                .iter()
                .map(|value| (value.to_string(), value.to_string())),
            178.0,
        ) {
            browser.kind = kind.unwrap_or_default();
            changed = true;
        }
        changed |= choice(
            ui,
            "import-slot",
            "Equipment Slot",
            "Slot",
            &mut filters.bucket,
            SLOTS
                .into_iter()
                .filter(|(bucket, _)| {
                    filters
                        .family
                        .is_none_or(|family| service::Family::from_bucket(*bucket) == Some(family))
                })
                .map(|(value, name)| (value, name.to_owned())),
            142.0,
        );
        changed |= choice(
            ui,
            "import-rarity",
            "Rarity",
            "Rarity",
            &mut filters.rarity,
            RARITIES
                .into_iter()
                .map(|(value, name)| (value, name.to_owned())),
            142.0,
        );
    });
    ui.horizontal_wrapped(|ui| {
        style::compact_controls(ui);
        let filters = &mut browser.filters;
        if filters
            .family
            .is_none_or(|family| family == service::Family::Weapon)
        {
            changed |= choice(
                ui,
                "import-damage",
                "Damage Type",
                "Damage",
                &mut filters.damage,
                DAMAGE
                    .into_iter()
                    .map(|value| (value.to_owned(), damage_label(value).to_owned())),
                142.0,
            );
            changed |= choice(
                ui,
                "import-ammo",
                "Ammo Type",
                "Ammo",
                &mut filters.ammo,
                AMMO.into_iter()
                    .map(|(value, name)| (value, name.to_owned())),
                142.0,
            );
        }
        if filters
            .family
            .is_none_or(|family| family == service::Family::Armor)
        {
            changed |= choice(
                ui,
                "import-class",
                "Armor Class",
                "Class",
                &mut filters.class,
                CLASSES
                    .into_iter()
                    .map(|(value, name)| (value, name.to_owned())),
                142.0,
            );
        }
        changed |= ui
            .checkbox(&mut browser.show_installed, "Show Installed")
            .on_hover_text("Include items already present in the target installation.")
            .changed();
        changed |= ui
            .checkbox(&mut browser.show_dummy, "Include Dummy Items")
            .on_hover_text("Include display-only definitions. Some lack the data needed to import.")
            .changed();
    });
    changed
}

pub(super) fn details(item: &Weapon) -> String {
    let mut parts = vec![item.weapon_type.as_str()];
    if let Some(class) = label(&CLASSES, item.class_type) {
        parts.push(class);
    }
    if let Some(rarity) = label(&RARITIES, item.rarity) {
        parts.push(rarity);
    }
    if let Some(damage) = item.damage.as_deref() {
        parts.push(damage_label(damage));
    }
    if let Some(ammo) = label(&AMMO, item.ammo) {
        parts.push(ammo);
    }
    if item.dummy {
        parts.push("Dummy");
    }
    parts.join(" · ")
}
