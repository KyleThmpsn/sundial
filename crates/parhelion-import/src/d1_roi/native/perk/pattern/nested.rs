//! A native component owns a separate private controller for the attachment's lifetime.
use super::*;
use sundial::package_authoring::sandbox_perk::program::NativeAssetProgram;

/// The shared framework network owner and its interface class. Force Multiplier's nested graph
/// names it after the controller and Grave Robber's before it, with seven exports and references
/// of its own, as every graph that names it does. It holds no per-graph state, and the native
/// nested envelope has no counterpart, so its rows are set aside and the controller's own are
/// checked.
const FRAMEWORK_NETWORK_OWNER: u32 = 0x80AA_CEA6;
const FRAMEWORK_NETWORK_CLASS: u32 = 0x80AA_CEA7;

/// The position of the nested controller among a pattern graph's owners: alone, or beside the
/// framework network in either order. `None` when the graph has another form or its candidate
/// owner is not a controller component (definition class `80802A94`).
pub(super) fn controller_index(source: &Source, graph: &Payload) -> Result<Option<usize>> {
    let owners = graph
        .array(0x20, 12, Some(0x80800715))?
        .into_iter()
        .map(|row| graph.u32(row))
        .collect::<Result<Vec<_>>>()?;
    let index = match owners.as_slice() {
        [_] | [_, FRAMEWORK_NETWORK_OWNER] => 0,
        [FRAMEWORK_NETWORK_OWNER, _] => 1,
        _ => return Ok(None),
    };
    let owner = source.read(owners[index])?;
    Ok((owner.u32(owner.pointer(24)? - 4)? == 0x80802A94).then_some(index))
}

pub(super) fn source_action(source: &Source, graph: &Payload) -> Result<u32> {
    let owners = graph
        .array(0x20, 12, Some(0x80800715))?
        .into_iter()
        .map(|row| graph.u32(row))
        .collect::<Result<Vec<_>>>()?;
    let conditions = graph.array(0x88, 2, Some(0x80800006))?;
    let index = match owners.as_slice() {
        [_] | [_, FRAMEWORK_NETWORK_OWNER] => 0,
        [FRAMEWORK_NETWORK_OWNER, _] => 1,
        _ => anyhow::bail!("Nested controller graph has additional owners"),
    };
    ensure!(
        graph.bytes::<32>(0x30)? == [0; 32]
            && conditions.iter().all(|&at| graph.u16(at).ok() == Some(0))
            && (owners.len() == 2 || conditions.is_empty()),
        "Nested controller graph has additional connections or conditions"
    );
    let owner = owners[index];
    ensure!(
        source.reference(owner)? == 0x80800861,
        "Nested controller owner class differs"
    );
    let p = source.read(owner)?;
    let (live, settings, action) = wrapper(&p, owner)?;
    // Exports and references name their owner. The bindings are an open-addressed table by
    // binding hash: export index at `+4` and probe length at `+6`, one in its home slot. Empty
    // slots hold `FFFFFFFF`. The framework network's own rows are set aside.
    let exports = graph.array(0x68, 40, Some(0x80800370))?;
    let references = graph.array(0x78, 24, Some(0x80800790))?;
    let (mut own_exports, mut own_references) = (Vec::new(), Vec::new());
    for &export in &exports {
        if graph.u32(export + 16)? == owner {
            own_exports.push(export);
        } else {
            ensure!(
                graph.u32(export + 12)? == FRAMEWORK_NETWORK_CLASS
                    && graph.u32(export + 16)? == FRAMEWORK_NETWORK_OWNER,
                "Nested controller exports additional behavior"
            );
        }
    }
    for &reference in &references {
        if graph.u32(reference)? == owner {
            own_references.push(reference);
        } else {
            ensure!(
                graph.u32(reference)? == FRAMEWORK_NETWORK_OWNER,
                "Nested controller references additional behavior"
            );
        }
    }
    let bindings = graph
        .array(0x58, 8, Some(0x80800361))?
        .into_iter()
        .filter(|&at| graph.u32(at).is_ok_and(|hash| hash == 0x0FFD6163))
        .collect::<Vec<_>>();
    ensure!(
        bindings.len() == 1 && own_exports.len() == 1 && own_references.len() == 1,
        "Nested controller exports additional behavior"
    );
    let (binding, export, reference) = (bindings[0], own_exports[0], own_references[0]);
    // The controller's export and reference take its owner's position. Both owner orders carry
    // the same binding table, whose `0FFD6163` entry names export zero: the controller's input
    // when it leads, otherwise the framework network's first export (`+188`).
    ensure!(
        graph.u32(binding)? == 0x0FFD6163
            && graph.u16(binding + 4)? == 0
            && (graph.u16(binding + 6)? == 1 || owners.len() == 2)
            && exports.get(index) == Some(&export)
            && references.get(index) == Some(&reference)
            && graph.bytes::<12>(export)? == [0; 12]
            && graph.u32(export + 12)? == 0x808026AD
            && graph.u32(export + 16)? == owner
            && graph.u32(export + 20)? == 0
            && graph.u64(export + 24)? == (settings + 0x70) as u64
            && graph.u64(export + 32)? == 0x80AAE896
            && graph.u32(reference)? == owner
            && graph.u32(reference + 4)? == 0
            && graph.u64(reference + 8)? == live as u64
            && graph.u64(reference + 16)? == index as u64,
        "Nested controller input export differs"
    );
    ensure!(
        source.reference(action)? == 0x80802711,
        "Nested reference is not a source controller"
    );
    Ok(action)
}

