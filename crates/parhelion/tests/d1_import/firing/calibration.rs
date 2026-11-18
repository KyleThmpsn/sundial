//! Independent source and staged route decoding. The converter's edits and channel map are not
//! inputs to this oracle. Curve domains and every tier remain observable after portable reload.
use super::*;

#[derive(Clone, Debug, PartialEq)]
enum Operand {
    Constant(u32),
    Input([u32; 3], usize),
    Curve {
        metadata: [u8; 24],
        samples: Vec<u32>,
    },
}

#[derive(Clone, Debug, PartialEq)]
struct Route {
    operation: u32,
    operands: Vec<Operand>,
}

/// Every scope the oracle reads, in the order a scope may depend on an earlier one.
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

fn scope(component: u32, input: u32) -> Option<&'static str> {
    Some(match (component, input) {
        (2, 0..=5 | 22..=25) | (4, 0..=1) => "firing",
        (3, 0..=2) => "reload",
        (0, 0..=6) => "handling",
        (0, 7..=15 | 17 | 19..=21) => "aiming",
        (2, 14 | 15 | 17) => "accuracy",
        (2, 54..=64) => "recoil",
        (2, 43 | 47) => "velocity",
        (2, 6..=13) => "cadence",
        (1, 0 | 3) => "magazine",
        (1, 1) => "reserves",
        (1, 2) => "reserve_input_2",
        (1, 4) => "reserve_input_4",
        (2, 32) => "damage",
        (2, 36) => "precision",
        (2, 30) => "hip_fire",
        (2, 66 | 67) => "blast_radius",
        _ => return None,
    })
}

/// The scopes whose component inputs or per-shot meaning a scope's source program relies on.
/// A scope that follows the source while one of these follows the base would mix calibrations.
fn requires(name: &str) -> &'static [&'static str] {
    match name {
        "accuracy" | "cadence" | "magazine" | "damage" => &["firing"],
        "reserves" | "reserve_input_2" | "reserve_input_4" => &["magazine"],
        "precision" => &["damage"],
        _ => &[],
    }
}

fn input(p: &Payload, at: usize, old: bool) -> Result<[u32; 3]> {
    let mut value = [p.u32(at)?, p.u32(at + 4)?, p.u32(at + 8)?];
    if old && value[0] == 2 {
        value[1] = match value[1] {
            0..=15 | 17 | 22..=25 | 30 | 32 => value[1],
            33 => 36,
            38 => 43,
            39 => 47,
            42..=52 => value[1] + 12,
            53 | 54 => value[1] + 13,
            _ => u32::MAX,
        };
    }
    Ok(value)
}

type Programs = BTreeMap<[u32; 3], Vec<Route>>;

/// A native row behind the player-state gate (flag byte `+0D`) that multiplies its own input by
/// a literal. Such rows have no source counterpart and must survive staging unchanged.
fn gated(old: bool, destination: [u32; 3], route: &Route) -> bool {
    !old && route.operation == 0x102
        && matches!(
            route.operands.as_slice(),
            [Operand::Input(read, _), Operand::Constant(_)] if *read == destination
        )
}

