//! Parts of a weapon that another weapon can supply, each a row under the donor card it follows:
//! Behavior and Type Markers under Base Weapon, Animations under Appearance. A row names where
//! the part comes from, muted while it is the card's own, and opens the weapon browser.
use super::*;
use crate::recipe::{AnimationAction, WeaponDonorReference};
use crate::weapon::lenders::Lender;

/// Every part row's label, so the rows of both cards line up.
/// Gameplay's Parts rows, so every value lines up.
pub(super) const GAMEPLAY_COLUMN: [&str; 6] = [
    "Behavior",
    "Type Markers",
    "Firing Behavior",
    "Barrel",
    "Magazine",
    "Reload",
];

/// The Appearance tab's Model and Animations rows.
pub(super) const APPEARANCE_COLUMN: [&str; 9] = [
    "Model",
    "Animations",
    "Hip Fire",
    "Aim Fire",
    "Holster",
    "Sprint",
    "Slide",
    "Combat Stance",
    "Reload Animation",
];
const ICON_SIZE: f32 = 16.0;

/// The width of the label column every part row shares.
pub(super) fn label_width(ui: &egui::Ui, column: &[&str]) -> f32 {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let widest = column
        .iter()
        .map(|label| {
            ui.fonts(|fonts| {
                fonts
                    .layout_no_wrap(
                        (*label).to_owned(),
                        font.clone(),
                        egui::Color32::PLACEHOLDER,
                    )
                    .size()
                    .x
            }) + nesting(label)
        })
        .fold(0.0, f32::max);
    widest + ui.spacing().interact_size.y + ui.spacing().item_spacing.x * 2.0
}

/// How far a row's label sits in from its column: the action rows nest under Animations.
fn nesting(label: &str) -> f32 {
    const NESTED: f32 = 16.0;
    if AnimationAction::ALL
        .iter()
        .any(|action| action.label() == label)
    {
        NESTED
    } else {
        0.0
    }
}

/// One part row.
pub(super) struct Part<'a> {
    pub(super) label: &'a str,
    /// Every label in the row's group, so their values line up.
    pub(super) column: &'a [&'a str],
    /// What the part is and what it changes, behind the info icon after the label.
    pub(super) hint: egui::WidgetText,
    /// The weapon the part comes from now.
    pub(super) value: &'a str,
    /// The weapon chosen in place of the card's own, when one is.
    pub(super) chosen: Option<u32>,
    /// The choice that gives the part back to the card's own weapon.
    pub(super) follow: &'a str,
    /// Why the row cannot open, while it cannot.
    pub(super) blocked: Option<&'a str>,
    /// A second line for each weapon in the browser.
    pub(super) detail: Option<&'a dyn Fn(u32) -> Option<String>>,
}

/// Draws a part row and returns the weapon picked in its browser, or `Clear` when the part goes
/// back to the card's own weapon.
pub(super) fn draw_part<'a>(
    ui: &mut egui::Ui,
    catalog: Option<&InvestmentCatalog>,
    scope: &str,
    query: &mut String,
    candidates: impl IntoIterator<Item = &'a WeaponDonorSummary>,
    part: Part<'_>,
) -> Option<WeaponDonorPickerAction> {
    let label_width = label_width(ui, part.column);
    ui.horizontal(|ui| {
        let height = ui.spacing().interact_size.y;
        ui.allocate_ui_with_layout(
            egui::vec2(label_width, height),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_width(label_width);
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.add_space(nesting(part.label));
                ui.weak(part.label);
                draw_authoring_info_icon(ui, part.hint.clone());
            },
        );
        let reset_width = if part.chosen.is_some() {
            height + ui.spacing().item_spacing.x
        } else {
            0.0
        };
        // The value hugs its text and caret, so neither floats away from the other.
        let width = value_width(ui, &part).min((ui.available_width() - reset_width).max(height));
        let enabled = catalog.is_some() && part.blocked.is_none();
        let trigger = ui
            .add_enabled_ui(enabled, |ui| value_button(ui, catalog, width, &part))
            .inner;
        let trigger = match part.blocked {
            Some(reason) => trigger.on_disabled_hover_text(reason),
            None => trigger,
        };
        let picked = catalog.filter(|_| enabled).and_then(|catalog| {
            catalog.draw_weapon_donor_picker_from(
                ui,
                trigger,
                (scope, "part"),
                query,
                candidates,
                WeaponDonorPickerOptions {
                    selected_hash: part.chosen,
                    selected_label: part.value,
                    header_label: None,
                    action_label: part.label,
                    selected_icon_override: None,
                    secondary_action_label: None,
                    row_detail: part.detail,
                    clear: Some(WeaponDonorPickerClearChoice {
                        label: part.follow,
                        tooltip: part.follow,
                        selected: part.chosen.is_none(),
                    }),
                    selected_detail: None,
                },
            )
        });
        let reset = part.chosen.is_some()
            && ui
                .scope(|ui| {
                    crate::app::style::quiet(ui);
                    let response = ui
                        .add(egui::Button::new(crate::app::style::light_icon(
                            ui,
                            egui_phosphor::regular::X,
                        )))
                        .on_hover_text(part.follow);
                    // Read out as what it does, not as the glyph.
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, part.follow)
                    });
                    response.clicked()
                })
                .inner;
        if reset {
            Some(WeaponDonorPickerAction::Clear)
        } else {
            picked
        }
    })
    .inner
}

