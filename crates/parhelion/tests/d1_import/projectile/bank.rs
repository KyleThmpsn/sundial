//! Decode the installed bank and its connections from the staged firing graph.
//!
//! The expected private bank is derived from the original source bank and the stock carrier
//! bank: incoming declarations first, then the others, each with the source's in source order
//! before the carrier's own in carrier order. Shared names keep source producers, retained names
//! keep carrier producers, and dependency names are the union of both originals. Connections
//! are the carrier's by consumer and channel name.
use super::*;

fn find(
    entity: &Payload,
    old: bool,
    mut read: impl FnMut(u32) -> Result<Payload>,
) -> Result<(u32, Payload)> {
    let mut banks = Vec::new();
    for at in entity.array(if old { 32 } else { 16 }, 12, None)? {
        let tag = entity.u32(at)?;
        let p = read(tag)?;
        if p.u32(p.pointer(16)? - 4)? == if old { 0x80800594 } else { 0x8080979F } {
            banks.push((tag, p));
        }
    }
    ensure!(banks.len() == 1, "Projectile does not own one channel bank");
    banks.pop().context("Projectile bank")
}

fn bytes(p: &Payload, field: usize, stride: usize) -> Result<Vec<Vec<u8>>> {
    p.array(field, stride, None)?
        .into_iter()
        .map(|at| {
            Ok(p.0
                .get(at..at + stride)
                .context("Bank array extent")?
                .to_vec())
        })
        .collect()
}

/// Declaration rows of a bank, with its name table and decoded name lookups.
struct Bank<'a> {
    p: &'a Payload,
    instance: usize,
    definition: usize,
    rows: Vec<usize>,
    names: Vec<u32>,
}

impl<'a> Bank<'a> {
    fn read(p: &'a Payload, old: bool) -> Result<Self> {
        let instance = p.pointer(16)?;
        let definition = usize::try_from(p.u64(instance + 8)?)?;
        let rows = p.array(definition + 0xD8, 112, (!old).then_some(0x808097A1))?;
        let names = rows
            .iter()
            .map(|&at| p.u32(at))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            p,
            instance,
            definition,
            rows,
            names,
        })
    }

    fn name(&self, ordinal: u32) -> Result<u32> {
        self.names
            .get(ordinal as usize)
            .copied()
            .context("Bank ordinal names no declaration")
    }

    /// Program inputs by declaration name.
    fn inputs(&self, row: usize) -> Result<Vec<u32>> {
        self.p
            .array(row + 56, 4, None)?
            .into_iter()
            .map(|at| self.name(self.p.u32(at)?))
            .collect()
    }

    /// Names whose state a write to this declaration invalidates.
    fn dependents(&self, row: usize) -> Result<BTreeSet<u32>> {
        let mut names = BTreeSet::new();
        for (word, at) in self.p.array(row + 80, 4, None)?.into_iter().enumerate() {
            let bits = self.p.u32(at)?;
            for bit in (0..32).filter(|bit| bits & (1 << bit) != 0) {
                names.insert(self.name(u32::try_from(word * 32 + bit)?)?);
            }
        }
        Ok(names)
    }

    /// Lookup rows as namespace, lookup name and the name of the addressed declaration.
    fn lookup(&self, field: usize) -> Result<Vec<(u32, u32, u32)>> {
        self.p
            .array(self.definition + field, 12, None)?
            .into_iter()
            .map(|at| {
                Ok((
                    self.p.u32(at)?,
                    self.p.u32(at + 4)?,
                    self.name(self.p.u32(at + 8)?)?,
                ))
            })
            .collect()
    }
}

/// The definition classes of a graph's owners.
fn owner_classes(
    entity: &Payload,
    mut read: impl FnMut(u32) -> Result<Payload>,
) -> Result<BTreeSet<u32>> {
    entity
        .array(16, 12, None)?
        .into_iter()
        .map(|row| {
            let owner = read(entity.u32(row)?)?;
            owner.u32(owner.pointer(24)? - 4)
        })
        .collect()
}

