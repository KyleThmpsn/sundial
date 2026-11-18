//! Source calibration with private curve columns. Native routing stays only when its operations,
//! destinations and ordered component reads implement the same selected source program.
use super::*;

#[derive(Clone, Debug, PartialEq)]
enum Read {
    Constant(u32),
    Curve(u32),
    Input(Input),
}

struct Writer {
    record: usize,
    destination: Input,
    operation: u32,
    order: u32,
    operands: Vec<(usize, Read)>,
    /// A native row behind the player-state gate that multiplies its own input by a literal.
    gated: bool,
}

struct Program<'a> {
    payload: &'a Payload,
    table: usize,
    index: usize,
    curves: Vec<usize>,
    overrides: Vec<[u8; 8]>,
    writers: Vec<Writer>,
    operand_scopes: BTreeMap<usize, BTreeSet<Option<&'static str>>>,
}

/// The native Barrel input a Rise of Iron Barrel input corresponds to. Other source ordinals
/// have no qualified counterpart and take a number no native scope uses.
fn mapped(mut input: Input, old: bool) -> Input {
    if old && input.0 == 2 {
        input.1 = match input.1 {
            0..=15 | 17 | 22..=25 | 30 | 32 => input.1,
            33 => 36,
            38 => 43,
            39 => 47,
            42..=52 => input.1 + 12,
            // Both tables copy the Blast Radius stat's output into this pair.
            53 | 54 => input.1 + 13,
            _ => u32::MAX,
        };
    }
    input
}

/// Every calibration scope, in the order a scope may depend on an earlier one.
const SCOPES: [&str; 16] = [
    "firing",
    "reload",
    "handling",
    "aiming",
    "accuracy",
    "recoil",
    "velocity",
    "cadence",
    "magazine",
    "reserves",
    "reserve_input_2",
    "reserve_input_4",
    "damage",
    "precision",
    "hip_fire",
    "blast_radius",
];

fn scope((component, input, _): Input) -> Option<&'static str> {
    Some(match (component, input) {
        (2, 0..=5 | 22..=25) | (4, 0..=1) => "firing",
        (3, 0..=2) => "reload",
        (0, 0..=6) => "handling",
        (0, 7..=15 | 17 | 19..=21) => "aiming",
        (2, 14 | 15 | 17) => "accuracy",
        (2, 54..=64) => "recoil",
        (2, 43 | 47) => "velocity",
        // Barrel inputs both tables derive from the rate output.
        (2, 6..=13) => "cadence",
        // Capacity and the refill quantum.
        (1, 0 | 3) => "magazine",
        // Rounds carried with the magazine included.
        (1, 1) => "reserves",
        (1, 2) => "reserve_input_2",
        (1, 4) => "reserve_input_4",
        (2, 32) => "damage",
        (2, 36) => "precision",
        // The hip-fire error input the Range stat sets.
        (2, 30) => "hip_fire",
        (2, 66 | 67) => "blast_radius",
        _ => return None,
    })
}

/// The scopes a scope's source program relies on. Accuracy rows read the burst interval the firing
/// scope writes. Magazine, damage and the rate-derived Barrel
/// inputs are stated per burst or per bullet, so they follow the source only with its firing
/// calibration. Reserves multiply the capacity, and the precision multiplier scales the damage.
fn requires(name: &str) -> &'static [&'static str] {
    match name {
        "accuracy" | "cadence" | "magazine" | "damage" => &["firing"],
        "reserves" | "reserve_input_2" | "reserve_input_4" => &["magazine"],
        "precision" => &["damage"],
        _ => &[],
    }
}

/// Whether both original evaluators compute `operation` alike. Source 7B0240 and native E86390
/// return equal bits for direct operations 0 through 7, 9 and 10 (add, subtract, multiply,
/// divide, first operand, ceiling, floor, round half to even, maximum and minimum). Operation 8
/// reads a third value through the recipient and has no contract. Source 7AEE60 and native
/// E82CC0 return equal bits for logic types 0 to 5, 7 and 8 with the operand counts they read.
/// Types 6 and 9 differ, and a flagged native row runs behind a gate the source lacks.
fn admitted(operation: u32, operands: usize) -> bool {
    match operation {
        0..=7 | 9 | 10 => true,
        0x10000 | 0x10003 | 0x10005 => operands == 3,
        0x10001 => operands == 1,
        0x10002 => operands == 4,
        0x10004 | 0x10007 | 0x10008 => operands == 2,
        _ => false,
    }
}