/// A part as a labelled dropdown, for a grid of dropdowns such as the Weapon tab's profile. It
/// opens the same browser as the part row and returns the same picks, so both write one field.
pub(super) fn draw_stacked_part<'a>(
    ui: &mut egui::Ui,
    catalog: Option<&InvestmentCatalog>,
    scope: &str,
    query: &mut String,
    candidates: impl IntoIterator<Item = &'a WeaponDonorSummary>,
    part: Part<'_>,
) -> Option<WeaponDonorPickerAction> {
    ui.horizontal(|ui| {
        ui.label(part.label);
        draw_authoring_info_icon(ui, part.hint.clone());
    });
    let Some(catalog) = catalog.filter(|_| part.blocked.is_none()) else {
        let response = ui.add_enabled(
            false,
            egui::Button::new(part.value)
                .truncate()
                .min_size(egui::vec2(ui.available_width(), 0.0)),
        );
        if let Some(reason) = part.blocked {
            response.on_disabled_hover_text(reason);
        }
        return None;
    };
    catalog.draw_weapon_donor_dropdown_picker(
        ui,
        (scope, "stacked"),
        query,
        candidates,
        WeaponDonorPickerOptions {
            selected_hash: part.chosen,
            selected_label: part.value,
            header_label: None,
            action_label: part.label,
            selected_icon_override: None,
            secondary_action_label: None,
            row_detail: part.detail,
            clear: Some(WeaponDonorPickerClearChoice {
                label: part.follow,
                tooltip: part.follow,
                selected: part.chosen.is_none(),
            }),
            selected_detail: None,
        },
    )
}

/// One entry in a part row's short list: what it is called, a second line, the weapon recorded
/// when it is picked, and whether it is the current choice.
pub(super) struct Choice {
    pub(super) label: String,
    pub(super) detail: Option<String>,
    pub(super) item: u32,
    pub(super) selected: bool,
}

/// One action's row under Animations: what the frames play for it, once per distinct outcome.
struct ActionRow {
    action: AnimationAction,
    choices: Vec<Choice>,
    value: String,
    chosen: Option<u32>,
    misfit: bool,
}

/// What a short list picked: an entry's weapon, or the card's own part back.
pub(super) enum Picked {
    Item(u32),
    Clear,
}

/// A part row whose value opens a short list of `choices` under it rather than every weapon,
/// for parts that a handful of distinct values cover. A long list gets a filter.
pub(super) fn draw_choice_part(
    ui: &mut egui::Ui,
    scope: &str,
    query: &mut String,
    choices: &[Choice],
    part: Part<'_>,
) -> Option<Picked> {
    let label_width = label_width(ui, part.column);
    ui.horizontal(|ui| {
        let height = ui.spacing().interact_size.y;
        ui.allocate_ui_with_layout(
            egui::vec2(label_width, height),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_width(label_width);
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.add_space(nesting(part.label));
                ui.weak(part.label);
                draw_authoring_info_icon(ui, part.hint.clone());
            },
        );
        let reset_width = if part.chosen.is_some() {
            height + ui.spacing().item_spacing.x
        } else {
            0.0
        };
        let width = value_width(ui, &part).min((ui.available_width() - reset_width).max(height));
        let enabled = part.blocked.is_none();
        // No weapon icon: an entry stands for every weapon that shares it.
        let trigger = ui
            .add_enabled_ui(enabled, |ui| value_button(ui, None, width, &part))
            .inner;
        let trigger = match part.blocked {
            Some(reason) => trigger.on_disabled_hover_text(reason),
            None => trigger,
        };
        let popup = ui.make_persistent_id(("part-choices", scope));
        if trigger.clicked() {
            ui.memory_mut(|memory| memory.toggle_popup(popup));
            query.clear();
        }
        let mut picked = None;
        egui::popup::popup_below_widget(
            ui,
            popup,
            &trigger,
            egui::PopupCloseBehavior::CloseOnClickOutside,
            |ui| {
                ui.set_min_width(trigger.rect.width().max(260.0));
                let filtered = crate::app::pickers::wants_filter(choices.len());
                if filtered {
                    ui.add(
                        egui::TextEdit::singleline(query)
                            .hint_text("Search")
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(4.0);
                }
                if ui
                    .selectable_label(part.chosen.is_none(), part.follow)
                    .clicked()
                {
                    picked = Some(Picked::Clear);
                }
                ui.separator();
                let needle = query.to_lowercase();
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        for choice in choices.iter().filter(|choice| {
                            !filtered
                                || crate::app::pickers::matches(&needle, &choice.label)
                                || choice.detail.as_deref().is_some_and(|detail| {
                                    crate::app::pickers::matches(&needle, detail)
                                })
                        }) {
                            let mut text = egui::text::LayoutJob::default();
                            let body = egui::TextStyle::Body.resolve(ui.style());
                            // As bright as the Follow item above, so the list does not read as
                            // disabled. The second line stays muted.
                            text.append(
                                &choice.label,
                                0.0,
                                egui::TextFormat::simple(
                                    body.clone(),
                                    ui.visuals().widgets.inactive.text_color(),
                                ),
                            );
                            if let Some(detail) = &choice.detail {
                                text.append(
                                    &format!("\n{detail}"),
                                    0.0,
                                    egui::TextFormat::simple(
                                        egui::FontId::proportional(body.size - 1.0),
                                        ui.visuals().weak_text_color(),
                                    ),
                                );
                            }
                            if ui.selectable_label(choice.selected, text).clicked() {
                                picked = Some(Picked::Item(choice.item));
                            }
                        }
                    });
            },
        );
        if picked.is_some() {
            ui.memory_mut(egui::Memory::close_popup);
        }
        let reset = part.chosen.is_some()
            && ui
                .scope(|ui| {
                    crate::app::style::quiet(ui);
                    let response = ui
                        .add(egui::Button::new(crate::app::style::light_icon(
                            ui,
                            egui_phosphor::regular::X,
                        )))
                        .on_hover_text(part.follow);
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, part.follow)
                    });
                    response.clicked()
                })
                .inner;
        if reset { Some(Picked::Clear) } else { picked }
    })
    .inner
}

/// A short list's second line: the first weapons that share an entry, and how many more do.
fn members(names: &[String]) -> Option<String> {
    match names {
        [] => None,
        [one] => Some(one.clone()),
        [one, two] => Some(format!("{one}, {two}")),
        [one, two, rest @ ..] => Some(format!("{one}, {two} and {} more", rest.len())),
    }
}

