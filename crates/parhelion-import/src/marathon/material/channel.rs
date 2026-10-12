//! Authored material channels, resolved through the source assembly hierarchy.
use super::*;

#[derive(Clone)]
pub(super) struct Declaration {
    pub name: u32,
    pub code: Vec<u8>,
    pub constants: Vec<[u8; 16]>,
    pub inputs: Vec<u64>,
    pub initial: [u8; 16],
}

pub(super) type Bank = Vec<Declaration>;

pub(super) fn bank(source: &mut Reader, entity: u32) -> Result<Bank> {
    let entity = source.tag(entity, None)?;
    let mut result = None;
    for row in entity.array(8, 12, None)? {
        let owner = source.tag(entity.u32(row)?, Some(0x8080badb))?;
        let schema = owner.pointer(24)?;
        if owner.u32(schema.checked_sub(4).context("Source channel schema")?)? != 0x8080af75 {
            continue;
        }
        ensure!(result.is_none(), "Ambiguous Marathon material channel bank");
        let instance = owner.pointer(16)?;
        let cached = owner
            .array(instance + 0x50, 16, None)?
            .into_iter()
            .map(|at| owner.bytes::<16>(at))
            .collect::<Result<Vec<_>>>()?;
        // A uniform initial bank is independent of its namespace and sorted
        // runtime slot ordering. Nonuniform banks need a separate slot adapter.
        let initial = *cached.first().context("Empty source channel storage")?;
        ensure!(
            cached.iter().all(|value| *value == initial),
            "Nonuniform source channel initial storage needs a slot mapping"
        );
        let mut bank = Vec::new();
        for row in owner.array(schema + 0x120, 112, Some(0x8080af86))? {
            bank.push(Declaration {
                name: owner.u32(row)?,
                code: owner
                    .array(row + 8, 1, None)?
                    .into_iter()
                    .map(|at| owner.u8(at))
                    .collect::<Result<_>>()?,
                constants: owner
                    .array(row + 24, 16, None)?
                    .into_iter()
                    .map(|at| owner.bytes(at))
                    .collect::<Result<_>>()?,
                inputs: owner
                    .array(row + 56, 8, None)?
                    .into_iter()
                    .map(|at| owner.u64(at))
                    .collect::<Result<_>>()?,
                initial,
            });
        }
        ensure!(
            bank.iter().map(|d| d.name).collect::<BTreeSet<_>>().len() == bank.len(),
            "Duplicate source channel name"
        );
        ensure!(
            bank.len() == cached.len(),
            "Source channel initial storage count differs"
        );
        result = Some(bank);
    }
    Ok(result.unwrap_or_default())
}

pub(super) struct Globals {
    pub native: BTreeMap<u32, u8>,
    pub source: Vec<(u32, [u8; 16])>,
}

fn globals(reader: &mut Reader, source: bool) -> Result<Vec<(u32, [u8; 16])>> {
    let name = if source {
        "render_globals"
    } else {
        "client_bootstrap_patchable"
    };
    let tags = reader
        .manager
        .lookup
        .named_tags
        .iter()
        .filter(|t| t.name == name)
        .map(|t| t.hash.0)
        .collect::<BTreeSet<_>>();
    ensure!(!tags.is_empty(), "Missing {name}");
    let mut result = None;
    for tag in tags {
        let bootstrap = reader.tag(tag, None)?;
        let globals = reader.tag(bootstrap.u32(if source { 0x48 } else { 0x4c })?, None)?;
        let defaults = reader.tag(globals.u32(0x34)?, None)?;
        let names = defaults.array(8, 4, None)?;
        let values = defaults.array(24, 16, None)?;
        ensure!(
            names.len() == values.len() && names.len() <= 256,
            "Invalid global channel table"
        );
        let rows = names
            .into_iter()
            .zip(values)
            .map(|(n, v)| Ok((defaults.u32(n)?, defaults.bytes::<16>(v)?)))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            rows.iter()
                .map(|(name, _)| *name)
                .collect::<BTreeSet<_>>()
                .len()
                == rows.len(),
            "Ambiguous global channel names"
        );
        if let Some(old) = &result {
            ensure!(*old == rows, "Ambiguous global channel defaults");
        }
        result = Some(rows);
    }
    result.context("Missing global channels")
}

impl Globals {
    pub fn read(source: &mut Reader, native: &mut Reader) -> Result<Self> {
        let source = globals(source, true)?;
        let native = globals(native, false)?
            .into_iter()
            .enumerate()
            .map(|(i, (name, _))| Ok((name, u8::try_from(i)?)))
            .collect::<Result<_>>()?;
        Ok(Self { source, native })
    }
}