fn outputs(
    entity: &Payload,
    bank: u32,
    names: &[u32],
    old: bool,
    mut read: impl FnMut(u32) -> Result<Payload>,
) -> Result<Vec<(usize, u32, u32, u32)>> {
    let mut result = Vec::new();
    for (kind, field) in (if old { [48, 64] } else { [32, 48] })
        .into_iter()
        .enumerate()
    {
        for row in entity.array(field, if old { 88 } else { 72 }, None)? {
            let provider = row + if old { 40 } else { 32 };
            if entity.u32(provider + 8)? != bank {
                continue;
            }
            let channel = entity.u32(row + if old { 80 } else { 64 })? as usize;
            let target = read(entity.u32(row + 8)?)?;
            let class = entity.u32(row + if old { 24 } else { 12 })?;
            let offset = usize::try_from(entity.u64(row + 16)?)?;
            // D1 endpoints keep their component offset at +10, class at +18.
            let input = if class == if old { 0x80800927 } else { 0x80809789 } {
                target.u32(offset + if old { 40 } else { 32 })?
            } else {
                ensure!(
                    class == if old { 0x8080099E } else { 0x80809BD9 },
                    "Unqualified bank output consumer"
                );
                0x80809BD9
            };
            let role = target.u32(target.pointer(16)? + if old { 16 } else { 4 })?;
            // Rise of Iron component definitions by their Shadowkeep counterparts.
            let role = match role {
                0x808020AC => 0x8080388F,
                0x80800610 => 0x808084E9,
                0x80802947 => 0x80803813,
                0x80802AD9 => 0x8080377D,
                0x80802A9A => 0x808037A9,
                0x8080081D => 0x80808A0C,
                0x80801A9C => 0x808072BD,
                0x808008FB => 0x80808507,
                other => other,
            };
            result.push((
                kind,
                role,
                input,
                *names.get(channel).context("Bank output ordinal")?,
            ));
        }
    }
    result.sort_unstable();
    Ok(result)
}

type IncomingGetter = (u32, u32, u32, u32, u32);

/// Each bank getter's name, consumer and provider namespaces, channel and the Shadowkeep class of
/// the component that supplies it.
fn incoming(
    entity: &Payload,
    bank: &Payload,
    old: bool,
    mut read: impl FnMut(u32) -> Result<Payload>,
) -> Result<Vec<IncomingGetter>> {
    let instance = bank.pointer(16)?;
    let owner = bank.u32(instance)?;
    let definition = usize::try_from(bank.u64(instance + 8)?)?;
    let getters = bank.array(definition + 0xC8, if old { 40 } else { 32 }, None)?;
    let lookup = bank.array(definition + 0x118, 12, None)?;
    let mut result = Vec::new();
    for field in if old { [48, 64] } else { [32, 48] } {
        for row in entity.array(field, if old { 88 } else { 72 }, None)? {
            if entity.u32(row + 8)? != owner {
                continue;
            }
            let offset = usize::try_from(entity.u64(row + 16)?)?;
            let ordinal = getters
                .iter()
                .position(|&at| at == offset)
                .context("Bank getter endpoint")?;
            let names = lookup
                .iter()
                .copied()
                .filter(|&at| bank.u32(at + 8).ok() == Some(ordinal as u32))
                .collect::<Vec<_>>();
            ensure!(names.len() == 1, "Bank getter has no unique name");
            let provider = row + if old { 40 } else { 32 };
            ensure!(
                entity.u32(provider + if old { 24 } else { 12 })?
                    == if old { 0x8080053B } else { 0x80809AE1 },
                "Bank incoming provider class differs"
            );
            // A disconnected provider has no supplier.
            let supplier = entity.u32(provider + 8)?;
            let class = if supplier == u32::MAX {
                u32::MAX
            } else {
                let supplier = read(supplier)?;
                supplier.u32(supplier.pointer(24)? - 4)?
            };
            // Rise of Iron component definitions by their Shadowkeep counterparts.
            let class = match (old, class) {
                (_, u32::MAX) | (false, _) => class,
                (true, 0x808020AC) => 0x8080388F,
                (true, 0x80800610) => 0x808084E9,
                (true, 0x80802202) => 0x80804AF6,
                (true, 0x808008F6) => 0x80809790,
                (true, 0x808005A8) => 0x80808BF7,
                (true, 0x80802A9A) => 0x808037A9,
                (true, class) => {
                    anyhow::bail!("Bank getter supplier {class:08X} has no counterpart")
                }
            };
            result.push((
                bank.u32(names[0] + 4)?,
                entity.u32(row)?,
                entity.u32(provider)?,
                entity.u32(row + if old { 80 } else { 64 })?,
                class,
            ));
        }
    }
    ensure!(
        result.len() == getters.len(),
        "Bank getter is unbound or multiply bound"
    );
    result.sort_unstable();
    Ok(result)
}

