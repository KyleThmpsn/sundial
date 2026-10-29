//! The tracking settings of a projectile a perk fires: its turn rate, how it leads a target and
//! the strength programs that hold a literal. They are the settings the Subclass editor offers on
//! an ability's parts, read and written through the same native places.
use super::*;
use sundial::package_authoring::ability_settings::{self, Kind, Setting, Unit};

/// The settings a tracking definition (`8080377D`) holds, in the order a card shows them.
const KINDS: [Kind; 15] = [
    Kind::TurnRate,
    Kind::TurnRateOffset,
    Kind::FastThrowTracking,
    Kind::TrackingStrength,
    Kind::DistanceTracking,
    Kind::BounceTracking,
    Kind::TrackingVariation,
    Kind::TrackingSpeed,
    Kind::LeadTimeLimit,
    Kind::LeadDistanceLimit,
    Kind::TargetLead,
    Kind::SteeringAxisThreshold,
    Kind::ProximityRange,
    Kind::MinimumProximityTime,
    Kind::MaximumProximityTime,
];

/// Every tracking setting of `graphs`, by graph. The settings reader withholds a lane another
/// reader replaces, such as Turn Rate under a turn-rate program, and one nothing reads.
pub(super) fn discover(
    manager: &sundial::package_authoring::PackageManager,
    graphs: &[(u32, WeaponRuntimeGraph)],
) -> Vec<(u32, Setting)> {
    let mut found = graphs
        .iter()
        .flat_map(|(tag, graph)| {
            ability_settings::discover(manager, graph)
                .into_iter()
                .filter(|setting| KINDS.contains(&setting.kind))
                .map(move |setting| (*tag, setting))
        })
        .collect::<Vec<_>>();
    found.sort_by_key(|(tag, setting)| {
        (
            *tag,
            setting.owner_tag,
            KINDS.iter().position(|kind| *kind == setting.kind),
        )
    });
    found
}

/// The tracking settings of the graphs `loaded` fires.
pub(in crate::app::custom_perks) fn mapped(
    loaded: &PrivatePerkRuntimeGraph,
) -> Vec<&(u32, Setting)> {
    loaded
        .tracking
        .iter()
        .filter(|(tag, _)| movement::fires(loaded, *tag))
        .collect()
}

/// `value` as the tile shows it.
pub(super) fn text(kind: Kind, value: f32) -> String {
    let (prefix, suffix) = affixes(kind);
    // Degrees come from radians, so a stock 2π reads as 360 rather than 359.99997.
    let number = format!("{value:.3}");
    let number = number.trim_end_matches('0').trim_end_matches('.');
    format!("{prefix}{number}{suffix}")
}

fn affixes(kind: Kind) -> (&'static str, &'static str) {
    if matches!(
        kind,
        Kind::TurnRate | Kind::FastThrowTracking | Kind::TurnRateOffset
    ) {
        return ("", "°/s");
    }
    match kind.unit() {
        Unit::Factor => ("×", ""),
        Unit::Seconds => ("", " s"),
        Unit::Distance => ("", " Units"),
        _ => ("", ""),
    }
}

/// Draws the tracking tiles of the projectile `tag`, writing their edits into `draft`.
pub(in crate::app::custom_perks) fn draw(
    ui: &mut egui::Ui,
    loaded: &PrivatePerkRuntimeGraph,
    tag: u32,
    draft: &mut Vec<WeaponRuntimeValueOverride>,
) -> Option<Result<(), String>> {
    let settings = mapped(loaded)
        .into_iter()
        .filter(|(graph, _)| *graph == tag)
        .collect::<Vec<_>>();
    if settings.is_empty() {
        return None;
    }
    crate::app::style::tiles(ui, |ui, width| {
        let mut result = None;
        for (_, setting) in settings {
            if let Some(edited) = draw_setting(ui, width, tag, setting, draft) {
                result = Some(edited);
            }
        }
        result
    })
}

fn draw_setting(
    ui: &mut egui::Ui,
    width: f32,
    tag: u32,
    setting: &Setting,
    draft: &mut Vec<WeaponRuntimeValueOverride>,
) -> Option<Result<(), String>> {
    let kind = setting.kind;
    let label = kind.label();
    let (low, high) = kind.range();
    let (prefix, suffix) = affixes(kind);
    let mut value = setting.value(draft);
    let (changed, reset) = crate::app::style::tile(
        ui,
        width,
        ("tracking-setting", tag, setting.owner_tag, label),
        label,
        kind.hint(),
        setting.is_modified(draft),
        |ui| {
            let response = ui
                .add_sized(
                    [ui.available_width(), ui.spacing().interact_size.y],
                    egui::DragValue::new(&mut value)
                        .speed(((high - low) / 2000.0).max(0.001))
                        .max_decimals(3)
                        .range(low..=high)
                        .clamp_existing_to_range(false)
                        .prefix(prefix)
                        .suffix(suffix),
                )
                .on_hover_text(format!("Original: {}", text(kind, setting.stock())));
            let changed = response.changed();
            crate::app::style::named_control(response, label);
            changed
        },
    );
    if reset {
        setting.reset(draft);
        Some(Ok(()))
    } else if changed {
        Some(setting.set(draft, value))
    } else {
        None
    }
}
