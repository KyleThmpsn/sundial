//! Payload builders for decoder and decompiler tests.
//!
//! These assemble the mapped single-policy action layout in memory, so the suites stay
//! runnable without an installed client.

use super::fields::{KILL_LABELS, KILL_REQUIRES_WEAPON};
use super::*;

/// Minimal builder for the mapped single-policy action layout.
pub(crate) struct Builder {
    pub(crate) bytes: Vec<u8>,
}

impl Builder {
    pub(crate) fn new() -> Self {
        let mut out = Self {
            bytes: vec![0; ROOT_SIZE],
        };
        // Every stock root carries the empty key here, as the compiler writes it.
        out.u32(ROOT_KEY, crate::sandbox_perk::program::EMPTY_KEY);
        out
    }

    pub(crate) fn u32(&mut self, at: usize, value: u32) {
        self.bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn u64(&mut self, at: usize, value: u64) {
        self.bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn f32(&mut self, at: usize, value: f32) {
        self.u32(at, value.to_bits());
    }

    pub(crate) fn pointer(&mut self, at: usize, target: usize) {
        self.u64(at, ((target as i64) - (at as i64)) as u64);
    }

    pub(crate) fn string(&mut self, value: &str) -> usize {
        let at = self.bytes.len();
        self.bytes.extend_from_slice(value.as_bytes());
        self.bytes.push(0);
        at
    }

    pub(crate) fn node(&mut self, class: u32, size: usize) -> usize {
        let at = (self.bytes.len() + 4).next_multiple_of(8);
        self.bytes.resize(at + size, 0);
        self.u32(at - 4, class);
        at
    }

    pub(crate) fn rows(
        &mut self,
        descriptor: usize,
        class: u32,
        count: usize,
        stride: usize,
    ) -> usize {
        let header = (self.bytes.len() + 4).next_multiple_of(16);
        self.bytes.resize(header, 0);
        self.u32(header - 4, 0x80809FBD);
        let rows = header + 16;
        self.bytes.resize(rows + count * stride, 0);
        self.u64(header, count as u64);
        self.u32(header + 8, class);
        self.u64(descriptor, count as u64);
        self.pointer(descriptor + 8, header);
        rows
    }

    pub(crate) fn pointer_list(&mut self, descriptor: usize, class: u32, nodes: &[usize]) {
        let rows = self.rows(descriptor, class, nodes.len(), 8);
        for (index, target) in nodes.iter().enumerate() {
            self.pointer(rows + index * 8, *target);
        }
    }

    pub(crate) fn condition(&mut self, class: u32, kind: u8, size: usize) -> usize {
        let at = self.node(class, size);
        self.f32(at, 1.0);
        self.bytes[at + 4] = LITERAL_PROBABILITY;
        self.bytes[at + 5] = kind;
        at
    }

    pub(crate) fn unconditional(&mut self) -> usize {
        self.condition(0x8080_3E03, 0, 8)
    }

    pub(crate) fn timer(&mut self, seconds: f32) -> usize {
        let at = self.condition(0x8080_3DCD, 1, 12);
        self.f32(at + 8, seconds);
        at
    }

    /// An Event Key Match condition, kind 30.
    pub(crate) fn event_key(&mut self, key: u32) -> usize {
        let at = self.condition(0x8080_3DEB, 30, 12);
        self.u32(at + 8, key);
        at
    }

    pub(crate) fn draw(&mut self) -> usize {
        let at = self.condition(0x8080_3DF5, 16, 112);
        self.bytes[at + 8] = 1;
        at
    }

    pub(crate) fn holster(&mut self) -> usize {
        let at = self.condition(0x8080_3DDB, 17, 112);
        self.bytes[at + 8] = 1;
        at
    }

    pub(crate) fn kill(&mut self, labels: &[u32], requires_weapon: bool, chance: f32) -> usize {
        let at = self.condition(0x8080_3DE7, 2, 344);
        self.f32(at, chance);
        self.bytes[at + KILL_REQUIRES_WEAPON] = u8::from(requires_weapon);
        if !labels.is_empty() {
            let rows = self.rows(
                at + KILL_LABELS,
                LABEL_ROW_CLASS,
                labels.len(),
                LABEL_ROW_SIZE,
            );
            for (index, label) in labels.iter().enumerate() {
                self.u32(rows + index * LABEL_ROW_SIZE, *label);
            }
        }
        at
    }

    /// A spawn at the owner position. Event position pairs only with a kill trigger.
    pub(crate) fn spawn(&mut self, tag: u32, path: &str) -> usize {
        let at = self.node(0x8080_3E43, 24);
        self.bytes[at] = 3;
        let target = self.string(path);
        self.pointer(at + 8, target);
        self.u64(at + 0x10, u64::from(tag));
        at
    }

    pub(crate) fn attach(&mut self, tag: u32, path: &str) -> usize {
        self.attach_with(tag, path, 1, [0x811C_9DC5; 2], [0.0; 4])
    }

    /// An attach node with explicit technical fields at `+0x02`, `+0x18`, `+0x1C` and
    /// `+0x20` through `+0x2C`.
    pub(crate) fn attach_with(
        &mut self,
        tag: u32,
        path: &str,
        mode: u8,
        keys: [u32; 2],
        floats: [f32; 4],
    ) -> usize {
        let at = self.node(0x8080_3E45, 64);
        self.bytes[at] = 1;
        self.bytes[at + 1] = 1;
        self.bytes[at + 2] = mode;
        let target = self.string(path);
        self.pointer(at + 8, target);
        self.u64(at + 0x10, u64::from(tag));
        self.u32(at + 0x18, keys[0]);
        self.u32(at + 0x1C, keys[1]);
        for (index, value) in floats.into_iter().enumerate() {
            self.f32(at + 0x20 + index * 4, value);
        }
        self.u32(at + 0x30, 0x811C_9DC5);
        at
    }

    pub(crate) fn pattern(&mut self, tag: u32, path: &str) -> usize {
        let at = self.node(0x8080_3E12, 24);
        self.bytes[at] = 26;
        self.bytes[at + 1] = 1;
        let target = self.string(path);
        self.pointer(at + 8, target);
        self.u64(at + 0x10, u64::from(tag));
        at
    }

    /// An Extend Timers node. Nested conditions take ordinal `0xFF`, and the mask at `+0x20`
    /// routes their event kinds, as the stock nodes do.
    pub(crate) fn extend_timers(&mut self, extend: f32, cap: f32, conditions: &[usize]) -> usize {
        let at = self.node(0x8080_3E3B, 40);
        self.bytes[at] = 32;
        self.f32(at + 4, extend);
        self.f32(at + 8, cap);
        let mut mask = 0u64;
        for &condition in conditions {
            self.bytes[condition + 7] = 0xFF;
            mask |= 1 << self.bytes[condition + 5];
        }
        self.pointer_list(
            at + TIMER_EXTENSION_CONDITIONS,
            CONDITION_ROW_CLASS,
            conditions,
        );
        self.u64(at + 0x20, mask);
        at
    }

    /// A Named Property node with the surveyed constant value program.
    pub(crate) fn named_property(
        &mut self,
        key: u32,
        value: f32,
        target: u8,
        operation: u8,
        removal: u8,
    ) -> usize {
        let at = self.node(0x8080_29ED, 80);
        self.bytes[at] = 10;
        self.bytes[at + 1] = 1;
        self.bytes[at + 2] = target;
        self.bytes[at + 3] = 1;
        self.u32(at + 8, key);
        let code = self.rows(at + 0x18, 0x8080_0009, 4, 1);
        self.bytes[code..code + 4].copy_from_slice(&[0x34, 0x00, 0x3E, 0x00]);
        let constants = self.rows(at + 0x28, 0x8080_0090, 1, 16);
        for lane in 0..4 {
            self.f32(constants + lane * 4, value);
        }
        self.u64(at + 0x38, 1);
        self.u32(at + 0x40, 1);
        self.bytes[at + 0x49] = operation;
        self.bytes[at + 0x4A] = removal;
        at
    }

    /// Writes `labels` into the flat label list at `descriptor`.
    pub(crate) fn labels(&mut self, descriptor: usize, labels: &[u32]) {
        let rows = self.rows(descriptor, LABEL_ROW_CLASS, labels.len(), LABEL_ROW_SIZE);
        for (index, label) in labels.iter().enumerate() {
            self.u32(rows + index * LABEL_ROW_SIZE, *label);
        }
    }

    /// Writes the surveyed constant value program at `program`.
    pub(crate) fn constant_program(&mut self, program: usize, value: f32) {
        let code = self.rows(program, 0x8080_0009, 4, 1);
        self.bytes[code..code + 4].copy_from_slice(&[0x34, 0x00, 0x3E, 0x00]);
        let constants = self.rows(program + 0x10, 0x8080_0090, 1, 16);
        for lane in 0..4 {
            self.f32(constants + lane * 4, value);
        }
        self.u64(program + 0x20, 1);
        self.u32(program + 0x28, 1);
    }

    /// An Object And Numeric Event Filter condition, kind 4, with its required labels.
    pub(crate) fn object_event_filter(&mut self, labels: &[u32], threshold: f32) -> usize {
        let at = self.condition(0x8080_2F5F, 4, 208);
        self.labels(at + 0x28, labels);
        self.f32(at + 0xA0, threshold);
        self.u32(at + 0x9C, 0x811C_9DC5);
        self.bytes[at + 0xA8] = 2;
        self.bytes[at + 0xC1] = 4;
        at
    }

    /// A Component Value Adjustment effect, kind 8, driven by a constant program.
    pub(crate) fn component_adjustment(&mut self, scale: f32, limit: f32, value: f32) -> usize {
        let at = self.node(0x8080_3E4D, 80);
        self.bytes[at] = 8;
        self.bytes[at + 2] = 1;
        self.f32(at + 8, scale);
        self.f32(at + 0x0C, limit);
        self.constant_program(at + 0x18, value);
        self.bytes[at + 0x48] = 0xFF;
        at
    }

    /// A Fixed Ammunition Adjustment effect, kind 14, with the owning slot and the first
    /// ammunition category amounts.
    pub(crate) fn fixed_ammunition(&mut self, labels: &[u32], owning: i32, category: i32) -> usize {
        let at = self.node(0x8080_3E3F, 136);
        self.bytes[at] = 14;
        self.ammunition_filter(at, labels);
        self.bytes[at + 0x68] = 1;
        self.u32(at + 0x6C, owning as u32);
        self.u32(at + 0x7C, category as u32);
        at
    }

    /// A Proportional Ammunition Adjustment effect, kind 15, with one owning slot amount.
    pub(crate) fn proportional_ammunition(
        &mut self,
        destination: u8,
        capacity: u8,
        owning: f32,
    ) -> usize {
        let at = self.node(0x8080_3E3E, 136);
        self.bytes[at] = 15;
        self.ammunition_filter(at, &[]);
        self.bytes[at + 0x68] = destination;
        self.bytes[at + 0x6A] = capacity;
        self.f32(at + 0x6C, owning);
        at
    }

    /// The source label filter of an ammunition node: the required labels list, the label
    /// globals reference and, for an empty filter, the `0xFF` byte stock nodes carry.
    fn ammunition_filter(&mut self, at: usize, labels: &[u32]) {
        if labels.is_empty() {
            self.bytes[at + 0x58] = 0xFF;
        } else {
            self.labels(at + 0x28, labels);
        }
        let path = self.string("content/common/native/sandbox/label_globals.label_globals.tft");
        self.pointer(at + 0x48, path);
        self.u64(at + 0x50, 0x80C7_0CA1);
    }

    /// An Event Numeric Modifier effect, kind 40, with one literal assignment and one
    /// stat-driven multiplication.
    pub(crate) fn event_modifier(&mut self, labels: &[u32], assign: f32, stat: u8) -> usize {
        let at = self.node(0x8080_2F16, 336);
        self.bytes[at] = 40;
        self.labels(at + 0xE0, labels);
        let rows = self.rows(at + 0x120, 0x8080_3E22, 1, 12);
        self.u32(rows, 1);
        self.f32(rows + 4, assign);
        self.bytes[rows + 8] = 0xFF;
        let rows = self.rows(at + 0x130, 0x8080_3E22, 1, 12);
        self.u32(rows, 0);
        self.f32(rows + 4, 0.65);
        self.bytes[rows + 8] = stat;
        at
    }

    pub(crate) fn finish(mut self) -> Vec<u8> {
        let size = self.bytes.len() as u64;
        self.u64(0, size);
        self.bytes
    }
}

pub(crate) fn drawn_pattern_action() -> Vec<u8> {
    let mut out = Builder::new();
    let activation = out.draw();
    out.pointer_list(
        PRIMARY_GROUP + GROUP_ACTIVATION,
        CONDITION_ROW_CLASS,
        &[activation],
    );
    let pattern = out.pattern(0x8161_F73A, "content/sandbox/weapons/demo/demo.pattern.tft");
    let spawn = out.spawn(0x80BC_2F21, "content/sandbox/effects/demo/demo.entity.tft");
    out.pointer_list(
        PRIMARY_GROUP + GROUP_EFFECTS,
        EFFECT_ROW_CLASS,
        &[pattern, spawn],
    );
    let removal = out.holster();
    out.pointer_list(
        PRIMARY_GROUP + GROUP_REMOVAL,
        CONDITION_ROW_CLASS,
        &[removal],
    );
    out.bytes[RETAINED_STATE_BUDGET] = 2;
    out.finish()
}

pub(crate) fn precision_kill_action() -> Vec<u8> {
    let mut out = Builder::new();
    let activation = out.kill(&[0x962E_A19B], true, 1.0);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_ACTIVATION,
        CONDITION_ROW_CLASS,
        &[activation],
    );
    let nested = out.kill(&[0x962E_A19B], true, 1.0);
    let extend = out.extend_timers(5.0, 5.0, &[nested]);
    out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[extend]);
    let duration = out.timer(5.0);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_REMOVAL,
        CONDITION_ROW_CLASS,
        &[duration],
    );
    let cooldown = out.timer(2.5);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_REARM,
        CONDITION_ROW_CLASS,
        &[cooldown],
    );
    out.u64(ACTIVATION_EVENT_MASK, 1 << 2);
    out.bytes[TIMER_BUDGET] = 2;
    out.finish()
}