/// The ordered program of every scoped input, and apart from it the gated rows.
fn routes(p: &Payload, table: usize, old: bool) -> Result<(Programs, Programs)> {
    let translations = p.array(table, 56, None)?;
    let operand = |at: usize| -> Result<Operand> {
        if p.u32(at + 16)? != 0x7F7FFFFF {
            return Ok(Operand::Constant(p.u32(at + 16)?));
        }
        let channel = p.u32(at + 12)?;
        if p.u32(at)? != 0 || channel < 0x1E00 {
            return Ok(Operand::Input(input(p, at, old)?, 0));
        }
        let number = (channel - 0x1E00) as usize;
        ensure!(
            channel & 0x1000 != 0 && (channel >> 5) & 15 != 15,
            "Readback operand does not select a native curve"
        );
        let row = *translations
            .get(((channel >> 5) & 15) as usize)
            .context("Readback curve index")?;
        let samples = p
            .array(row + 16, 16, None)?
            .into_iter()
            .map(|tier| {
                let cells = p.array(tier, 4, Some(0x8080000F))?;
                p.u32(*cells.get(number % 32).context("Readback curve column")?)
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(!samples.is_empty(), "Readback curve is empty");
        Ok(Operand::Curve {
            metadata: p.bytes(row + 32)?,
            samples,
        })
    };
    let mut program = Vec::new();
    for at in p.array(table + 16, 80, None)? {
        let destination = input(p, at, old)?;
        if scope(destination[0], destination[1]).is_none() {
            continue;
        }
        let operation = p.u32(at + 12)?;
        let arity = if operation & 255 == 4 { 1 } else { 2 };
        let operands = (0..arity)
            .map(|i| operand(at + 16 + i * 20))
            .collect::<Result<Vec<_>>>()?;
        // A row that copies its own input to itself changes nothing. The importer leaves one
        // where the base has a row the source program has none for.
        if !old
            && operation == 4
            && matches!(operands.as_slice(), [Operand::Input(read, _)] if *read == destination)
        {
            continue;
        }
        program.push((
            p.u32(at + 76)?,
            vec![destination],
            Route {
                operation,
                operands,
            },
        ));
    }
    for at in p.array(table + 32, 40, None)? {
        let destinations = p
            .array(at, 12, None)?
            .into_iter()
            .map(|d| input(p, d, old))
            .collect::<Result<Vec<_>>>()?;
        if !destinations.iter().any(|d| scope(d[0], d[1]).is_some()) {
            continue;
        }
        let operands = p
            .array(at + 16, 20, None)?
            .into_iter()
            .map(operand)
            .collect::<Result<Vec<_>>>()?;
        program.push((
            p.u32(at + 36)?,
            destinations,
            Route {
                operation: 0x10000 + p.u32(at + 32)?,
                operands,
            },
        ));
    }
    program.sort_by_key(|r| r.0);
    let mut result = BTreeMap::<_, Vec<_>>::new();
    let mut scaled = Programs::new();
    for (_, destinations, mut route) in program {
        if let [destination] = destinations.as_slice()
            && gated(old, *destination, &route)
        {
            scaled.entry(*destination).or_default().push(route);
            continue;
        }
        for operand in &mut route.operands {
            if let Operand::Input(input, generation) = operand {
                *generation = result.get(input).map_or(0, Vec::len);
            }
        }
        for destination in destinations {
            if scope(destination[0], destination[1]).is_some() {
                result.entry(destination).or_default().push(route.clone());
            }
        }
    }
    ensure!(!result.is_empty(), "Readback stat program is empty");
    Ok((result, scaled))
}

pub(super) fn check(
    source: (&Payload, u32),
    stock: (&Payload, u32),
    staged: (&Payload, u32),
    case: &Value,
) -> Result<Value> {
    let (expected, absent) = routes(source.0, selected(source.0, source.1, true)?, true)?;
    let (original, scaled) = routes(stock.0, selected(stock.0, stock.1, false)?, false)?;
    let (actual, kept) = routes(staged.0, selected(staged.0, staged.1, false)?, false)?;
    ensure!(
        absent.is_empty() && kept == scaled,
        "Gated native rows changed during staging"
    );
    let mut scopes = Vec::new();
    let mut followed = BTreeMap::new();
    for name in SCOPES {
        let select = |program: &BTreeMap<[u32; 3], Vec<Route>>| {
            program
                .iter()
                .filter(|(d, _)| scope(d[0], d[1]) == Some(name))
                .map(|(d, r)| (*d, r.clone()))
                .collect::<BTreeMap<_, _>>()
        };
        let got = select(&actual);
        let source = !got.is_empty() && got == select(&expected);
        let native = got == select(&original);
        ensure!(
            source || native,
            "Staged {name} program follows neither source nor native input contract"
        );
        let required = case["required_stat_scopes"]
            .as_array()
            .is_some_and(|names| names.iter().any(|n| n == name))
            || (name == "firing" && case["required_firing_timing"] == true);
        ensure!(
            !required || source,
            "Required dynamic source {name} program did not survive staging"
        );
        let retained = case["required_native_stat_scopes"]
            .as_array()
            .is_some_and(|names| names.iter().any(|n| n == name));
        ensure!(
            !retained || native,
            "Unqualified {name} program did not retain native calibration"
        );
        // A program equal in both tables follows either. Otherwise a source scope needs every
        // scope it depends on to follow the source too.
        for dependency in requires(name) {
            ensure!(
                !source || native || followed.get(dependency) == Some(&true),
                "Staged {name} program follows the source while {dependency} follows the base"
            );
        }
        followed.insert(name, source);
        scopes.push(
            json!({"scope":name,"follows":if source {"source"} else {"native"},
            "inputs":got.len(),"program":format!("{got:?}"),
            "gated_native_rows":kept.keys().filter(|d| scope(d[0], d[1]) == Some(name)).count()}),
        );
    }
    Ok(json!({"scopes":scopes,"dynamic_curves":true,"gameplay_verified":false}))
}