/// A source nested controller owner's checks that need no graph: the `808026AD/80802A94` pair,
/// default host state, the self-relative declaration `80AAE896` and the `0001FFFF` allocation
/// row. Returns the runtime and definition offsets and the child action at definition `+68`.
pub(crate) fn wrapper(p: &Payload, owner: u32) -> Result<(usize, usize, u32)> {
    let (live, settings) = (p.pointer(16)?, p.pointer(24)?);
    ensure!(
        p.u32(live - 4)? == 0x808026AD && p.u32(settings - 4)? == 0x80802A94,
        "Nested controller component pair differs"
    );
    status::pair(p, owner, live, settings, 0x808026AD, 0x80802A94)?;
    for (offset, value) in [
        (0x18, 0),
        (0x20, u32::MAX as u64),
        (0x28, 0),
        (0x30, u32::MAX as u64),
        (0x38, u32::MAX as u64),
        (0x40, 0),
        (0x48, u32::MAX as u64),
        (0x50, 0),
        (0x58, 0),
        (0x60, 0),
        (0x80, 0),
    ] {
        ensure!(
            p.u64(settings + offset)? == value,
            "Nested controller has additional host state"
        );
    }
    ensure!(
        p.pointer(settings + 0x70)? == settings
            && p.u64(settings + 0x78)? == 0x80AAE896
            && p.u32(settings + 0x6C)? == 0
            && p.bytes::<40>(live + 0x18)?
                == [
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0, 255, 255,
                    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
                ],
        "Nested controller has a live recipient or another input driver"
    );
    let resources = p.array(0x30, 16, Some(0x80800930))?;
    ensure!(
        resources.len() == 1
            && p.pointer(resources[0])? == live
            && p.u32(resources[0] + 8)? == 0x808026AD
            && p.u32(resources[0] + 12)? == 0x1FFFF,
        "Nested controller runtime allocation differs"
    );
    Ok((live, settings, p.u32(settings + 0x68)?))
}

/// A native nested controller owner's pair, default host state and declaration `80BFD2C6`, as
/// envelope `80C191F3`'s owner holds them. Returns the runtime and definition offsets and the
/// stock child action at definition `+50`.
pub(crate) fn native_wrapper(p: &Payload, owner: u32) -> Result<(usize, usize, u32)> {
    let (live, settings) = (p.pointer(16)?, p.pointer(24)?);
    ensure!(
        p.u32(live - 4)? == 0x80803E51
            && p.u32(settings - 4)? == 0x80803E52
            && p.u32(live)? == owner
            && p.u32(live + 4)? == 0x80803E52
            && p.u64(live + 8)? == settings as u64
            && p.u32(settings)? == owner
            && p.u32(settings + 4)? == 0x80803E51
            && p.u64(settings + 8)? == live as u64,
        "Native nested controller pair differs"
    );
    for (offset, value) in [
        (0x10, 0),
        (0x18, u64::MAX),
        (0x20, 0),
        (0x28, u64::MAX),
        (0x30, 0),
        (0x38, 0),
        (0x40, 0),
        (0x48, 0),
        (0x68, 0),
    ] {
        ensure!(
            p.u64(settings + offset)? == value,
            "Native nested controller has additional host state"
        );
    }
    ensure!(
        p.pointer(settings + 0x58)? == settings
            && p.u64(settings + 0x60)? == 0x80BFD2C6
            && p.u32(settings + 0x54)? == 0,
        "Native nested controller has another input driver"
    );
    Ok((live, settings, p.u32(settings + 0x50)?))
}