impl<'a> Program<'a> {
    fn read(payload: &'a Payload, key: u32, old: bool) -> Result<Self> {
        let dialect = if old { &RISE_OF_IRON } else { &SHADOWKEEP };
        let d = payload.pointer(24)?;
        let groups = payload.array(d + dialect.groups.0, 48, Some(dialect.groups.1))?;
        let keys = payload.array(d + dialect.keys.0, 40, Some(dialect.keys.1))?;
        ensure!(
            !groups.is_empty() && groups.len() == keys.len(),
            "Stat table keys differ"
        );
        ensure!(
            payload.u64(d + dialect.shared)? == 0,
            "Shared stat logic needs translation"
        );
        let matches = keys
            .iter()
            .enumerate()
            .filter_map(|(i, &at)| (payload.u32(at + 16).ok() == Some(key)).then_some(i))
            .collect::<Vec<_>>();
        ensure!(matches.len() <= 1, "Ambiguous stat translation key");
        let index = matches.first().copied().unwrap_or(0);
        let table = groups[index];
        let mut overrides = stat_overrides(payload, d + dialect.overrides.0, dialect.overrides.1)?;
        overrides.extend(stat_overrides(payload, keys[index], dialect.overrides.1)?);
        let curves = payload.array(table, 56, Some(dialect.translations))?;
        if old {
            for &curve in &curves {
                for outputs in payload.array(curve, 16, None)? {
                    ensure!(
                        payload.u64(outputs)? == 0,
                        "Source direct stat recipients need translation"
                    );
                }
            }
        }
        let input = |at| -> Result<Input> { Ok(mapped(dialect.input(payload, at)?, old)) };
        let operand = |at| -> Result<(usize, Read)> {
            let bits = payload.u32(at + 16)?;
            let channel = payload.u32(at + 12)?;
            let value = if bits != 0x7F7FFFFF {
                payload.f32(at + 16)?;
                Read::Constant(bits)
            } else if payload.bytes::<12>(at)? == [0; 12] && channel >= 0x1E00 {
                Read::Curve(channel)
            } else {
                ensure!(channel == 0, "Stat operand has an unqualified channel");
                Read::Input(input(at)?)
            };
            Ok((at, value))
        };
        let mut writers = Vec::new();
        let mut operand_scopes = BTreeMap::<_, BTreeSet<_>>::new();
        for at in payload.array(table + 16, 80, Some(dialect.copies.1))? {
            let destination = input(at)?;
            let operation = payload.u32(at + 12)?;
            let arity = if operation & 255 == 4 { 1 } else { 2 };
            for i in 0..arity {
                operand_scopes
                    .entry(at + 16 + i * 20)
                    .or_default()
                    .insert(scope(destination));
            }
            if scope(destination).is_none() {
                continue;
            }
            let operands = (0..arity)
                .map(|i| operand(at + 16 + i * 20))
                .collect::<Result<Vec<_>>>()?;
            // Native E85B50 runs a row whose flag byte is set only while a player state byte
            // is set. Every stock one scales what earlier rows wrote to its own input, so the
            // base's scaling can stay over source calibration. The source has no gate.
            let gated = !old
                && operation == 0x102
                && matches!(
                    operands.as_slice(),
                    [(_, Read::Input(read)), (_, Read::Constant(_))] if *read == destination
                );
            writers.push(Writer {
                record: at,
                destination,
                operation,
                order: payload.u32(at + 76)?,
                operands,
                gated,
            });
        }
        for at in payload.array(table + 32, 40, Some(dialect.logic))? {
            let outputs = payload.array(at, 12, Some(dialect.outputs.1))?;
            let reads = payload.array(at + 16, 20, Some(dialect.operands.1))?;
            for output in outputs {
                let destination = input(output)?;
                for &read in &reads {
                    operand_scopes
                        .entry(read)
                        .or_default()
                        .insert(scope(destination));
                }
                if scope(destination).is_none() {
                    continue;
                }
                let operands = reads.iter().copied().map(operand).collect::<Result<_>>()?;
                writers.push(Writer {
                    record: at,
                    destination,
                    operation: 0x10000u32
                        .checked_add(payload.u32(at + 32)?)
                        .context("Stat logic kind overflow")?,
                    order: payload.u32(at + 36)?,
                    operands,
                    gated: false,
                });
            }
        }
        writers.sort_by_key(|w| (w.order, w.destination));
        Ok(Self {
            payload,
            table,
            index,
            curves,
            overrides,
            writers,
            operand_scopes,
        })
    }

