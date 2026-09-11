//! Compact reward tiles with claim status and details on demand.

use eframe::egui;

use crate::{
    app::{
        glyphs::{self, Glyph},
        inspector,
        progression::CollectionStateSnapshot,
    },
    catalog::{Catalog, ProgressionRewardDefinition},
};

pub(super) const HEIGHT: f32 = 112.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Status {
    Claimed,
    Reached,
    Locked,
    Unknown,
}

impl Status {
    pub(super) fn from_claim(claimed: Option<bool>, rank: Option<i32>, required: i32) -> Self {
        match (claimed, rank) {
            (Some(true), _) => Self::Claimed,
            (_, Some(rank)) if rank < required => Self::Locked,
            (Some(false), Some(_)) => Self::Reached,
            _ => Self::Unknown,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Claimed => "Claimed",
            Self::Reached => "Rank Reached",
            Self::Locked => "Rank Locked",
            Self::Unknown => "Claim State Unavailable",
        }
    }

    fn help(self) -> &'static str {
        match self {
            Self::Claimed => "This reward is marked as claimed in the account.",
            Self::Reached => "The required rank has been reached. Claim this reward in Sunrise.",
            Self::Locked => "Increase seasonal XP to reach this reward's rank.",
            Self::Unknown => "The saved claim state could not be read for this reward.",
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Reward<'a> {
    pub index: usize,
    pub definition: &'a ProgressionRewardDefinition,
    pub status: Status,
}

pub(super) fn draw_rank(ui: &mut egui::Ui, width: f32, level: i32, current: Option<i32>) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 28.0), egui::Sense::hover());
    let reached = current.is_some_and(|rank| rank >= level);
    let accent = ui.visuals().selection.bg_fill;
    if current == Some(level) {
        ui.painter()
            .rect_filled(rect, 4.0, accent.gamma_multiply(0.3));
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        level.to_string(),
        egui::FontId::proportional(14.0),
        if reached {
            ui.visuals().strong_text_color()
        } else {
            ui.visuals().weak_text_color()
        },
    );
    ui.painter().line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        egui::Stroke::new(
            2.0,
            if reached {
                accent
            } else {
                ui.visuals().widgets.noninteractive.bg_stroke.color
            },
        ),
    );
    response.on_hover_text(if current == Some(level) {
        format!("Rank {level} · Current Rank")
    } else {
        format!("Rank {level}")
    });
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    reward: Reward<'_>,
    width: f32,
    selected: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, HEIGHT), egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let name = catalog
        .package_item_name(reward.definition.item_hash)
        .unwrap_or("Unnamed Reward");
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Button,
            true,
            selected,
            format!(
                "{name}, Rank {}, {}",
                reward.definition.rewarded_at_progression_level,
                reward.status.label()
            ),
        )
    });
    let reached = reward.status == Status::Reached;
    let accent = ui.visuals().selection.bg_fill;
    let fill = if selected {
        accent.gamma_multiply(0.24)
    } else if reached {
        accent.gamma_multiply(0.1)
    } else {
        ui.visuals().faint_bg_color
    };
    let stroke = if selected || response.has_focus() {
        egui::Stroke::new(2.0, accent)
    } else if response.hovered() {
        ui.visuals().widgets.hovered.bg_stroke
    } else if reached {
        egui::Stroke::new(1.0, accent.gamma_multiply(0.7))
    } else {
        ui.visuals().widgets.noninteractive.bg_stroke
    };
    ui.painter()
        .rect(rect, 4.0, fill, stroke, egui::StrokeKind::Inside);
    let icon = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + 35.0),
        egui::vec2(52.0, 52.0),
    );
    draw_icon(
        ui,
        catalog,
        reward.definition.item_hash,
        icon,
        reward.status == Status::Locked,
    );
    paint_status(ui, rect, reward.status);
    if reward.definition.quantity != 1 {
        let text = format!("×{}", reward.definition.quantity);
        let galley = ui.painter().layout_no_wrap(
            text,
            egui::FontId::proportional(11.0),
            ui.visuals().strong_text_color(),
        );
        let badge = egui::Rect::from_min_size(
            egui::pos2(rect.right() - galley.size().x - 10.0, icon.bottom() - 9.0),
            galley.size() + egui::vec2(6.0, 2.0),
        );
        ui.painter()
            .rect_filled(badge, 3.0, ui.visuals().extreme_bg_color);
        ui.painter().galley(
            badge.min + egui::vec2(3.0, 1.0),
            galley,
            ui.visuals().strong_text_color(),
        );
    }
    let color = if reward.status == Status::Locked {
        ui.visuals().weak_text_color()
    } else {
        ui.visuals().text_color()
    };
    let mut text = egui::text::LayoutJob::simple(
        name.into(),
        egui::FontId::proportional(11.5),
        color,
        width - 12.0,
    );
    text.halign = egui::Align::Center;
    text.wrap.max_rows = 3;
    let galley = ui.fonts(|fonts| fonts.layout_job(text));
    ui.painter().galley(
        egui::pos2(rect.center().x, rect.top() + 68.0),
        galley,
        color,
    );
    response.context_menu(|ui| {
        inspector::draw_catalog_hash_link(
            ui,
            catalog,
            reward.definition.item_hash,
            "Inspect Reward",
        );
        if let Some(flag) = reward
            .definition
            .claim_flag
            .and_then(|index| catalog.unlock_flag_definition(usize::from(index)))
        {
            inspector::draw_catalog_hash_link(ui, catalog, flag.hash, "Inspect Claim State");
        }
    });
    response.on_hover_ui(|ui| {
        ui.set_max_width(300.0);
        crate::ui_help::tooltip_title(ui, name);
        ui.label(format!(
            "Rank {} · Quantity {}",
            reward.definition.rewarded_at_progression_level, reward.definition.quantity
        ));
        ui.label(reward.status.help());
    })
}

