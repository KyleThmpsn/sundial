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
    /// Nodes added in order, all or none.
    AddAll(Vec<NativeNode>),
    Replace(usize, NativeNode),
    Remove(usize),
    Move(usize, usize),
    /// Requires this condition together with the list's current alternatives.
    Require(NativeNode),
    /// Rows moved in whole from a list of the same class, nested records and all.
    Append(Vec<Row>),
    Clear,
    /// A workbench trigger preset on a group's trigger list, with the ending it brings.
    Preset(Trigger),
    /// Moves the list's first condition inside a state check that the weapon is in hand, as
    /// Grave Robber holds its melee kill.
    Hold,
    /// Takes the first condition back out of the state check `Hold` put it in.
    Release,
}

/// One list row: its own bytes and the records it links, by offset within the row.
pub(super) type Row = (Vec<u8>, BTreeMap<usize, usize>);

/// Every requirement of this kind must pass, and each requirement is a list of alternatives.
const ALL_REQUIREMENTS: u8 = 31;

/// Seconds a new requirement stays met after it passes. Stock requirements that wait on an
/// event hold it, as Trinity Ghoul's kill and state requirements both do for 0.1 seconds.
const REQUIREMENT_HOLD: f32 = 0.1;

/// How long a kill holds what its actions change: Outlaw, Killing Wind, Impetus, Blood Magic
/// and Assassin's Blade all hold theirs for 5 seconds, the most common stock kill timer.
const KILL_HOLD_MS: u32 = 5_000;

