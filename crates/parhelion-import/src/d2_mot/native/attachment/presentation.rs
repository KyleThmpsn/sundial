//! Explicit cosmetic differences preserve the surrounding gameplay schedule.
use super::*;

#[derive(Default)]
pub(super) struct Plan {
    pub events: BTreeMap<u32, BTreeSet<usize>>,
    pub query_actions: BTreeMap<u32, BTreeSet<usize>>,
    pub report: Vec<Value>,
}

fn occurrences(p: &Payload, name: u32) -> usize {
    p.0.windows(4)
        .filter(|bytes| *bytes == name.to_le_bytes())
        .count()
}

/// A timed visual graph may have a marker used to deduplicate itself. It cannot
/// publish numeric values or carry health, modifiers, hosted perks or gameplay
/// sequence events. No other imported record or stock action may read its marker.
fn visual_graph(closure: &closure::Closure, native: &mut Reader, tag: u32) -> Result<Vec<u32>> {
    let payload = closure
        .graphs
        .get(&tag)
        .context("conditional visual graph is outside the closure")?;
    let graph = Graph::read(payload, true)?;
    let mut markers = Vec::new();
    for &owner in &graph.components {
        let p = &closure.owners[&owner];
        let d = p.pointer(24)?;
        match p.u32(d - 4)? {
            0x80803F64 | 0x80808EDD => {}
            0x80809597 => {
                ensure!(
                    p.array(d + 0x168, 12, Some(0x808095A8))?.is_empty(),
                    "conditional presentation publishes values outside its graph"
                );
            }
            0x80808179 => {
                sequence::Sequence::read(p)?;
                ensure!(
                    p.array(d + 0x1F8, 56, Some(0x8080816F))?.is_empty(),
                    "conditional presentation publishes sequence values"
                );
                for row in p.array(d + 0x1D8, 24, Some(0x808091F1))? {
                    let event = p.pointer(row + 16)?;
                    ensure!(
                        matches!(
                            (p.u32(event - 4)?, p.u32(event + 0x1C)?),
                            (0x808091D7, 0)
                                | (0x808067B9, 4)
                                | (0x808066AD, 5)
                                | (0x80806640, 6)
                                | (0x808091CF, 23)
                                | (0x80806A52, 33)
                                | (0x8080694C, 34)
                        ),
                        "conditional presentation contains a gameplay event"
                    );
                }
            }
            0x8080B11F => {
                ensure!(
                    p.u32(d + 0x610)? == u32::MAX && p.u64(d + 0x618)? == 0,
                    "conditional presentation creates another timed child"
                );
                let marker = p.u32(d + 0x2A8)?;
                if !matches!(marker, 0 | u32::MAX | 0x811C9DC5) {
                    markers.push(marker);
                }
            }
            class => {
                anyhow::bail!("conditional graph component {class:08X} is not presentation-only")
            }
        }
    }
    for &marker in &markers {
        let count: usize = closure
            .owners
            .values()
            .chain(closure.graphs.values())
            .chain(closure.actions.values())
            .map(|p| occurrences(p, marker))
            .sum::<usize>()
            + closure
                .resources
                .values()
                .map(|r| occurrences(&r.payload, marker))
                .sum::<usize>();
        ensure!(
            count == 1,
            "conditional visual marker has another imported consumer"
        );
    }
    if !markers.is_empty() {
        let actions = native.classes(0x808040B5);
        ensure!(
            !actions.is_empty(),
            "native action corpus is empty during visual marker inspection"
        );
        for tag in actions {
            let p = native.tag(tag, Some(0x808040B5))?;
            ensure!(
                markers.iter().all(|&marker| occurrences(&p, marker) == 0),
                "conditional visual marker is used by a native action"
            );
        }
    }
    Ok(markers)
}

impl Plan {
    pub fn sounds(
        &mut self,
        source: &Reader,
        closure: &closure::Closure,
        skipped: &BTreeMap<u32, u32>,
    ) -> Result<()> {
        for (&owner, p) in &closure.owners {
            let d = p.pointer(24)?;
            if p.u32(d - 4)? != 0x80808179 {
                continue;
            }
            for row in p.array(d + 0x1D8, 24, Some(0x808091F1))? {
                let event = p.pointer(row + 16)?;
                if p.u32(event - 4)? != 0x80806640 {
                    continue;
                }
                let cue = source.ref64(p, event + 0x50)?;
                if let Some(bank) = skipped.get(&cue) {
                    self.events.entry(owner).or_default().insert(event);
                    self.report.push(json!({"owner":owner,"event_offset":event,"cue":cue,"bank":bank,
                        "unsupported_hirc_type":22,
                        "difference":"An effect sound is omitted because its bank contains an unsupported audio object. Its sequence timing is retained."}));
                }
            }
        }
        Ok(())
    }

