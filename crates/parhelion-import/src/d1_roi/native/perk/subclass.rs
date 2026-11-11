//! Subclass grid steps lowered through the item talent route. A subclass node, like armor, has no
//! owning weapon, so its actions take the armor host's target contracts.
use super::*;

/// One step's executable contributions, each perk action and the property grants lowered on their
/// own, with the reason each part that did not lower failed. Grants that equip an ability, and
/// grants into a private bank the caller writes itself, stay with the caller.
pub(crate) struct Lowered {
    pub programs: Vec<Program>,
    pub failures: Vec<String>,
    pub report: Value,
}

pub(crate) struct Lowering {
    grants: grant::Catalog,
}

impl Lowering {
    pub(crate) fn new(source: &Source, native: &mut Reader) -> Result<Self> {
        Ok(Self {
            grants: grant::Catalog::read(source, native)?,
        })
    }

    /// The native graph equipping the source ability row's pattern, when one exists.
    pub(crate) fn native_graph(&self, row: u8) -> Option<u32> {
        self.grants.native_graph(row)
    }

    /// Lower the step's perk actions and property grants. A perk without an action and without
    /// a runtime key only presents the step, so it contributes nothing to lower. A grant gated on
    /// a key lowers only when `gates` maps it to the key the caller applies alongside. A
    /// choice (not a foundation) restores the source defaults its groups leave in place, as it
    /// always applies alone beside its foundation.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn lower(
        &self,
        source: &Source,
        native: &mut Reader,
        folder: &Path,
        perks: &[Value],
        grants: &[Value],
        name: &str,
        gates: &BTreeMap<u32, u32>,
        choice: bool,
    ) -> Lowered {
        let mut programs = Vec::new();
        let mut failures = Vec::new();
        let mut actions = Vec::new();
        let mut equivalents = Vec::new();
        let mut omitted = 0;
        for perk in perks {
            let Some(tag) = perk["action"].as_u64() else {
                if perk["runtime_key"].as_u64() != Some(0x811C9DC5) {
                    failures.push(format!(
                        "Source perk {} has no resolved standalone action",
                        perk["ordinal"]
                    ));
                }
                continue;
            };
            // Stat-driven recharge: effects of kind 1, class 80802B8D, each attaching a regen
            // scalar on one ability. Every native subclass grants the same through its hidden
            // stat passives (entry 19, perks 1 to 3, the same input sources), so the native
            // entry keeps them and these are recorded as matched rather than ported.
            match stat_recharge(source, u32::try_from(tag).unwrap_or(u32::MAX)) {
                Ok(true) => {
                    equivalents.push(tag);
                    continue;
                }
                Ok(false) => {}
                Err(error) => {
                    failures.push(format!("Action {tag:08X}: {error:#}"));
                    continue;
                }
            }
            let lowered = (|| -> Result<(Program, usize)> {
                let tag = u32::try_from(tag)?;
                Host::Armor.validate_action(source, tag)?;
                program(source, tag, name, Some(&mut *native), Some(folder))
            })();
            match lowered {
                Ok((program, skipped)) => {
                    omitted += skipped;
                    actions.push(tag);
                    programs.push(program);
                }
                Err(error) => failures.push(format!("Action {tag:08X}: {error:#}")),
            }
        }
        let mut granted = Value::Null;
        if !grants.is_empty() {
            match self.grants.lower(
                source,
                native,
                &json!({"ability_grants":grants}),
                name,
                gates,
                choice,
            ) {
                Ok((program, report)) => {
                    granted = json!(report);
                    if !program.actions.is_empty() {
                        programs.push(program);
                    }
                }
                Err(error) => failures.push(format!("Ability properties: {error:#}")),
            }
        }
        Lowered {
            programs,
            report: json!({"source_actions":actions,"ability_grants":granted,
                "native_equivalents":equivalents,"omitted_feedback_records":omitted,
                "failures":failures}),
            failures,
        }
    }
}

/// Whether every effect of the action is a stat-driven recharge scalar (kind 1, `80802B8D`).
fn stat_recharge(source: &Source, tag: u32) -> Result<bool> {
    let data = source.read(tag)?;
    let rows = data.array(0x38, 8, None)?;
    if rows.is_empty() {
        return Ok(false);
    }
    for row in rows {
        let at = data.pointer(row)?;
        if data.u8(at)? != 1 || data.u32(at.checked_sub(4).context("Effect marker")?)? != 0x80802B8D
        {
            return Ok(false);
        }
    }
    Ok(true)
}
