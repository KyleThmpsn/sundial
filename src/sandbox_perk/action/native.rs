//! Complete, relocatable native node data used by the perk reader and editor.
//!
//! Native declarations identify every pointer and inline record. Scalar bytes without
//! recovered names remain explicit data, never guessed pointers or discarded padding.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub mod fields;
pub mod labels;
pub mod predicate;
mod read;
pub mod schema;
pub mod value;
mod write;

const MAX_BLOCKS: usize = 8192;
const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_ROWS: usize = 4096;

/// A native allocation. Arrays contain their rows, with their header owned by the writer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Block {
    /// Zero identifies a terminated UTF-8 string, otherwise a recovered native class.
    pub class: u32,
    pub count: Option<usize>,
    pub bytes: Vec<u8>,
    /// Pointer fields contain zero in `bytes` and refer to other blocks here.
    pub links: BTreeMap<usize, usize>,
}

/// A closed native allocation graph whose first block is its root object.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Graph {
    pub blocks: Vec<Block>,
}

impl Graph {
    /// Validate a closed node before it enters an authored program.
    pub fn validate_node(&self, condition: bool, kind: u8) -> Result<(), String> {
        use crate::sandbox_perk::nodes;
        self.validate()?;
        let entry = if condition {
            nodes::condition(kind)
        } else {
            nodes::effect(kind)
        }
        .ok_or("Unknown native node kind.")?;
        if self.root_class() != Some(entry.class) {
            return Err("The native root has the wrong class.".into());
        }
        self.validate_contents()
    }

    /// Validate the complete action, including every group, policy and nested node.
    pub fn validate_program(&self) -> Result<(), String> {
        self.validate()?;
        if self.root_class() != Some(super::ACTION_ROOT_CLASS) {
            return Err("The native root is not an action program.".into());
        }
        self.validate_contents()?;
        Ok(())
    }

    fn validate_contents(&self) -> Result<(), String> {
        use crate::sandbox_perk::nodes;
        let mut states = vec![0; self.blocks.len()];
        let mut pending = vec![(0, false, 0)];
        while let Some((index, leaving, depth)) = pending.pop() {
            if leaving {
                states[index] = 2;
                continue;
            }
            if states[index] == 1 {
                return Err("Native nodes cannot contain a pointer cycle.".into());
            }
            if states[index] == 2 {
                continue;
            }
            if depth >= 64 {
                return Err("Native nodes nest too deeply.".into());
            }
            states[index] = 1;
            pending.push((index, true, depth));
            let block = &self.blocks[index];
            if let Some(node) = nodes::CONDITIONS
                .iter()
                .find(|n| n.class == block.class && n.observed())
            {
                if block.count.is_some() || block.bytes[5] != node.kind || block.bytes[6] > 1 {
                    return Err("Invalid native condition header.".into());
                }
                let probability =
                    f32::from_le_bytes(block.bytes[..4].try_into().expect("condition header"));
                if !probability.is_finite() {
                    return Err("Condition probability must be finite.".into());
                }
            } else if let Some(node) = nodes::EFFECTS
                .iter()
                .find(|n| n.class == block.class && n.observed())
            {
                if block.count.is_some() || block.bytes[0] != node.kind || block.bytes[1] > 1 {
                    return Err("Invalid native effect header.".into());
                }
            }
            if block.class != 0 {
                let stride = schema::record(block.class)?.size;
                for row in 0..block.count.unwrap_or(1) {
                    for (offset, class, _) in schema::inline(block.class)? {
                        if class == value::CLASS {
                            value::validate(self, index, row * stride + offset)?;
                        }
                    }
                }
            }
            for &target in block.links.values() {
                pending.push((target, false, depth + 1));
            }
        }
        Ok(())
    }

    /// Read every declared field and allocation reachable from a native object.
    pub fn read(data: &[u8], offset: usize, class: u32) -> Result<Self, String> {
        read::capture(data, offset, class)
    }

    /// Emit a closed relative-pointer graph. No original resource offsets are retained.
    pub fn emit(&self) -> Result<Vec<u8>, String> {
        write::emit(self)
    }

    /// Allocation positions let callers patch declared resource lanes after relocation.
    pub fn emit_with_offsets(&self) -> Result<(Vec<u8>, BTreeMap<usize, usize>), String> {
        write::emit_with_offsets(self)
    }

    pub fn root_class(&self) -> Option<u32> {
        self.blocks.first().map(|block| block.class)
    }

    pub fn validate(&self) -> Result<(), String> {
        write::validate(self)
    }

    /// Import an independently owned node and return its new allocation index.
    pub fn append(&mut self, other: &Self) -> Result<usize, String> {
        other.validate()?;
        if self.blocks.len().saturating_add(other.blocks.len()) > MAX_BLOCKS {
            return Err("The native program contains too many allocations.".into());
        }
        let first = self.blocks.len();
        self.blocks
            .extend(other.blocks.iter().cloned().map(|mut block| {
                for target in block.links.values_mut() {
                    *target += first;
                }
                block
            }));
        Ok(first)
    }