/// Whether stock action `stock` is the native form of source child action `child`, as the
/// evidence that a translated graph may keep the stock controller that owns it.
///
/// An imported attachment group cannot carry a privately compiled child: groups are authored
/// before perk programs compile and their patches name only their own nodes. A nested
/// controller copied from a stock template keeps the stock child instead, when that child is
/// what the importer would make of the source child:
///
/// - one group, whose activation, removal and rearm lists hold the source child's conditions as
///   the condition translator emits them, byte for byte;
/// - one attach effect (source class `80802953` kind 0, native `80803E45` kind 1), retained, with
///   the same target selector, keys and zero floats, which share one layout in both builds;
/// - attached graphs of a Duration host and one modifier bank each, whose bank rows equal the
///   source rows after the importer's conversion (Player Stats scale, reciprocal operation 1).
///
/// The host's Duration identity and fields are recorded, not compared. Nothing in either chain
/// reads them while the attach keys are empty.
pub(crate) fn equivalent(
    source: &Source,
    native: &mut Reader,
    child: u32,
    stock: u32,
) -> Result<Value> {
    let data = source.read(child)?;
    let action = native.tag(stock, Some(0x808040B5))?;
    let decoded = sundial::package_authoring::sandbox_perk::action::decode(&action.0)
        .map_err(anyhow::Error::msg)?;
    let [group] = decoded.groups.as_slice() else {
        anyhow::bail!("Stock nested child {stock:08X} runs additional groups");
    };
    compare_child_conditions(source, native, &data, group, stock)?;
    let (source_graph, stock_graph) = child_graphs(&data, group, child, stock)?;
    let rows = host_bank(source, source_graph)?;
    let stock_rows = native_host_bank(native, stock_graph)?;
    ensure!(
        rows.len() == stock_rows.len()
            && rows.iter().zip(&stock_rows).all(|(row, stock)| {
                row.amount == stock.0
                    && row.operation == stock.1
                    && row.ability == stock.2
                    && row.input == stock.3
                    && row.component == stock.4
            }),
        "Stock nested child {stock:08X} attaches other modifier rows than the source child"
    );
    Ok(
        json!({"source_child":child,"stock_child":stock,"source_graph":source_graph,
        "stock_graph":stock_graph,"rows":rows.len(),
        "unchecked":"the attached hosts' Duration identities and fields"}),
    )
}

/// The modifier rows of a source graph that holds one Duration host and one modifier bank.
fn host_bank(source: &Source, tag: u32) -> Result<Vec<Modifier>> {
    let graph = source.read(tag)?;
    let owners = graph
        .array(32, 12, Some(0x80800715))?
        .into_iter()
        .map(|row| graph.u32(row))
        .collect::<Result<Vec<_>>>()?;
    let mut classes = Vec::new();
    for &owner in &owners {
        let p = source.read(owner)?;
        classes.push((owner, p.u32(p.pointer(24)? - 4)?));
    }
    let (Some((bank, _)), 2, true) = (
        classes.iter().find(|(_, class)| *class == 0x808020DE),
        classes.len(),
        classes.iter().any(|(_, class)| *class == 0x808029C3),
    ) else {
        anyhow::bail!("Source nested child graph {tag:08X} is not a host and a modifier bank");
    };
    modifier_owner(source, &graph, *bank)
}

/// The rows of a native graph that holds one Duration host and one modifier bank, as amount,
/// operation, ability, input and component, with default weight and enable.
fn native_host_bank(native: &mut Reader, tag: u32) -> Result<Vec<HostModifier>> {
    let graph = native.tag(tag, Some(0x80809C0F))?;
    let owners = graph
        .array(16, 12, Some(0x80809C04))?
        .into_iter()
        .map(|row| graph.u32(row))
        .collect::<Result<Vec<_>>>()?;
    let mut bank = None;
    let mut host = false;
    for owner in &owners {
        let p = native.tag(*owner, Some(0x80809C36))?;
        match p.u32(p.pointer(24)? - 4)? {
            0x80803AFE => bank = Some(p),
            0x80803B30 => host = true,
            _ => {}
        }
    }
    let (Some(target), 2, true) = (bank, owners.len(), host) else {
        anyhow::bail!("Stock nested child graph {tag:08X} is not a host and a modifier bank");
    };
    let settings = target.pointer(24)?;
    ensure!(
        target.u64(settings + 0x78)? == 0
            && target.u64(settings + 0x90)? == 0
            && target.u64(settings + 0x98)? == 1,
        "Stock nested child modifier bank default enable differs"
    );
    let mut rows = Vec::new();
    for at in target.array(settings + 88, 88, Some(0x80803B06))? {
        let instance = usize::try_from(target.u64(at + 8)?)?;
        ensure!(
            target.u64(at + 32)? == 0
                && target.u64(at + 64)? == 1
                && target.f32(instance + 64)? == 1.0
                && target.u8(instance + 68)? == 1,
            "Stock nested child modifier row has another weight or enable"
        );
        rows.push((
            target.f32(at + 40)?,
            target.u8(at + 44)?,
            target.u16(at + 72)?,
            target.u16(at + 74)?,
            target.u8(at + 76)?,
        ));
    }
    Ok(rows)
}

