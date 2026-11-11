//! Follow the emitted query action and compare its allocation shape to an independent native owner.
use super::*;

/// Read the source selection contract through the staged controller's two definition views.
/// Compiled masks are reconstructed from stock dictionary bytes, independently of the writer.
pub(super) fn selection(
    owner: &Payload,
    tag: u32,
    source: &Payload,
    stock: &PackageManager,
) -> serde_json::Value {
    let from = source.pointer(24).unwrap() + 0x740;
    let source_shape = source.pointer(from + 0x20).unwrap();
    let source_bound = source.pointer(source_shape + 0xB0).unwrap();
    assert_eq!(source.u32(source_shape - 4).unwrap(), 0x8080250A);
    assert_eq!(source.u32(source_bound - 4).unwrap(), 0x8080246A);
    assert_eq!(source.u8(from + 0xC0).unwrap(), 0xA1);
    assert_eq!(source.u32(from + 0x398).unwrap(), 1);
    assert_eq!(source.u32(from + 0x39C).unwrap(), 0x808020AE);
    let marker = Payload(stock.read_tag(TagHash(0x80C2262A)).unwrap());
    assert_eq!(marker.u32(12).unwrap(), 0x80803B70);
    let registry = Payload(stock.read_tag(TagHash(0x80C70CA1)).unwrap());
    let masks = read_masks(&registry);
    let source_lists: [Vec<u32>; 4] = std::array::from_fn(|operation| {
        source
            .array(from + 0xC8 + operation * 16, 24, Some(0x80800365))
            .unwrap()
            .into_iter()
            .map(|row| source.u32(row).unwrap())
            .collect()
    });
    let expression = from + 0x58;
    let bytecode = source
        .array_range(expression + 0x18, 1, Some(0x80800009))
        .unwrap();
    let constants = source
        .array(expression + 0x28, 16, Some(0x80800090))
        .unwrap();
    let live = owner.pointer(16).unwrap();
    let definition = usize::try_from(owner.u64(live + 8).unwrap()).unwrap();
    let sealed = owner.pointer(24).unwrap();
    let selection = SourceSelection {
        payload: source,
        from,
        shape: source_shape,
        bound: source_bound,
        lists: source_lists,
        expression,
        bytecode,
        constants,
    };
    let views = [definition, sealed].map(|root| {
        read_selection_view(owner, tag, (definition, root), &selection, &marker, &masks)
    });
    json!({"views":views,"native_marker_metadata":0x80C2262Au32,
        "marker_metadata_sha256":sha(&marker.0),"label_registry_sha256":sha(&registry.0)})
}

