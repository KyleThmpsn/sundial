//! Whole-loadout armor stat targeting.
//!
//! This module owns the dedicated window, cached package-backed socket model,
//! best-effort optimizer, preview, atomic apply path, and focused solver tests.
//! The single-item randomizer keeps its independent allocator in
//! `armor_stat_allocation.rs`.

mod apply;
mod input;
mod solver;
mod ui;

use apply::*;
use input::*;
use solver::*;
pub(super) use ui::equipped_totals;
#[cfg(test)]
use ui::preview_column_widths;
pub(in crate::app) use ui::{draw_entry_button, draw_window};
use ui::{is_armor_stat_mod_plug, is_armor_stat_mod_socket};

use std::{
    cmp::{Ordering, Reverse},
    collections::HashMap,
    sync::{
        Arc,
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
    time::Duration,
};

use crate::{
    app::{
        ARMOR_SLOTS, ConfirmationDialog, PlugSelectionMode, SLOTS, SundialApp, inventory, settings,
    },
    catalog::{Catalog, ItemDef, ItemRarity},
    hash::parse_hash_hex,
};
use eframe::egui;
use sundial_account::NO_DEFINITION_HASH;

use super::{
    EquippedItemPlugs, EquippedItemSnapshot, EquippedPlugValue, armor_stat_allocation,
    equip_inventory_item,
};

const WINDOW_SIZE: egui::Vec2 = egui::vec2(980.0, 720.0);
const WINDOW_MIN_SIZE: egui::Vec2 = egui::vec2(620.0, 460.0);
const MAX_SOLVER_STATES: usize = 1_000;
const MAX_PIECE_STATES: usize = 128;
const MAX_SLOT_PLANS: usize = 512;
const PREVIEW_DEBOUNCE_SECONDS: f64 = 0.15;

#[derive(Default)]
pub(in crate::app) struct State {
    open: bool,
    character_index: usize,
    targets: [u16; 6],
    source_key: Option<SourceKey>,
    input: Option<LoadoutInput>,
    preview: Option<Solution>,
    preview_task: Option<PreviewTask>,
    preview_due_at: Option<f64>,
    feedback: Option<Feedback>,
    preserve_feedback_once: bool,
    window_generation: u64,
    allow_inventory_swaps: Option<bool>,
}

impl State {
    pub(in crate::app) fn open(&mut self, character_index: usize) {
        if self.character_index != character_index {
            self.character_index = character_index;
            self.targets = [0; 6];
            self.source_key = None;
            self.input = None;
            self.preview = None;
            self.preview_task = None;
            self.preview_due_at = None;
            self.feedback = None;
            self.preserve_feedback_once = false;
        }
        self.window_generation = self.window_generation.wrapping_add(1);
        self.open = true;
    }
}

#[derive(Clone, Debug)]
struct Feedback {
    text: String,
    detail: Option<String>,
    is_error: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceKey {
    character_index: usize,
    plug_mode: PlugSelectionMode,
    allow_inventory_swaps: bool,
    class_type: u64,
    equipment: Vec<EquippedItemSnapshot>,
    inventory: Vec<inventory::InventoryItemSnapshot>,
}

#[derive(Clone)]
struct LoadoutInput {
    pieces: Vec<ArmorPiece>,
    candidates: Vec<Vec<ArmorCandidate>>,
    current_totals: [u16; 6],
}

#[derive(Clone)]
struct ArmorCandidate {
    piece: ArmorPiece,
    origin: ArmorOrigin,
    fixed_totals: [i32; 6],
    sockets: Vec<MutableSocket>,
    exotic: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ArmorOrigin {
    Equipped,
    Inventory {
        instance_soid: u64,
        definition_hash: u32,
    },
}

#[derive(Clone)]
struct ArmorPiece {
    slot: &'static str,
    label: &'static str,
    name: String,
    item: Option<Arc<ItemDef>>,
    current_plugs: Vec<Option<u64>>,
    current_totals: [u16; 6],
    locked: bool,
    masterworked: bool,
    issue: Option<String>,
}

#[derive(Clone)]
struct MutableSocket {
    socket_index: usize,
    current: Option<u64>,
    choices: Vec<SocketChoice>,
    kind: SocketKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SocketKind {
    Stat,
    Masterwork,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SocketChoice {
    hash: Option<u64>,
    values: [i32; 6],
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SocketAssignment {
    piece_index: usize,
    socket_index: usize,
    previous: Option<u64>,
    selected: Option<u64>,
    kind: SocketKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PieceSelection {
    candidate_index: usize,
    projected_totals: [u16; 6],
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ArmorSwap {
    piece_index: usize,
    instance_soid: u64,
    definition_hash: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Solution {
    projected_totals: [u16; 6],
    selections: Vec<PieceSelection>,
    swaps: Vec<ArmorSwap>,
    assignments: Vec<SocketAssignment>,
    shortfalls: [u16; 6],
    exact: bool,
}

#[derive(Clone)]
struct SearchState {
    totals: [i32; 6],
    plans: Vec<PiecePlan>,
    swaps: usize,
    plug_changes: usize,
    masterworks: usize,
    exotics: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PiecePlan {
    candidate_index: usize,
    totals: [i32; 6],
    selections: Vec<Option<u64>>,
    plug_changes: usize,
    masterworks: usize,
}

#[derive(Clone)]
struct PieceSearchState {
    totals: [i32; 6],
    selections: Vec<Option<u64>>,
    plug_changes: usize,
    masterworks: usize,
}

struct PreviewTask {
    source_key: SourceKey,
    targets: [u16; 6],
    receiver: Receiver<Solution>,
}

#[cfg(test)]
mod tests;
