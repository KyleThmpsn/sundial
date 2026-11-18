//! Exact-name native selector emission over qualified runtime input names.
use super::super::selection::Tree;
use super::*;

pub(super) type Choice = Vec<(u32, f32)>;

fn weights(route: &Choice) -> Result<Vec<u8>> {
    let mut previous = 0.;
    let mut rows = Vec::new();
    for &(descriptor, weight) in route {
        ensure!(
            weight.is_finite() && weight >= previous && weight <= 1.,
            "Translated action has invalid cumulative weights"
        );
        rows.extend(descriptor.to_le_bytes());
        rows.extend(weight.to_le_bytes());
        previous = weight;
    }
    ensure!(
        !route.is_empty() && previous == 1.,
        "Translated action does not cover its choice interval"
    );
    Ok(rows)
}

#[derive(Default)]
struct Selector {
    choices: Vec<Choice>,
    records: Vec<[u8; 28]>,
    operations: Vec<u32>,
    parameters: BTreeSet<u8>,
}

fn record(name: u32, alternate: i16, result: i16) -> [u8; 28] {
    let mut row = [0; 28];
    // A zero mask admits only an exact name even if the native dictionary gives it ancestors.
    row[16..20].copy_from_slice(&name.to_le_bytes());
    row[20..22].copy_from_slice(&(-1i16).to_le_bytes());
    row[22..24].copy_from_slice(&alternate.to_le_bytes());
    row[24..26].copy_from_slice(&result.to_le_bytes());
    row
}

impl Selector {
    fn add(&mut self, tree: &Tree<Choice>) -> Result<i16> {
        match tree {
            Tree::Choice { value } => {
                weights(value)?;
                let index = match self.choices.iter().position(|old| old == value) {
                    Some(index) => index,
                    None => {
                        self.choices.push(value.clone());
                        self.choices.len() - 1
                    }
                };
                ensure!(
                    self.choices.len() <= i16::MAX as usize,
                    "Native selector choice capacity exceeded"
                );
                Ok(!i16::try_from(index)?)
            }
            Tree::Input {
                builtin,
                fallback,
                cases,
            } => {
                ensure!(
                    *builtin < 24 && !cases.is_empty(),
                    "Invalid dynamic input selector"
                );
                let operation = self.operations.len();
                let start = self.records.len();
                ensure!(
                    start + cases.len() < i16::MAX as usize && operation + 1 < i16::MAX as usize,
                    "Native selector record or operation capacity exceeded"
                );
                self.operations
                    .push((u32::try_from(start)? << 16) | u32::from(*builtin));
                self.parameters.insert(*builtin);
                self.records.resize(start + cases.len() + 1, [0; 28]);
                let default = self.add(fallback)?;
                self.records[start] = record(controller::UNSET, i16::try_from(start + 1)?, default);
                let mut names = BTreeSet::new();
                for (index, (name, branch)) in cases.iter().enumerate() {
                    ensure!(
                        *name != controller::UNSET && names.insert(*name),
                        "Ambiguous dynamic input names"
                    );
                    let alternate = if index + 1 == cases.len() {
                        -1
                    } else {
                        i16::try_from(start + index + 2)?
                    };
                    let result = self.add(branch)?;
                    self.records[start + index + 1] = record(*name, alternate, result);
                }
                Ok(i16::try_from(operation + 1)?)
            }
        }
    }
}

pub(super) fn node(out: &mut Vec<u8>, tree: &Tree<Choice>) -> Result<(u64, usize)> {
    let at = (out.len() + 19) & !15;
    out.resize(at - 4, 0);
    if let Tree::Choice { value } = tree {
        out.extend(0x80803434u32.to_le_bytes());
        out.resize(at + 16, 0);
        append(out, at, 0x80803439, &weights(value)?, 8)?;
        return Ok((1, at));
    }
    let mut selector = Selector::default();
    ensure!(
        selector.add(tree)? == 1,
        "Native selector entry is not its first operation"
    );
    out.extend(0x80803432u32.to_le_bytes());
    out.resize(at + 120, 0);
    append(
        out,
        at,
        0x80803437,
        &vec![0; selector.choices.len() * 16],
        16,
    )?;
    let fields = Payload(out.clone()).array(at, 16, Some(0x80803437))?;
    for (field, choice) in fields.into_iter().zip(&selector.choices) {
        append(out, field, 0x80803439, &weights(choice)?, 8)?;
    }
    append(
        out,
        at + 16,
        0x80808DA4,
        &selector.parameters.into_iter().collect::<Vec<_>>(),
        1,
    )?;
    append(
        out,
        at + 32,
        0x8080347D,
        &selector.records.into_iter().flatten().collect::<Vec<_>>(),
        28,
    )?;
    append(
        out,
        at + 48,
        0x8080347C,
        &selector
            .operations
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>(),
        4,
    )?;
    Ok((2, at))
}