#[allow(clippy::cognitive_complexity)]
pub(super) fn readback(owner: &Payload, tag: u32, witness: &Payload) -> (usize, serde_json::Value) {
    let live = owner.pointer(16).unwrap();
    let definition = usize::try_from(owner.u64(live + 8).unwrap()).unwrap();
    let boundary = owner.pointer(24).unwrap();
    let actions = owner.array(live + 0xD10, 48, Some(0x80804244)).unwrap();
    let definitions = owner
        .array(definition + 0x950, 24, Some(0x80804245))
        .unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(definitions.len(), actions.len());
    let wrapper = actions[0];
    let settings = definitions[0];
    let action = owner.pointer(wrapper + 32).unwrap();
    let action_definition = owner.pointer(settings + 16).unwrap();
    assert_eq!(owner.u32(action - 4).unwrap(), 0x80803C7D);
    assert_eq!(owner.u32(action_definition - 4).unwrap(), 0x80803C7E);
    let recipient = action + 0x20;
    let recipient_definition = action_definition + 0x18;
    for (at, twin, runtime_class, definition_class, parent, size) in [
        (wrapper, settings, 0x80804244, 0x80804245, live + 0xCF0, 48),
        (
            action,
            action_definition,
            0x80803C7D,
            0x80803C7E,
            wrapper,
            0xF0,
        ),
        (
            recipient,
            recipient_definition,
            0x80803B36,
            0x80803B28,
            action,
            0xD0,
        ),
        (
            recipient + 0x50,
            recipient_definition + 0x78,
            0x80808518,
            0x80808519,
            recipient,
            0x30,
        ),
        (
            recipient + 0xA0,
            recipient_definition + 0x18,
            0x808089F5,
            0x808089F6,
            recipient,
            0x30,
        ),
    ] {
        assert!(
            at >= live && at + size <= boundary,
            "runtime must survive instance copy"
        );
        assert_eq!(owner.u32(at).unwrap(), tag);
        assert_eq!(owner.u32(at + 4).unwrap(), definition_class);
        assert_eq!(owner.u64(at + 8).unwrap(), twin as u64);
        assert_eq!(owner.u32(twin).unwrap(), tag);
        assert_eq!(owner.u32(twin + 4).unwrap(), runtime_class);
        assert_eq!(owner.u64(twin + 8).unwrap(), at as u64);
        assert_eq!(owner.pointer(at + 16).unwrap(), parent);
    }
    // This candidate supplies native solar presentation without borrowing a donor's damage.
    assert_eq!(owner.u32(recipient + 0x40).unwrap(), u32::MAX);
    assert_eq!(owner.u32(recipient_definition + 0xA0).unwrap(), u32::MAX);
    let rows = owner.array(recipient + 0x88, 20, Some(0x80803B2C)).unwrap();
    let native_rows = witness.array(0x330 + 0x88, 20, Some(0x80803B2C)).unwrap();
    assert!(!native_rows.is_empty());
    assert_eq!(rows.len(), native_rows.len());
    assert!(rows[0] >= live && rows.last().unwrap() + 20 <= boundary);
    assert_eq!(owner.u32(recipient + 0x98).unwrap(), 0);
    let native_init = witness.array(48, 16, Some(0x808091A4)).unwrap();
    assert!(native_init.iter().any(|&at| {
        witness.pointer(at).unwrap() == 0x330
            && witness.u32(at + 8).unwrap() == 0x80803B36
            && witness.u16(at + 12).unwrap() == 0
            && witness.u16(at + 14).unwrap() == 1
    }));
    (
        recipient,
        json!({"wrapper":wrapper,"action":action,"recipient":recipient,
        "definition":action_definition,"tracking_capacity":rows.len(),"mutable_boundary":boundary,
        "witness":0x80BC2BF4u32,"witness_sha256":sha(&witness.0),"damage_resource":null}),
    )
}

fn children(payload: &Payload, field: usize) -> Vec<serde_json::Value> {
    payload
        .array(field, 40, Some(0x80808852))
        .unwrap()
        .into_iter()
        .map(|at| {
            assert_eq!(
                payload.u64(at + 8).unwrap(),
                0,
                "unexpected indirect action allocation"
            );
            json!({"name":payload.u32(at).unwrap(),"class":payload.u32(at+16).unwrap(),
            "count":payload.u32(at+20).unwrap(),"children":children(payload,at+24)})
        })
        .collect()
}

pub(super) fn allocation(payload: &Payload, witness: &Payload) {
    assert_ne!(
        payload.u64(16).unwrap(),
        0,
        "common base allocation is required"
    );
    let actual = children(payload, 32);
    let mut native = children(witness, 32);
    assert_eq!(native.len(), 1);
    assert_eq!(native[0]["name"], 0x562C8525u32);
    // The same embedded helper occupies a differently named member in the retained controller.
    native[0]["name"] = json!(0xC3BA2CF7u32);
    assert_eq!(actual, native);
    assert_eq!(payload.u64(0).unwrap(), payload.0.len() as u64);
}