    /// Give an owner its own copy of the allocation it links at `field`, when that allocation
    /// is reached from anywhere else as well.
    ///
    /// Reading a native object memoizes by resource position, so two nodes that pointed at
    /// one resource become one block here. Writing through such a block changes both nodes,
    /// and the second one is usually somewhere else entirely in the program. Returns the
    /// index to write through, which is the original when nothing else refers to it.
    pub fn make_unique(&mut self, owner: usize, field: usize) -> Result<usize, String> {
        let target = *self
            .blocks
            .get(owner)
            .ok_or("Missing native pointer owner.")?
            .links
            .get(&field)
            .ok_or("Missing native allocation.")?;
        let referrers = self
            .blocks
            .iter()
            .enumerate()
            .flat_map(|(index, block)| block.links.iter().map(move |link| (index, link)))
            .filter(|(_, (_, referenced))| **referenced == target)
            .count();
        if referrers <= 1 {
            return Ok(target);
        }
        if self.blocks.len() >= MAX_BLOCKS {
            return Err("The native program contains too many allocations.".into());
        }
        let copy = self.blocks.len();
        let block = self
            .blocks
            .get(target)
            .ok_or("Missing native allocation.")?
            .clone();
        self.blocks.push(block);
        self.blocks
            .get_mut(owner)
            .ok_or("Missing native pointer owner.")?
            .links
            .insert(field, copy);
        Ok(copy)
    }

    /// Drop the allocations no longer reachable from the root.
    ///
    /// An edit that repoints a link allocates a fresh target rather than writing through one
    /// that may be shared with another node, so the allocation it replaced stays behind.
    /// `emit` walks from the root and never writes those, but `validate` counts every block
    /// against `MAX_BLOCKS` and an authored program stores its block list as it stands, so a
    /// long editing session otherwise grows its own saved recipe until no frame can validate
    /// it and the program can no longer be edited at all.
    ///
    /// This renumbers the blocks it keeps, so a caller holding an allocation index must
    /// re-read it afterwards. Call it where an edit is committed, not between the steps of
    /// one.
    pub fn compact(&mut self) {
        let mut reachable = vec![false; self.blocks.len()];
        let mut pending = vec![0usize];
        while let Some(index) = pending.pop() {
            match reachable.get_mut(index) {
                Some(seen) if !*seen => *seen = true,
                _ => continue,
            }
            if let Some(block) = self.blocks.get(index) {
                pending.extend(block.links.values().copied());
            }
        }
        if reachable.iter().all(|seen| *seen) {
            return;
        }
        let mut moved = BTreeMap::new();
        for (index, _) in reachable.iter().enumerate().filter(|(_, seen)| **seen) {
            moved.insert(index, moved.len());
        }
        let mut blocks = Vec::with_capacity(moved.len());
        for (index, mut block) in std::mem::take(&mut self.blocks).into_iter().enumerate() {
            if !reachable[index] {
                continue;
            }
            for target in block.links.values_mut() {
                if let Some(index) = moved.get(target) {
                    *target = *index;
                }
            }
            blocks.push(block);
        }
        self.blocks = blocks;
    }

    /// Change an array length while preserving its existing records and their pointers.
    pub fn resize_array(&mut self, block: usize, count: usize) -> Result<(), String> {
        let mut changed = self.clone();
        let item = changed.blocks.get(block).ok_or("Missing native array.")?;
        let previous = item.count.ok_or("This allocation is not an array.")?;
        if count > MAX_ROWS {
            return Err("A native array contains too many entries.".into());
        }
        let stride = schema::record(item.class)?.size;
        let size = stride
            .checked_mul(count)
            .filter(|n| *n <= MAX_BYTES)
            .ok_or("The native array is too large.")?;
        let sample = item
            .bytes
            .get(..stride)
            .map(<[u8]>::to_vec)
            .unwrap_or_else(|| vec![0; stride]);
        let links = item
            .links
            .range(..stride)
            .map(|(k, v)| (*k, *v))
            .collect::<Vec<_>>();
        changed.blocks[block].bytes.resize(size, 0);
        changed.blocks[block]
            .links
            .retain(|offset, _| *offset < size);
        changed.blocks[block].count = Some(count);
        for row in previous..count {
            changed.blocks[block].bytes[row * stride..(row + 1) * stride].copy_from_slice(&sample);
            let mut imported = BTreeMap::new();
            for &(offset, target) in &links {
                let target = changed.copy_allocation(target, &mut imported)?;
                changed.blocks[block]
                    .links
                    .insert(row * stride + offset, target);
            }
        }
        changed.synchronize_counts()?;
        if previous == 0 {
            for row in 0..count {
                changed.initialize_values(block, row * stride)?;
            }
        }
        changed.validate()?;
        *self = changed;
        Ok(())
    }

