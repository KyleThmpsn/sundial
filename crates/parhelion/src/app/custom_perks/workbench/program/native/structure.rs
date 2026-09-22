//! Authoring edits to native lists. Copy the owning list before changing its rows.
use super::*;
use sundial::package_authoring::sandbox_perk::action;

#[derive(Clone, Copy)]
pub(super) enum Part {
    Trigger,
    Actions,
    Ending,
    Rearm,
}

impl Part {
    fn offset(self) -> usize {
        match self {
            Self::Trigger => 0,
            Self::Actions => 0x18,
            Self::Ending => 0x28,
            Self::Rearm => 0x38,
        }
    }
    fn class(self) -> u32 {
        if matches!(self, Self::Actions) {
            action::EFFECT_ROW_CLASS
        } else {
            action::CONDITION_ROW_CLASS
        }
    }
}

pub(super) enum Edit {
    AddGroup,
    Add(NativeNode),
    Replace(usize, NativeNode),
    Remove(usize),
    Move(usize, usize),
}

/// A path follows the selected owner, including every shared ancestor.
#[derive(Clone, Debug)]
pub(super) struct List {
    pub owner: Vec<usize>,
    pub field: usize,
    pub class: u32,
}

pub(super) fn resolve(graph: &Graph, path: &[usize]) -> Result<usize, String> {
    path.iter().try_fold(0, |owner, field| {
        graph
            .blocks
            .get(owner)
            .and_then(|block| block.links.get(field))
            .copied()
            .ok_or_else(|| "This condition no longer exists.".into())
    })
}

pub(super) fn path_to(graph: &Graph, target: usize) -> Result<Vec<usize>, String> {
    let mut pending = vec![(0, Vec::new())];
    let mut seen = std::collections::BTreeSet::new();
    while let Some((index, path)) = pending.pop() {
        if index == target {
            return Ok(path);
        }
        if !seen.insert(index) {
            continue;
        }
        for (&at, &child) in &graph.blocks[index].links {
            let mut next = path.clone();
            next.push(at);
            pending.push((child, next));
        }
    }
    Err("This record is no longer part of the effect.".into())
}

fn unique(graph: &mut Graph, path: &[usize]) -> Result<usize, String> {
    path.iter()
        .try_fold(0, |owner, field| graph.make_unique(owner, *field))
}

/// No-op drawing leaves the graph byte-for-byte alone. Actual edits own their entire path.
pub(super) fn scoped<R>(
    graph: &mut Graph,
    path: &[usize],
    draw: impl FnOnce(&mut Graph, usize) -> Result<R, String>,
) -> Result<R, String> {
    let mut changed = graph.clone();
    let index = unique(&mut changed, path)?;
    let baseline = changed.clone();
    let result = draw(&mut changed, index)?;
    if changed != baseline {
        *graph = changed;
    }
    Ok(result)
}

