//! Resolve acyclic vertex branches only when their actual immutable inputs prove the path.
use super::*;

#[derive(Default)]
pub(super) struct Flow {
    definitions: BTreeMap<u32, u32>,
    branches: BTreeMap<u32, String>,
}

impl Flow {
    pub fn read(body: &str) -> Result<Self> {
        let mut result = Self::default();
        let mut block = 0;
        for line in body.lines().map(str::trim) {
            let label = line
                .strip_prefix("; <label>:")
                .and_then(|s| s.split_whitespace().next())
                .or_else(|| {
                    line.split_once(':')
                        .map(|(s, _)| s)
                        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                });
            if let Some(label) = label {
                block = label.parse()?;
                continue;
            }
            let code = line.split(';').next().unwrap().trim();
            if let Some((name, _)) = code.split_once(" = ") {
                ensure!(
                    result.definitions.insert(id(name)?, block).is_none(),
                    "Repeated source vertex identifier"
                );
            } else if code.starts_with("br ") || code == "ret void" {
                ensure!(
                    result.branches.insert(block, code.to_owned()).is_none(),
                    "Repeated source vertex terminator"
                );
            }
        }
        Ok(result)
    }

    pub fn predecessor(
        &self,
        definition: u32,
        condition: impl Fn(&str) -> Result<bool>,
    ) -> Result<u32> {
        let destination = *self
            .definitions
            .get(&definition)
            .context("Source phi definition block")?;
        let incoming = self.reachable(
            0,
            destination,
            &condition,
            &mut BTreeSet::new(),
            &mut BTreeMap::new(),
        )?;
        ensure!(
            incoming.len() == 1,
            "Source phi requires a dynamic predecessor"
        );
        incoming
            .into_iter()
            .next()
            .flatten()
            .context("Source phi block is unreachable")
    }

    fn reachable(
        &self,
        block: u32,
        destination: u32,
        condition: &impl Fn(&str) -> Result<bool>,
        active: &mut BTreeSet<u32>,
        cached: &mut BTreeMap<u32, BTreeSet<Option<u32>>>,
    ) -> Result<BTreeSet<Option<u32>>> {
        if let Some(result) = cached.get(&block) {
            return Ok(result.clone());
        }
        ensure!(
            active.len() + cached.len() < 512 && active.insert(block),
            "Source vertex flow has a loop or exceeds budget"
        );
        let code = self
            .branches
            .get(&block)
            .context("Missing source vertex terminator")?;
        let mut result = BTreeSet::new();
        if code == "ret void" {
            result.insert(None);
        } else {
            let operands = arguments(code.strip_prefix("br ").context("Source vertex branch")?)?;
            let targets = match operands.as_slice() {
                [target] => vec![*target],
                [value, yes, no] => match condition(value) {
                    Ok(true) => vec![*yes],
                    Ok(false) => vec![*no],
                    // A dynamic normal-decoding branch can reconverge before the
                    // requested phi. Accept only a single proven incoming block.
                    Err(_) => vec![*yes, *no],
                },
                _ => anyhow::bail!("Unsupported source vertex branch arity"),
            };
            for target in targets {
                ensure!(target.starts_with("label %"), "Source vertex branch target");
                let next = id(target)?;
                if next == destination {
                    result.insert(Some(block));
                } else {
                    result.extend(self.reachable(next, destination, condition, active, cached)?);
                }
            }
        }
        active.remove(&block);
        cached.insert(block, result.clone());
        Ok(result)
    }
}
