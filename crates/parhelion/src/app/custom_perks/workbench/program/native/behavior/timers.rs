//! The Timing row's timer tiles, and the timer conditions they read and write.
use super::*;

const DURATION_HINT: &str =
    "How long this effect stays active. Zero keeps it until the perk is removed.";
const EVENT_DURATION_HINT: &str =
    "How long this effect stays active. At Once lets every trigger fire it again.";
const COOLDOWN_HINT: &str = "The delay before this effect can trigger again.";
const REPEAT_HINT: &str = "The interval at which an always-active effect runs its actions again.";

/// Duration and Cooldown, or the Repeat Interval of an effect nothing triggers, as tiles.
pub(super) fn timing_row(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    group_index: usize,
    group: &action::DecodedGroup,
    always: bool,
    pending: &mut Option<(List, Edit)>,
) -> Result<(), String> {
    // An effect an event fires ends at once at zero, as stock kill perks whose actions happen
    // once do. With no ending it would run once and never again.
    let event = validation::event_fired(group);
    canvas::row(ui, "Timing", "", |ui| {
        crate::app::style::tiles(ui, |ui, width| {
            if !always {
                timer_tile(
                    ui,
                    graph,
                    width,
                    List::group(group_index, Part::Ending),
                    &group.removal,
                    (
                        "Duration",
                        if event {
                            EVENT_DURATION_HINT
                        } else {
                            DURATION_HINT
                        },
                    ),
                    Timer::Duration { event },
                    pending,
                )?;
            }
            let (name, hint) = if always {
                ("Repeat Interval", REPEAT_HINT)
            } else {
                ("Cooldown", COOLDOWN_HINT)
            };
            timer_tile(
                ui,
                graph,
                width,
                List::group(group_index, Part::Rearm),
                &group.rearm,
                (name, hint),
                Timer::Rearm,
                pending,
            )
        })
    })
}

/// Which timer a tile sets.
#[derive(Clone, Copy)]
enum Timer {
    /// The ending. Where an event fires the effect, zero ends it at once.
    Duration {
        event: bool,
    },
    Rearm,
}

/// One timer as a tile. Zero removes the timer, or ends at once where an event fires the effect,
/// and a value on an empty list, or on an ending at once, sets one.
#[allow(clippy::too_many_arguments)]
fn timer_tile(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    width: f32,
    list: List,
    entries: &[DecodedCondition],
    (name, hint): (&str, &str),
    timer: Timer,
    pending: &mut Option<(List, Edit)>,
) -> Result<(), String> {
    if entries.is_empty() || at_once(entries) {
        if let Some(seconds) = unset_timer(ui, width, (name, hint), at_once(entries)) {
            let node = timer_node(seconds, matches!(timer, Timer::Rearm))?;
            *pending = Some((
                list,
                if entries.is_empty() {
                    Edit::Add(node)
                } else {
                    Edit::Replace(0, node)
                },
            ));
        }
        return Ok(());
    }
    let path = list.node(0)?;
    let cleared = structure::scoped(graph, &path, |graph, index| {
        let class = graph.blocks[index].class;
        let field = fields::describe(class)?
            .into_iter()
            .find(|field| field.label == "Duration")
            .ok_or("The timer duration field is missing.")?;
        let control = super::super::tile_width(class, &field, width).unwrap_or(width);
        let before = graph.blocks[index].bytes.clone();
        crate::app::style::tile(ui, width, name, name, hint, false, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().interact_size.x = control;
                super::super::scalar(ui, &field, &mut graph.blocks[index], 0)
            })
            .inner
        })
        .0?;
        // Only setting zero removes the timer. A zero timer that is only drawn stays, since
        // a stock effect or a newly added condition may hold one.
        Ok(graph.blocks[index].bytes != before
            && super::super::timer_seconds(&graph.blocks[index]) == Some(0.0))
    })?;
    if cleared {
        *pending = Some((
            list,
            match timer {
                Timer::Duration { event: true } => Edit::Replace(0, structure::always()?),
                _ => Edit::Remove(0),
            },
        ));
    }
    Ok(())
}

/// A timer tile with no timer yet, reading zero, or At Once for an ending at once. Returns the
/// seconds a change sets.
fn unset_timer(
    ui: &mut egui::Ui,
    width: f32,
    (name, hint): (&str, &str),
    ends_at_once: bool,
) -> Option<f32> {
    let mut seconds = 0.0_f32;
    // The same widgets, in the same order and at the same width, as the tile of a set timer,
    // so the drag that adds the timer carries on into the timer it added instead of stopping
    // at its first tick when the tile is redrawn around the new condition.
    let control = nodes::condition(1)
        .and_then(|kind| {
            let field = fields::describe(kind.class)
                .ok()?
                .into_iter()
                .find(|field| field.label == "Duration")?;
            super::super::tile_width(kind.class, &field, width)
        })
        .unwrap_or(width);
    let (changed, _) = crate::app::style::tile(ui, width, name, name, hint, false, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().interact_size.x = control;
            let response = ui.add(
                egui::DragValue::new(&mut seconds)
                    .range(0.0..=3600.0)
                    .clamp_existing_to_range(false)
                    .custom_formatter(|value, _| {
                        if ends_at_once && value == 0.0 {
                            "At Once".to_owned()
                        } else {
                            seconds_text(value)
                        }
                    })
                    .custom_parser(|text| {
                        let text = text.trim();
                        if text.eq_ignore_ascii_case("at once") {
                            Some(0.0)
                        } else {
                            text.parse().ok()
                        }
                    })
                    .suffix(if ends_at_once { "" } else { " s" }),
            );
            pickers::name_response(ui, &response, name);
            response.changed()
        })
        .inner
    });
    (changed && seconds > 0.0).then_some(seconds)
}

/// A timer condition's seconds, sized for the value, on the line beside its title. Any other
/// condition kind draws nothing here.
pub(super) fn timer_seconds_inline(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    index: usize,
    kind: u8,
) -> Result<(), String> {
    if kind != 1 {
        return Ok(());
    }
    let class = graph.blocks[index].class;
    let field = fields::describe(class)?
        .into_iter()
        .find(|field| field.label == "Duration")
        .ok_or("The timer duration field is missing.")?;
    // An allocation wraps with the condition's line where a scope would not.
    sized(ui, 64.0, |ui| {
        ui.spacing_mut().interact_size.x = 64.0;
        super::super::scalar(ui, &field, &mut graph.blocks[index], 0)
    })
}

/// Seconds with no trailing zeros, as a set timer reads.
pub(super) fn seconds_text(value: f64) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// A timer condition of the given length, as the compiler writes one for a guided Duration or
/// Cooldown.
fn timer_node(seconds: f32, rearm: bool) -> Result<NativeNode, String> {
    let millis = ((seconds * 1000.0).round() as u32).max(1);
    let draft = Program {
        trigger: Trigger::WeaponKill,
        duration_ms: millis,
        cooldown_ms: if rearm { millis } else { 0 },
        ..Program::default()
    };
    let payload = sundial::package_authoring::sandbox_perk::program::native_draft(&draft)?
        .graph
        .emit()?;
    let decoded = action::decode(&payload)?;
    let group = decoded
        .groups
        .first()
        .ok_or("The timer could not be built.")?;
    let node = if rearm {
        group.rearm.first()
    } else {
        group.removal.first()
    }
    .ok_or("The timer could not be built.")?;
    Ok(NativeNode {
        kind: node.kind,
        bytes: node.native.clone(),
    })
}