impl List {
    pub fn group(group: usize, part: Part) -> Self {
        let (owner, base) = if group == 0 {
            (vec![], 0x20)
        } else {
            (vec![0x70], (group - 1) * 0x48)
        };
        Self {
            owner,
            field: base + part.offset() + 8,
            class: part.class(),
        }
    }
    pub fn node(&self, row: usize) -> Result<Vec<usize>, String> {
        let mut path = self.owner.clone();
        path.push(self.field);
        if self.class != 0 {
            path.push(row * schema::record(self.class)?.size);
        }
        Ok(path)
    }
    pub fn edit(&self, graph: &mut Graph, edit: Edit) -> Result<(), String> {
        let mut changed = graph.clone();
        let owner = unique(&mut changed, &self.owner)?;
        if self.class == 0 {
            match edit {
                Edit::Add(node) | Edit::Replace(0, node) => {
                    let class = nodes::condition(node.kind)
                        .ok_or("Unknown condition kind.")?
                        .class;
                    let source = Graph::read(&node.bytes, 0, class)?;
                    source.validate_node(true, node.kind)?;
                    let target = changed.append(&source)?;
                    changed.blocks[owner].links.insert(self.field, target);
                }
                Edit::Remove(0) => {
                    changed.blocks[owner].links.remove(&self.field);
                }
                _ => return Err("This predicate holds one condition.".into()),
            }
        } else {
            if !changed.blocks[owner].links.contains_key(&self.field) {
                changed.create_target(owner, self.field, self.class, true)?;
            }
            let list = changed.make_unique(owner, self.field)?;
            let stride = schema::record(self.class)?.size;
            let count = changed.blocks[list].count.ok_or("Missing list length.")?;
            let mut rows: Vec<_> = (0..count)
                .map(|i| {
                    let block = &changed.blocks[list];
                    (
                        block.bytes[i * stride..(i + 1) * stride].to_vec(),
                        block
                            .links
                            .range(i * stride..(i + 1) * stride)
                            .map(|(at, target)| (at - i * stride, *target))
                            .collect::<BTreeMap<_, _>>(),
                    )
                })
                .collect();
            let reversed = self.class == action::EFFECT_ROW_CLASS;
            if reversed {
                rows.reverse();
            }
            let replacement = match &edit {
                Edit::Replace(index, _) => Some(*index),
                _ => None,
            };
            match edit {
                Edit::Add(node) | Edit::Replace(_, node) => {
                    if replacement.is_none() && count >= 256 {
                        return Err("This list has reached its entry limit.".into());
                    }
                    let class = if reversed {
                        nodes::effect(node.kind)
                    } else {
                        nodes::condition(node.kind)
                    }
                    .ok_or("Unknown node kind.")?
                    .class;
                    let source = Graph::read(&node.bytes, 0, class)?;
                    source.validate_node(!reversed, node.kind)?;
                    let target = changed.append(&source)?;
                    if let Some(index) = replacement {
                        rows.get_mut(index)
                            .ok_or("This condition no longer exists.")?
                            .1
                            .insert(0, target);
                    } else {
                        let mut bytes = vec![0; stride];
                        if self.class == 0x80803E32 {
                            bytes[12..16].copy_from_slice(&1.0f32.to_le_bytes());
                        }
                        rows.push((bytes, BTreeMap::from([(0, target)])));
                    }
                }
                Edit::AddGroup if self.class == action::SUBGROUP_ROW_CLASS && count < 256 => {
                    rows.push((vec![0; stride], BTreeMap::new()))
                }
                Edit::Remove(index) if index < rows.len() => {
                    rows.remove(index);
                }
                Edit::Move(from, to) if from < rows.len() && to < rows.len() => {
                    let row = rows.remove(from);
                    rows.insert(to, row);
                }
                _ => return Err("This list changed before the edit could be applied.".into()),
            }
            if reversed {
                rows.reverse();
            }
            let block = &mut changed.blocks[list];
            block.count = Some(rows.len());
            block.bytes = rows
                .iter()
                .flat_map(|(bytes, _)| bytes.iter().copied())
                .collect();
            block.links = rows
                .iter()
                .enumerate()
                .flat_map(|(row, (_, links))| {
                    links
                        .iter()
                        .map(move |(at, target)| (row * stride + at, *target))
                })
                .collect();
            changed.blocks[owner].bytes[self.field - 8..self.field]
                .copy_from_slice(&(rows.len() as u64).to_le_bytes());
        }
        changed.validate_program()?;
        *graph = changed;
        Ok(())
    }
}

#[cfg(test)]
pub(super) fn edit(graph: &mut Graph, group: usize, part: Part, edit: Edit) -> Result<(), String> {
    List::group(group, part).edit(graph, edit)
}

pub(super) fn add_group(graph: &mut Graph) -> Result<(), String> {
    let mut changed = graph.clone();
    if !changed.blocks[0].links.contains_key(&0x70) {
        changed.create_target(0, 0x70, 0x8080407D, true)?;
    }
    let list = changed.make_unique(0, 0x70)?;
    let count = changed.blocks[list]
        .count
        .ok_or("Missing behavior groups.")?;
    if count >= 255 {
        return Err("This effect has reached its behavior group limit.".into());
    }
    changed.resize_array(list, count + 1)?;
    let stride = schema::record(0x8080407D)?.size;
    changed.blocks[list].bytes[count * stride..].fill(0);
    changed.blocks[list]
        .links
        .retain(|at, _| *at < count * stride);
    changed.validate_program()?;
    *graph = changed;
    Ok(())
}

pub(super) fn remove_group(graph: &mut Graph, group: usize) -> Result<(), String> {
    if group == 0 {
        return Err("The main behavior group cannot be removed.".into());
    }
    let mut changed = graph.clone();
    let list = changed.make_unique(0, 0x70)?;
    let count = changed.blocks[list]
        .count
        .ok_or("Missing behavior groups.")?;
    if group > count {
        return Err("This behavior group no longer exists.".into());
    }
    let stride = schema::record(0x8080407D)?.size;
    let start = (group - 1) * stride;
    let block = &mut changed.blocks[list];
    block.bytes.drain(start..start + stride);
    block.links = block
        .links
        .iter()
        .filter_map(|(&at, &target)| {
            if at < start {
                Some((at, target))
            } else if at >= start + stride {
                Some((at - stride, target))
            } else {
                None
            }
        })
        .collect();
    block.count = Some(count - 1);
    changed.synchronize_counts()?;
    changed.validate_program()?;
    *graph = changed;
    Ok(())
}

