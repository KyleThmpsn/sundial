//! Compare persisted native graphs to the mounted source, independently of the converter.
use super::*;

fn resource(p: &Payload, native: bool) -> Result<usize> {
    if native {
        // Follow the instance pair. The wrapper can retain an earlier template resource.
        Ok(usize::try_from(p.u64(p.pointer(16)? + 8)?)?)
    } else {
        p.pointer(24)
    }
}

fn query(p: &Payload, native: bool) -> Result<Value> {
    let r = resource(p, native)?;
    let mut groups = Vec::new();
    for g in p.array(r + if native { 0x80 } else { 0x98 }, 24, None)? {
        let mut filters = Vec::new();
        for f in p.array(g + 8, 40, None)? {
            let mut terms = Vec::new();
            for t in p.array(f + 16, 16, None)? {
                let v = p.pointer(t + 8)?;
                let body = match p.u32(v - 4)? {
                    0x8080295D | 0x80804D76 => json!(["enum", p.u32(v)?]),
                    0x808026B3 | 0x80804D7A => json!(["pair", p.u32(v)?, p.u32(v + 4)?]),
                    0x80802C1C | 0x80804D5F => {
                        let provider = p.pointer(v)?;
                        ensure!(
                            p.u32(provider - 4)? == if native { 0x80804D61 } else { 0x8080306D }
                                && p.u32(provider + 8)? == p.u32(p.pointer(16)?)?
                                && p.u64(provider + 16)? == provider as u64,
                            "Query range provider lost its private owner"
                        );
                        json!(["range", p.u32(v + 8)?, p.u32(v + 12)?, p.u32(v + 16)?])
                    }
                    class => anyhow::bail!("Unchecked query predicate {class:08X}"),
                };
                terms.push(json!([p.u64(t)?, body]));
            }
            filters.push(json!([p.bytes::<16>(f)?, p.bytes::<8>(f + 32)?, terms]));
        }
        groups.push(json!([p.u32(p.pointer(g)?)?, filters]));
    }
    Ok(
        json!({"groups":groups,"parameters":[p.bytes::<8>(r + if native {0x78} else {0x90})?,p.bytes::<24>(r + if native {0xB8} else {0xD8})?]}),
    )
}

struct Sequence<'a> {
    p: &'a Payload,
    native: bool,
    controls: Vec<usize>,
    events: Vec<usize>,
    inputs: Vec<u32>,
    outputs: Vec<u32>,
    visited: BTreeSet<usize>,
}

// Original consumers use an inclusive source maximum and an exclusive native maximum.
// Normalize the emitted interval back to its inclusive Float32 endpoint for comparison.
fn interval(p: &Payload, at: usize, native: bool) -> Result<Value> {
    let maximum = f32::from_bits(p.u32(at + 4)?);
    ensure!(!maximum.is_nan(), "Sequence interval contains NaN");
    let mut maximum = if native {
        if maximum == 0. {
            0x80000001
        } else if maximum > 0. {
            maximum.to_bits() - 1
        } else {
            maximum.to_bits() + 1
        }
    } else {
        maximum.to_bits()
    };
    if f32::from_bits(maximum) == 0. {
        maximum = 0;
    }
    Ok(json!([p.u32(at)?, maximum]))
}

fn sequence_condition(p: &Payload, field: usize, native: bool) -> Result<Value> {
    if p.u64(field)? == 0 {
        return Ok(Value::Null);
    }
    let at = p.pointer(field)?;
    ensure!(
        p.u32(at - 4)? == if native { 0x808093ED } else { 0x8080045E } && p.u32(at)? == 0,
        "Unverified sequence predicate"
    );
    Ok(json!({"range":interval(p,at+4,native)?,"input":p.u32(at+12)?}))
}