pub(super) fn check(
    source: &Source,
    stock: &mut Reader,
    staged: &mut Reader,
    (source_graph, omitted): (u32, &[u32]),
    entity: &Payload,
    stock_entity: &Payload,
) -> Result<Value> {
    let original = source.read(source_graph)?;
    let (_, from) = find(&original, true, |tag| source.read(tag))?;
    let (carrier_tag, carrier) = find(stock_entity, false, |tag| {
        Ok((*stock.tag(tag, None)?).clone())
    })?;
    let (tag, to) = find(entity, false, |tag| Ok((*staged.tag(tag, None)?).clone()))?;
    ensure!(
        !stock_entity
            .array(16, 12, None)?
            .iter()
            .any(|&at| stock_entity.u32(at).ok() == Some(tag)),
        "Projectile bank still uses a stock owner"
    );
    let s = Bank::read(&from, true)?;
    let c = Bank::read(&carrier, false)?;
    let n = Bank::read(&to, false)?;
    let boundary = to.pointer(24)?;
    // Carrier components the private graph leaves out are the stock carrier's owners whose
    // definition class no staged owner has. A getter only such a component supplies names a
    // carrier-only declaration that leaves with it.
    let carrier_classes = owner_classes(stock_entity, |tag| Ok((*stock.tag(tag, None)?).clone()))?;
    let staged_classes = owner_classes(entity, |tag| Ok((*staged.tag(tag, None)?).clone()))?;
    let omitted_names = incoming(stock_entity, &carrier, false, |tag| {
        Ok((*stock.tag(tag, None)?).clone())
    })?
    .into_iter()
    .filter(|row| carrier_classes.contains(&row.4) && !staged_classes.contains(&row.4))
    .map(|row| row.0)
    .collect::<BTreeSet<_>>();
    let retained = c
        .names
        .iter()
        .copied()
        .filter(|name| !s.names.contains(name) && !omitted_names.contains(name))
        .collect::<Vec<_>>();
    // Native refresh treats declarations below the getter count as imported inputs, so incoming
    // names come first: the source's, then the carrier's own, then the rest in the same order.
    let incoming_names = |bank: &Bank| -> Result<BTreeSet<u32>> {
        Ok(bank.lookup(0x118)?.into_iter().map(|row| row.2).collect())
    };
    let (source_incoming, carrier_incoming) = (incoming_names(&s)?, incoming_names(&c)?);
    let expected = s
        .names
        .iter()
        .filter(|name| source_incoming.contains(name))
        .chain(
            retained
                .iter()
                .filter(|name| carrier_incoming.contains(name)),
        )
        .chain(
            s.names
                .iter()
                .filter(|name| !source_incoming.contains(name)),
        )
        .chain(
            retained
                .iter()
                .filter(|name| !carrier_incoming.contains(name)),
        );
    let expected = expected.copied().collect::<Vec<_>>();
    ensure!(
        n.names == expected,
        "Private bank is not incoming then other declarations, each source before carrier:          staged {:08X?}, expected {expected:08X?}",
        n.names
    );
    check_declarations(&s, &c, &n)?;
    let mut lookups = Vec::new();
    for field in [0xF8, 0x108, 0x118] {
        let mut expected = s.lookup(field)?;
        expected.extend(
            c.lookup(field)?
                .into_iter()
                .filter(|row| !omitted_names.contains(&row.2)),
        );
        expected.sort_unstable();
        expected.dedup();
        let observed = n.lookup(field)?;
        ensure!(
            observed
                .windows(2)
                .all(|pair| (pair[0].0, pair[0].1) < (pair[1].0, pair[1].1))
                && observed.iter().cloned().collect::<BTreeSet<_>>()
                    == expected.iter().cloned().collect::<BTreeSet<_>>(),
            "Private bank public, inherited or incoming lookup is not the sorted union"
        );
        lookups.push(observed);
    }
    let [public, inherited, _] = [&lookups[0], &lookups[1], &lookups[2]];
    for (ordinal, &row) in n.rows.iter().enumerate() {
        for (offset, table) in [(72, inherited), (106, public)] {
            let slot = n.p.u16(row + offset)?;
            let named = table
                .iter()
                .position(|entry| entry.2 == n.names[ordinal] && entry.0 == 0x811C9DC5);
            ensure!(
                (slot != u16::MAX).then_some(usize::from(slot)) == named,
                "Private bank stored slot or public index does not address its lookup row"
            );
        }
    }
    let allocation_tag =
        check_runtime_allocation(staged, &n, boundary, (public.len(), inherited.len()))?;
    // Every carrier consumer of a named input keeps its channel name, and those reading names
    // the source declares match the source's consumers. Settings inputs have no name. Their
    // selection is not stored by Shadowkeep's binder, so the staged row keeps the source's.
    let carrier_outputs = outputs(stock_entity, carrier_tag, &c.names, false, |tag| {
        Ok((*stock.tag(tag, None)?).clone())
    })?;
    let source_outputs = outputs(&original, from.u32(s.instance)?, &s.names, true, |tag| {
        source.read(tag)
    })?;
    let observed = outputs(entity, tag, &n.names, false, |tag| {
        Ok((*staged.tag(tag, None)?).clone())
    })?;
    // Consumers of names only the carrier declares stay the carrier's. Consumers of names the
    // source declares are the source's, including inputs the private movement gains.
    let named = |rows: &[(usize, u32, u32, u32)], shared: bool| {
        rows.iter()
            .copied()
            .filter(|row| row.2 != 0x80809BD9 && s.names.contains(&row.3) == shared)
            .collect::<Vec<_>>()
    };
    let unnamed = |rows: &[(usize, u32, u32, u32)]| {
        rows.iter()
            .copied()
            .filter(|row| row.2 == 0x80809BD9)
            .collect::<Vec<_>>()
    };
    let consumers = |rows: Vec<(usize, u32, u32, u32)>| {
        rows.into_iter()
            .map(|row| (row.0, row.1))
            .collect::<Vec<_>>()
    };
    // A carrier scalar input nothing kept reads may take a source name in place of a carrier-only
    // one. The source scalar check then finds it among the staged scalars.
    let mut retargeted = named(&carrier_outputs, false);
    for row in named(&observed, false) {
        let position = retargeted
            .iter()
            .position(|other| *other == row)
            .context("Private bank has a carrier-only consumer the carrier lacks")?;
        retargeted.remove(position);
    }
    ensure!(
        retargeted
            .iter()
            .all(|row| row.0 == 0 && row.1 == 0x8080388F),
        "Private bank consumers of carrier-only names are not the carrier's"
    );
    // Consumers of shared names are the source's, plus carrier connections the private graph keeps
    // without a source counterpart: effect-sequence inputs that read a name more often than the
    // source's sequence does, and inputs of carrier groups left out of the movement, which stay
    // connected but unread. Each extra is a carrier connection, counted. The input of a source
    // child parameter left out unread is the one consumer that may go.
    let mut extra = named(&observed, true);
    let mut unread = omitted.to_vec();
    // The private graph keeps the carrier's effect sequence, so source sequence inputs that read
    // a name more often than the carrier's sequence does have no staged consumer.
    let sequence = |rows: &[(usize, u32, u32, u32)], name: u32| {
        rows.iter()
            .filter(|row| row.1 == 0x808084E9 && row.3 == name)
            .count()
    };
    let mut sequence_only = BTreeMap::new();
    for row in named(&source_outputs, true) {
        if row.1 == 0x808084E9 {
            sequence_only.entry(row.3).or_insert_with(|| {
                sequence(&source_outputs, row.3).saturating_sub(sequence(&carrier_outputs, row.3))
            });
        }
    }
    for row in named(&source_outputs, true) {
        if let Some(position) = extra.iter().position(|other| *other == row) {
            extra.remove(position);
            continue;
        }
        if let Some(left) = sequence_only.get_mut(&row.3).filter(|left| **left > 0)
            && row.1 == 0x808084E9
        {
            *left -= 1;
            continue;
        }
        let position = unread
            .iter()
            .position(|&name| row.1 == 0x8080388F && row.2 == name)
            .context("Private bank lost a source consumer")?;
        unread.remove(position);
    }
    let mut carrier_shared = named(&carrier_outputs, true);
    for row in &extra {
        let position = carrier_shared
            .iter()
            .position(|other| other == row)
            .context(
                "Private bank has a shared-name consumer neither the source nor the carrier has",
            )?;
        carrier_shared.remove(position);
        ensure!(
            matches!(row.1, 0x808084E9 | 0x8080388F),
            "Private bank keeps a carrier shared-name consumer of another role"
        );
    }
    ensure!(
        unnamed(&observed) == unnamed(&source_outputs)
            && consumers(unnamed(&observed)) == consumers(unnamed(&carrier_outputs)),
        "Private bank settings inputs do not keep the source selection"
    );
    let settings = json!({"source":unnamed(&source_outputs),"carrier":unnamed(&carrier_outputs),
        "staged":unnamed(&observed)});
    // Each getter for a source name is supplied by the component role that supplies it in the
    // source, and each getter for a carrier-only name by the carrier's.
    let mut expected = incoming(&original, &from, true, |tag| source.read(tag))?;
    expected.extend(
        incoming(stock_entity, &carrier, false, |tag| {
            Ok((*stock.tag(tag, None)?).clone())
        })?
        .into_iter()
        .filter(|(name, ..)| retained.contains(name)),
    );
    expected.sort_unstable();
    let observed_incoming = incoming(entity, &to, false, |tag| {
        Ok((*staged.tag(tag, None)?).clone())
    })?;
    ensure!(
        expected == observed_incoming,
        "Private bank incoming getter selection or supplier changed"
    );
    Ok(
        json!({"owner":tag,"allocation":allocation_tag,"names":n.names,"retained_native":retained,
        "inherited":inherited.len(),"public":public.len(),"connections":observed,"incoming":observed_incoming,
        "settings":settings,"omitted_carrier_names":omitted_names,"mutable_span_checked":true}),
    )
}

