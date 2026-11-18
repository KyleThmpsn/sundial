//! Source reload and handling curves on private native columns. Each stat moves as one scope,
//! including every consumer of its native inputs, so carrying its source score cannot retune an
//! unconverted recipient. The native controller and its stat modifiers remain in charge.
use super::*;
mod overrides;
mod source;

struct Calibration {
    name: &'static str,
    stat: u32,
    inputs: &'static [u32],
    remove_overrides: bool,
    scope: Scope,
}

const CALIBRATIONS: [Calibration; 2] = [
    Calibration {
        name: "reload",
        stat: 0xF9A04987,
        inputs: &[9],
        remove_overrides: false,
        scope: Scope {
            name: "Reload",
            accepts: |(component, input, _)| component == 3 && matches!(input, 1 | 2),
            required: &[(3, 1, u32::MAX), (3, 2, u32::MAX)],
            interval: false,
        },
    },
    Calibration {
        name: "handling",
        stat: 0x383D6DBC,
        inputs: &[14, 15, 16],
        remove_overrides: true,
        scope: Scope {
            name: "Handling",
            accepts: |(component, input, _)| {
                matches!((component, input), (0, 0 | 2 | 4 | 5 | 6) | (7, 0 | 1))
            },
            required: &[
                (0, 0, u32::MAX),
                (0, 2, u32::MAX),
                (0, 4, u32::MAX),
                (0, 5, u32::MAX),
                (0, 6, u32::MAX),
                (7, 0, u32::MAX),
                (7, 1, u32::MAX),
            ],
            interval: false,
        },
    },
];

struct Curve {
    identity: (usize, usize),
    metadata: [u8; 24],
    samples: Vec<u32>,
}

/// Appended columns share one allocator across accepted scopes. A rejected scope discards its
/// complete candidate, including its reserved columns, without disturbing earlier scopes.
#[derive(Clone, Default)]
struct Plan {
    added: Vec<Value>,
    channels: BTreeMap<(usize, usize, u32), u32>,
    edits: BTreeMap<usize, u32>,
    unfixed: BTreeSet<u32>,
}

impl Plan {
    fn apply(
        &mut self,
        from: &Payload,
        group: usize,
        to: &Table,
        calibration: &Calibration,
    ) -> Result<()> {
        let curves = source::curves(from, group, calibration)?;
        ensure!(
            to.routes
                .keys()
                .all(|destination| curves.contains_key(destination)),
            "Native {} has an additional scoped destination",
            calibration.scope.name
        );
        validate_native(to, calibration)?;
        if calibration.remove_overrides {
            self.unfixed.extend(calibration.inputs);
        }
        for (destination, mut curve) in curves {
            let routes = to
                .routes
                .get(&destination)
                .context("Native timing destination")?;
            let [route] = routes.as_slice() else {
                anyhow::bail!("Native timing input has several writers");
            };
            ensure!(!route.logic, "Native timing writer is not a direct copy");
            let [Operand::Output(original)] = route.operands.as_slice() else {
                anyhow::bail!("Native timing is not stat driven");
            };
            let (row, _) = to.curve(*original)?;
            let input = to.payload.u32(row + 32)?;
            ensure!(
                calibration.inputs.contains(&input),
                "Native timing reads a different stat"
            );
            // Native ADS can share input 14 with ready/stow while the source uses its separate
            // handling input 16. Keep the recipient's native input and its modifier path.
            curve.metadata[..4].copy_from_slice(&input.to_le_bytes());
            let identity = (curve.identity.0, curve.identity.1, input);
            let channel = if let Some(&channel) = self.channels.get(&identity) {
                channel
            } else {
                let (index, column) = place(to, &self.added, &curve)?;
                let channel = 0x1E00 + u32::try_from(index * 32 + column)?;
                self.added.push(json!({"translation":index,"column":column,
                    "metadata":curve.metadata,"samples":curve.samples}));
                self.channels.insert(identity, channel);
                channel
            };
            ensure!(
                self.edits.insert(route.at[0] + 12, channel).is_none(),
                "Timing scopes overlap one native operand"
            );
        }
        Ok(())
    }
}

fn validate_native(to: &Table, calibration: &Calibration) -> Result<()> {
    if !calibration.remove_overrides {
        for &input in calibration.inputs {
            ensure!(
                stat_overrides_of(&to.overrides, input).is_empty(),
                "Native {} has a stat override",
                calibration.scope.name
            );
        }
    }
    // The raw source score will feed every native input of this stat. A retained consumer
    // of one of those inputs would inherit a different value through its old curve.
    for channels in to.others.values() {
        for &channel in channels.iter().filter(|&&c| c != 0) {
            let (row, _) = to.curve(channel)?;
            ensure!(
                !calibration.inputs.contains(&to.payload.u32(row + 32)?),
                "Native {} has an unconverted stat consumer",
                calibration.scope.name
            );
        }
    }
    for &row in &to.translations {
        if calibration.inputs.contains(&to.payload.u32(row + 32)?) {
            for recipient in to.payload.array(row, 16, Some(0x8080397F))? {
                ensure!(
                    to.payload.u64(recipient)? == 0,
                    "Native {} has direct recipients",
                    calibration.scope.name
                );
            }
        }
    }
    Ok(())
}