    /// The outputs the Barrel's bullets-per-shot inputs copy.
    fn bullets(&self) -> BTreeSet<u32> {
        self.writers
            .iter()
            .filter(|w| w.destination.0 == 2 && (22..=25).contains(&w.destination.1))
            .filter(|w| w.operation == 4)
            .filter_map(|w| match w.operands.first() {
                Some((_, Read::Curve(channel))) => Some(*channel),
                _ => None,
            })
            .collect()
    }

    /// The one value a literal or a curve gives at every tier, if it gives one.
    fn fixed(&self, read: &Read) -> Result<Option<u32>> {
        let channel = match read {
            Read::Constant(bits) => return Ok(Some(*bits)),
            Read::Curve(channel) => *channel,
            Read::Input(_) => return Ok(None),
        };
        let ordinal = usize::try_from(channel.checked_sub(0x1E00).context("Stat output number")?)?;
        let row = *self
            .curves
            .get(ordinal / 32)
            .context("Stat output exceeds its table")?;
        let mut values = BTreeSet::new();
        for tier in self.payload.array(row + 16, 16, None)? {
            let cells = self.payload.array(tier, 4, Some(CELLS))?;
            let cell = *cells
                .get(ordinal % 32)
                .context("Stat output column is absent")?;
            values.insert(self.payload.u32(cell)?);
        }
        Ok(if values.len() == 1 {
            values.into_iter().next()
        } else {
            None
        })
    }

    /// How many rows wrote `input` before the row ordered at `order`.
    fn generation(&self, input: Input, order: u32) -> usize {
        self.writers
            .iter()
            .filter(|w| w.destination == input && w.order < order && !w.gated)
            .count()
    }