fn check_declarations(s: &Bank<'_>, c: &Bank<'_>, n: &Bank<'_>) -> Result<()> {
    for (ordinal, &row) in n.rows.iter().enumerate() {
        let name = n.names[ordinal];
        let (origin, at) = match s.names.iter().position(|&v| v == name) {
            Some(index) => (s, s.rows[index]),
            None => (
                c,
                c.rows[c
                    .names
                    .iter()
                    .position(|&v| v == name)
                    .context("Carrier row")?],
            ),
        };
        for (offset, stride) in [(8, 1), (24, 16)] {
            ensure!(
                bytes(origin.p, at + offset, stride)? == bytes(n.p, row + offset, stride)?,
                "Private bank procedure differs from its originating declaration"
            );
        }
        ensure!(
            origin.p.0.get(at + 40..at + 56) == n.p.0.get(row + 40..row + 56)
                && origin.inputs(at)? == n.inputs(row)?,
            "Private bank procedure inputs differ from their originating declaration"
        );
        if std::ptr::eq(origin, c) {
            ensure!(
                c.p.u16(at + 74)? == n.p.u16(row + 74)?,
                "Retained native declaration flags changed"
            );
        }
        let mut expected = BTreeSet::new();
        for bank in [s, c] {
            if let Some(index) = bank.names.iter().position(|&v| v == name) {
                expected.extend(bank.dependents(bank.rows[index])?);
            }
        }
        ensure!(
            n.dependents(row)? == expected,
            "Private bank invalidation differs from the source and carrier union"
        );
    }
    Ok(())
}

