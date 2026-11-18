//! Constant source pose corrections selected by runtime names before retargeting. Numeric layer
//! drivers and time-varying correction clips remain separate.
use super::*;

pub(super) struct Correction {
    values: Vec<Pose>,
    report: Value,
}

pub(super) struct Fixed {
    dimensions: Vec<u8>,
    variants: BTreeMap<Vec<(u8, u32)>, Variant>,
    corrections: Vec<Correction>,
}

/// The first matching option wins. An unresolved condition before that option keeps the choice
/// dynamic, while a later option cannot invalidate an already selected correction.
fn option(
    table: &Payload,
    choice: usize,
    controller: &controller::Controller,
    inputs: &BTreeMap<u8, u32>,
) -> Result<Option<(usize, usize)>> {
    for (option, at) in table
        .array(choice, 80, Some(0x80803C51))?
        .into_iter()
        .enumerate()
    {
        let conditions = table.array(at, 8, Some(0x80803C58))?;
        let mut matches = true;
        let mut known = true;
        for condition in conditions {
            let builtin = u8::try_from(table.u32(condition)?)?;
            ensure!(
                builtin < 24,
                "Source layer builtin is outside its dictionary"
            );
            let name = table.u32(condition + 4)?;
            if inputs.contains_key(&builtin) || selection::names(builtin).is_some() {
                matches &= controller.accepts(
                    builtin,
                    name,
                    inputs.get(&builtin).copied().unwrap_or(controller::UNSET),
                );
            } else {
                known = false;
            }
        }
        if matches {
            return Ok(known.then_some((option, at)));
        }
    }
    Ok(None)
}

fn constant(
    source: &mut Reader,
    table: &Payload,
    at: usize,
    controller: &controller::Controller,
    rig: &source::Rig,
) -> Result<Option<(u32, Vec<Pose>)>> {
    // Constant weight one, no weight/phase parameter, unit rate, loop playback.
    // Other producers need their numeric parameter programs and runtime clocks.
    if table.bytes::<32>(at + 32)?
        != [
            1, 0, 0, 0, 0, 0, 128, 63, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 128, 63, 0, 0, 255, 255,
            0, 0, 0, 0, 1, 0, 0, 0,
        ]
    {
        return Ok(None);
    }
    let weights = table.array(at + 16, 12, Some(0x80803C59))?;
    let [weight] = weights.as_slice() else {
        return Ok(None);
    };
    if table.f32(weight + 4)? != 1. {
        return Ok(None);
    }
    let ordinal = usize::try_from(table.u32(*weight)?)?;
    let tag = *controller
        .clips
        .get(ordinal)
        .context("Source fixed layer clip is absent")?;
    let original = source.tag(tag, Some(0x8080AE01))?;
    if original.u64(0x160)? != 0 {
        return Ok(None);
    }
    let (candidate, _) = clip::lower(&original, None)?;
    if candidate.u8(0x181)? != 1 || candidate.u64(0x30)? != 0 {
        return Ok(None);
    }
    let (frames, slots, common) = layout(&candidate, rig)?;
    let identity = vec![Pose::IDENTITY.0; slots];
    let values = native_clip::sample(&candidate, &identity, 0)?;
    let mut constant = true;
    for frame in 1..usize::from(frames) {
        constant &= native_clip::sample(&candidate, &identity, frame)?
            .iter()
            .zip(&values)
            .all(|(a, b)| a.iter().zip(b).all(|(x, y)| (x - y).abs() <= 1e-6));
    }
    if !constant {
        return Ok(None);
    }
    // A layer can cover only the shared leading control groups. Its omitted groups contribute
    // neutral deltas rather than replacing additional rig controls with rest transforms.
    let mut correction = vec![Pose::IDENTITY; rig.controls.len()];
    for (target, value) in correction.iter_mut().zip(&values[..common]) {
        *target = Pose::checked(*value)?;
    }
    Ok(Some((tag, correction)))
}

type Choice = (usize, usize);
type Choices = BTreeMap<Choice, usize>;
type Dispatch = BTreeMap<u32, Vec<Choice>>;
type Variant = BTreeMap<u32, Vec<usize>>;

fn dimensions(table: &Payload, choice: usize, fixed: &BTreeMap<u8, u32>) -> Result<BTreeSet<u8>> {
    let mut found = BTreeSet::new();
    for option in table.array(choice, 80, Some(0x80803C51))? {
        for condition in table.array(option, 8, Some(0x80803C58))? {
            let builtin = u8::try_from(table.u32(condition)?)?;
            ensure!(
                builtin < 24,
                "Source layer builtin is outside its dictionary"
            );
            if !fixed.contains_key(&builtin) && selection::names(builtin).is_some() {
                found.insert(builtin);
            }
        }
    }
    Ok(found)
}

fn choices(
    table: &Payload,
    layers: &[usize],
    fixed: &BTreeMap<u8, u32>,
) -> Result<(Choices, Vec<u8>)> {
    let mut choices = BTreeMap::new();
    let mut inputs = BTreeSet::new();
    for (layer, &row) in layers.iter().enumerate() {
        for (choice, at) in table
            .array(row, 16, Some(0x80803C4F))?
            .into_iter()
            .enumerate()
        {
            choices.insert((layer, choice), at);
            inputs.extend(dimensions(table, at, fixed)?);
        }
    }
    Ok((choices, inputs.into_iter().collect()))
}