    fn curve(&self, channel: u32) -> Result<Value> {
        ensure!(
            (0x1E00..0x1FE0).contains(&channel),
            "Stat operand exceeds the native channel encoding"
        );
        let ordinal = usize::try_from(channel.checked_sub(0x1E00).context("Stat output number")?)?;
        let row = *self
            .curves
            .get(ordinal / 32)
            .context("Stat output exceeds its table")?;
        let metadata = self.payload.bytes::<24>(row + 32)?;
        ensure!(
            self.payload.u32(row + 36)? == 0
                && self.payload.u32(row + 32)? <= 18
                && self.payload.f32(row + 44)? > self.payload.f32(row + 40)?
                && metadata[17..] == [0; 7],
            "Unqualified source stat curve domain"
        );
        // Both original routing samplers implement nonzero interpolation. Their zero-mode
        // bulk samplers have separate rounding behavior, so it is not admitted here.
        ensure!(
            metadata[16] != 0,
            "Noninterpolating stat curves need a sampling contract"
        );
        let samples = self
            .payload
            .array(row + 16, 16, None)?
            .into_iter()
            .map(|tier| {
                let cells = self.payload.array(tier, 4, Some(CELLS))?;
                let cell = *cells
                    .get(ordinal % 32)
                    .context("Stat output column is absent")?;
                self.payload.f32(cell)?;
                self.payload.u32(cell)
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(!samples.is_empty(), "Source stat curve is empty");
        Ok(json!({"metadata":metadata,"samples":samples}))
    }
}

/// A damage row divides over the bullets or pellets one shot fires. The firing scope translates
/// bullets per shot. Pellets per bullet stay the base's, so the source damage holds only where
/// both rows divide by the bullets-per-shot output or by the same count.
fn shared_count(
    (source, s): (&Program<'_>, &Writer),
    (native, n): (&Program<'_>, &Writer),
) -> Result<()> {
    let (divisor, native_divisor) = (&s.operands[1].1, &n.operands[1].1);
    let bullets = matches!(divisor, Read::Curve(c) if source.bullets().contains(c))
        && matches!(native_divisor, Read::Curve(c) if native.bullets().contains(c));
    let count = source.fixed(divisor)?;
    ensure!(
        bullets || (count.is_some() && count == native.fixed(native_divisor)?),
        "Source damage is divided over a count the base does not fire"
    );
    Ok(())
}

/// A component read takes what earlier rows wrote. It must come from scope `name` or one it
/// depends on, after the same number of writers in both programs.
fn dependency(
    (source, s): (&Program<'_>, &Writer),
    (native, n): (&Program<'_>, &Writer),
    name: &str,
    input: Input,
) -> bool {
    let earlier = source.generation(input, s.order);
    scope(input).is_some_and(|owner| owner == name || requires(name).contains(&owner))
        && earlier > 0
        && earlier == native.generation(input, n.order)
        // A gated row before the read would hand it a scaled value the source never forms.
        && !native
            .writers
            .iter()
            .any(|w| w.gated && w.destination == input && w.order < n.order)
}

/// Whether a rewritten row's component read sees what the source row sees. An input of the
/// scope itself counts the rows already placed. An input of a required scope counts that scope's
/// native rows, which already follow the source.
fn follows(
    source: &Program<'_>,
    native: &Program<'_>,
    name: &str,
    placed: &[(u32, Input)],
    (s, n): (&Writer, &Writer),
    input: Input,
) -> bool {
    if scope(input) != Some(name) {
        return dependency((source, s), (native, n), name, input);
    }
    let earlier = source.generation(input, s.order);
    earlier > 0
        && earlier
            == placed
                .iter()
                .filter(|(_, written)| *written == input)
                .count()
}

/// The gated rows of scope `name`, which stay as they are. Each must follow every other writer
/// of its input, so it scales the finished source value as it scales the base's.
fn gated(native: &Program<'_>, name: &str) -> Result<usize> {
    let rows = native
        .writers
        .iter()
        .filter(|w| w.gated && scope(w.destination) == Some(name))
        .collect::<Vec<_>>();
    for row in &rows {
        ensure!(
            native
                .writers
                .iter()
                .filter(|w| !w.gated && w.destination == row.destination)
                .all(|w| w.order < row.order),
            "Native {name} scales an input before its last writer"
        );
    }
    Ok(rows.len())
}

#[derive(Clone, Default)]
struct Changes {
    curves: Vec<Value>,
    channels: BTreeMap<u32, u32>,
    cells: BTreeMap<usize, [u8; 4]>,
}

impl Changes {
    fn word(&mut self, at: usize, value: u32) -> Result<()> {
        let bytes = value.to_le_bytes();
        ensure!(
            self.cells.insert(at, bytes).is_none_or(|old| old == bytes),
            "Source routes disagree on a shared operand"
        );
        Ok(())
    }

    /// Returns how many gated native rows keep scaling the scope's inputs.
    fn copy(&mut self, source: &Program<'_>, native: &Program<'_>, name: &str) -> Result<usize> {
        let from = source
            .writers
            .iter()
            .filter(|w| scope(w.destination) == Some(name))
            .collect::<Vec<_>>();
        let to = native
            .writers
            .iter()
            .filter(|w| scope(w.destination) == Some(name) && !w.gated)
            .collect::<Vec<_>>();
        ensure!(
            !from.is_empty() && from.len() == to.len(),
            "Source and native {name} writers differ"
        );
        ensure!(
            from.windows(2)
                .chain(to.windows(2))
                .all(|pair| pair[0].order != pair[1].order || pair[0].record == pair[1].record),
            "Source or native {name} has tied writer order"
        );
        for (s, n) in from.into_iter().zip(to) {
            ensure!(
                s.destination == n.destination
                    && s.destination.2 == u32::MAX
                    && s.operation == n.operation
                    && s.operands.len() == n.operands.len(),
                "Source and native {name} routing differs"
            );
            ensure!(
                admitted(s.operation, s.operands.len()),
                "Source {name} operation needs a native arithmetic contract"
            );
            if name == "damage" && s.operation == 3 {
                shared_count((source, s), (native, n))?;
            }
            for ((_, read), (at, native_read)) in s.operands.iter().zip(&n.operands) {
                if let Read::Input(input) = read {
                    ensure!(
                        native_read == read && dependency((source, s), (native, n), name, *input),
                        "Source {name} reads an input without a matching earlier source writer"
                    );
                } else {
                    self.scalar(source, native, name, read, native_read, *at)?;
                }
            }
        }
        gated(native, name)
    }

    /// Writes the source program over the scope's native rows where their shapes differ. Direct
    /// routing rows hold their destination, operation and operands inline, so each takes the
    /// next source row in order, and a native row left over copies its own input to itself. Logic
    /// rows keep their shape and must pair as they stand. Returns the rows rewritten, the rows
    /// left as copies of their own input and the gated rows kept.
    fn rewrite(
        &mut self,
        source: &Program<'_>,
        native: &Program<'_>,
        name: &str,
    ) -> Result<(usize, usize, usize)> {
        let in_scope = |w: &&Writer| scope(w.destination) == Some(name) && !w.gated;
        let from = source.writers.iter().filter(in_scope).collect::<Vec<_>>();
        let to = native.writers.iter().filter(in_scope).collect::<Vec<_>>();
        // The damage divisor rule compares paired rows, which a rewrite does not keep.
        ensure!(
            name != "damage" && !from.is_empty() && from.len() <= to.len(),
            "Source {name} has more writers than the base"
        );
        ensure!(
            from.windows(2)
                .chain(to.windows(2))
                .all(|pair| pair[0].order != pair[1].order || pair[0].record == pair[1].record),
            "Source or native {name} has tied writer order"
        );
        // An input the base writes and the source does not would fall back to its definition.
        ensure!(
            to.iter()
                .all(|n| from.iter().any(|s| s.destination == n.destination)),
            "The source leaves a {name} input the base writes unset"
        );
        let mut placed = Vec::<(u32, Input)>::new();
        let mut rows = to.iter();
        let mut rewritten = 0;
        for &s in &from {
            ensure!(
                s.destination.2 == u32::MAX && admitted(s.operation, s.operands.len()),
                "Source {name} operation needs a native arithmetic contract"
            );
            let n = *rows.next().context("Native stat rows are exhausted")?;
            if s.operation >= 0x10000 {
                self.logic_row(source, native, name, &placed, (s, n))?;
            } else {
                ensure!(
                    n.operation < 0x10000,
                    "Native {name} row order places a logic row where the source routes directly"
                );
                self.row(source, native, name, &placed, (s, n))?;
                rewritten += 1;
            }
            placed.push((n.order, s.destination));
        }
        let neutral = self.neutralize(rows.copied(), name)?;
        let scaled = native
            .writers
            .iter()
            .filter(|w| w.gated && scope(w.destination) == Some(name))
            .collect::<Vec<_>>();
        ensure!(
            scaled.iter().all(|g| placed
                .iter()
                .all(|(order, input)| *input != g.destination || *order < g.order)),
            "Native {name} scales an input before its last writer"
        );
        Ok((rewritten, neutral, scaled.len()))
    }

    /// A logic row keeps its shape, so it must pair with the source row as it stands.
    fn logic_row(
        &mut self,
        source: &Program<'_>,
        native: &Program<'_>,
        name: &str,
        placed: &[(u32, Input)],
        (s, n): (&Writer, &Writer),
    ) -> Result<()> {
        ensure!(
            n.operation == s.operation
                && n.destination == s.destination
                && n.operands.len() == s.operands.len(),
            "Source and native {name} logic rows differ"
        );
        for ((_, read), (at, native_read)) in s.operands.iter().zip(&n.operands) {
            if let Read::Input(input) = read {
                ensure!(
                    native_read == read && follows(source, native, name, placed, (s, n), *input),
                    "Source {name} reads an input without a matching earlier source writer"
                );
            } else {
                self.scalar(source, native, name, read, native_read, *at)?;
            }
        }
        Ok(())
    }

    /// Native rows the source has no row for copy their own input to itself.
    fn neutralize<'a>(
        &mut self,
        rows: impl Iterator<Item = &'a Writer>,
        name: &str,
    ) -> Result<usize> {
        let mut neutral = 0;
        for n in rows {
            ensure!(
                n.operation < 0x10000,
                "Native {name} has a logic row the source lacks"
            );
            self.operand(n.record + 12, [4])?;
            let (component, input, selector) = n.destination;
            self.operand(n.record + 16, [component, input, selector, 0, 0x7F7FFFFF])?;
            neutral += 1;
        }
        Ok(neutral)
    }

    fn operand<const N: usize>(&mut self, at: usize, words: [u32; N]) -> Result<()> {
        for (index, word) in words.into_iter().enumerate() {
            self.word(at + index * 4, word)?;
        }
        Ok(())
    }

    /// One native routing row rewritten to source row `s`.
    fn row(
        &mut self,
        source: &Program<'_>,
        native: &Program<'_>,
        name: &str,
        placed: &[(u32, Input)],
        (s, n): (&Writer, &Writer),
    ) -> Result<()> {
        let (component, input, selector) = s.destination;
        self.operand(n.record, [component, input, selector, s.operation])?;
        for (index, (_, read)) in s.operands.iter().enumerate() {
            let words = match read {
                Read::Constant(value) => [0, 0, 0, 0, *value],
                Read::Curve(channel) => {
                    let column = self.column(source, native, *channel, &Read::Constant(0))?;
                    [0, 0, 0, column, 0x7F7FFFFF]
                }
                Read::Input(read) => {
                    ensure!(
                        follows(source, native, name, placed, (s, n), *read),
                        "Source {name} reads an input without a matching earlier source writer"
                    );
                    [read.0, read.1, read.2, 0, 0x7F7FFFFF]
                }
            };
            self.operand(n.record + 16 + index * 20, words)?;
        }
        Ok(())
    }

    fn scalar(
        &mut self,
        source: &Program<'_>,
        native: &Program<'_>,
        name: &str,
        read: &Read,
        native_read: &Read,
        at: usize,
    ) -> Result<()> {
        ensure!(
            native
                .operand_scopes
                .get(&at)
                .is_some_and(|scopes| scopes.iter().all(|scope| *scope == Some(name))),
            "Native {name} shares an operand with another stat group"
        );
        ensure!(
            !matches!(native_read, Read::Input(_)),
            "Source {name} changes a component dependency"
        );
        let (channel, value) = match read {
            Read::Constant(value) => (0, *value),
            Read::Curve(channel) => (
                self.column(source, native, *channel, native_read)?,
                0x7F7FFFFF,
            ),
            Read::Input(_) => anyhow::bail!("Component inputs are not scalar calibration"),
        };
        for word in [0, 4, 8] {
            self.word(at + word, 0)?;
        }
        self.word(at + 12, channel)?;
        self.word(at + 16, value)
    }

    fn column(
        &mut self,
        source: &Program<'_>,
        native: &Program<'_>,
        channel: u32,
        native_read: &Read,
    ) -> Result<u32> {
        let curve = source.curve(channel)?;
        let row = source.curves[(channel as usize - 0x1E00) / 32];
        let stat = source.payload.u32(row + 32)?;
        ensure!(
            stat_overrides_of(&source.overrides, stat)
                == stat_overrides_of(&native.overrides, stat),
            "Source and native stat overrides differ"
        );
        if let Read::Curve(channel) = native_read {
            ensure!(
                (0x1E00..0x1FE0).contains(channel),
                "Native stat channel encoding differs"
            );
            let target = *native
                .curves
                .get((*channel as usize - 0x1E00) / 32)
                .context("Native calibration curve")?;
            ensure!(
                native.payload.u32(target + 32)? == stat,
                "Source and native stat identities differ"
            );
        }
        if let Some(&channel) = self.channels.get(&channel) {
            return Ok(channel);
        }
        let (index, column) = self.place(native, &curve)?;
        let new = 0x1E00 + u32::try_from(index * 32 + column)?;
        let mut curve = curve;
        curve["translation"] = json!(index);
        curve["column"] = json!(column);
        self.curves.push(curve);
        self.channels.insert(channel, new);
        Ok(new)
    }

    fn place(&self, native: &Program<'_>, curve: &Value) -> Result<(usize, usize)> {
        // E83C40 masks the row to four bits and reserves row 15 for another operand kind.
        // Packing beside existing columns keeps every original channel identity unchanged.
        for (index, &row) in native.curves.iter().enumerate().take(15) {
            if json!(native.payload.bytes::<24>(row + 32)?) != curve["metadata"] {
                continue;
            }
            let tiers = native.payload.array(row + 16, 16, None)?;
            if tiers.len()
                != curve["samples"]
                    .as_array()
                    .context("Source stat samples")?
                    .len()
            {
                continue;
            }
            let count = native.payload.array(row, 16, None)?.len();
            ensure!(
                tiers
                    .iter()
                    .all(|&at| native.payload.u64(at).ok() == Some(count as u64)),
                "Native curve output counts differ"
            );
            let appended = self
                .curves
                .iter()
                .filter(|c| c["translation"].as_u64() == Some(index as u64))
                .count();
            if count + appended < 32 {
                return Ok((index, count + appended));
            }
        }
        let mut index = native.curves.len();
        while index < 15 {
            let existing = self
                .curves
                .iter()
                .filter(|c| c["translation"].as_u64() == Some(index as u64))
                .collect::<Vec<_>>();
            if existing.is_empty() {
                return Ok((index, 0));
            }
            if existing[0]["metadata"] == curve["metadata"]
                && existing[0]["samples"].as_array().map(Vec::len)
                    == curve["samples"].as_array().map(Vec::len)
                && existing.len() < 32
            {
                return Ok((index, existing.len()));
            }
            index += 1;
        }
        anyhow::bail!("Source calibration exceeds the native 15-row, 32-column operand layout")
    }
}

pub(super) fn translate(source: &Translator, native: &mut Reader, donor: u32) -> Result<Value> {
    let manager = sundial::package_authoring::PackageManager::new(
        &native.manager.package_dir,
        native.manager.version,
        Some(native.manager.platform),
    )
    .map_err(anyhow::Error::msg)?;
    let runtime = runtime::load_runtime_entity_for_item_with_manager(&manager, donor)
        .map_err(anyhow::Error::msg)?;
    let bindings =
        weapon_component_bindings(&runtime.payload, TRANSLATOR).map_err(anyhow::Error::msg)?;
    let [binding] = bindings.as_slice() else {
        anyhow::bail!("Native stat translator binding differs");
    };
    let payload = native.tag(binding.owner_tag, Some(0x80809C36))?;
    let from = Program::read(&source.payload, source.key, true)?;
    let to = Program::read(&payload, runtime.weapon_translation_group_hash, false)?;
    let mut changes = Changes::default();
    let mut scopes = BTreeMap::<&str, Value>::new();
    for name in SCOPES {
        let blocked = requires(name)
            .iter()
            .find(|dependency| scopes[**dependency]["status"] != "translated");
        if let Some(dependency) = blocked {
            scopes.insert(
                name,
                json!({"status":"native","reason":format!(
                    "Source {name} calibration depends on {dependency}, which stays native")}),
            );
            continue;
        }
        let mut next = changes.clone();
        let report = match next.copy(&from, &to, name) {
            Ok(scaled) => {
                changes = next;
                json!({"status":"translated","gated_native_rows":scaled})
            }
            Err(error) => {
                crate::cancellation::check()?;
                // Rows of another shape can still take the source program whole.
                let mut next = changes.clone();
                match next.rewrite(&from, &to, name) {
                    Ok((rewritten, neutral, scaled)) => {
                        changes = next;
                        json!({"status":"translated","gated_native_rows":scaled,
                            "rewritten_rows":rewritten,"neutral_rows":neutral})
                    }
                    Err(second) => {
                        crate::cancellation::check()?;
                        json!({"status":"native","reason":format!("{error:#}"),
                            "rewrite_reason":format!("{second:#}")})
                    }
                }
            }
        };
        scopes.insert(name, report);
    }
    ensure!(
        !changes.curves.is_empty(),
        "No source stat calibration was qualified"
    );
    let edits = changes.cells.into_iter().filter(|(at, bytes)| payload.0.get(*at..at + 4) != Some(bytes.as_slice()))
        .map(|(at, after)| Ok(json!({"offset":u32::try_from(u64::try_from(at)?.checked_sub(binding.resource_offset)
            .context("Stat edit precedes its resource")?)?,"before":payload.bytes::<4>(at)?,"after":after})))
        .collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"status":"translated","source_owner":source.owner,"native_owner":binding.owner_tag,
        "native_resource":binding.resource_offset,"native_sha256":hex::encode(Sha256::digest(&payload.0)),
        "source_key":source.key,"native_key":runtime.weapon_translation_group_hash,
        "source_table":from.index,"native_table":to.index,
        "native_translations":u32::try_from(u64::try_from(to.table)?.checked_sub(binding.resource_offset)
            .context("Stat table precedes its resource")?)?,
        "appended_curves":changes.curves,"edits":edits,"firing":scopes["firing"],"reload":scopes["reload"],
        "calibration":scopes,"scope":"Qualified source curves and constants with matching ordered native routes, including magazine, reserves, damage, precision, hip-fire accuracy and blast radius. Native controllers, named channels, conditional native rows and unsupported scopes remain.",
        "gameplay_verified":false}),
    )
}