/// The width a value needs: its text, the chosen weapon's icon, the caret and the padding around
/// them, never narrower than a short name.
fn value_width(ui: &egui::Ui, part: &Part<'_>) -> f32 {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let text = ui.fonts(|fonts| {
        fonts
            .layout_no_wrap(part.value.to_owned(), font, egui::Color32::PLACEHOLDER)
            .size()
            .x
    });
    let padding = ui.spacing().button_padding.x;
    let icon = if part.chosen.is_some() {
        ICON_SIZE + 6.0
    } else {
        0.0
    };
    (padding * 3.0 + icon + text + ui.spacing().icon_width)
        .max(140.0)
        .ceil()
}

/// The row's value: the chosen weapon's icon and name in a field framed like a dropdown, or the
/// card's own weapon muted with only the caret, framed while hovered. A muted value in a filled
/// field read as a disabled control.
fn value_button(
    ui: &mut egui::Ui,
    catalog: Option<&InvestmentCatalog>,
    width: f32,
    part: &Part<'_>,
) -> egui::Response {
    let height = ui.spacing().interact_size.y;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    let enabled = ui.is_enabled();
    let spoken = format!("{}: {}", part.label, part.value);
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, enabled, &spoken));
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let visuals = *ui.style().interact(&response);
    let inherited = part.chosen.is_none();
    if !inherited {
        ui.painter().rect(
            rect,
            visuals.corner_radius,
            visuals.weak_bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
    } else if response.hovered() || response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            visuals.corner_radius,
            ui.visuals().widgets.hovered.bg_stroke,
            egui::StrokeKind::Inside,
        );
    }
    let padding = ui.spacing().button_padding;
    let icon_width = ui.spacing().icon_width;
    let caret = egui::Rect::from_center_size(
        egui::pos2(rect.right() - padding.x - icon_width * 0.5, rect.center().y),
        egui::vec2(icon_width * 0.7, icon_width * 0.45),
    );
    ui.painter().add(egui::Shape::convex_polygon(
        vec![caret.left_top(), caret.right_top(), caret.center_bottom()],
        if inherited {
            ui.visuals().weak_text_color()
        } else {
            visuals.fg_stroke.color
        },
        egui::Stroke::NONE,
    ));
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.left() + padding.x, rect.top()),
        egui::pos2(caret.left() - padding.x, rect.bottom()),
    );
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    row.spacing_mut().item_spacing.x = 6.0;
    if let (Some(hash), Some(catalog)) = (part.chosen, catalog) {
        catalog.draw_perk_icon(&mut row, hash, ICON_SIZE);
    }
    // Body text either way: the muted label carries the row's name, and the field and icon are
    // what mark a part another weapon supplies.
    let color = ui.visuals().text_color();
    row.add(
        egui::Label::new(egui::RichText::new(part.value).color(color))
            .truncate()
            .selectable(false),
    );
    response
}

/// Every stock weapon's owners and type marker, read once per catalog in the background.
#[derive(Default)]
pub(in crate::app) struct Lenders {
    index: Option<Result<Arc<BTreeMap<u32, Lender>>, String>>,
    job: Option<LenderJob>,
    /// The frame of each weapon the offered attachment owners lend, keyed by those owners.
    /// Animations follow the frame, so it is the line under each weapon in the browser.
    frames: Option<(Vec<u32>, BTreeMap<u32, String>)>,
}

struct LenderJob {
    receiver: Receiver<Result<BTreeMap<u32, Lender>, String>>,
    worker: thread::JoinHandle<()>,
}

impl Drop for Lenders {
    fn drop(&mut self) {
        // No package handle may outlive the catalog or survive into an install.
        if let Some(job) = self.job.take() {
            let _ = job.worker.join();
        }
    }
}

impl Lenders {
    /// Takes a finished read. Called every frame, so a read started on the Weapon tab never
    /// keeps holding packages after it finishes on another page.
    pub(in crate::app) fn poll(&mut self) {
        if let Some(job) = &self.job {
            let result = match job.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    Some(Err("Weapon reading stopped without a result".to_owned()))
                }
            };
            if let Some(result) = result {
                let LenderJob { worker, .. } = self.job.take().expect("job was checked");
                let result = if worker.join().is_err() {
                    Err("Weapon reading crashed".to_owned())
                } else {
                    result
                };
                self.index = Some(result.map(Arc::new));
            }
        }
    }

    fn update(&mut self, ctx: &egui::Context, packages: &Path, weapons: Vec<(u32, u16)>) {
        self.poll();
        if self.index.is_some() || self.job.is_some() || weapons.is_empty() {
            return;
        }
        let packages = packages.to_owned();
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        let worker = thread::spawn(move || {
            let result = open_shadowkeep_package_manager(&packages).and_then(|manager| {
                crate::weapon::lenders::index(&manager, &weapons).map_err(|error| error.to_string())
            });
            let _ = sender.send(result);
            ctx.request_repaint();
        });
        self.job = Some(LenderJob { receiver, worker });
    }

    fn index(&self) -> Option<&Arc<BTreeMap<u32, Lender>>> {
        self.index.as_ref().and_then(|index| index.as_ref().ok())
    }

    fn failed(&self) -> bool {
        matches!(self.index, Some(Err(_)))
    }

    /// Whether the read still holds package handles, which an installation has to wait out.
    pub(in crate::app) fn busy(&self) -> bool {
        self.job.is_some()
    }
}

/// The weapon's frame, or an exotic's intrinsic: the plug its first socket starts with.
fn frame(catalog: &InvestmentCatalog, hash: u32) -> Option<String> {
    let donor = catalog.weapon_donor(hash)?;
    let socket = donor.sockets.first()?;
    let plug = socket
        .native_default
        .or_else(|| socket.ordered_embedded_choices.first().copied())?;
    catalog.item_display_name(plug).map(str::to_owned)
}

/// Whose rig the model plays: the weapon whose attachment row the build writes, the weapon
/// whose attachment owner the entity carries, and the appearance when it is from another family.
#[derive(Clone, Copy)]
struct Rig {
    row: u32,
    owner: u32,
    other_family: Option<u32>,
}

/// Where a part row's choices stand while the index is read.
enum Reading {
    Ready,
    Waiting,
    Failed,
}

impl Reading {
    fn blocked(&self) -> Option<&'static str> {
        match self {
            Self::Ready => None,
            Self::Waiting => Some("Reading\u{2026}"),
            Self::Failed => Some("Unavailable"),
        }
    }
}