impl Sequence<'_> {
    #[allow(clippy::cognitive_complexity)]
    fn node(&mut self, event: bool, index: usize, parent: i16) -> Result<Value> {
        let a = *(if event { &self.events } else { &self.controls })
            .get(index)
            .context("Sequence child index")?;
        ensure!(self.visited.insert(a), "Sequence repeats a child");
        let at = a + if self.native { 16 } else { 0 };
        ensure!(self.p.i16(at + 6)? == parent, "Sequence parent changed");
        let mut result = json!({"name":self.p.u32(at)?,"timing":self.p.bytes::<20>(at + 8)?});
        let kind = self.p.u16(a + if self.native { 0x30 } else { 0x1c })?;
        result["kind"] = json!(kind);
        if event {
            if kind == 0 {
                ensure!(
                    self.p.u32(a - 4)? == if self.native { 0x808093CB } else { 0x808006CE },
                    "Delay event class differs"
                );
                return Ok(result);
            }
            ensure!(kind == 2, "Expected scalar event");
            let body = a + if self.native { 0x38 } else { 0x20 };
            let code = a + if self.native { 0x40 } else { 0x28 };
            let inputs = self.p.array(code + 48, 4, None)?;
            ensure!(
                self.p.u64(code + 32)? == 1
                    && self.p.u32(code + 40)? == 1
                    && if self.native {
                        self.p.u32(code + 44)? == 0 && inputs.is_empty()
                    } else {
                        inputs.len() == 1 && self.p.u32(inputs[0])? == u32::MAX
                    },
                "Scalar event builtin time or dimensions changed"
            );
            result["output"] = json!(
                self.outputs
                    .get(self.p.u32(body + 4)? as usize)
                    .context("Event output name")?
            );
            result["mode"] = json!(self.p.u32(body)?);
            result["code"] = json!(values(self.p, code, 1, 0x80800009)?);
            result["constants"] = json!(values(self.p, code + 16, 16, 0x80800090)?);
        } else {
            result["repeat"] = json!(self.p.u16(a + if self.native { 0x32 } else { 0x1e })?);
            result["entry_condition"] = sequence_condition(
                self.p,
                a + if self.native { 0x48 } else { 0x30 },
                self.native,
            )?;
            result["retirement_condition"] = sequence_condition(
                self.p,
                a + if self.native { 0x50 } else { 0x38 },
                self.native,
            )?;
            let mut children = Vec::new();
            for child in self
                .p
                .array(a + if self.native { 0x38 } else { 0x20 }, 4, None)?
            {
                let selector = self.p.u16(child)?;
                ensure!(selector <= 1, "Unexpected sequence child selector");
                children.push(self.node(
                    selector == 1,
                    self.p.u16(child + 2)? as usize,
                    index as i16,
                )?);
            }
            result["children"] = json!(children);
            let field = a + if self.native { 0x58 } else { 0x40 };
            if kind == 6 {
                result["range"] = interval(self.p, field, self.native)?;
                result["input"] = json!(
                    self.inputs
                        .get(self.p.u32(field + 8)? as usize)
                        .context("Range input name")?
                );
            } else if kind == 2 {
                result["weights"] = json!(values(self.p, field, 4, 0x8080000F)?);
            } else {
                ensure!(matches!(kind, 0 | 1), "Unexpected scalar sequence control");
            }
        }
        Ok(result)
    }
}

fn sequence(p: &Payload, native: bool) -> Result<Value> {
    let r = resource(p, native)?;
    if native {
        // Original root completion at 58BB34 tests bit 1 before retiring its owner.
        // An appearance controller owns these parts and must retain that lifetime.
        ensure!(
            p.u8(r + 0x2e0)? & 2 == 0 && p.u8(p.pointer(24)? + 0x2e0)? & 2 == 0,
            "A scalar presentation sequence retires its appearance part on completion"
        );
    }
    let nodes = |field| -> Result<Vec<usize>> {
        p.array(r + field, if native { 24 } else { 8 }, None)?
            .into_iter()
            .map(|a| p.pointer(a + if native { 16 } else { 0 }))
            .collect()
    };
    let inputs = p
        .array(
            r + if native { 0x188 } else { 0x1a0 },
            if native { 40 } else { 48 },
            None,
        )?
        .into_iter()
        .map(|a| p.u32(a + if native { 32 } else { 40 }))
        .collect::<Result<Vec<_>>>()?;
    let outputs = p
        .array(
            r + if native { 0x178 } else { 0x190 },
            if native { 48 } else { 56 },
            None,
        )?
        .into_iter()
        .map(|a| {
            let i = usize::try_from(p.u64(a + 8)?)?;
            ensure!(
                p.u64(i + 8)? == a as u64 && p.u32(i)? == p.u32(p.pointer(16)?)?,
                "Sequence output pair lost its owner"
            );
            p.u32(a + if native { 40 } else { 48 })
        })
        .collect::<Result<Vec<_>>>()?;
    let mut tree = Sequence {
        p,
        native,
        controls: nodes(if native { 0x158 } else { 0x160 })?,
        events: nodes(if native { 0x168 } else { 0x170 })?,
        inputs,
        outputs,
        visited: BTreeSet::new(),
    };
    let result = tree.node(false, 0, -1)?;
    ensure!(
        tree.visited.len() == tree.controls.len() + tree.events.len(),
        "Unreachable sequence event"
    );
    Ok(
        json!({"tree":result,"inputs":tree.inputs,"outputs":tree.outputs,
        "root_condition":sequence_condition(p,r+if native {0x2D8} else {0x2C8},native)?}),
    )
}

