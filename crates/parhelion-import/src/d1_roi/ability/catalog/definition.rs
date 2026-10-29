//! Whole ability-table intake retains execution records without claiming native conversion.
use super::*;
use crate::d1_roi::{connections, lifecycle, sequence};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AbilityCatalog {
    pub row: u8,
    pub pattern: u32,
    pub graph: u32,
    pub controller: u32,
    pub controller_class: u32,
    pub controller_definition: u32,
    pub bank: u32,
    pub bank_class: u32,
    pub bank_definition: u32,
    pub source_inputs: Value,
}

pub(super) struct SourceAbility {
    pub catalog: AbilityCatalog,
    pub inputs: BTreeMap<u32, Payload>,
}

// These are source root pairs, not donor identities or translation equivalences.
// The paired bank families share the checked +A0 property-table contract.
type Roots = (u32, u32);
const FAMILIES: &[(Roots, Roots)] = &[
    ((0x80802274, 0x8080228B), (0x8080200F, 0x80802AC8)),
    ((0x80802433, 0x80802627), (0x808021B0, 0x808025C6)),
    ((0x808020CA, 0x80802601), (0x808022AA, 0x80802B60)),
    ((0x8080211E, 0x80802672), (0x808022E9, 0x8080259E)),
    ((0x808025B8, 0x80802A4A), (0x80802343, 0x80802B06)),
    ((0x808029AF, 0x80802C19), (0x8080250F, 0x808024BC)),
    ((0x808023C7, 0x808027B4), (0x808025EB, 0x808025C9)),
    ((0x8080259A, 0x80802AFE), (0x80802682, 0x80802467)),
    ((0x808028FC, 0x80802519), (0x8080275C, 0x80802593)),
    ((0x8080230D, 0x8080212D), (0x808027BE, 0x808029B5)),
    ((0x808022DF, 0x808025F6), (0x80802883, 0x80802200)),
    ((0x808020B1, 0x80802910), (0x808029ED, 0x80802979)),
    ((0x808024DC, 0x808023E7), (0x80802AAC, 0x80802ADC)),
    ((0x80802549, 0x80802504), (0x80802B75, 0x808028B0)),
    ((0x80803063, 0x808030E3), (0x8080316B, 0x80803076)),
    ((0x808030BF, 0x808030F7), (0x808031E7, 0x80803210)),
];

fn pair(owner: &Payload, tag: u32) -> Result<(u32, u32)> {
    let runtime = owner.pointer(16)?;
    let definition = owner.pointer(24)?;
    ensure!(
        runtime >= 4 && definition >= 4,
        "Source ability owner has no root markers"
    );
    let classes = (owner.u32(runtime - 4)?, owner.u32(definition - 4)?);
    ensure!(
        owner.u64(0)? == owner.0.len() as u64
            && owner.u64(runtime)? == u64::from(tag)
            && owner.u64(definition)? == u64::from(tag)
            && owner.u64(runtime + 8)? == definition as u64
            && owner.u64(definition + 8)? == runtime as u64
            && owner.u64(runtime + 16)? == u64::from(classes.1)
            && owner.u64(definition + 16)? == u64::from(classes.0),
        "Source ability owner {tag:08X} is not a reciprocal typed root"
    );
    Ok(classes)
}

fn callbacks(owner: &Payload) -> Result<Vec<Value>> {
    let runtime = owner.pointer(16)?;
    let boundary = owner.pointer(24)?;
    owner
        .array(0x20, 8, Some(0x8080423F))?
        .into_iter()
        .map(|row| {
            let reference = owner.pointer(row)?;
            let callback = reference.checked_sub(8).context("Source callback prefix")?;
            let target = usize::try_from(owner.u64(reference + 8)?)?;
            ensure!(
                callback >= runtime
                    && callback.checked_add(32).is_some_and(|end| end <= boundary)
                    && target >= runtime
                    && target.checked_add(32).is_some_and(|end| end <= boundary)
                    && owner.u64(reference)? == owner.u64(runtime)?,
                "Source callback leaves its mutable owner"
            );
            let definition = usize::try_from(owner.u64(target + 8)?)?;
            ensure!(
                owner.u64(target)? == owner.u64(runtime)?
                    && owner.u64(definition)? == owner.u64(runtime)?
                    && owner.u64(definition + 8)? == target as u64
                    && owner.u64(definition + 16)? == owner.u64(reference + 16)?,
                "Source callback target is not its declared reciprocal object"
            );
            Ok(
                json!({"offset":callback,"reference":reference,"target":target,
            "class":owner.u64(reference+16)?,"implementation":owner.u32(callback)?,
            "method":owner.u32(callback+4)?}),
            )
        })
        .collect()
}

