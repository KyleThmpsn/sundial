//! Observable layer dispatch of staged imported actions and unchanged donor actions.
use super::*;

type Edges = Vec<[u16; 2]>;

fn routes(p: &Payload) -> Result<Vec<[Edges; 2]>> {
    p.array(56, 32, Some(0x808034EF))?
        .into_iter()
        .map(|row| {
            let edges = |field| {
                p.array(field, 4, Some(0x808034F1))?
                    .into_iter()
                    .map(|edge| Ok([p.u16(edge)?, p.u16(edge + 2)?]))
                    .collect::<Result<Vec<_>>>()
            };
            Ok([edges(row)?, edges(row + 16)?])
        })
        .collect()
}

fn aliases(p: &Payload) -> Result<Vec<u16>> {
    p.array(40, 2, Some(0x80800006))?
        .into_iter()
        .map(|at| p.u16(at))
        .collect()
}

fn qualify(
    original: &mut Reader,
    stock: &Payload,
    bank: &Payload,
) -> Result<(BTreeSet<u16>, Vec<Value>)> {
    let layer_rows = stock.array(8, 24, Some(0x80803724))?;
    let names = layer_rows
        .iter()
        .map(|&r| stock.u32(r + 16))
        .collect::<Result<Vec<_>>>()?;
    let bank_clips = bank.array(8, 4, Some(0x80808F48))?;
    let mut removable = BTreeSet::new();
    let mut qualified = Vec::new();
    for (index, &layer) in layer_rows.iter().enumerate() {
        // Donor firing/settle offsets are incompatible with independent source actions. Sprint
        // remains a required source of movement, and absolute/replacement exceptions stay.
        if ![0xFC23AA50, 0xD3B8D0AC, 0xE4E61EE3].contains(&names[index]) {
            continue;
        }
        let mut clips = BTreeMap::new();
        for choice in stock.array(layer, 16, Some(0x8080372E))? {
            for option in stock.array(choice, 64, Some(0x80803730))? {
                for weight in stock.array(option, 12, Some(0x80803737))? {
                    let tag = bank.u32(
                        *bank_clips
                            .get(stock.u32(weight)? as usize)
                            .context("Layer clip extent")?,
                    )?;
                    let payload = original.tag(tag, Some(0x80808F49))?;
                    clips.insert(tag, payload.u8(0x181)?);
                }
            }
        }
        ensure!(!clips.is_empty(), "Required coupled layer has no clips");
        let additive = clips.values().all(|mode| *mode == 1);
        if additive {
            removable.insert(index as u16);
        }
        qualified
            .push(json!({"name":format!("{:08X}",names[index]),"additive":additive,"clips":clips}));
    }
    Ok((removable, qualified))
}