fn read_masks(registry: &Payload) -> BTreeMap<u32, [u8; 40]> {
    let mut masks = BTreeMap::new();
    for (index, row) in registry
        .array(8, 4, Some(0x80800070))
        .unwrap()
        .into_iter()
        .enumerate()
    {
        let mut bits = [0u8; 40];
        bits[index / 8] = 1 << (index % 8);
        assert!(masks.insert(registry.u32(row).unwrap(), bits).is_none());
    }
    for row in registry.array(24, 44, Some(0x808094BE)).unwrap() {
        assert!(
            masks
                .insert(
                    registry.u32(row).unwrap(),
                    registry.bytes::<40>(row + 4).unwrap()
                )
                .is_none()
        );
    }
    masks
}

struct SourceSelection<'a> {
    payload: &'a Payload,
    from: usize,
    shape: usize,
    bound: usize,
    lists: [Vec<u32>; 4],
    expression: usize,
    bytecode: std::ops::Range<usize>,
    constants: Vec<usize>,
}

fn read_selection_view(
    owner: &Payload,
    tag: u32,
    (definition, root): (usize, usize),
    selection: &SourceSelection<'_>,
    marker: &Payload,
    masks: &BTreeMap<u32, [u8; 40]>,
) -> serde_json::Value {
    let source = selection.payload;
    let from = selection.from;
    let source_lists = &selection.lists;
    let bytecode = &selection.bytecode;
    let helper = root + 0x7F0;
    let (shape, bound) = check_query_shape(owner, helper, tag, root - definition, selection);
    assert_eq!(
        owner.f32(helper + 0xE0).unwrap(),
        source.f32(from + 0x28).unwrap()
    );
    assert_eq!(
        owner.u32(helper + 0x40).unwrap(),
        source.u32(from + 0x18).unwrap()
    );
    assert_eq!(owner.u32(helper + 0x170).unwrap(), 1);
    assert_eq!(owner.u32(helper + 0x174).unwrap(), marker.u32(12).unwrap());
    check_query_terms(owner, helper, source_lists, masks);
    check_scale_program(owner, helper + 0xF8, selection);
    json!({"definition":root,"geometry":shape,"radius":owner.f32(shape+4).unwrap(),
            "bound":bound,"bound_scale":owner.f32(bound+0x60).unwrap(),
            "vertical_offset":owner.f32(helper+0xE0).unwrap(),"labels":source_lists,
            "scale_program":hex::encode(&source.0[bytecode.clone()]),
            "required_component":owner.u32(helper+0x174).unwrap()})
}

fn check_query_shape(
    owner: &Payload,
    helper: usize,
    tag: u32,
    relocated: usize,
    selection: &SourceSelection<'_>,
) -> (usize, usize) {
    let shape = owner.pointer(helper + 0x20).unwrap();
    let bound = owner.pointer(shape + 8).unwrap();
    assert_eq!(owner.u32(shape - 4).unwrap(), 0x808092B1);
    assert_eq!(owner.u32(shape).unwrap(), 2);
    assert_eq!(
        owner.f32(shape + 4).unwrap(),
        selection.payload.f32(selection.shape + 0xA8).unwrap()
    );
    assert_eq!(owner.u32(bound - 4).unwrap(), 0x808092C3);
    assert_eq!(
        owner.f32(bound + 0x60).unwrap(),
        selection.payload.f32(selection.bound + 0x80).unwrap()
    );
    for method in 0..4usize {
        let row = bound + method * 24;
        assert_eq!(owner.u32(row).unwrap(), 0x808092C3);
        assert_eq!(owner.u32(row + 4).unwrap() as usize, method);
        assert_eq!(owner.u32(row + 8).unwrap(), tag);
        assert_eq!(owner.u32(row + 12).unwrap(), 0x808092C3);
        assert_eq!(owner.u64(row + 16).unwrap() as usize, bound - relocated);
    }
    (shape, bound)
}