fn sequence_report(owner: &Payload) -> Result<Value> {
    let definition = owner.pointer(24)?;
    let controls = owner.array(definition + 0x160, 8, Some(0x80800629))?;
    let mut events = Vec::new();
    for (field, domain) in [(0x170, "Numeric"), (0x180, "External")] {
        for row in owner.array(definition + field, 8, Some(0x80800629))? {
            let offset = owner.pointer(row)?;
            let class = owner.u32(
                offset
                    .checked_sub(4)
                    .context("Source sequence event marker")?,
            )?;
            events.push(json!({"offset":offset,"class":class,"kind":owner.u32(offset+0x1C)?,"domain":domain}));
        }
    }
    // The source census retains unsupported control/body shapes. A converter's
    // refusal is a boundary in this report, never a reason to omit the sequence.
    let topology = match sequence::Topology::read(owner) {
        Ok(topology) => {
            json!({"validated":true,"inputs":topology.input_count(),"controls":topology.control_count()})
        }
        Err(error) => json!({"validated":false,"error":format!("{error:#}")}),
    };
    Ok(
        json!({"controls":controls.len(),"events":events,"topology":topology,
        "transitive_resources_verified":false}),
    )
}

fn bank(owner: &Payload) -> Result<Value> {
    let runtime = owner.pointer(16)?;
    let definition = owner.pointer(24)?;
    let priorities = owner
        .array(runtime + 0x190, 1, Some(0x80800009))?
        .into_iter()
        .map(|at| owner.u8(at))
        .collect::<Result<Vec<_>>>()?;
    let parameters = owner
        .array(definition + 0xB0, 16, Some(0x80802122))?
        .into_iter()
        .map(|at| Ok(json!({"offset":at,"record":hex::encode(owner.bytes::<16>(at)?)})))
        .collect::<Result<Vec<_>>>()?;
    let properties = owner.array(definition + 0xA0, 40, Some(0x80802A84))?
        .into_iter().map(|at| {
            let body = owner.pointer(at+32)?;
            let class = owner.u32(body.checked_sub(4).context("Source property marker")?)?;
            let gate = if owner.u64(at+24)? == 0 { Value::Null } else {
                let gate = owner.pointer(at+24)?;
                json!({"offset":gate,"class":owner.u32(gate.checked_sub(4).context("Source property gate marker")?)?})
            };
            ensure!((owner.u32(at+12)? as usize) < priorities.len(), "Source property priority slot leaves its bank");
            Ok(json!({"offset":at,"record":hex::encode(owner.bytes::<40>(at)?),
                "key":owner.u32(at)?,"name":owner.u32(at+4)?,"priority":owner.u32(at+8)?,
                "priority_slot":owner.u32(at+12)?,"mode":owner.u32(at+16)?,
                "gate":gate,"body":body,"class":class}))
        }).collect::<Result<Vec<_>>>()?;
    Ok(json!({"properties":properties,"parameters":parameters,"priority_slots":priorities}))
}

