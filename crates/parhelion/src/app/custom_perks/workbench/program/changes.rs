//! Short readings of the numeric rows saved on each private action.
use super::*;
use sundial::package_authoring::runtime::modifiers;
use sundial::package_authoring::sandbox_perk::program::{ModifierRow, ValueSource};

pub(super) struct Reading {
    pub title: String,
    pub lines: Vec<String>,
}

pub(super) fn reading(program: &Program, index: usize) -> Option<Reading> {
    let changes = program.action_changes(index)?;
    let dynamic = changes.driver.is_some();
    let mut groups: Vec<(u8, u32, bool, Vec<String>)> = Vec::new();
    for row in &changes.rows {
        let seconds = row.operation == 0 && seconds(row);
        let name = name(row);
        if let Some((_, _, _, names)) = groups.iter_mut().find(|(operation, amount, unit, _)| {
            (*operation, *amount, *unit) == (row.operation, row.amount_bits, seconds)
        }) {
            names.push(name);
        } else {
            groups.push((row.operation, row.amount_bits, seconds, vec![name]));
        }
    }
    let title = if changes.rows.iter().all(seconds) {
        "Change Timing".to_owned()
    } else {
        let names = changes.rows.iter().map(name).collect::<Vec<_>>();
        let verb = if changes.rows.iter().all(|row| row.operation == 1) {
            "Scale"
        } else {
            "Change"
        };
        format!("{verb} {}", join(&names))
    };
    let mut lines = groups
        .into_iter()
        .map(|(operation, amount, seconds, names)| {
            let amount = f32::from_bits(amount);
            let amount = if operation == 1 {
                format!("×{amount}")
            } else {
                format!("{amount:+}{}", if seconds { " s" } else { "" })
            };
            format!(
                "{}: {amount}{}",
                join(&names),
                if dynamic { " at full weight" } else { "" }
            )
        })
        .collect::<Vec<_>>();
    if let Some(driver) = changes.driver.filter(|driver| driver.uses_input) {
        let source = match driver.source {
            ValueSource::Stacks if has_counter(program) => "the effect's counter".into(),
            ValueSource::Stacks => "the effect's value".into(),
            ValueSource::MissingRounds => "missing magazine rounds".into(),
            ValueSource::MagazineRounds => "remaining magazine rounds".into(),
            ValueSource::NearbyEnemies => "nearby enemies".into(),
            ValueSource::NearbyAllies => "nearby allies".into(),
            source => format!("input {}", source.byte()),
        };
        lines.push(format!(
            "Strength follows {source}{}.",
            if driver.normalized {
                " (normalized)"
            } else {
                ""
            }
        ));
    }
    match changes.target {
        0 => {}
        1 => lines.push("Applies to the player.".into()),
        2 => lines.push("Applies to the triggering weapon.".into()),
        3 => lines.push("Applies to the other combatant.".into()),
        target => lines.push(format!("Attachment target {target}.")),
    }
    if changes.additional {
        lines.push("Additional private resource edits are preserved.".into());
    }
    Some(Reading { title, lines })
}

fn has_counter(program: &Program) -> bool {
    program
        .native_removal
        .as_ref()
        .is_some_and(|node| node.kind == 26)
        || program
            .native_trigger
            .as_ref()
            .is_some_and(|node| node.kind == 26)
        || program
            .alternative_removals
            .iter()
            .any(|node| node.kind == 26)
        || program
            .alternative_triggers
            .iter()
            .any(|node| node.kind == 26)
}

fn name(row: &ModifierRow) -> String {
    let input = modifiers::input_choices(i64::from(row.component))
        .iter()
        .find(|(input, _)| *input == i64::from(row.input));
    let name = input.map_or_else(
        || format!("Component {} / Input {}", row.component, row.input),
        |(_, name)| (*name).to_owned(),
    );
    if row.component != 9 {
        return name;
    }
    let ability = sundial::package_authoring::sandbox_perk::action::ability_slot(row.ability as u8)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Ability {}", row.ability));
    format!("{ability} / {name}")
}

fn seconds(row: &ModifierRow) -> bool {
    matches!((row.component, row.input), (2, 4 | 5) | (3 | 4, 0..=2))
}

fn join(names: &[String]) -> String {
    match names.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, first)) => format!("{} and {last}", first.join(", ")),
    }
}