/// State Check with a Condition, the predicate that holds one required condition to a state.
pub(super) const STATE_CHECK: u8 = 35;
/// Where a state check keeps its required condition.
const REQUIRED_CONDITION: usize = 0x100;
/// Weapon State's Holding the Weapon bit. The weapon perks that apply while the weapon is in
/// hand set it, as Celerity does, and Grave Robber sets it around its melee kill.
pub(super) const HOLDING_THE_WEAPON: u8 = 1;

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
    /// The rows of this list as they stand, without copying anything.
    fn rows(&self, graph: &Graph) -> Result<Vec<Row>, String> {
        let owner = resolve(graph, &self.owner)?;
        let Some(&list) = graph.blocks[owner].links.get(&self.field) else {
            return Ok(Vec::new());
        };
        let stride = schema::record(self.class)?.size;
        let block = &graph.blocks[list];
        let count = block.count.ok_or("Missing list length.")?;
        Ok((0..count)
            .map(|i| {
                (
                    block.bytes[i * stride..(i + 1) * stride].to_vec(),
                    block
                        .links
                        .range(i * stride..(i + 1) * stride)
                        .map(|(at, target)| (at - i * stride, *target))
                        .collect(),
                )
            })
            .collect())
    }

    /// And. A list that is already one All Requirements condition gains a requirement.
    /// Otherwise that condition takes the list's place, with the current alternatives as its
    /// first requirement and `node` as its second, so (A or B) and C keeps A and B intact.
    fn require(&self, graph: &mut Graph, node: NativeNode) -> Result<(), String> {
        if self.class != action::CONDITION_ROW_CLASS {
            return Err("Only a condition list can require another condition.".into());
        }
        let mut changed = graph.clone();
        let rows = self.rows(&changed)?;
        if rows.is_empty() {
            self.edit(&mut changed, Edit::Add(node))?;
            *graph = changed;
            return Ok(());
        }
        let class = nodes::condition(ALL_REQUIREMENTS)
            .ok_or("Unknown condition kind.")?
            .class;
        let wrapped = rows.len() == 1
            && rows[0]
                .1
                .get(&0)
                .is_some_and(|&target| changed.blocks[target].class == class);
        if !wrapped {
            self.edit(&mut changed, Edit::Clear)?;
            self.edit(
                &mut changed,
                Edit::Add(NativeNode::condition(ALL_REQUIREMENTS).ok_or("Missing template.")?),
            )?;
        }
        let all = self.node(0)?;
        let requirements = List {
            owner: all.clone(),
            field: 0x10,
            class: action::SUBGROUP_ROW_CLASS,
        };
        let requirement = |row: usize| {
            let mut owner = all.clone();
            owner.push(0x10);
            List {
                owner,
                field: row * 0x20 + 0x10,
                class: action::CONDITION_ROW_CLASS,
            }
        };
        let mut count = requirements.rows(&changed)?.len();
        if !wrapped {
            // The new condition starts empty, so any requirements it came with go first.
            while count > 0 {
                requirements.edit(&mut changed, Edit::Remove(count - 1))?;
                count -= 1;
            }
            requirements.edit(&mut changed, Edit::AddGroup)?;
            requirement(0).edit(&mut changed, Edit::Append(rows))?;
            count = 1;
        }
        requirements.edit(&mut changed, Edit::AddGroup)?;
        requirement(count).edit(&mut changed, Edit::Add(node))?;
        *graph = changed;
        Ok(())
    }

    /// A trigger preset replaces the trigger list. Its ending, and the reactivation it allows,
    /// replace the ones a trigger adds for itself (a timer, or a weapon event's own end), as
    /// choosing a trigger did on a guided card. An ending someone chose stays.
    fn preset(&self, graph: &mut Graph, trigger: Trigger) -> Result<(), String> {
        let preset = preset(trigger)?;
        let nodes = |conditions: &[action::DecodedCondition]| {
            conditions
                .iter()
                .map(|node| NativeNode {
                    kind: node.kind,
                    bytes: node.native.clone(),
                })
                .collect::<Vec<_>>()
        };
        let mut changed = graph.clone();
        self.replace_rows(&mut changed, nodes(&preset.activation))?;
        let sibling = |part: Part| List {
            owner: self.owner.clone(),
            field: self.field - Part::Trigger.offset() + part.offset(),
            class: part.class(),
        };
        let (actions, ending) = (sibling(Part::Actions), sibling(Part::Ending));
        // A kill keeps a Duration or At Once it already ends with, so a Duration someone set
        // survives changing which kill fires the effect.
        let kept = trigger.is_event() && matches!(ending.kinds(&changed)?[..], [Some(0 | 1)]);
        if !kept && ending.defaults_only(&changed)? {
            let removal = if !trigger.is_event() {
                nodes(&preset.removal)
            } else if actions.holds_state(&changed)? {
                kill_timer()?
            } else {
                vec![always()?]
            };
            ending.replace_rows(&mut changed, removal)?;
        }
        let rearm = sibling(Part::Rearm);
        if !trigger.supports_cooldown() && rearm.defaults_only(&changed)? {
            rearm.replace_rows(&mut changed, Vec::new())?;
        }
        // Only a kill supplies an event to place things at, so a spawn or an orb drop placed
        // there falls back to the player, as a guided card's trigger change did.
        if !trigger.is_event() {
            for row in 0..actions.rows(&changed)?.len() {
                scoped(&mut changed, &actions.node(row)?, |graph, index| {
                    let block = &mut graph.blocks[index];
                    let at = match nodes::EFFECTS.iter().find(|node| node.class == block.class) {
                        Some(node) if node.kind == 3 => Some(4),
                        Some(node) if node.kind == 5 => Some(2),
                        _ => None,
                    };
                    if let Some(byte) = at.and_then(|at| block.bytes.get_mut(at)) {
                        *byte = 0;
                    }
                    Ok(())
                })?;
            }
        }
        *graph = changed;
        Ok(())
    }

    /// Replaces the list's conditions, leaving a list that holds none untouched when none
    /// replace them, so an absent list is not written as an empty one.
    fn replace_rows(&self, graph: &mut Graph, nodes: Vec<NativeNode>) -> Result<(), String> {
        if nodes.is_empty() && self.rows(graph)?.is_empty() {
            return Ok(());
        }
        self.edit(graph, Edit::Clear)?;
        for node in nodes {
            self.edit(graph, Edit::Add(node))?;
        }
        Ok(())
    }

    /// Whether every condition in the list is one a trigger adds for itself: a timer, the
    /// unequip or holster that ends a weapon trigger, or the Always that ends a kill at once.
    fn defaults_only(&self, graph: &Graph) -> Result<bool, String> {
        Ok(self
            .kinds(graph)?
            .iter()
            .all(|kind| matches!(kind, Some(0 | 1 | 15 | 17))))
    }

    /// The node kind of each row, when its node is a known one.
    fn kinds(&self, graph: &Graph) -> Result<Vec<Option<u8>>, String> {
        let table: &[nodes::NodeKind] = if self.class == action::EFFECT_ROW_CLASS {
            &nodes::EFFECTS
        } else {
            &nodes::CONDITIONS
        };
        Ok(self
            .rows(graph)?
            .iter()
            .map(|(_, links)| {
                links.get(&0).and_then(|&target| {
                    table
                        .iter()
                        .find(|node| node.class == graph.blocks[target].class)
                        .map(|node| node.kind)
                })
            })
            .collect())
    }

    /// Whether any action in this list changes something its effect's ending undoes, so an
    /// effect that ended as it started would undo it as it applied.
    fn holds_state(&self, graph: &Graph) -> Result<bool, String> {
        Ok(self.rows(graph)?.iter().any(|(_, links)| {
            links
                .get(&0)
                .is_some_and(|&target| undone_by_ending(&graph.blocks[target]))
        }))
    }

    /// The list holding `part` in the same behavior group, when this list is one of a group's
    /// own lists holding `from`.
    fn sibling(&self, from: Part, part: Part) -> Option<List> {
        let base = self.field.checked_sub(from.offset() + 8)?;
        let grouped = match self.owner.as_slice() {
            [] => base == 0x20,
            [0x70] => base % 0x48 == 0,
            _ => false,
        };
        grouped.then(|| List {
            owner: self.owner.clone(),
            field: base + part.offset() + 8,
            class: part.class(),
        })
    }

    /// A kill that ends at once, as its preset leaves one whose actions happen once, takes a
    /// timer when an action added to it holds state, since ending as it starts would undo a
    /// stat change or an attachment the moment it applied. The timer is the effect's Duration.
    fn fit_ending(&self, graph: &mut Graph) -> Result<(), String> {
        let (Some(trigger), Some(ending)) = (
            self.sibling(Part::Actions, Part::Trigger),
            self.sibling(Part::Actions, Part::Ending),
        ) else {
            return Ok(());
        };
        if ending.kinds(graph)? == [Some(0)]
            && trigger.fires_on_kill(graph)?
            && self.holds_state(graph)?
        {
            ending.replace_rows(graph, kill_timer()?)?;
        }
        Ok(())
    }

    /// Whether a kill fires this trigger list: a kill condition in it, or one it requires or
    /// counts, as an And or a counter of kills holds it.
    fn fires_on_kill(&self, graph: &Graph) -> Result<bool, String> {
        let kill = nodes::condition(2).ok_or("Unknown condition kind.")?.class;
        let mut pending = self
            .rows(graph)?
            .into_iter()
            .filter_map(|(_, links)| links.get(&0).copied())
            .collect::<Vec<_>>();
        let mut seen = std::collections::BTreeSet::new();
        while let Some(index) = pending.pop() {
            if !seen.insert(index) {
                continue;
            }
            if graph.blocks[index].class == kill {
                return Ok(true);
            }
            pending.extend(graph.blocks[index].links.values().copied());
        }
        Ok(false)
    }

    pub fn edit(&self, graph: &mut Graph, edit: Edit) -> Result<(), String> {
        match edit {
            Edit::Require(node) => self.require(graph, node),
            Edit::Preset(trigger) => self.preset(graph, trigger),
            Edit::AddAll(nodes) => self.add_all(graph, nodes),
            Edit::Hold => self.hold(graph),
            Edit::Release => self.release(graph),
            edit => self.apply(graph, edit),
        }
    }

    /// The row edits: add, replace, remove, move, clear or append rows, add a group's row, or
    /// set a predicate's one condition.
    fn apply(&self, graph: &mut Graph, edit: Edit) -> Result<(), String> {
        let added = matches!(edit, Edit::Add(_) | Edit::Replace(..));
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
                    let mut bytes = vec![0; stride];
                    bytes[..4].copy_from_slice(&REQUIREMENT_HOLD.to_le_bytes());
                    rows.push((bytes, BTreeMap::new()))
                }
                Edit::Append(moved) if rows.len() + moved.len() <= 256 => rows.extend(moved),
                Edit::Clear => rows.clear(),
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
        if added && self.class == action::EFFECT_ROW_CLASS {
            self.fit_ending(&mut changed)?;
        }
        changed.validate_program()?;
        *graph = changed;
        Ok(())
    }

    /// Moves the first condition inside a State Check with a Condition whose Weapon State is
    /// Holding the Weapon, the shape Grave Robber's melee kill takes, so it counts only while
    /// the weapon is in hand. The condition moves whole, with its filters and chance.
    fn hold(&self, graph: &mut Graph) -> Result<(), String> {
        let target = self
            .rows(graph)?
            .first()
            .and_then(|(_, links)| links.get(&0).copied())
            .ok_or("There is no trigger to hold.")?;
        let inner = captured(graph, target)?;
        let mut check =
            NativeNode::condition(STATE_CHECK).ok_or("The state check has no template.")?;
        let class = nodes::condition(STATE_CHECK)
            .ok_or("Unknown condition kind.")?
            .class;
        let state = fields::describe(class)?
            .into_iter()
            .find(|field| field.label == "Weapon State")
            .ok_or("The state check has no Weapon State.")?;
        *check
            .bytes
            .get_mut(state.offset)
            .ok_or("Truncated state check.")? |= HOLDING_THE_WEAPON;
        let mut changed = graph.clone();
        self.edit(&mut changed, Edit::Replace(0, check))?;
        List {
            owner: self.node(0)?,
            field: REQUIRED_CONDITION,
            class: 0,
        }
        .edit(&mut changed, Edit::Add(inner))?;
        *graph = changed;
        Ok(())
    }

    /// Puts the condition a state check holds back in the check's place.
    fn release(&self, graph: &mut Graph) -> Result<(), String> {
        let check = resolve(graph, &self.node(0)?)?;
        let target = *graph.blocks[check]
            .links
            .get(&REQUIRED_CONDITION)
            .ok_or("This trigger holds no condition.")?;
        let inner = captured(graph, target)?;
        self.edit(graph, Edit::Replace(0, inner))
    }

    /// Adds the nodes in order. The graph changes only if every one is accepted.
    fn add_all(&self, graph: &mut Graph, nodes: Vec<NativeNode>) -> Result<(), String> {
        let mut changed = graph.clone();
        for node in nodes {
            self.edit(&mut changed, Edit::Add(node))?;
        }
        *graph = changed;
        Ok(())
    }
}

