//! Sequence scalar inputs. A native kind-zero control condition reads its input through the
//! input's bound provider with no check, so an unbound input faults the client when the
//! condition runs (`exe+0x587D5D` in `86657.20.08.23.1800.d2_rc`). Every input of every private
//! sequence must therefore read a channel bank declaration of its own name, as the source Super
//! graph connects the same input to its own bank. A name the stock bank lacks must be declared
//! the way the source bank declares it, and stock declarations keep their ordinals.
use super::*;

/// One source sequence input connected to the Super graph's own bank: the input name, which
/// equals the bank declaration it reads, and that declaration's inherited lookup row, if any.
pub(super) struct SourceInput {
    pub name: u32,
    pub inherited: Option<(u32, u32)>,
}

fn names(bank: &Payload, class: u32) -> Vec<u32> {
    let schema = bank.pointer(24).unwrap();
    bank.array(schema + 0xD8, 112, Some(class))
        .unwrap()
        .into_iter()
        .map(|row| bank.u32(row).unwrap())
        .collect()
}

fn inherited(bank: &Payload) -> BTreeMap<usize, (u32, u32)> {
    let schema = bank.pointer(24).unwrap();
    bank.array(schema + 0x108, 12, None)
        .unwrap()
        .into_iter()
        .map(|at| {
            (
                usize::try_from(bank.u32(at + 8).unwrap()).unwrap(),
                (bank.u32(at).unwrap(), bank.u32(at + 4).unwrap()),
            )
        })
        .collect()
}

fn instance_class(owner: &Payload) -> u32 {
    owner.u32(owner.pointer(16).unwrap() - 4).unwrap()
}

/// Every source sequence input row connected to a source channel bank in the Super graph.
pub(super) fn source(source: &Source, graph: &Payload) -> Vec<SourceInput> {
    let mut result = Vec::new();
    for row in graph.array(0x30, 88, Some(0x8080054E)).unwrap() {
        if graph.u32(row + 24).unwrap() != 0x80800927 || graph.u32(row + 64).unwrap() != 0x808005AB
        {
            continue;
        }
        let consumer = source.read(graph.u32(row + 8).unwrap()).unwrap();
        if instance_class(&consumer) != 0x8080079A {
            continue;
        }
        let bank = source.read(graph.u32(row + 48).unwrap()).unwrap();
        let channel = usize::try_from(graph.u64(row + 80).unwrap()).unwrap();
        let declared = names(&bank, 0x80800883);
        let offset = usize::try_from(graph.u64(row + 16).unwrap()).unwrap();
        let name = consumer.u32(offset + 0x28).unwrap();
        assert_eq!(
            declared[channel], name,
            "a source sequence input reads another channel name"
        );
        result.push(SourceInput {
            name,
            inherited: inherited(&bank).get(&channel).copied(),
        });
    }
    assert!(!result.is_empty(), "required source input corpus is empty");
    result
}

/// Read back every private sequence input's binding. Returns the evidence and the private
/// channel banks whose declarations grew, which therefore need private allocations.
pub(super) fn readback(
    graph: &Payload,
    owners: &BTreeMap<u32, Payload>,
    originals: &BTreeMap<u32, Vec<u8>>,
    expected: &[SourceInput],
) -> (serde_json::Value, BTreeSet<u32>) {
    let links = graph.array(0x20, 72, Some(0x80809BC9)).unwrap();
    let stock_banks = originals
        .values()
        .map(|bytes| Payload(bytes.clone()))
        .filter(|owner| {
            owner.pointer(16).is_ok_and(|i| i >= 4) && instance_class(owner) == 0x8080979F
        })
        .collect::<Vec<_>>();
    let [stock_bank] = stock_banks.as_slice() else {
        panic!("the donor graph needs one channel bank");
    };
    let stock_names = names(stock_bank, 0x808097A1);
    let mut bound = Vec::new();
    let mut banks = BTreeSet::new();
    let mut evidence = Vec::new();
    for (&tag, owner) in owners {
        if instance_class(owner) != 0x808084D7 {
            continue;
        }
        let (_, definition) = pair(owner, tag, (0x808084D7, 0x808084E9));
        // Main inputs, then auxiliary inputs owned by the embedded input owner.
        let mut inputs = owner
            .array(definition + 0x188, 40, Some(0x80809789))
            .unwrap();
        inputs.extend(
            owner
                .array(definition + 0x2B8, 40, Some(0x80809789))
                .unwrap(),
        );
        for input in inputs {
            let name = owner.u32(input + 0x20).unwrap();
            let rows = links
                .iter()
                .copied()
                .filter(|&at| {
                    graph.u32(at + 8).unwrap() == tag
                        && graph.u32(at + 12).unwrap() == 0x80809789
                        && graph.u64(at + 16).unwrap() == input as u64
                })
                .collect::<Vec<_>>();
            assert_eq!(
                rows.len(),
                1,
                "sequence {tag:08X} input {name:08X} is unbound"
            );
            let provider = graph.u32(rows[0] + 40).unwrap();
            assert_eq!(graph.u32(rows[0] + 44).unwrap(), 0x808097C2);
            let bank = &owners[&provider];
            assert_eq!(instance_class(bank), 0x8080979F);
            let channel = usize::try_from(graph.u32(rows[0] + 64).unwrap()).unwrap();
            assert_eq!(
                names(bank, 0x808097A1)[channel],
                name,
                "sequence {tag:08X} input {name:08X} reads another channel"
            );
            banks.insert(provider);
            bound.push(name);
            evidence.push(json!({"sequence":tag,"input":name,"bank":provider,"channel":channel}));
        }
    }
    let mut wanted = expected.iter().map(|input| input.name).collect::<Vec<_>>();
    wanted.sort_unstable();
    bound.sort_unstable();
    assert_eq!(
        bound, wanted,
        "bound sequence inputs differ from the source"
    );
    let extended = check_bank_declarations(owners, &banks, &stock_names, expected);
    (
        json!({"bindings":evidence,"stock_declarations":stock_names.len(),
            "extended_banks":extended}),
        extended,
    )
}

fn check_bank_declarations(
    owners: &BTreeMap<u32, Payload>,
    banks: &BTreeSet<u32>,
    stock_names: &[u32],
    expected: &[SourceInput],
) -> BTreeSet<u32> {
    let mut extended = BTreeSet::new();
    for &tag in banks {
        let bank = &owners[&tag];
        let declared = names(bank, 0x808097A1);
        assert_eq!(
            &declared[..stock_names.len()],
            stock_names,
            "stock channel ordinals moved"
        );
        let lookups = inherited(bank);
        for input in expected {
            if stock_names.contains(&input.name) {
                continue;
            }
            let ordinal = declared
                .iter()
                .position(|&name| name == input.name)
                .expect("a source channel is undeclared");
            assert_eq!(
                lookups.get(&ordinal).copied(),
                input.inherited,
                "channel {:08X} is declared unlike the source",
                input.name
            );
            assert!(
                input.inherited.is_some(),
                "an added channel needs its source producer"
            );
        }
        if declared.len() > stock_names.len() {
            extended.insert(tag);
        }
    }
    extended
}