impl PackageAuthoringApp {
    /// Starts the background read the first time a part row needs it.
    fn update_lenders(&mut self, ctx: &egui::Context) {
        // The installer replaces the packages the read would open.
        if self.install_receiver.is_some() {
            return;
        }
        let weapons = if self.lenders.index.is_none() && self.lenders.job.is_none() {
            self.donor_summaries
                .iter()
                .filter_map(|donor| Some((donor.hash, donor.weapon_pattern_index?)))
                .collect()
        } else {
            Vec::new()
        };
        self.lenders.update(ctx, &self.packages, weapons);
    }

    /// The Technical Build's account of the part rows: the rig the build keeps, whose hold and
    /// animations it uses, and the type its markers carry. Starts the type read when needed.
    pub(in crate::app) fn technical_parts(&mut self, ctx: &egui::Context) -> String {
        use crate::app::technical_build::field;
        use std::fmt::Write as _;
        if !self.recipe.kind.is_weapon() {
            return String::new();
        }
        self.update_lenders(ctx);
        let mut out = String::new();
        let _ = writeln!(out, "\nRIG");
        let name = |hash: u32| {
            self.summary_name(hash)
                .unwrap_or_else(|| format!("0x{hash:08X}"))
        };
        let family = |hash: u32| {
            self.donor_summaries
                .iter()
                .find(|donor| donor.hash == hash)
                .map_or_else(
                    || "not in the catalog".to_owned(),
                    |donor| match donor.weapon_translation_group {
                        Some(group) => format!("{}  (translation group {group})", donor.type_name),
                        None => format!("{}  (no translation group)", donor.type_name),
                    },
                )
        };
        let base = self.runtime_base();
        field(
            &mut out,
            "base family",
            base.map_or_else(|| "not read".to_owned(), family),
        );
        let appearance = self
            .recipe
            .presentation_donor
            .as_ref()
            .and_then(|donor| donor.item_hash.parse_u32().ok())
            .filter(|hash| *hash != 0);
        if let Some(appearance) = appearance {
            field(&mut out, "appearance family", family(appearance));
        }
        let rig = self.animation_rig();
        match rig.as_ref().map(|rig| (rig.owner, rig.other_family)) {
            None => field(&mut out, "rig", "checking"),
            Some((_, None)) => field(&mut out, "rig", "base weapon's family"),
            Some((owner, Some(appearance))) if owner == appearance => {
                field(&mut out, "rig", format!("moved from {}", name(appearance)));
            }
            Some((_, Some(appearance))) => {
                field(
                    &mut out,
                    "rig",
                    "base weapon's, model pinned to the root bone",
                );
                field(&mut out, "hold", format!("{}'s family", name(appearance)));
            }
        }
        let animations = match self
            .recipe
            .overrides
            .animation_donor
            .as_ref()
            .and_then(|donor| donor.item_hash.parse_u32().ok())
        {
            Some(donor) => name(donor),
            None => rig
                .as_ref()
                .map_or_else(|| "checking".to_owned(), |rig| name(rig.row)),
        };
        field(&mut out, "animations", animations);
        for (action, donor) in &self.recipe.overrides.animation_actions {
            if let Ok(hash) = donor.item_hash.parse_u32() {
                field(
                    &mut out,
                    &action.label().to_lowercase(),
                    format!("{}'s animations, on a private arms rig", name(hash)),
                );
            }
        }
        let source = self
            .recipe
            .overrides
            .type_marker_donor
            .as_ref()
            .and_then(|donor| donor.item_hash.parse_u32().ok())
            .or(base);
        let kind = match (self.lenders.index(), source) {
            (Some(index), Some(source)) => index
                .get(&source)
                .and_then(|lender| lender.type_key)
                .map_or_else(
                    || "not in this runtime".to_owned(),
                    crate::weapon::lenders::type_label,
                ),
            _ if self.lenders.failed() => "unavailable".to_owned(),
            _ => "reading".to_owned(),
        };
        match source {
            Some(source) => field(
                &mut out,
                "type markers",
                format!("{kind}  ({})", name(source)),
            ),
            None => field(&mut out, "type markers", kind),
        }
        out
    }

    /// Firing Behavior, Barrel and Magazine on Advanced Gameplay: each takes its values from
    /// another weapon while the weapon keeps its own runtime and wiring, so each can come from a
    /// different weapon.
    pub(in crate::app) fn draw_component_splices(&mut self, ui: &mut egui::Ui) {
        use sundial::package_authoring::entity::{
            WEAPON_BARREL_COMPONENT_KEY, WEAPON_MAGAZINE_COMPONENT_KEY,
            WEAPON_TRIGGER_COMPONENT_KEY,
        };
        const ROWS: [(u32, &str, &str); 3] = [
            (
                WEAPON_TRIGGER_COMPONENT_KEY,
                "Firing Behavior",
                "How the trigger fires. Test in game.",
            ),
            (
                WEAPON_BARREL_COMPONENT_KEY,
                "Barrel",
                "Barrel values. Test in game.",
            ),
            (
                WEAPON_MAGAZINE_COMPONENT_KEY,
                "Magazine",
                "Magazine and reserves. Test in game.",
            ),
        ];
        self.update_lenders(ui.ctx());
        let index = self.lenders.index().cloned();
        let reading = match &index {
            Some(_) => Reading::Ready,
            None if self.lenders.failed() => Reading::Failed,
            None => Reading::Waiting,
        };
        let base = self
            .runtime_base()
            .and_then(|hash| self.summary_name(hash))
            .unwrap_or_else(|| "Base Weapon".to_owned());
        for (binding, label, hint) in ROWS {
            let chosen = self.recipe.component_splice(binding).and_then(|donor| {
                Some((
                    donor.item_hash.parse_u32().ok()?,
                    donor.expected_name.clone(),
                ))
            });
            let value = match &chosen {
                Some((hash, expected)) => self
                    .summary_name(*hash)
                    .or_else(|| expected.clone())
                    .unwrap_or_else(|| format!("0x{hash:08X}")),
                None => base.clone(),
            };
            // Only a weapon with a gameplay owner of its own carries these components.
            let lends = |hash: u32| {
                index.as_ref().is_some_and(|index| {
                    index.get(&hash).is_some_and(|l| l.gameplay_owner.is_some())
                })
            };
            let summaries = &self.donor_summaries;
            let detail = |hash: u32| {
                summaries
                    .iter()
                    .find(|donor| donor.hash == hash)
                    .map(|donor| donor.type_name.clone())
            };
            let candidates = self
                .donor_summaries
                .iter()
                .filter(|donor| lends(donor.hash));
            let scope = format!("weapon-component-{binding:08X}");
            let query = self.runtime_component_queries.entry(binding).or_default();
            let action = draw_part(
                ui,
                self.catalog.as_ref(),
                &scope,
                query,
                candidates,
                Part {
                    column: &GAMEPLAY_COLUMN,
                    label,
                    hint: hint.into(),
                    value: &value,
                    chosen: chosen.as_ref().map(|(hash, _)| *hash),
                    follow: "Follow Base Weapon",
                    blocked: reading.blocked(),
                    detail: Some(&detail),
                },
            );
            match action {
                Some(WeaponDonorPickerAction::Select(hash)) => {
                    let expected_name = self.summary_name(hash);
                    self.recipe.set_component_splice(
                        binding,
                        Some(WeaponDonorReference {
                            item_hash: hash.into(),
                            expected_name,
                        }),
                    );
                    self.runtime_component_queries.remove(&binding);
                }
                Some(WeaponDonorPickerAction::Clear) => {
                    self.recipe.set_component_splice(binding, None);
                    self.runtime_component_queries.remove(&binding);
                }
                Some(WeaponDonorPickerAction::Secondary) | None => {}
            }
        }
    }