fn programs(p: &Payload, r: usize) -> Result<Vec<String>> {
    let mut programs = Vec::new();
    let declarations = p.array(r + 0xd8, 112, None)?;
    for &a in &declarations {
        if p.u64(a + 8)? == 0 {
            continue;
        }
        let inputs = p
            .array(a + 56, 4, None)?
            .into_iter()
            .map(|input| {
                let ordinal = usize::try_from(p.u32(input)?)?;
                p.u32(
                    *declarations
                        .get(ordinal)
                        .context("Procedure input declaration")?,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        programs.push(
            json!({"name":p.u32(a)?,"inputs":inputs,
            "code":values(p,a+8,1,0x80800009)?,
            "constants":values(p,a+24,16,0x80800090)?})
            .to_string(),
        );
    }
    Ok(programs)
}

fn getter(
    owner: &Payload,
    provider: &Payload,
    def: usize,
    output: usize,
    native: bool,
) -> Result<String> {
    let r = resource(owner, native)?;
    let defs = owner.array(r + 0xc8, if native { 32 } else { 40 }, None)?;
    let ordinal = defs
        .iter()
        .position(|a| *a == def)
        .context("Connected bank input")?;
    let map = owner.array(r + 0x118, 12, None)?;
    let name = map
        .into_iter()
        .find(|&a| owner.u32(a + 8).ok() == Some(ordinal as u32))
        .context("Connected bank input name")?;
    let published = provider.pointer(output)?;
    let pr = resource(provider, native)?;
    let sequence = matches!(provider.u32(pr - 4)?, 0x80800610 | 0x808084E9);
    let name_at = if sequence {
        published + if native { 40 } else { 48 }
    } else {
        output + 24
    };
    Ok(json!([
        "getter",
        owner.u32(name + 4)?,
        sequence,
        provider.u32(name_at)?
    ])
    .to_string())
}

fn sequence_input(
    owner: &Payload,
    provider: &Payload,
    def: usize,
    output: usize,
    ordinal: usize,
    native: bool,
) -> Result<String> {
    let name = owner.u32(def + if native { 32 } else { 40 })?;
    let bank = resource(provider, native)?;
    // Follow the bound provider, not graph metadata or the emitter's list of names.
    ensure!(
        output == usize::try_from(provider.u64(provider.pointer(16)? + 8)?)? + 0x60,
        "Sequence input does not address the bank getter"
    );
    let declarations = provider.array(bank + 0xd8, 112, None)?;
    let declared = provider.u32(
        *declarations
            .get(ordinal)
            .context("Sequence input bank ordinal")?,
    )?;
    Ok(json!(["sequence_input", name, declared]).to_string())
}

fn connections(
    entity: &Payload,
    owners: &BTreeMap<u32, Payload>,
    native: bool,
) -> Result<Vec<String>> {
    let rows = entity.array(
        if native { 0x20 } else { 0x30 },
        if native { 72 } else { 88 },
        None,
    )?;
    let mut links = Vec::new();
    for row in rows {
        let input_class = entity.u32(row + if native { 12 } else { 24 })?;
        if !matches!(
            input_class,
            0x80800668 | 0x808097A8 | 0x80800927 | 0x80809789
        ) {
            continue;
        }
        let owner = owners
            .get(&entity.u32(row + 8)?)
            .context("Connected input owner")?;
        let def = usize::try_from(entity.u64(row + 16)?)?;
        let r = resource(owner, native)?;
        let provider_tag = entity.u32(row + if native { 40 } else { 48 })?;
        let provider = owners
            .get(&provider_tag)
            .context("Connected output owner")?;
        let output = usize::try_from(entity.u64(row + if native { 48 } else { 56 })?)?;
        if matches!(input_class, 0x80800668 | 0x808097A8) {
            links.push(getter(owner, provider, def, output, native)?);
        } else if matches!(owner.u32(r - 4)?, 0x80800610 | 0x808084E9) {
            let ordinal = usize::try_from(entity.u64(row + if native { 64 } else { 80 })?)?;
            links.push(sequence_input(
                owner, provider, def, output, ordinal, native,
            )?);
        }
    }
    links.sort();
    Ok(links)
}

fn snapshot(entity: &Payload, owners: &BTreeMap<u32, Payload>, native: bool) -> Result<Value> {
    let mut queries = Vec::new();
    let mut sequences = Vec::new();
    let mut procedures = Vec::new();
    for p in owners.values() {
        let r = resource(p, native)?;
        let class = p.u32(r - 4)?;
        if matches!(class, 0x808008F6 | 0x80809790) {
            procedures.extend(programs(p, r)?);
        }
        if matches!(class, 0x80803136 | 0x80803820) {
            queries.push(query(p, native)?.to_string());
        }
        if matches!(class, 0x80800610 | 0x808084E9)
            && p.u64(r + if native { 0x178 } else { 0x190 })? != 0
        {
            sequences.push(sequence(p, native)?.to_string());
        }
    }
    let links = connections(entity, owners, native)?;
    queries.sort();
    sequences.sort();
    procedures.sort();
    Ok(json!({"queries":queries,"sequences":sequences,"connections":links,"programs":procedures}))
}

pub(super) fn check(
    source: &Source,
    reader: &mut Reader,
    from: &Payload,
    to: &Payload,
) -> Result<Value> {
    let mut originals = BTreeMap::new();
    for row in from.array(0x20, 12, Some(0x80800715))? {
        let tag = from.u32(row)?;
        originals.insert(tag, source.read(tag)?);
    }
    let mut emitted = BTreeMap::new();
    for row in to.array(16, 12, Some(0x80809C04))? {
        let tag = to.u32(row)?;
        emitted.insert(tag, (*reader.tag(tag, Some(0x80809C36))?).clone());
    }
    let expected = snapshot(from, &originals, false)?;
    let actual = snapshot(to, &emitted, true)?;
    ensure!(
        expected == actual,
        "Source query, scalar sequence or connected channel changed: expected {expected}, actual {actual}"
    );
    Ok(actual)
}