    pub fn read(source: &Reader, native: &mut Reader, closure: &closure::Closure) -> Result<Self> {
        let mut plan = Self::default();
        for (&owner, p) in &closure.owners {
            let d = p.pointer(24)?;
            if p.u32(d - 4)? != 0x80808179 {
                continue;
            }
            for row in p.array(d + 0x1D8, 24, Some(0x808091F1))? {
                let event = p.pointer(row + 16)?;
                let class = p.u32(event - 4)?;
                if matches!(class, 0x808066AD | 0x808091CF | 0x80806A52 | 0x8080694C) {
                    plan.events.entry(owner).or_default().insert(event);
                    plan.report.push(json!({"owner":owner,"event_offset":event,"class":class,
                        "difference":"An auxiliary renderer is omitted. Its sequence timing and event ordinal are retained."}));
                }
                if class != 0x80803F24 {
                    continue;
                }
                for row in p.array(event + 0x160, 96, Some(0x80803F2A))? {
                    let action = p.pointer(row + 88)?;
                    if p.u32(action - 4)? != 0x8080211B {
                        continue;
                    }
                    // An evaluated probability consumes the shared random state
                    // even when it is a literal one. Only empty gates may vanish.
                    ensure!(
                        p.bytes::<64>(row)? == [0; 64]
                            && p.u64(action + 0x20)? == 0
                            && p.u64(action + 0x28)? == 0,
                        "conditional presentation has a probability or auxiliary program"
                    );
                    let children = p.array(action + 0x30, 96, Some(0x80803F2A))?;
                    ensure!(
                        !children.is_empty(),
                        "conditional presentation branch is empty"
                    );
                    let mut targets = Vec::new();
                    for child in children {
                        let node = p.pointer(child + 88)?;
                        ensure!(
                            p.bytes::<64>(child)? == [0; 64]
                                && p.u64(child + 64)? == 0
                                && p.u32(child + 72)? == u32::MAX
                                && p.u32(child + 76)? == 1
                                && p.u64(child + 80)? == 0
                                && p.u32(node - 4)? == 0x80803F31
                                && p.u64(node)? == 0,
                            "conditional presentation contains another action or probability"
                        );
                        let graph = source.ref64(p, node + 8)?;
                        let markers = visual_graph(closure, native, graph)?;
                        targets.push(json!({"graph":graph,"visual_markers":markers}));
                    }
                    plan.query_actions.entry(owner).or_default().insert(action);
                    plan.report.push(json!({"owner":owner,"query_action_offset":action,"targets":targets,
                        "difference":"A conditional visual attachment is omitted. Its visual marker has no other imported or native-action consumer."}));
                }
            }
        }
        Ok(plan)
    }

    pub fn particles(&mut self, closure: &closure::Closure, converted: &Value) -> Result<()> {
        for (&owner, p) in &closure.owners {
            let d = p.pointer(24)?;
            if p.u32(d - 4)? != 0x80808179 {
                continue;
            }
            for row in p.array(d + 0x1D8, 24, Some(0x808091F1))? {
                let event = p.pointer(row + 16)?;
                if p.u32(event - 4)? != 0x808067B9 {
                    continue;
                }
                let mut missing = Vec::new();
                for row in p.array(event + 0x28, 24, Some(0x808067BB))? {
                    for field in sequence::presentation::fields(p, row)? {
                        let tag = p.u32(field)?;
                        if converted["systems"][format!("{tag:08X}")]
                            .as_str()
                            .is_none()
                        {
                            missing.push(tag);
                        }
                    }
                }
                if !missing.is_empty() {
                    self.events.entry(owner).or_default().insert(event);
                    self.report.push(json!({"owner":owner,"event_offset":event,"unconverted_particles":missing,
                        "difference":"A particle event is omitted because a required visual system could not be translated. Its sequence timing is retained."}));
                }
            }
        }
        Ok(())
    }
}