    fn summary_name(&self, hash: u32) -> Option<String> {
        self.donor_summaries
            .iter()
            .find(|donor| donor.hash == hash)
            .map(|donor| donor.name.clone())
    }

    /// The weapon whose runtime the build starts from: the Swap Runtime row's weapon when one
    /// is set, otherwise the base weapon.
    pub(in crate::app) fn runtime_base(&self) -> Option<u32> {
        let key = self.runtime_graph_key()?;
        key.pattern_index
            .and_then(|index| {
                self.donor_summaries
                    .iter()
                    .find(|donor| {
                        donor.hash == key.fallback_item_hash
                            && donor.weapon_pattern_index == Some(index)
                    })
                    .or_else(|| {
                        self.donor_summaries
                            .iter()
                            .find(|donor| donor.weapon_pattern_index == Some(index))
                    })
                    .map(|donor| donor.hash)
            })
            .or(Some(key.fallback_item_hash))
    }

    /// Whose rig the model plays. `None` while the runtime scan is still deciding whether an
    /// appearance from another family brings its rig.
    fn animation_rig(&self) -> Option<Rig> {
        let base = self.runtime_base()?;
        let Some(appearance) = self
            .recipe
            .presentation_donor
            .as_ref()
            .and_then(|donor| donor.item_hash.parse_u32().ok())
            .filter(|hash| *hash != 0)
        else {
            return Some(Rig {
                row: base,
                owner: base,
                other_family: None,
            });
        };
        let group = |hash: u32| {
            self.donor_summaries
                .iter()
                .find(|donor| donor.hash == hash)
                .and_then(|donor| donor.weapon_translation_group)
        };
        // One family: the entity is the base weapon's and the row the appearance's.
        if group(appearance) == group(self.recipe.donor.item_hash.parse_u32().ok()?) {
            return Some(Rig {
                row: appearance,
                owner: base,
                other_family: None,
            });
        }
        let key = self.runtime_graph_key()?;
        let kept = Rig {
            row: base,
            owner: base,
            other_family: Some(appearance),
        };
        // Animations from the base weapon's family keep its rig, so nothing has to be scanned.
        if key.appearance_rig.is_none() {
            return Some(kept);
        }
        let checked = self
            .runtime_graph
            .as_ref()
            .is_some_and(|(loaded, _)| *loaded == key);
        checked.then(|| {
            if self.runtime_rig_appearance == Some(appearance) {
                Rig {
                    row: appearance,
                    owner: appearance,
                    other_family: Some(appearance),
                }
            } else {
                kept
            }
        })
    }