pub(super) fn readback(
    reader: &mut Reader,
    original: &mut Reader,
    components: &[Owner],
    donor: &runtime::RuntimeEntitySource,
    bank: &Payload,
    first: usize,
    output: &Path,
) -> Result<Value> {
    let donor_owners = owners(original, donor.entity_tag)?;
    let (_, attachment, base) = one(&donor_owners, 0x80804221)?;
    let entities = parhelion_import::tiger::rig::attachment_rows(
        &attachment,
        base,
        donor.weapon_content_group_hash,
        false,
    )?
    .into_iter()
    .map(|row| attachment.u32(row + 0x78))
    .collect::<Result<BTreeSet<_>>>()?;
    ensure!(
        entities.len() == 1,
        "Stock first-person selection is ambiguous"
    );
    let stock_components = owners(original, *entities.first().unwrap())?;
    let (_, stock_lookup, base) = one(&stock_components, 0x8080344B)?;
    let stock_bank = original.tag(stock_lookup.u32(base + 0x90)?, Some(0x808036F6))?;
    let stock_states = original.tag(stock_lookup.u32(base + 0x9C)?, Some(0x80803465))?;
    let (_, stock_pose, base) = one(&stock_components, 0x80803640)?;
    let stock_tag = stock_pose.u32(base + 0x11C)?;
    let stock = original.tag(stock_tag, Some(0x808034EB))?;
    ensure!(
        reader.tag(stock_tag, Some(0x808034EB))?.0 == stock.0,
        "Stock layer table changed"
    );
    let (_, pose, base) = one(components, 0x80803640)?;
    let tag = pose.u32(base + 0x11C)?;
    ensure!(tag != stock_tag, "Authored layers are not private");
    let table = reader.tag(tag, Some(0x808034EB))?;
    let (_, lookup, base) = one(components, 0x8080344B)?;
    let states = reader.tag(lookup.u32(base + 0x9C)?, Some(0x80803465))?;
    let before = routes(&stock)?;
    let after = routes(&table)?;
    ensure!(
        after.starts_with(&before),
        "An original layer route changed"
    );
    let stock_aliases = aliases(&stock)?;
    let dispatch = aliases(&table)?;
    ensure!(
        stock_aliases.len() == first
            && dispatch.starts_with(&stock_aliases)
            && dispatch.len() == bank.array(0x68, 32, Some(0x80809002))?.len(),
        "Layer dispatch changed stock descriptors or does not cover the bank"
    );
    // All original choices, options, conditions, phase inputs and clip references remain intact.
    for (field, class) in [(8, 0x80803724), (24, 0x80803721)] {
        let old = stock.array(field, 24, Some(class))?;
        let new = table.array(field, 24, Some(class))?;
        ensure!(old.len() == new.len(), "Layer population changed");
        for (&a, &b) in old.iter().zip(&new) {
            ensure!(
                a == b && stock.bytes::<24>(a)? == table.bytes::<24>(b)?,
                "Native layer definition changed"
            );
        }
    }
    let (removable, qualified) = qualify(original, &stock, &stock_bank)?;
    let stock_names = stock_states
        .array(8, 8, Some(0x8080342E))?
        .into_iter()
        .map(|row| stock_states.u32(row))
        .collect::<Result<BTreeSet<_>>>()?;
    let mut selecting = BTreeMap::<usize, BTreeSet<u32>>::new();
    for row in states.array(8, 8, Some(0x8080342E))? {
        let action = states.u32(row)?;
        for descriptor in parhelion_import::tiger::animation::state::descriptors(&states, action)? {
            if descriptor >= first {
                selecting.entry(descriptor).or_default().insert(action);
            }
        }
    }
    let mut observations = Vec::new();
    for (descriptor, actions) in selecting {
        let route = dispatch[descriptor];
        if actions.contains(&0xB9EEE3BE) {
            ensure!(
                route != u16::MAX,
                "Translated sprint lost its locomotion layers"
            );
        }
        if route == u16::MAX {
            continue;
        }
        let actual = after.get(route as usize).context("Authored route extent")?;
        ensure!(
            actual[0].iter().all(|edge| !removable.contains(&edge[1])),
            "Donor firing offset still reaches translated action"
        );
        let mut compatible = false;
        for action in actions {
            if !stock_names.contains(&action) {
                continue;
            }
            for native in
                parhelion_import::tiger::animation::state::descriptors(&stock_states, action)?
            {
                let old = stock_aliases[native];
                if old == u16::MAX {
                    continue;
                }
                let expected = &before[old as usize];
                let retained = expected[0]
                    .iter()
                    .filter(|edge| !removable.contains(&edge[1]))
                    .copied()
                    .collect::<Vec<_>>();
                if actual[0] == retained && actual[1] == expected[1] {
                    compatible = true;
                }
            }
        }
        ensure!(
            compatible,
            "Translated layer route lost sprint, procedural layers or a non-additive exception"
        );
        observations.push(json!({"descriptor":descriptor,"route":route,"first_family":actual[0],"second_family":actual[1]}));
    }
    ensure!(
        !observations.is_empty(),
        "Required translated layer routes are empty"
    );
    fs::write(output.join("layers.bin"), &table.0)?;
    fs::write(output.join("stock-layers.bin"), &stock.0)?;
    fs::write(output.join("stock-bank.bin"), &stock_bank.0)?;
    fs::write(output.join("states.bin"), &states.0)?;
    fs::write(output.join("stock-states.bin"), &stock_states.0)?;
    Ok(
        json!({"file":"layers.bin","tag":tag,"sha256":digest(&table.0),"stock_sha256":digest(&stock.0),"qualified":qualified,"routes":observations,"original_routes":before.len(),"private_routes":after.len()-before.len()}),
    )
}