fn dispatch(table: &Payload, count: usize, choices: &Choices) -> Result<Dispatch> {
    let aliases = table.array(0x28, 2, Some(0x80800006))?;
    let routes = table.array(0x38, 32, Some(0x808039DF))?;
    ensure!(
        aliases.len() == count,
        "Source layer dispatch extent differs"
    );
    let mut result = BTreeMap::new();
    for (descriptor, &at) in aliases.iter().enumerate() {
        let alias = table.u16(at)?;
        if alias == u16::MAX {
            continue;
        }
        let route = *routes
            .get(usize::from(alias))
            .context("Source fixed layer route is absent")?;
        let mut seen = BTreeSet::new();
        let mut selected = Vec::new();
        for edge in table.array(route, 4, Some(0x808039E1))? {
            let key = (
                usize::from(table.u16(edge + 2)?),
                usize::from(table.u16(edge)?),
            );
            ensure!(
                seen.insert(key),
                "Source correction route repeats a layer choice"
            );
            ensure!(
                choices.contains_key(&key),
                "Source correction choice is absent"
            );
            selected.push(key);
        }
        result.insert(u32::try_from(descriptor)?, selected);
    }
    Ok(result)
}

struct Baker<'a> {
    table: &'a Payload,
    tag: u32,
    layers: &'a [usize],
    controller: &'a controller::Controller,
    rig: &'a source::Rig,
    cached: BTreeMap<usize, Option<usize>>,
    corrections: Vec<Correction>,
}

impl Baker<'_> {
    fn choice(
        &mut self,
        source: &mut Reader,
        (layer, choice): Choice,
        at: usize,
        inputs: &BTreeMap<u8, u32>,
    ) -> Result<Option<usize>> {
        let Some((option, at)) = option(self.table, at, self.controller, inputs)? else {
            return Ok(None);
        };
        if let Some(&value) = self.cached.get(&at) {
            return Ok(value);
        }
        let name = self.table.u32(self.layers[layer] + 16)?;
        let value =
            constant(source, self.table, at, self.controller, self.rig)?.map(|(tag, values)| {
                let index = self.corrections.len();
                self.corrections.push(Correction {
                    values,
                    report: json!({"table":self.tag,"layer":layer,"name":format!("{name:08X}"),
                    "choice":choice,"option":option,"clip":tag}),
                });
                index
            });
        self.cached.insert(at, value);
        Ok(value)
    }

    fn variant(
        &mut self,
        source: &mut Reader,
        choices: &Choices,
        dispatch: &Dispatch,
        inputs: &BTreeMap<u8, u32>,
    ) -> Result<Variant> {
        let mut selected = BTreeMap::new();
        for (&descriptor, route) in dispatch {
            let mut found = Vec::new();
            for &key in route {
                if let Some(index) = self.choice(source, key, choices[&key], inputs)? {
                    found.push(index);
                }
            }
            if !found.is_empty() {
                selected.insert(descriptor, found);
            }
        }
        Ok(selected)
    }
}

impl Fixed {
    pub(super) fn read(
        source: &mut Reader,
        pose: &Component,
        controller: &controller::Controller,
        rig: &source::Rig,
        fixed: &BTreeMap<u8, u32>,
    ) -> Result<Self> {
        let (_, owner, base, _) = pose;
        ensure!(
            owner.u32(base + 0x170)? == controller.bank_tag,
            "Source pose and action controllers use different banks"
        );
        let tag = owner.u32(base + 0x184)?;
        let table = source.tag(tag, Some(0x808039DB))?;
        let layers = table.array(8, 32, Some(0x80803C45))?;
        let (choices, dimensions) = choices(&table, &layers, fixed)?;
        let dispatch = dispatch(&table, controller.descriptors.len(), &choices)?;
        let mut baker = Baker {
            table: &table,
            tag,
            layers: &layers,
            controller,
            rig,
            cached: BTreeMap::new(),
            corrections: Vec::new(),
        };
        let mut variants = BTreeMap::new();
        selection::expand(&dimensions, &mut fixed.clone(), &mut |inputs| {
            let key = dimensions
                .iter()
                .filter_map(|b| inputs.get(b).map(|v| (*b, *v)))
                .collect();
            variants.insert(key, baker.variant(source, &choices, &dispatch, inputs)?);
            Ok(())
        })?;
        Ok(Self {
            dimensions,
            variants,
            corrections: baker.corrections,
        })
    }

    pub(super) fn dimensions(&self) -> &[u8] {
        &self.dimensions
    }

    pub(super) fn selection(
        &self,
        descriptor: u32,
        inputs: &BTreeMap<u8, u32>,
    ) -> Result<&[usize]> {
        let key = self
            .dimensions
            .iter()
            .filter_map(|b| inputs.get(b).map(|v| (*b, *v)))
            .collect::<Vec<_>>();
        Ok(self
            .variants
            .get(&key)
            .context("Source correction inputs are outside the adapted names")?
            .get(&descriptor)
            .map(Vec::as_slice)
            .unwrap_or_default())
    }

    pub(super) fn get(&self, selected: &[usize]) -> Vec<&Correction> {
        selected.iter().map(|&i| &self.corrections[i]).collect()
    }
}

pub(super) fn apply(values: &mut [Pose], corrections: &[&Correction]) {
    for correction in corrections {
        for (value, delta) in values.iter_mut().zip(&correction.values) {
            let mut a = *value;
            let mut b = *delta;
            a.0[4..].copy_from_slice(&[0., 0., 0., 1.]);
            b.0[4..].copy_from_slice(&[0., 0., 0., 1.]);
            value.0[..4].copy_from_slice(&a.then(b).0[..4]);
            for axis in 4..7 {
                value.0[axis] += delta.0[axis];
            }
            value.0[7] *= delta.0[7];
        }
    }
}

pub(super) fn report(corrections: &[&Correction]) -> Vec<Value> {
    corrections
        .iter()
        .map(|correction| correction.report.clone())
        .collect()
}
