//! Authored material channels, resolved through the source assembly hierarchy.
use super::*;

#[derive(Clone)]
pub(super) struct Declaration {
    pub name: u32,
    pub code: Vec<u8>,
    pub constants: Vec<[u8; 16]>,
    pub inputs: Vec<u64>,
    pub initial: [u8; 16],
    pub providers: Vec<Value>,
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
        let providers = providers(source, &entity, &owner, schema)?;
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
                providers: providers.get(&owner.u32(row)?).cloned().unwrap_or_default(),
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

/// Incoming scalar getters write mutable channel storage even when a declaration has no
/// bytecode. These are runtime producers, not zero-valued authored constants.
fn providers(
    source: &mut Reader,
    entity: &Payload,
    bank: &Payload,
    schema: usize,
) -> Result<BTreeMap<u32, Vec<Value>>> {
    let definitions = bank.array(schema + 0x110, 32, Some(0x8080AF8D))?;
    let mut result = BTreeMap::new();
    for row in bank.array(schema + 0x160, 12, Some(0x8080AF85))? {
        ensure!(
            bank.u32(row)? == 0x811C9DC5,
            "Source getter namespace differs"
        );
        let name = bank.u32(row + 4)?;
        let definition = *definitions
            .get(bank.u32(row + 8)? as usize)
            .context("Source getter index")?;
        let mut outputs = Vec::new();
        for event in entity.array(24, 56, Some(0x8080BA61))? {
            if entity.u32(event)? != bank.u32(schema)?
                || entity.u32(event + 4)? != 0x8080AF8D
                || entity.u64(event + 8)? != definition as u64
            {
                continue;
            }
            let tag = entity.u32(event + 24)?;
            let target = source.tag(tag, Some(0x8080BADB))?;
            let offset = usize::try_from(entity.u64(event + 32)?)?;
            let mut methods = Vec::new();
            if entity.u32(event + 28)? == 0x8080B88F {
                let metadata = source.tag(target.u32(offset + 8)?, Some(0x8080BAF8))?;
                ensure!(
                    metadata.u32(8)? == 0x8080B88F && metadata.u32(12)? == 0x8080B890,
                    "Source getter dispatch differs"
                );
                for method in metadata.array(16, 24, Some(0x8080BAFA))? {
                    methods.push(json!({"interface":format!("{:08X}",metadata.u32(method)?),"method":metadata.u32(method+4)?}));
                }
            }
            outputs.push(json!({"owner":format!("{tag:08X}"),"component":format!("{:08X}",target.u32(target.pointer(24)?-4)?),
                "endpoint":offset,"interface":format!("{:08X}",entity.u32(event+28)?),"methods":methods,
                "translated":false}));
        }
        ensure!(
            result.insert(name, outputs).is_none(),
            "Duplicate source getter channel"
        );
    }
    Ok(result)
}

pub(super) struct Globals {
    pub native: BTreeMap<u32, u8>,
    pub source: Vec<(u32, [u8; 16])>,
    pub wetness: Option<u8>,
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
        let wetness = wetness(source)?;
        let source = globals(source, true)?;
        let native = globals(native, false)?
            .into_iter()
            .enumerate()
            .map(|(i, (name, _))| Ok((name, u8::try_from(i)?)))
            .collect::<Result<_>>()?;
        Ok(Self {
            source,
            native,
            wetness,
        })
    }
}

fn wetness(reader: &mut Reader) -> Result<Option<u8>> {
    let tags: BTreeSet<_> = reader
        .manager
        .lookup
        .named_tags
        .iter()
        .filter(|t| t.name == "render_globals")
        .map(|t| t.hash.0)
        .collect();
    let mut result = None;
    for tag in tags {
        let bootstrap = reader.tag(tag, None)?;
        let globals = reader.tag(bootstrap.u32(0x48)?, None)?;
        let inputs = reader.tag(globals.u32(0x38)?, Some(0x8080a002))?;
        let mut names = BTreeMap::new();
        for row in inputs.array(8, 8, Some(0x8080a004))? {
            let name = inputs.u32(row)?;
            let index = u8::try_from(inputs.u32(row + 4)?).context("Renderer input index")?;
            ensure!(
                names.insert(name, index).is_none(),
                "Duplicate renderer input name"
            );
        }
        // FNV-1 of wetness_amount, corroborated by the material's albedo,
        // roughness and normal-blending consumers. Match the name, not ordinal 13.
        let current = names.get(&0x324ad09d).copied();
        if let Some(previous) = result {
            ensure!(previous == current, "Ambiguous renderer wetness input");
        }
        result = Some(current);
    }
    result.context("Missing source renderer inputs")
}