#[cfg(test)]
pub(super) fn edit(graph: &mut Graph, group: usize, part: Part, edit: Edit) -> Result<(), String> {
    List::group(group, part).edit(graph, edit)
}

/// The trigger, ending and reactivation a workbench trigger preset compiles to.
pub(super) fn preset(trigger: Trigger) -> Result<action::DecodedGroup, String> {
    lasting(trigger, Program::default().duration_ms)
}

/// A trigger preset whose timer ending, where it has one, lasts `duration_ms`.
fn lasting(trigger: Trigger, duration_ms: u32) -> Result<action::DecodedGroup, String> {
    let draft = Program {
        trigger,
        duration_ms,
        ..Program::default()
    };
    let payload = sundial::package_authoring::sandbox_perk::program::native_draft(&draft)?
        .graph
        .emit()?;
    action::decode(&payload)?
        .groups
        .into_iter()
        .next()
        .ok_or_else(|| "The trigger could not be built.".into())
}

/// Whether a trigger condition is a kill held to the weapon in hand, as `Hold` leaves it and
/// Grave Robber's is: a state check whose Weapon State is Holding the Weapon, around one kill.
pub(super) fn held_kill(condition: &action::DecodedCondition) -> bool {
    let holding = fields::describe(condition.class)
        .ok()
        .and_then(|fields| {
            fields
                .into_iter()
                .find(|field| field.label == "Weapon State")
        })
        .and_then(|field| condition.native.get(field.offset).copied())
        .is_some_and(|bits| bits & HOLDING_THE_WEAPON != 0);
    condition.kind == STATE_CHECK
        && holding
        && matches!(condition.children.as_slice(), [kill] if kill.kind == 2)
}