pub(super) fn read(
    source: &Source,
    native: &mut Reader,
    graph: &Payload,
    folder: Option<&Path>,
    ancestors: &mut Vec<u32>,
) -> Result<(u32, NativeAssetProgram, usize)> {
    let action = source_action(source, graph)?;
    // Original source methods 111B2E0/111B430 and native EFA440/EFA010 retain
    // the recipient and register/remove the referenced controller. Source definition
    // +68 becomes native +50. The child keeps its own events and retained effects.
    let (program, omitted_feedback) = super::super::program_inner(
        source,
        action,
        "Nested Source Effect",
        Some(native),
        // The child carries its own imported attachment groups, which the package build
        // loads and binds through the owned programs as it does the parent's.
        folder,
        ancestors,
    )?;
    let tag = 0x80C191F5;
    let graph = native.tag(tag, Some(0x80809C0F))?;
    let owners = graph.array(0x10, 12, Some(0x80809C04))?;
    ensure!(
        owners.len() == 1
            && graph.bytes::<32>(0x20)? == [0; 32]
            && graph.u64(0x78)? == 0
            && graph.u64(0x48)? == 1
            && graph.u64(0x58)? == 1
            && graph.u64(0x68)? == 1,
        "Native nested controller envelope has additional topology"
    );
    let owner = graph.u32(owners[0])?;
    let p = native.tag(owner, Some(0x80809C36))?;
    let (live, settings, _) = native_wrapper(&p, owner)?;
    let bindings = weapon_component_bindings(&graph.0, 0x95A60F29).map_err(anyhow::Error::msg)?;
    ensure!(
        bindings.len() == 1
            && bindings[0].owner_tag == owner
            && bindings[0].concrete_class == 0x80803E51
            && bindings[0].resource_offset == live as u64,
        "Native nested controller binding differs"
    );
    let expected_action = p.u32(settings + 0x50)?;
    native.tag(expected_action, Some(0x808040B5))?;
    Ok((
        tag,
        NativeAssetProgram {
            binding_hash: bindings[0].binding_hash,
            resource_index: u16::try_from(bindings[0].resource_index)?,
            offset: u32::try_from(
                (settings + 0x50)
                    .checked_sub(live)
                    .context("Nested controller definition precedes its runtime resource")?,
            )?,
            expected_action,
            program: Box::new(program),
        },
        omitted_feedback,
    ))
}

type HostModifier = (f32, u8, u16, u16, u8);

fn compare_child_conditions(
    source: &Source,
    native: &mut Reader,
    data: &Payload,
    group: &sundial::package_authoring::sandbox_perk::action::DecodedGroup,
    stock: u32,
) -> Result<()> {
    for (descriptor, list, label) in [
        (0x20, &group.activation, "activation"),
        (0x48, &group.removal, "removal"),
        (0x58, &group.rearm, "rearm"),
    ] {
        let mut translated = super::super::conditions(source, data, descriptor, Some(native))?
            .into_iter()
            .map(|node| (node.kind, node.bytes))
            .collect::<Vec<_>>();
        let mut stocked = list
            .iter()
            .map(|condition| (condition.kind, condition.native.clone()))
            .collect::<Vec<_>>();
        translated.sort();
        stocked.sort();
        ensure!(
            translated == stocked,
            "Stock nested child {stock:08X} {label} conditions differ from the source child's"
        );
    }
    Ok(())
}

fn child_graphs(
    data: &Payload,
    group: &sundial::package_authoring::sandbox_perk::action::DecodedGroup,
    child: u32,
    stock: u32,
) -> Result<(u32, u32)> {
    let effects = data.array(0x38, 8, None)?;
    let [row] = effects.as_slice() else {
        anyhow::bail!("Source nested child {child:08X} has another effect count");
    };
    let at = data.pointer(*row)?;
    let [effect] = group.effects.as_slice() else {
        anyhow::bail!("Stock nested child {stock:08X} has another effect count");
    };
    ensure!(
        data.u32(at - 4)? == 0x80802953
            && data.u8(at)? == 0
            && data.u8(at + 1)? == 1
            && effect.class == 0x80803E45
            && effect.kind == 1
            && effect.retained
            && effect.native.get(2) == Some(&data.u8(at + 2)?)
            && effect.native.get(0x18..0x20) == Some(&data.bytes::<8>(at + 24)?[..])
            && effect.native.get(0x20..0x30) == Some(&[0; 16][..])
            && data.bytes::<16>(at + 32)? == [0; 16],
        "Stock nested child {stock:08X} attaches differently from the source child"
    );
    let (source_graph, stock_graph) = (
        data.u32(at + 16)?,
        effect
            .referenced_tag
            .context("Stock nested child attaches no graph")?,
    );
    Ok((source_graph, stock_graph))
}