fn check_runtime_allocation(
    staged: &mut Reader,
    n: &Bank<'_>,
    boundary: usize,
    (public, inherited): (usize, usize),
) -> Result<u32> {
    let to = n.p;
    for (field, stride, count, fill) in [
        (0x40, 4, n.names.len().div_ceil(32), 0u8),
        (0x50, 16, n.names.len(), 0),
        (0x80, 8, public, 255),
        (0x90, 16, inherited, 255),
        (0xA0, 4, inherited.div_ceil(32), 0),
    ] {
        let rows = to.array(n.instance + field, stride, None)?;
        ensure!(rows.len() == count, "Private bank runtime capacity differs");
        for row in rows {
            ensure!(
                row >= n.instance
                    && row + stride <= boundary
                    && to.0[row..row + stride].iter().all(|&v| v == fill),
                "Private bank runtime storage escapes its copied span or has live state"
            );
        }
    }
    let allocation_tag = to.u32(0x44)?;
    let allocation = staged.tag(allocation_tag, None)?;
    for (name, class, count) in [
        (0x4A423762, 0x8080000B, n.names.len().div_ceil(32)),
        (0xFC2F3D6F, 0x80800090, n.names.len()),
        (0x3A55A801, 0x8080979C, inherited),
        (0x083C8E12, 0x8080000B, inherited.div_ceil(32)),
        (0xBAFA25AF, 0x8080979D, public),
    ] {
        let rows = allocation
            .array(0x20, 40, None)?
            .into_iter()
            .filter(|&at| allocation.u32(at).ok() == Some(name))
            .collect::<Vec<_>>();
        ensure!(
            (count == 0 && rows.is_empty())
                || (rows.len() == 1
                    && allocation.u32(rows[0] + 16)? == class
                    && allocation.u32(rows[0] + 20)? as usize == count),
            "Private bank allocation disagrees with stored arrays"
        );
    }
    Ok(allocation_tag)
}