/// Whether a kill counts whichever weapon is in hand: one This Weapon Only does not already tie
/// to this weapon, such as On Melee Kill.
pub(super) fn loose_kill(condition: &action::DecodedCondition) -> bool {
    condition.kind == 2
        && !condition.facts.iter().any(|fact| {
            fact.label == "Requires Owning Weapon"
                && matches!(fact.value, action::FactValue::Flag(true))
        })
}

/// A condition as a node of its own, with every record it holds, ready to be added elsewhere.
fn captured(graph: &Graph, target: usize) -> Result<NativeNode, String> {
    let class = graph.blocks[target].class;
    let kind = nodes::CONDITIONS
        .iter()
        .find(|node| node.class == class)
        .ok_or("Unknown condition kind.")?
        .kind;
    let (bytes, starts) = graph.emit_with_offsets()?;
    let start = *starts
        .get(&target)
        .ok_or("This condition is no longer part of the effect.")?;
    Ok(NativeNode {
        kind,
        bytes: native::capture(&bytes, start, class)?,
    })
}

/// The timer a kill ends on when its actions hold state.
fn kill_timer() -> Result<Vec<NativeNode>, String> {
    Ok(lasting(Trigger::WeaponKill, KILL_HOLD_MS)?
        .removal
        .iter()
        .map(|node| NativeNode {
            kind: node.kind,
            bytes: node.native.clone(),
        })
        .collect())
}

/// The ending of an effect that ends as it starts, so the next kill fires it again.
pub(super) fn always() -> Result<NativeNode, String> {
    NativeNode::condition(0).ok_or_else(|| "The Always condition has no template.".into())
}