fn check_query_terms(
    owner: &Payload,
    helper: usize,
    source_lists: &[Vec<u32>; 4],
    masks: &BTreeMap<u32, [u8; 40]>,
) {
    assert_eq!(
        owner.u8(helper + 0x28).unwrap(),
        0,
        "all predicates must hold"
    );
    let terms = owner.array(helper + 0x30, 16, Some(0x80809316)).unwrap();
    assert_eq!(terms.len(), 3);
    let mut terms_by_class = BTreeMap::new();
    for row in terms {
        let term = owner.pointer(row + 8).unwrap();
        assert!(
            terms_by_class
                .insert(owner.u32(term - 4).unwrap(), (row, term))
                .is_none()
        );
    }
    let &(label_row, label) = terms_by_class.get(&0x80804D73).unwrap();
    assert_eq!(owner.u8(label_row).unwrap(), 0);
    assert_eq!(owner.u32(label + 0x48).unwrap(), 0x80C70CA1);
    check_label_mask(owner, label, source_lists, masks);
    for class in [0x80804D76, 0x80804D7B] {
        assert_eq!(owner.u8(terms_by_class[&class].0).unwrap(), 1);
    }
    assert_eq!(owner.u8(terms_by_class[&0x80804D76].1).unwrap(), 1);
}

fn check_label_mask(
    owner: &Payload,
    label: usize,
    source_lists: &[Vec<u32>; 4],
    masks: &BTreeMap<u32, [u8; 40]>,
) {
    let mut compiled = [[0u8; 40]; 4];
    for (operation, names) in source_lists.iter().enumerate() {
        let rows = owner
            .array(label + operation * 16, 24, Some(0x808094B3))
            .unwrap();
        let actual = rows
            .iter()
            .map(|&row| owner.u32(row).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(&actual, names);
        for name in names {
            for (to, value) in compiled[operation].iter_mut().zip(masks[name]) {
                *to |= value;
            }
        }
    }
    let body = owner.pointer(label + 0x58).unwrap();
    let operations: &[usize] = match owner.u8(label + 0x50).unwrap() {
        0 => {
            assert_eq!(owner.u32(body - 4).unwrap(), 0x808093F6);
            assert!(source_lists[1].is_empty() && source_lists[3].is_empty());
            &[0, 2]
        }
        1 => {
            assert_eq!(owner.u32(body - 4).unwrap(), 0x808093F5);
            &[0, 1, 2, 3]
        }
        mode => panic!("unsupported compiled label mode {mode}"),
    };
    for (index, &operation) in operations.iter().enumerate() {
        assert_eq!(
            owner.bytes::<40>(body + index * 40).unwrap(),
            compiled[operation]
        );
        assert_eq!(
            owner.u8(body + operations.len() * 40 + index).unwrap(),
            u8::from(source_lists[operation].is_empty())
        );
    }
}

fn check_scale_program(owner: &Payload, scale: usize, selection: &SourceSelection<'_>) {
    let source = selection.payload;
    let expression = selection.expression;
    let (bytecode, constants) = (&selection.bytecode, &selection.constants);
    let code = owner
        .array_range(scale + 0x10, 1, Some(0x80800009))
        .unwrap();
    assert_eq!(&owner.0[code], &source.0[bytecode.clone()]);
    let native_constants = owner.array(scale + 0x20, 16, Some(0x80800090)).unwrap();
    assert_eq!(native_constants.len(), constants.len());
    for (to, from) in native_constants.into_iter().zip(constants) {
        assert_eq!(
            owner.bytes::<16>(to).unwrap(),
            source.bytes::<16>(*from).unwrap()
        );
    }
    assert_eq!(
        owner.u64(scale + 0x30).unwrap(),
        source.u64(expression + 0x38).unwrap()
    );
    assert_eq!(
        owner.u64(scale + 0x38).unwrap(),
        u64::from(source.u32(expression + 0x40).unwrap())
    );
    assert!(
        owner
            .array(scale + 0x40, 40, Some(0x80809789))
            .unwrap()
            .is_empty()
    );
}