    /// Type Markers, under the Base Weapon card: the type the runtime block names, `scout_rifle`
    /// or `pulse_rifle`, and which weapon supplies it.
    pub(in crate::app) fn draw_type_marker_part(&mut self, ui: &mut egui::Ui) {
        self.update_lenders(ui.ctx());
        let index = self.lenders.index().cloned();
        let lender = |hash: u32| index.as_ref().and_then(|index| index.get(&hash).copied());
        let base = self.runtime_base().and_then(lender);
        let reading = match (&index, base) {
            (Some(_), Some(_)) => Reading::Ready,
            (Some(_), None) => Reading::Failed,
            (None, _) if self.lenders.failed() => Reading::Failed,
            (None, _) => Reading::Waiting,
        };
        let owner = base.and_then(|base| base.content_owner);
        let chosen = self
            .recipe
            .overrides
            .type_marker_donor
            .as_ref()
            .and_then(|donor| donor.item_hash.parse_u32().ok());
        // One entry per type marker the base weapon's runtime holds: its type name, and its
        // frame key where one type carries several, as hand cannons do per frame.
        let key = |entry: &Lender| entry.type_key.map(|kind| (kind, entry.frame_key));
        let mut groups = BTreeMap::<(u32, Option<u32>), Vec<u32>>::new();
        if let (Some(index), Some(owner)) = (&index, owner) {
            for (hash, entry) in index.iter() {
                if entry.content_owner == Some(owner)
                    && let Some(key) = key(entry)
                {
                    groups.entry(key).or_default().push(*hash);
                }
            }
        }
        let frames_per_type = |kind: u32| groups.keys().filter(|(other, _)| *other == kind).count();
        let catalog = self.catalog.as_ref();
        let named = |hashes: &[u32]| {
            let mut names = hashes
                .iter()
                .filter_map(|hash| self.summary_name(*hash))
                .collect::<Vec<_>>();
            names.sort();
            names
        };
        let chosen_key = chosen.and_then(lender).and_then(|entry| key(&entry));
        let label = |(kind, frame_key): (u32, Option<u32>), hashes: &[u32]| {
            // A key whose type name is not recovered reads as the weapons that carry it, never as
            // a hash. The empty name's hash marks a block with no type at all.
            let Some(name) = crate::weapon::lenders::type_name(kind) else {
                return if kind == sundial::package_authoring::fnv1_name_hash("") {
                    "None".to_owned()
                } else {
                    members(&named(hashes)).unwrap_or_else(|| "Unnamed".to_owned())
                };
            };
            let name = name.to_owned();
            // A frame is named only where the key is one frame's: the type has several keys, this
            // one is not the type's own generic key, and every weapon carrying it shares a frame.
            // Nature of the Beast's generic key is shared by 19 hand cannons of mixed frames.
            let frame = (frames_per_type(kind) > 1 && frame_key != Some(kind))
                .then(|| {
                    let catalog = catalog?;
                    let frames = hashes
                        .iter()
                        .filter_map(|hash| frame(catalog, *hash))
                        .collect::<std::collections::BTreeSet<_>>();
                    (frames.len() == 1)
                        .then(|| frames.into_iter().next())
                        .flatten()
                })
                .flatten();
            match frame {
                Some(frame) => format!("{name} \u{b7} {frame}"),
                None => name,
            }
        };
        let mut choices = groups
            .iter()
            .map(|(&group, hashes)| {
                let names = named(hashes);
                // The entry records its first weapon by name, so a save reads the same.
                let item = hashes
                    .iter()
                    .copied()
                    .min_by_key(|hash| self.summary_name(*hash))
                    .unwrap_or(hashes[0]);
                Choice {
                    label: label(group, hashes),
                    detail: members(&names),
                    item,
                    selected: chosen_key == Some(group),
                }
            })
            .collect::<Vec<_>>();
        // Alphabetical, so entries of one weapon type sit together and repeated frames line up.
        choices.sort_by(|a, b| a.label.cmp(&b.label));
        let value = match (&reading, chosen_key, base.and_then(|base| key(&base))) {
            (Reading::Waiting, None, _) => "Reading\u{2026}".to_owned(),
            (_, Some(group), _) | (_, None, Some(group)) => {
                label(group, groups.get(&group).map_or(&[][..], Vec::as_slice))
            }
            _ => "Unknown".to_owned(),
        };
        let misfit = owner.is_some()
            && chosen.is_some_and(|hash| lender(hash).and_then(|l| l.content_owner) != owner);
        let mut query = std::mem::take(&mut self.type_marker_query);
        let picked = draw_choice_part(
            ui,
            "weapon-type-markers",
            &mut query,
            &choices,
            Part {
                column: &GAMEPLAY_COLUMN,
                label: "Type Markers",
                hint: "The weapon type the runtime names, such as pulse_rifle, with its frame key \
                       and type label. Replaces the base weapon's own. The behavior, values and \
                       rig stay. Test in game."
                    .into(),
                value: &value,
                chosen,
                follow: "Follow Base Weapon",
                blocked: reading.blocked(),
                detail: None,
            },
        );
        self.type_marker_query = query;
        if misfit {
            ui.horizontal(|ui| {
                ui.add_space(label_width(ui, &GAMEPLAY_COLUMN));
                ui.colored_label(ui.visuals().error_fg_color, "Not in this runtime.");
            });
        }
        match picked {
            Some(Picked::Item(hash)) => {
                self.recipe.overrides.type_marker_donor = Some(WeaponDonorReference {
                    item_hash: hash.into(),
                    expected_name: self.summary_name(hash),
                });
            }
            Some(Picked::Clear) => self.recipe.overrides.type_marker_donor = None,
            None => {}
        }
    }