/// Whether ending the effect undoes what this action did: any action that keeps state to clean
/// up, as its Retain Effect State byte records, except an attachment that ends on its own or
/// sits on the killed target. Stock kill perks draw nearly the same line. Of the 258, 148 of
/// the 196 whose actions leave nothing to undo end at once, as Firefly's explosion on the
/// killed target does, and 50 of the 62 whose actions hold state last, as Outlaw's buff does.
fn undone_by_ending(block: &native::Block) -> bool {
    if block.bytes.get(1).is_none_or(|flag| *flag == 0) {
        return false;
    }
    let attaches = nodes::EFFECTS
        .iter()
        .any(|node| node.class == block.class && matches!(node.kind, 1 | 2));
    if !attaches {
        return true;
    }
    let value = |label: &str| {
        let field = fields::describe(block.class)
            .ok()?
            .into_iter()
            .find(|field| field.label == label)?;
        match field.bytes(block, 0)? {
            [byte] => Some(u32::from(*byte)),
            [a, b, c, d] => Some(u32::from_le_bytes([*a, *b, *c, *d])),
            _ => None,
        }
    };
    // Attach To's Other Combatant is the killed target when a kill fires the effect.
    let on_target = value(action::CREATE_ENTITY_MODE_LABEL) == Some(3);
    // Only an empty lifetime key retires the attachment with the effect.
    let own_lifetime = value(action::CREATE_ENTITY_KEY_LABELS[0])
        .is_some_and(|key| key != 0 && key != sundial::package_authoring::FNV1_EMPTY_HASH);
    !on_target && !own_lifetime
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

    /// A trigger preset replaces the endings a trigger implies and keeps one someone chose.
    /// Leaving a kill trigger places event spawns and orb drops at the player, since only a
    /// kill supplies an event.
    #[test]
    fn trigger_presets_replace_implied_endings_and_reset_event_positions() {
        use sundial::package_authoring::sandbox_perk::program::native_draft;
        let trigger = List::group(0, Part::Trigger);
        let kinds = |graph: &Graph| {
            let decoded = action::decode(&graph.emit().unwrap()).unwrap();
            let group = &decoded.groups[0];
            let kinds = |list: &[action::DecodedCondition]| {
                list.iter().map(|node| node.kind).collect::<Vec<_>>()
            };
            (
                kinds(&group.activation),
                kinds(&group.removal),
                kinds(&group.rearm),
            )
        };
        let mut graph = native_draft(&Program {
            trigger: Trigger::WeaponKill,
            cooldown_ms: 2_000,
            actions: vec![Action::add_rounds(1)],
            ..Program::default()
        })
        .unwrap()
        .graph;
        trigger
            .edit(&mut graph, Edit::Preset(Trigger::Drawn))
            .unwrap();
        assert_eq!(kinds(&graph), (vec![16], vec![17], vec![]));
        trigger
            .edit(&mut graph, Edit::Preset(Trigger::WeaponKill))
            .unwrap();
        assert_eq!(kinds(&graph), (vec![2], vec![0], vec![]));

        let mut graph = native_draft(&Program {
            trigger: Trigger::Always,
            native_removal: NativeNode::condition(29),
            actions: vec![Action::add_rounds(1)],
            ..Program::default()
        })
        .unwrap()
        .graph;
        trigger
            .edit(&mut graph, Edit::Preset(Trigger::WeaponKill))
            .unwrap();
        assert_eq!(kinds(&graph).1, [29]);

        let Action::Native { mut node } = Action::generate_orb(Position::Event) else {
            panic!("orb node")
        };
        node.bytes[8..12].copy_from_slice(&0.25f32.to_le_bytes());
        let mut graph = native_draft(&Program {
            trigger: Trigger::WeaponKill,
            actions: vec![
                Action::Native { node },
                Action::Spawn {
                    asset: Asset {
                        graph: 1,
                        ..Asset::default()
                    },
                    position: Position::Event,
                },
            ],
            ..Program::default()
        })
        .unwrap()
        .graph;
        trigger
            .edit(&mut graph, Edit::Preset(Trigger::Drawn))
            .unwrap();
        let decoded = action::decode(&graph.emit().unwrap()).unwrap();
        let effect = |kind| {
            decoded.groups[0]
                .effects
                .iter()
                .find(|effect| effect.kind == kind)
                .unwrap()
        };
        assert_eq!(effect(5).native[2], 0);
        assert_eq!(effect(5).native[8..12], 0.25f32.to_le_bytes());
        assert_eq!(effect(3).native[4], 0);
    }
}