fn draw_icon(ui: &egui::Ui, catalog: &Catalog, hash: u64, rect: egui::Rect, dimmed: bool) {
    ui.painter()
        .rect_filled(rect, 3.0, crate::app::ui::package_icon_backdrop(ui));
    if let Some(icon) = catalog.icon_texture(ui.ctx(), hash) {
        ui.painter().image(
            icon.id(),
            rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            if dimmed {
                egui::Color32::from_gray(140)
            } else {
                egui::Color32::WHITE
            },
        );
    } else {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "?",
            egui::FontId::proportional(22.0),
            ui.visuals().weak_text_color(),
        );
    }
}

fn paint_status(ui: &egui::Ui, rect: egui::Rect, status: Status) {
    let badge = egui::Rect::from_min_size(
        rect.right_top() + egui::vec2(-18.0, 4.0),
        egui::vec2(14.0, 14.0),
    );
    match status {
        Status::Claimed => {
            let color = if ui.visuals().dark_mode {
                egui::Color32::from_rgb(123, 203, 163)
            } else {
                egui::Color32::from_rgb(28, 112, 71)
            };
            ui.painter().line_segment(
                [
                    badge.min + egui::vec2(2.0, 7.0),
                    badge.min + egui::vec2(6.0, 11.0),
                ],
                egui::Stroke::new(1.8, color),
            );
            ui.painter().line_segment(
                [
                    badge.min + egui::vec2(6.0, 11.0),
                    badge.min + egui::vec2(13.0, 3.0),
                ],
                egui::Stroke::new(1.8, color),
            );
        }
        Status::Locked => glyphs::paint_with_stroke(
            ui,
            badge,
            Glyph::Lock,
            egui::Stroke::new(1.2, ui.visuals().weak_text_color()),
        ),
        Status::Reached | Status::Unknown => {}
    }
}

pub(super) fn draw_details(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    snapshot: Option<&CollectionStateSnapshot>,
    reward: Reward<'_>,
) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            let (icon, _) = ui.allocate_exact_size(egui::vec2(52.0, 52.0), egui::Sense::hover());
            draw_icon(ui, catalog, reward.definition.item_hash, icon, false);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                ui.strong(
                    catalog
                        .package_item_name(reward.definition.item_hash)
                        .unwrap_or("Unnamed Reward"),
                );
                ui.weak(format!(
                    "Rank {} · Quantity {} · {}",
                    reward.definition.rewarded_at_progression_level,
                    reward.definition.quantity,
                    reward.status.label()
                ));
                if let Some(description) = catalog
                    .description(reward.definition.item_hash)
                    .filter(|text| !text.trim().is_empty())
                {
                    ui.label(crate::app::ui::destiny_text(ui, description));
                }
                ui.horizontal_wrapped(|ui| {
                    inspector::draw_catalog_hash_link(
                        ui,
                        catalog,
                        reward.definition.item_hash,
                        "Inspect Reward",
                    );
                    ui.menu_button("Claim Details", |ui| {
                        ui.label(reward.status.help());
                        ui.separator();
                        super::draw_claim_flag(ui, catalog, snapshot, reward.definition);
                        super::draw_grant(
                            ui,
                            catalog,
                            catalog.seasonal().and_then(|definition| {
                                definition.reward_grants.get(&reward.definition.item_hash)
                            }),
                        );
                    });
                });
            });
        });
    });
}