    pub(in crate::app) fn draw_animation_part(&mut self, ui: &mut egui::Ui) {
        self.update_lenders(ui.ctx());
        let index = self.lenders.index().cloned();
        let lender = |hash: u32| index.as_ref().and_then(|index| index.get(&hash).copied());
        let rig = self.animation_rig();
        let attachment = |hash: u32| lender(hash).and_then(|l| l.attachment_owner);
        let owner = rig.and_then(|rig| attachment(rig.owner));
        // An appearance from another family can bring its rig or be pinned to the base's, so
        // both families' animations are offered, and the choice decides which.
        let mut offered = owner.into_iter().collect::<Vec<_>>();
        if let Some(appearance) = rig.and_then(|rig| rig.other_family) {
            offered.extend(self.runtime_base().and_then(attachment));
            offered.extend(attachment(appearance));
        }
        offered.sort_unstable();
        offered.dedup();
        let reading = match (&index, rig) {
            (Some(_), Some(_)) if owner.is_some() => Reading::Ready,
            (Some(_), Some(_)) => Reading::Failed,
            (None, _) if self.lenders.failed() => Reading::Failed,
            _ => Reading::Waiting,
        };
        // Name each weapon's frame once per set of owners, not on every frame drawn.
        if let (Some(index), Some(catalog)) = (&index, self.catalog.as_ref())
            && !offered.is_empty()
            && self
                .lenders
                .frames
                .as_ref()
                .is_none_or(|(named, _)| *named != offered)
        {
            let frames = index
                .iter()
                .filter(|(_, lender)| {
                    lender
                        .attachment_owner
                        .is_some_and(|owner| offered.contains(&owner))
                })
                .filter_map(|(hash, _)| Some((*hash, frame(catalog, *hash)?)))
                .collect();
            self.lenders.frames = Some((offered.clone(), frames));
        }
        let chosen = self
            .recipe
            .overrides
            .animation_donor
            .as_ref()
            .and_then(|donor| donor.item_hash.parse_u32().ok());
        // One entry per animation profile the offered rigs hold: the rig and the row's keys.
        let key = |entry: &Lender| Some((entry.attachment_owner?, entry.animation_keys?));
        let mut groups = BTreeMap::<(u32, [u32; 2]), Vec<u32>>::new();
        if let Some(index) = &index {
            for (hash, entry) in index.iter() {
                if let Some(key) = key(entry)
                    && offered.contains(&key.0)
                {
                    groups.entry(key).or_default().push(*hash);
                }
            }
        }
        let frames = self.lenders.frames.as_ref().map(|(_, frames)| frames);
        // With both families offered, the weapon type says which rig a choice keeps: the base
        // weapon's own keeps its rig and pins the model, the appearance's brings its rig.
        let both = offered.len() > 1;
        let summaries = &self.donor_summaries;
        let named = |hashes: &[u32]| {
            let mut names = hashes
                .iter()
                .filter_map(|hash| {
                    summaries
                        .iter()
                        .find(|donor| donor.hash == *hash)
                        .map(|donor| donor.name.clone())
                })
                .collect::<Vec<_>>();
            names.sort();
            names
        };
        // A profile is named the way a player knows it: one weapon, or one weapon's versions such
        // as Drang and Drang (Baroque), by that weapon. Otherwise by the legendary frame its
        // weapons share, and failing that by its weapons. An exotic's intrinsic, such as
        // "Together Forever", is not a frame and names nothing a player would look for.
        let base_label = |hashes: &[u32]| {
            let mut counts = BTreeMap::<String, usize>::new();
            for hash in hashes {
                if let Some(frame) = frames.and_then(|frames| frames.get(hash)) {
                    *counts.entry(frame.clone()).or_default() += 1;
                }
            }
            let common = counts
                .into_iter()
                .max_by_key(|(_, count)| *count)
                .map(|(frame, _)| frame)
                .filter(|frame| frame.ends_with("Frame"));
            let names = named(hashes);
            let weapon = |name: &str| name.split(" (").next().unwrap_or(name).to_owned();
            let one_weapon = names
                .first()
                .map(|first| weapon(first))
                .filter(|first| names.iter().all(|name| weapon(name) == *first));
            let (name, by_frame) = match (one_weapon.clone(), common) {
                (Some(weapon), _) => (weapon, false),
                (None, Some(frame)) => (frame, true),
                (None, None) => (
                    members(&names).unwrap_or_else(|| "Unknown".to_owned()),
                    false,
                ),
            };
            // A frame is shared across weapon types, so it always says whose. With both families
            // offered, a weapon's own animations say it too, since the type decides the rig.
            let kind = (both || by_frame)
                .then(|| hashes.first())
                .flatten()
                .and_then(|first| summaries.iter().find(|donor| donor.hash == *first))
                .map(|donor| donor.type_name.clone());
            match kind {
                Some(kind) => format!("{name} ({kind})"),
                None => name,
            }
        };
        // Two profiles can share a frame, such as three Precision Frame profiles on the hand
        // cannon rig. A repeated name takes its lead weapon, so every entry reads apart.
        let mut repeated = BTreeMap::<String, usize>::new();
        for hashes in groups.values() {
            *repeated.entry(base_label(hashes)).or_default() += 1;
        }
        let label = |hashes: &[u32]| {
            let base = base_label(hashes);
            match named(hashes).first() {
                Some(lead) if repeated.get(&base).copied().unwrap_or(0) > 1 => {
                    format!("{base} \u{b7} {lead}")
                }
                _ => base,
            }
        };
        let chosen_key = chosen.and_then(lender).and_then(|entry| key(&entry));
        let mut choices = groups
            .iter()
            .map(|(&group, hashes)| {
                let names = named(hashes);
                let item = hashes
                    .iter()
                    .copied()
                    .min_by_key(|hash| self.summary_name(*hash))
                    .unwrap_or(hashes[0]);
                Choice {
                    label: label(hashes),
                    detail: (hashes.len() > 1).then(|| members(&names)).flatten(),
                    item,
                    selected: chosen_key == Some(group),
                }
            })
            .collect::<Vec<_>>();
        // Alphabetical, so entries of one weapon type sit together and repeated frames line up.
        choices.sort_by(|a, b| a.label.cmp(&b.label));
        let own = rig
            .and_then(|rig| lender(rig.row))
            .and_then(|entry| key(&entry))
            .and_then(|group| groups.get(&group));
        let value = match (&reading, chosen_key, own) {
            (Reading::Waiting, None, _) => "Reading\u{2026}".to_owned(),
            (_, Some(group), _) => groups
                .get(&group)
                .map_or_else(|| "Unknown".to_owned(), |hashes| label(hashes)),
            (_, None, Some(hashes)) => label(hashes),
            (_, None, None) => rig
                .and_then(|rig| self.summary_name(rig.row))
                .unwrap_or_default(),
        };
        let misfit = owner.is_some()
            && chosen.is_some_and(|hash| lender(hash).and_then(|l| l.attachment_owner) != owner);
        // The base weapon's own animations under another type's appearance keep the base rig and
        // pin the model to it, as Slab Shotgun and Long Sphere showed in game.
        let base_rig = self.runtime_base().and_then(attachment);
        let pins = rig
            .and_then(|rig| rig.other_family)
            .and_then(attachment)
            .is_some_and(|appearance| {
                chosen_key
                    .is_some_and(|(chosen, _)| Some(chosen) == base_rig && chosen != appearance)
            });
        let mut query = std::mem::take(&mut self.animation_query);
        let picked = draw_choice_part(
            ui,
            "weapon-animation-donor",
            &mut query,
            &choices,
            Part {
                column: &APPEARANCE_COLUMN,
                label: "Animations",
                hint: "First-person animations: how the weapon is held, fired, reloaded and \
                       equipped. Another weapon type's brings its whole rig, including how that \
                       type fires, such as a pulse rifle's burst. The base weapon's own type keeps \
                       its rig. Test in game."
                    .into(),
                value: &value,
                chosen,
                follow: "Follow Appearance",
                blocked: reading.blocked(),
                detail: None,
            },
        );
        self.animation_query = query;
        if misfit {
            ui.horizontal(|ui| {
                ui.add_space(label_width(ui, &APPEARANCE_COLUMN));
                ui.colored_label(ui.visuals().error_fg_color, "Does not fit this model.");
            });
        } else if pins {
            ui.horizontal_wrapped(|ui| {
                ui.add_space(label_width(ui, &APPEARANCE_COLUMN));
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "Plays the base rig. The model holds still and can sit or aim wrong.",
                );
            });
        }
        // Single actions mix within the rig the weapon plays now: the chosen animations' or the
        // model's own. Each lists what the frames play for it, once per distinct outcome.
        let playing = chosen_key.or_else(|| {
            rig.and_then(|rig| lender(rig.row))
                .and_then(|entry| key(&entry))
        });
        let rows = playing
            .map(|(owner, _)| {
                AnimationAction::ALL
                    .into_iter()
                    .enumerate()
                    .filter_map(|(index, action)| {
                        let outcome = |hashes: &[u32]| {
                            hashes
                                .iter()
                                .filter_map(|hash| lender(*hash))
                                .map(|entry| entry.actions[index])
                                .find(|outcome| *outcome != 0)
                        };
                        let mut outcomes = BTreeMap::<u32, Vec<&Vec<u32>>>::new();
                        for (group, hashes) in &groups {
                            if group.0 == owner
                                && let Some(outcome) = outcome(hashes)
                            {
                                outcomes.entry(outcome).or_default().push(hashes);
                            }
                        }
                        if outcomes.len() < 2 {
                            return None;
                        }
                        let chosen = self
                            .recipe
                            .overrides
                            .animation_actions
                            .get(&action)
                            .and_then(|donor| donor.item_hash.parse_u32().ok());
                        let chosen_outcome = chosen.and_then(|hash| outcome(&[hash]));
                        let current = chosen_outcome.or_else(|| {
                            playing
                                .and_then(|group| groups.get(&group))
                                .and_then(|hashes| outcome(hashes))
                        });
                        let mut choices = outcomes
                            .iter()
                            .map(|(&found, playing_it)| {
                                let mut labels = playing_it
                                    .iter()
                                    .map(|hashes| label(hashes))
                                    .collect::<Vec<_>>();
                                labels.sort();
                                labels.dedup();
                                let item = playing_it
                                    .iter()
                                    .flat_map(|hashes| hashes.iter().copied())
                                    .min_by_key(|hash| self.summary_name(*hash))
                                    .unwrap_or(playing_it[0][0]);
                                (
                                    found,
                                    Choice {
                                        label: labels[0].clone(),
                                        detail: members(&labels[1..]),
                                        item,
                                        selected: chosen_outcome == Some(found),
                                    },
                                )
                            })
                            .collect::<Vec<_>>();
                        choices.sort_by(|a, b| a.1.label.cmp(&b.1.label));
                        let value = current
                            .and_then(|current| choices.iter().find(|(found, _)| *found == current))
                            .map_or_else(
                                || "Unknown".to_owned(),
                                |(_, choice)| choice.label.clone(),
                            );
                        let misfit = chosen.is_some_and(|hash| {
                            lender(hash).and_then(|entry| entry.attachment_owner) != Some(owner)
                        });
                        Some(ActionRow {
                            action,
                            choices: choices.into_iter().map(|(_, choice)| choice).collect(),
                            value,
                            chosen,
                            misfit,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        match picked {
            Some(Picked::Item(hash)) => {
                self.recipe.overrides.animation_donor = Some(WeaponDonorReference {
                    item_hash: hash.into(),
                    expected_name: self.summary_name(hash),
                });
                // Another rig's animations leave the single actions of the old rig behind.
                let rig = lender(hash).and_then(|entry| entry.attachment_owner);
                self.recipe.overrides.animation_actions.retain(|_, donor| {
                    donor
                        .item_hash
                        .parse_u32()
                        .ok()
                        .and_then(lender)
                        .and_then(|entry| entry.attachment_owner)
                        == rig
                });
            }
            Some(Picked::Clear) => self.recipe.overrides.animation_donor = None,
            None => {}
        }
        self.draw_animation_actions(ui, rows, reading.blocked());
    }

    /// The Actions disclosure under Animations: one row per action the frames play differently.
    /// It opens by itself while any action is mixed.
    fn draw_animation_actions(
        &mut self,
        ui: &mut egui::Ui,
        rows: Vec<ActionRow>,
        blocked: Option<&str>,
    ) {
        if rows.is_empty() {
            return;
        }
        let mixed = rows.iter().filter(|row| row.chosen.is_some()).count();
        let id = ui.make_persistent_id("weapon-animation-actions");
        let mut picks = Vec::new();
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, mixed > 0)
            .show_header(ui, |ui| {
                ui.weak("Actions");
                draw_authoring_info_icon(
                    ui,
                    "Single actions played from another frame's animations. The rest follow \
                     Animations. Test in game.",
                );
                if mixed > 0 {
                    ui.weak(format!("{mixed} Mixed"));
                }
            })
            .body_unindented(|ui| {
                for row in &rows {
                    let mut query = String::new();
                    let picked = draw_choice_part(
                        ui,
                        &format!("weapon-animation-action-{:?}", row.action),
                        &mut query,
                        &row.choices,
                        Part {
                            column: &APPEARANCE_COLUMN,
                            label: row.action.label(),
                            hint: row.action.hint().into(),
                            value: &row.value,
                            chosen: row.chosen,
                            follow: "Follow Animations",
                            blocked,
                            detail: None,
                        },
                    );
                    if row.misfit {
                        ui.horizontal(|ui| {
                            ui.add_space(label_width(ui, &APPEARANCE_COLUMN));
                            ui.colored_label(
                                ui.visuals().error_fg_color,
                                "Does not fit this model.",
                            );
                        });
                    }
                    if let Some(picked) = picked {
                        picks.push((row.action, picked));
                    }
                }
            });
        for (action, picked) in picks {
            match picked {
                Picked::Item(hash) => {
                    self.recipe.overrides.animation_actions.insert(
                        action,
                        WeaponDonorReference {
                            item_hash: hash.into(),
                            expected_name: self.summary_name(hash),
                        },
                    );
                }
                Picked::Clear => {
                    self.recipe.overrides.animation_actions.remove(&action);
                }
            }
        }
    }
}