pub(super) fn read(
    source: &Source,
    common: &mut BTreeMap<u32, Payload>,
) -> Result<BTreeMap<u8, SourceAbility>> {
    let (table_tag, table) = source.table(0x80803432)?;
    let rows = table.array(8, 40, Some(0x8080343C))?;
    ensure!(
        !rows.is_empty() && rows.len() < 255,
        "Source ability table is empty or too large"
    );
    let assignments = source.runtime_assignments(common)?;
    let mut patterns = BTreeSet::new();
    let mut result = BTreeMap::new();
    for (ordinal, row) in rows.into_iter().enumerate() {
        let pattern = table.u32(row)?;
        ensure!(
            patterns.insert(pattern),
            "Duplicate source ability identity"
        );
        let graph_tag = *assignments
            .get(&pattern)
            .context("Source ability has no runtime assignment")?;
        ensure!(
            source.reference(graph_tag)? == 0x80800734,
            "Source ability graph type differs"
        );
        let graph = source.read(graph_tag)?;
        let connections = connections::Graph::read(&graph)?;
        let mut inputs = BTreeMap::new();
        let mut classes = BTreeMap::new();
        for &tag in &connections.components {
            if inputs.contains_key(&tag) {
                continue;
            }
            ensure!(
                source.reference(tag)? == 0x80800861,
                "Source ability component type differs"
            );
            let owner = source.read(tag)?;
            classes.insert(tag, pair(&owner, tag)?);
            inputs.insert(tag, owner);
        }
        connections.validate_owners(&inputs)?;
        let banks = classes
            .iter()
            .filter_map(|(&tag, pair)| {
                FAMILIES
                    .iter()
                    .find(|(_, bank)| pair == bank)
                    .map(|family| (tag, *family))
            })
            .collect::<Vec<_>>();
        let [(bank_tag, (controller_classes, bank_classes))] = banks.as_slice() else {
            anyhow::bail!("Source ability {pattern:08X} has no unique qualified bank family");
        };
        let controllers = classes
            .iter()
            .filter_map(|(&tag, pair)| (pair == controller_classes).then_some(tag))
            .collect::<Vec<_>>();
        let [controller] = controllers.as_slice() else {
            anyhow::bail!("Source ability {pattern:08X} has no unique paired controller family");
        };
        ensure!(
            connections
                .components
                .iter()
                .filter(|tag| *tag == controller)
                .count()
                == 1
                && connections
                    .components
                    .iter()
                    .filter(|tag| *tag == bank_tag)
                    .count()
                    == 1,
            "Source ability {pattern:08X} repeats its controller or property-bank occurrence"
        );
        let mut components = Vec::new();
        for (occurrence, tag) in connections.components.iter().enumerate() {
            let owner = &inputs[tag];
            let pair = classes[tag];
            let sequence = if pair == (0x8080079A, 0x80800610) {
                sequence_report(owner)?
            } else {
                Value::Null
            };
            components.push(json!({"occurrence":occurrence,"tag":tag,"runtime_class":pair.0,
                "definition_class":pair.1,"bytes":owner.0.len(),"sha256":hex::encode(Sha256::digest(&owner.0)),
                "lifecycle":lifecycle::Lifecycle::read(owner)?,"callbacks":callbacks(owner)?,"sequence":sequence}));
        }
        let bank = bank(&inputs[bank_tag])?;
        inputs.insert(graph_tag, graph);
        let payloads = inputs
            .iter()
            .map(|(&tag, payload)| {
                json!({"tag":tag,"bytes":payload.0.len(),
            "sha256":hex::encode(Sha256::digest(&payload.0))})
            })
            .collect::<Vec<_>>();
        let definition_inputs = common
            .iter()
            .chain(std::iter::once((&table_tag, &table)))
            .map(|(&tag, payload)| {
                json!({"tag":tag,"bytes":payload.0.len(),
                "sha256":hex::encode(Sha256::digest(&payload.0))})
            })
            .collect::<Vec<_>>();
        let catalog = AbilityCatalog {
            row: u8::try_from(ordinal)?,
            pattern,
            graph: graph_tag,
            controller: *controller,
            controller_class: controller_classes.0,
            controller_definition: controller_classes.1,
            bank: *bank_tag,
            bank_class: bank_classes.0,
            bank_definition: bank_classes.1,
            source_inputs: json!({"source":"d1_roi","builds":source.report.builds,"stamp":source.report.stamp,
                "ability_table":table_tag,"definition_record":hex::encode(table.bytes::<40>(row)?),
                "components":components,"connections":connections,"bank":bank,"payloads":payloads,"definition_inputs":definition_inputs,
                "native_emission_verified":false,
                "unresolved":[
                    "Source controller fields, typed callback methods and initialization need a complete native mapping for this family.",
                    "Sequence event bodies and their effect, attachment, projectile, sound, HUD and animation resources require their own complete typed conversion.",
                    "A declared graph and bank census does not establish transitive resource closure, native loading, activation or cleanup."]}),
        };
        result.insert(catalog.row, SourceAbility { catalog, inputs });
    }
    common.insert(table_tag, table);
    Ok(result)
}
