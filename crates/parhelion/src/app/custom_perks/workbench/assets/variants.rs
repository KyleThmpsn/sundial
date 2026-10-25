//! Rows that belong together, listed as one row that opens to them: assets that differ only by
//! a variant number, and the effects of one stock perk in Add from Perk.
//!
//! Discovery numbers the assets that share a name as "Name · Variant N". A picker lists each
//! such family once, where its first result falls, and opening it lists the variants in number
//! order. Grouping only folds rows, so every result stays one click away.
use std::collections::{BTreeMap, BTreeSet};

/// A family's name and an asset's number in it.
fn variant(label: &str) -> Option<(&str, u32)> {
    let (name, number) = label.rsplit_once(" · Variant ")?;
    Some((name, number.parse().ok()?))
}

/// Results that belong together, by their positions in the result list.
#[derive(Clone)]
pub(in crate::app::custom_perks::workbench) struct Group {
    pub name: String,
    /// What the family is known by while the list changes, such as its name's hash or the
    /// plug its effects come from. Two families never share one.
    pub key: u64,
    /// The members' positions, in variant order. A group of one is an asset alone.
    pub members: Vec<usize>,
}

/// The families of an ordered result list, each where its first result falls.
pub(in crate::app::custom_perks::workbench) fn group<'a>(
    labels: impl IntoIterator<Item = &'a str>,
) -> Vec<Group> {
    let mut groups = Vec::<Group>::new();
    let mut families = BTreeMap::<&str, usize>::new();
    let mut numbers = Vec::new();
    for (position, label) in labels.into_iter().enumerate() {
        let (name, number) = variant(label).unwrap_or((label, 0));
        let family = if number == 0 {
            None
        } else {
            families.get(name).copied()
        };
        if let Some(family) = family {
            groups[family].members.push(position);
        } else {
            if number != 0 {
                families.insert(name, groups.len());
            }
            groups.push(Group {
                name: name.to_owned(),
                key: egui::Id::new(("asset-family", name)).value(),
                members: vec![position],
            });
        }
        numbers.push(number);
    }
    for group in &mut groups {
        group.members.sort_by_key(|position| numbers[*position]);
    }
    groups
}

/// One row a picker shows.
#[derive(Clone, Copy)]
pub(in crate::app::custom_perks::workbench) enum Shown<'a> {
    /// An asset alone under its name, by its position in the results.
    Asset(usize),
    /// A family of variants, and whether it is open.
    Family(&'a Group, bool),
    /// One variant of an open family, by its position in the results.
    Variant(usize),
}

impl Shown<'_> {
    /// The asset a row stands for: its own, or a family's first variant.
    pub(in crate::app::custom_perks::workbench) fn position(self) -> usize {
        match self {
            Self::Asset(position) | Self::Variant(position) => position,
            Self::Family(group, _) => group.members[0],
        }
    }

    /// A key that stays with the row while the list changes: the member's own, or the
    /// family's with the top bit set, which no member's 32-bit key can equal.
    pub(in crate::app::custom_perks::workbench) fn key(self, member: impl Fn(usize) -> u32) -> u64 {
        match self {
            Self::Asset(position) | Self::Variant(position) => u64::from(member(position)),
            Self::Family(group, _) => group.key | (1 << 63),
        }
    }
}

/// The rows to show, each family closed unless `open` holds its key.
pub(in crate::app::custom_perks::workbench) fn shown<'a>(
    groups: &'a [Group],
    open: &BTreeSet<u64>,
) -> Vec<Shown<'a>> {
    let mut rows = Vec::new();
    for group in groups {
        if let [only] = group.members.as_slice() {
            rows.push(Shown::Asset(*only));
            continue;
        }
        let opened = open.contains(&group.key);
        rows.push(Shown::Family(group, opened));
        if opened {
            rows.extend(
                group
                    .members
                    .iter()
                    .map(|position| Shown::Variant(*position)),
            );
        }
    }
    rows
}

/// The open families of one picker.
pub(in crate::app::custom_perks::workbench) fn opened(
    ctx: &egui::Context,
    id: egui::Id,
) -> BTreeSet<u64> {
    ctx.data(|data| data.get_temp::<BTreeSet<u64>>(id))
        .unwrap_or_default()
}