    fn copy_allocation(
        &mut self,
        source: usize,
        copied: &mut BTreeMap<usize, usize>,
    ) -> Result<usize, String> {
        if let Some(&index) = copied.get(&source) {
            return Ok(index);
        }
        if self.blocks.len() >= MAX_BLOCKS {
            return Err("The native program contains too many allocations.".into());
        }
        let block = self
            .blocks
            .get(source)
            .ok_or("Missing native allocation.")?
            .clone();
        let index = self.blocks.len();
        copied.insert(source, index);
        self.blocks.push(block.clone());
        for (field, target) in block.links {
            let target = self.copy_allocation(target, copied)?;
            self.blocks[index].links.insert(field, target);
        }
        Ok(index)
    }

    /// Replace a pointer with a separately owned object or an empty typed array.
    pub fn create_target(
        &mut self,
        owner: usize,
        field: usize,
        class: u32,
        array: bool,
    ) -> Result<(), String> {
        let mut changed = self.clone();
        let target = if !array {
            // Class zero is a string allocation, not a node. The unobserved kinds carry a
            // zero class of their own, so looking one up by it finds a node with no template
            // and fails a request that only ever meant "make me a string".
            let node = (class != 0)
                .then(|| {
                    crate::sandbox_perk::nodes::CONDITIONS
                        .iter()
                        .find(|n| n.class == class)
                        .map(|n| (true, n.kind))
                        .or_else(|| {
                            crate::sandbox_perk::nodes::EFFECTS
                                .iter()
                                .find(|n| n.class == class)
                                .map(|n| (false, n.kind))
                        })
                })
                .flatten();
            if let Some((condition, kind)) = node {
                let bytes = template(condition, kind).ok_or("Missing native node template.")?;
                changed.append(&Graph::read(&bytes, 0, class)?)?
            } else {
                let index = changed.blocks.len();
                let bytes = if class == 0 {
                    vec![0]
                } else {
                    vec![0; schema::record(class)?.size]
                };
                changed.blocks.push(Block {
                    class,
                    count: None,
                    bytes,
                    links: BTreeMap::new(),
                });
                index
            }
        } else {
            schema::record(class)?;
            let index = changed.blocks.len();
            changed.blocks.push(Block {
                class,
                count: Some(0),
                bytes: Vec::new(),
                links: BTreeMap::new(),
            });
            index
        };
        let owner = changed
            .blocks
            .get_mut(owner)
            .ok_or("Missing native pointer owner.")?;
        owner
            .bytes
            .get_mut(field..field + 8)
            .ok_or("Invalid native pointer field.")?
            .fill(0);
        owner.links.insert(field, target);
        changed.synchronize_counts()?;
        if !array && class != 0 {
            changed.initialize_values(target, 0)?;
        }
        changed.validate()?;
        *self = changed;
        Ok(())
    }

    fn initialize_values(&mut self, target: usize, base: usize) -> Result<(), String> {
        for (offset, child, _) in schema::inline(self.blocks[target].class)? {
            let offset = base + offset;
            if child == value::CLASS
                && crate::package_payload::u64_at(&self.blocks[target].bytes, offset)? == 0
            {
                value::Program {
                    instructions: vec![
                        value::Instruction {
                            opcode: 52,
                            operand: Some(0),
                        },
                        value::Instruction {
                            opcode: 62,
                            operand: Some(0),
                        },
                    ],
                    constants: vec![[0; 4]],
                    fast_path: 0,
                }
                .write(self, target, offset)?;
            }
        }
        Ok(())
    }

    /// Keep all array descriptors consistent with their referenced allocations.
    pub fn synchronize_counts(&mut self) -> Result<(), String> {
        let counts = self
            .blocks
            .iter()
            .map(|block| block.count)
            .collect::<Vec<_>>();
        for block in &mut self.blocks {
            for (&field, &target) in &block.links {
                if let Some(count) = *counts.get(target).ok_or("Missing native pointer target.")? {
                    let start = field
                        .checked_sub(8)
                        .ok_or("Invalid native array descriptor.")?;
                    block
                        .bytes
                        .get_mut(start..field)
                        .ok_or("Truncated native array descriptor.")?
                        .copy_from_slice(&(count as u64).to_le_bytes());
                }
            }
        }
        Ok(())
    }
}

/// One observed native configuration, independent of any installed stock action.
pub fn template(condition: bool, kind: u8) -> Option<Vec<u8>> {
    schema::template(condition, kind)
}

/// Capture only the node's declared native data, including all its nested records.
pub fn capture(data: &[u8], offset: usize, class: u32) -> Result<Vec<u8>, String> {
    Graph::read(data, offset, class)?.emit()
}

#[cfg(test)]
mod tests;