pub(super) fn action_node(
    action: Action,
    group: &action::DecodedGroup,
) -> Result<NativeNode, String> {
    if let Action::Native { node } = action {
        return Ok(node);
    }
    let mut program = Program {
        trigger: Trigger::Always,
        actions: vec![action],
        ..Program::default()
    };
    if let Some(trigger) = group.activation.iter().find(|node| node.kind == 2) {
        program.trigger = Trigger::Native;
        program.native_trigger = Some(NativeNode {
            kind: trigger.kind,
            bytes: trigger.native.clone(),
        });
    }
    let draft = sundial::package_authoring::sandbox_perk::program::native_draft(&program)?;
    let decoded = action::decode(&draft.graph.emit()?)?;
    let action = decoded.groups[0].effects.first().ok_or("Missing action.")?;
    Ok(NativeNode {
        kind: action.kind,
        bytes: action.native.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sundial::package_authoring::sandbox_perk::program::NativeProgram;

    #[test]
    fn nested_edits_copy_every_shared_ancestor_and_preserve_contributions() {
        let mut graph = NativeProgram::empty().graph;
        edit(
            &mut graph,
            0,
            Part::Trigger,
            Edit::Add(NativeNode::condition(26).unwrap()),
        )
        .unwrap();
        let parent = List::group(0, Part::Trigger).node(0).unwrap();
        let children = List {
            owner: parent.clone(),
            field: 0x10,
            class: 0x80803E32,
        };
        children
            .edit(&mut graph, Edit::Add(NativeNode::condition(1).unwrap()))
            .unwrap();
        let rows = resolve(&graph, &children.node(0).unwrap()[..3]).unwrap();
        graph.blocks[rows].bytes[12..16].copy_from_slice(&0x7fc01234u32.to_le_bytes());
        let shared = graph.blocks[0].links[&0x28];
        graph.blocks[0].links.insert(0x50, shared);
        graph.synchronize_counts().unwrap();
        let original = action::decode(&graph.emit().unwrap()).unwrap().groups[0].removal[0]
            .native
            .clone();
        let before = graph.clone();
        scoped(&mut graph, &parent, |_, _| Ok(())).unwrap();
        assert_eq!(
            graph, before,
            "reading a shared branch must not allocate copies"
        );
        let child = children.node(0).unwrap();
        scoped(&mut graph, &child, |graph, node| {
            let duration = fields::describe(graph.blocks[node].class)?
                .into_iter()
                .find(|field| field.label == "Duration")
                .unwrap()
                .offset;
            graph.blocks[node].bytes[duration..duration + 4]
                .copy_from_slice(&0x80000000u32.to_le_bytes());
            Ok(())
        })
        .unwrap();
        assert_eq!(
            action::decode(&graph.emit().unwrap()).unwrap().groups[0].removal[0].native,
            original
        );
        children
            .edit(
                &mut graph,
                Edit::Replace(0, NativeNode::condition(14).unwrap()),
            )
            .unwrap();
        let decoded = action::decode(&graph.emit().unwrap()).unwrap();
        assert_eq!(decoded.groups[0].removal[0].native, original);
        assert_eq!(decoded.groups[0].activation[0].children[0].kind, 14);
        assert_eq!(
            decoded.groups[0].activation[0].children[0]
                .accumulator_row
                .as_ref()
                .unwrap()
                .success_value
                .to_bits(),
            0x7fc01234
        );
        children.edit(&mut graph, Edit::Remove(0)).unwrap();
        assert!(
            action::decode(&graph.emit().unwrap()).unwrap().groups[0].activation[0]
                .children
                .is_empty()
        );
        assert_eq!(
            action::decode(&graph.emit().unwrap()).unwrap().groups[0].removal[0].native,
            original
        );
    }

    #[test]
    fn nested_predicates_and_requirements_can_be_created_without_raw_allocations() {
        let mut graph = NativeProgram::empty().graph;
        edit(
            &mut graph,
            0,
            Part::Trigger,
            Edit::Add(NativeNode::condition(31).unwrap()),
        )
        .unwrap();
        let parent = List::group(0, Part::Trigger).node(0).unwrap();
        let groups = List {
            owner: parent.clone(),
            field: 0x10,
            class: action::SUBGROUP_ROW_CLASS,
        };
        let count = action::decode(&graph.emit().unwrap()).unwrap().groups[0].activation[0]
            .subgroups
            .len();
        groups.edit(&mut graph, Edit::AddGroup).unwrap();
        let mut owner = parent;
        owner.push(0x10);
        let children = List {
            owner,
            field: count * 0x20 + 0x10,
            class: action::CONDITION_ROW_CLASS,
        };
        children
            .edit(&mut graph, Edit::Add(NativeNode::condition(35).unwrap()))
            .unwrap();
        let predicate = List {
            owner: children.node(0).unwrap(),
            field: 0x100,
            class: 0,
        };
        predicate
            .edit(&mut graph, Edit::Add(NativeNode::condition(1).unwrap()))
            .unwrap();
        let decoded = action::decode(&graph.emit().unwrap()).unwrap();
        assert_eq!(
            decoded.groups[0].activation[0].subgroups[count].conditions[0].children[0].kind,
            1
        );
        predicate.edit(&mut graph, Edit::Remove(0)).unwrap();
        groups.edit(&mut graph, Edit::Remove(count)).unwrap();
        assert_eq!(
            action::decode(&graph.emit().unwrap()).unwrap().groups[0].activation[0]
                .subgroups
                .len(),
            count
        );
    }

    fn kinds(graph: &Graph, group: usize) -> Vec<u8> {
        action::decode(&graph.emit().unwrap()).unwrap().groups[group]
            .effects
            .iter()
            .rev()
            .map(|node| node.kind)
            .collect()
    }

    #[test]
    fn action_edits_follow_execution_order_and_leave_shared_lists_untouched() {
        let mut graph = NativeProgram::empty().graph;
        for kind in [47, 6, 30] {
            edit(
                &mut graph,
                0,
                Part::Actions,
                Edit::Add(NativeNode::effect(kind).unwrap()),
            )
            .unwrap();
        }
        assert_eq!(kinds(&graph, 0), [47, 6, 30]);
        add_group(&mut graph).unwrap();
        let groups = graph.blocks[0].links[&0x70];
        let shared = graph.blocks[0].links[&0x40];
        graph.blocks[groups].links.insert(0x20, shared);
        graph.synchronize_counts().unwrap();
        let original = action::decode(&graph.emit().unwrap()).unwrap().groups[0]
            .effects
            .clone();
        edit(&mut graph, 1, Part::Actions, Edit::Move(0, 2)).unwrap();
        assert_eq!(kinds(&graph, 1), [6, 30, 47]);
        edit(&mut graph, 1, Part::Actions, Edit::Remove(1)).unwrap();
        assert_eq!(kinds(&graph, 1), [6, 47]);
        let after = action::decode(&graph.emit().unwrap()).unwrap();
        for (a, b) in original.iter().zip(&after.groups[0].effects) {
            assert_eq!(a.native, b.native);
        }
        add_group(&mut graph).unwrap();
        assert!(kinds(&graph, 2).is_empty());
        remove_group(&mut graph, 1).unwrap();
        assert_eq!(kinds(&graph, 0), [47, 6, 30]);
        assert!(kinds(&graph, 1).is_empty());
        let before = graph.clone();
        assert!(remove_group(&mut graph, 0).is_err());
        assert!(edit(&mut graph, 1, Part::Actions, Edit::Remove(10)).is_err());
        assert_eq!(graph, before);
    }

    #[test]
    fn replacing_an_aliased_condition_changes_only_the_selected_list() {
        let mut graph = NativeProgram::empty().graph;
        let node = NativeNode::condition(1).unwrap();
        edit(&mut graph, 0, Part::Ending, Edit::Add(node)).unwrap();
        let shared = graph.blocks[0].links[&0x50];
        graph.blocks[0].links.insert(0x60, shared);
        graph.synchronize_counts().unwrap();
        let before = action::decode(&graph.emit().unwrap()).unwrap().groups[0].rearm[0]
            .native
            .clone();
        edit(
            &mut graph,
            0,
            Part::Ending,
            Edit::Replace(0, NativeNode::condition(14).unwrap()),
        )
        .unwrap();
        let decoded = action::decode(&graph.emit().unwrap()).unwrap();
        assert_eq!(decoded.groups[0].removal[0].kind, 14);
        assert_eq!(decoded.groups[0].rearm[0].native, before);
    }
}