/// The family holding the asset at `position`, when the asset is one of several variants.
pub(in crate::app::custom_perks::workbench) fn family_of(
    groups: &[Group],
    position: usize,
) -> Option<&Group> {
    groups
        .iter()
        .find(|group| group.members.len() > 1 && group.members.contains(&position))
}

/// Opens a family, as when the picker reveals one of its variants.
pub(in crate::app::custom_perks::workbench) fn open(ctx: &egui::Context, id: egui::Id, key: u64) {
    let mut open = opened(ctx, id);
    if open.insert(key) {
        ctx.data_mut(|data| data.insert_temp(id, open));
    }
}

/// Opens a closed family or closes an open one.
pub(in crate::app::custom_perks::workbench) fn toggle(ctx: &egui::Context, id: egui::Id, key: u64) {
    let mut open = opened(ctx, id);
    if !open.remove(&key) {
        open.insert(key);
    }
    ctx.data_mut(|data| data.insert_temp(id, open));
}

/// A family row's title and the line under it, from its first variant's source line with that
/// variant's tag left off.
pub(in crate::app::custom_perks::workbench) fn family_text(
    group: &Group,
    source: &str,
) -> (String, String) {
    let source = source
        .rsplit_once(" · 0x")
        .map_or(source, |(source, _)| source);
    (
        group.name.clone(),
        format!("{} Variants · {source}", group.members.len()),
    )
}

/// The room at a family row's right end that its disclosure triangle takes, which the row's
/// text leaves free.
const CARET_ROOM: f32 = 22.0;

/// Draws a family row narrower by the caret's room, so a long second line ends before the
/// caret rather than running under it.
pub(in crate::app::custom_perks::workbench) fn with_caret_room(
    ui: &mut egui::Ui,
    row: impl FnOnce(&mut egui::Ui) -> egui::Response,
) -> egui::Response {
    let width = (ui.available_width() - CARET_ROOM).max(0.0);
    ui.allocate_ui_with_layout(
        egui::vec2(width, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(width);
            row(ui)
        },
    )
    .inner
}

/// Whether the keyboard asked to open a closed family or close an open one: Right and Left
/// on the selected family row, while the picker holding it is the top window.
pub(in crate::app::custom_perks::workbench) fn keyboard_toggle(
    ui: &egui::Ui,
    selected: bool,
    open: bool,
) -> bool {
    if !selected || egui::Popup::is_any_open(ui) {
        return false;
    }
    let top = ui.ctx().memory(|memory| {
        memory
            .layer_ids()
            .filter(|layer| layer.order != egui::Order::Tooltip && memory.areas().is_visible(layer))
            .last()
    });
    if top != Some(ui.layer_id()) {
        return false;
    }
    let key = if open {
        egui::Key::ArrowLeft
    } else {
        egui::Key::ArrowRight
    };
    ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, key))
}

/// Paints a family row's disclosure triangle in the room past its right end, pointing down
/// while it is open. It is painted rather than typed, because the row's font has no arrow
/// glyphs.
pub(in crate::app::custom_perks::workbench) fn paint_caret(
    ui: &egui::Ui,
    row: &egui::Response,
    open: bool,
    selected: bool,
) {
    let center = egui::pos2(row.rect.right() + CARET_ROOM * 0.5, row.rect.center().y);
    let points = if open {
        [(-5.0, -2.5), (5.0, -2.5), (0.0, 3.5)]
    } else {
        [(-2.5, -5.0), (-2.5, 5.0), (3.5, 0.0)]
    }
    .map(|(x, y)| center + egui::vec2(x, y))
    .to_vec();
    let color = ui
        .style()
        .interact_selectable(row, selected)
        .fg_stroke
        .color;
    ui.painter().add(egui::Shape::convex_polygon(
        points,
        color,
        egui::Stroke::NONE,
    ));
}

/// A variant row's title under its open family.
pub(in crate::app::custom_perks::workbench) fn variant_title(label: &str) -> String {
    variant(label).map_or_else(
        || label.to_owned(),
        |(_, number)| format!("Variant {number}"),
    )
}

/// Draws a variant row indented under its family.
pub(in crate::app::custom_perks::workbench) fn indented(
    ui: &mut egui::Ui,
    row: impl FnOnce(&mut egui::Ui) -> egui::Response,
) -> egui::Response {
    ui.horizontal(|ui| {
        ui.add_space(20.0);
        row(ui)
    })
    .inner
}