/// Existing channels retain their ordinals and cells. Packing requires the same stat input,
/// sampling domain and tier count. A distinct domain requires an addressable new native row.
fn place(to: &Table, added: &[Value], curve: &Curve) -> Result<(usize, usize)> {
    for (index, &row) in to.translations.iter().enumerate().take(15) {
        if to.payload.bytes::<24>(row + 32)? != curve.metadata {
            continue;
        }
        let tiers = to.payload.array(row + 16, 16, Some(0x80809F2A))?;
        if tiers.len() != curve.samples.len() {
            continue;
        }
        let columns = to.payload.array(row, 16, Some(0x8080397F))?.len();
        ensure!(
            tiers
                .iter()
                .all(|&r| to.payload.u64(r).ok() == Some(columns as u64)),
            "Native timing curve columns differ"
        );
        let next = columns
            + added
                .iter()
                .filter(|c| c["translation"].as_u64() == Some(index as u64))
                .count();
        if next < 32 {
            return Ok((index, next));
        }
    }
    for index in to.translations.len()..15 {
        let same = added
            .iter()
            .filter(|c| c["translation"].as_u64() == Some(index as u64))
            .collect::<Vec<_>>();
        if same.is_empty() {
            return Ok((index, 0));
        }
        if same[0]["metadata"] == json!(curve.metadata)
            && same[0]["samples"].as_array().map(Vec::len) == Some(curve.samples.len())
            && same.len() < 32
        {
            return Ok((index, same.len()));
        }
    }
    anyhow::bail!("Source timing exceeds the native curve address space")
}

pub(crate) fn translate(
    source: &Payload,
    group: usize,
    native: &mut Reader,
    donor: u32,
    stats: &BTreeSet<u32>,
) -> Result<Value> {
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
        anyhow::bail!("Native timing translator binding differs");
    };
    let p = (*native.tag(binding.owner_tag, Some(0x80809C36))?).clone();
    let mut plan = Plan::default();
    let mut passes = serde_json::Map::new();
    let mut table = None;
    for calibration in &CALIBRATIONS {
        let mut candidate = plan.clone();
        let attempt = (|| -> Result<usize> {
            ensure!(
                stats.contains(&calibration.stat),
                "Source stat has no literal investment value"
            );
            let to = Table::read(
                p.clone(),
                runtime.weapon_translation_group_hash,
                (&SHADOWKEEP, calibration.scope),
            )?;
            candidate.apply(source, group, &to, calibration)?;
            let definition = p.pointer(24)?;
            Ok(p.array(definition + 0x48, 48, Some(0x80803975))?[to.index])
        })();
        let pass = match attempt {
            Ok(selected) => {
                ensure!(
                    table.is_none_or(|table| table == selected),
                    "Timing scopes selected different tables"
                );
                table = Some(selected);
                plan = candidate;
                json!({"status":"translated"})
            }
            Err(error) => {
                crate::cancellation::check()?;
                json!({"status":"native","reason":format!("{error:#}")})
            }
        };
        passes.insert(calibration.name.into(), pass);
    }
    let Some(table) = table else {
        return Ok(
            json!({"status":"native","reload":passes["reload"],"handling":passes["handling"]}),
        );
    };
    let relative = |at: usize| -> Result<u32> {
        Ok(u32::try_from(
            u64::try_from(at)?
                .checked_sub(binding.resource_offset)
                .context("Timing field precedes its resource")?,
        )?)
    };
    let overrides = overrides::plan(
        &p,
        binding.resource_offset,
        runtime.weapon_translation_group_hash,
        &plan.unfixed,
    )?;
    let edits = plan.edits.into_iter().map(|(at, value)| -> Result<Value> {
        Ok(json!({"offset":relative(at)?,"before":p.bytes::<4>(at)?,"after":value.to_le_bytes()}))
    }).collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"status":"translated","native_key":runtime.weapon_translation_group_hash,
        "native_resource":binding.resource_offset,"native_owner":binding.owner_tag,
        "native_sha256":hex::encode(Sha256::digest(&p.0)),"native_translations":relative(table)?,
        "source_sha256":hex::encode(Sha256::digest(&source.0)),"appended_curves":plan.added,
        "edits":edits,"stat_overrides":overrides,"reload":passes["reload"],"handling":passes["handling"],
        "scope":"Qualified source reload and handling curves on private native columns. Native state transitions and stat modifiers remain."}),
    )
}
