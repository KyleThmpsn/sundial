//! Independent animation-event readback. Body events on the cached-provider route must name a
//! private action that the staged human action table resolves to private clips carrying the
//! source clips' animated samples with the source weights. The player-route event keeps its
//! source action, which the held weapon's first-person table resolves. Stock actions,
//! descriptors, clip rows, lookup and pose stay as they were. A native revision clip, a stock
//! action name, a dropped weight or a moved stock ordinal is a failure.
use super::*;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Native human lookup, pose, action table and bank.
const LOOKUP: u32 = 0x80EF62E5;
const POSE: u32 = 0x8161D5F7;
const STATES: u32 = 0x80BB2C46;
const BANK: u32 = 0x80C0CA92;
/// Source human lookup, whose action table the source body events name.
const SOURCE_LOOKUP: u32 = 0x80A7E159;

const MARKERS: [u32; 2] = [0x80800184, 0x80809FBD];
/// Animated transform codec classes, source and native, by encoding.
const CODECS: [(u32, u32); 3] = [
    (0x808004A9, 0x80808F6F),
    (0x808005E6, 0x80808F71),
    (0x80800432, 0x80808F72),
];
/// Channel classes whose headers can end the animated block: four-lane and scalar.
const CHANNELS: [(u32, u32); 2] = [(0x8080055A, 0x80808F77), (0x80800770, 0x80808F7C)];

/// Frame count and the animated codec block with array markers and codec class normalized.
/// Converted clips move this block unchanged, so its digest identifies the source motion.
fn motion(p: &Payload, source: bool) -> Value {
    let (frames, size) = if source {
        (0x128, 0x11C)
    } else {
        (0x13C, 0x12C)
    };
    let start = p.pointer(0x18).unwrap();
    let length = usize::try_from(p.u32(size).unwrap()).unwrap();
    let class = p.u32(start - 4).unwrap();
    let codec = CODECS
        .iter()
        .position(|&(from, to)| class == if source { from } else { to })
        .expect("animated codec class");
    // Array markers and record class headers are the only words each build labels differently.
    let mut block = p.0[start..start + length].to_vec();
    for at in (0..block.len().saturating_sub(3)).step_by(4) {
        let word = u32::from_le_bytes(block[at..at + 4].try_into().unwrap());
        let label = if MARKERS.contains(&word) {
            Some(0)
        } else {
            CODECS
                .iter()
                .chain(&CHANNELS)
                .find(|&&(from, to)| word == if source { from } else { to })
                .map(|&(_, to)| to)
        };
        if let Some(label) = label {
            block[at..at + 4].copy_from_slice(&label.to_le_bytes());
        }
    }
    json!({"frames":p.u16(frames).unwrap(),"codec":codec,"bytes":length,
        "sha256":hex::encode(Sha256::digest(&block))})
}

/// Source body actions with each weighted clip's motion, by action name.
pub(super) fn source_actions(
    source: &parhelion_import::d1_roi::Source,
    names: &BTreeSet<u32>,
) -> BTreeMap<u32, Value> {
    let plan = parhelion_import::d1_roi::ability::animation::read(source, SOURCE_LOOKUP, names)
        .expect("source human actions");
    plan.actions
        .iter()
        .map(|action| {
            let choices = action
                .choices
                .iter()
                .map(|choice| {
                    json!({"weight":choice.weight,
                        "motion":motion(&source.read(choice.clip).unwrap(), true)})
                })
                .collect::<Vec<_>>();
            (action.name, json!(choices))
        })
        .collect()
}

/// Every source kind-13 event of one source sequence: action, route, lane and state bytes.
pub(super) fn source_events(owner: &Payload) -> Vec<(u32, [u8; 3])> {
    let d = owner.pointer(24).unwrap();
    let mut events = Vec::new();
    for field in [0x170, 0x180] {
        for row in owner.array(d + field, 8, Some(0x80800629)).unwrap() {
            let e = owner.pointer(row).unwrap();
            if owner.u32(e - 4).unwrap() == 0x808025C1 {
                assert_eq!(owner.u32(e + 0x1C).unwrap(), 13);
                events.push((
                    owner.u32(e + 0x20).unwrap(),
                    owner.bytes::<3>(e + 0x24).unwrap(),
                ));
            }
        }
    }
    events
}

fn names(states: &Payload) -> BTreeMap<u32, u32> {
    states
        .array(8, 8, Some(0x8080342E))
        .unwrap()
        .into_iter()
        .map(|at| (states.u32(at).unwrap(), states.u32(at + 4).unwrap()))
        .collect()
}

/// Staged human tables keep every stock action, descriptor and clip row, and the lookup and
/// pose owners stay stock.
pub(super) fn stock_routes(staged: &PackageManager, stock: &PackageManager) -> Value {
    for tag in [LOOKUP, POSE] {
        assert_eq!(
            staged.read_tag(TagHash(tag)).unwrap(),
            stock.read_tag(TagHash(tag)).unwrap(),
            "human animation owner changed"
        );
    }
    let read = |manager: &PackageManager, tag| Payload(manager.read_tag(TagHash(tag)).unwrap());
    let (old_states, new_states) = (read(stock, STATES), read(staged, STATES));
    let (old, new) = (names(&old_states), names(&new_states));
    for (name, ordinal) in &old {
        assert_eq!(new.get(name), Some(ordinal), "stock action route moved");
    }
    let (old_bank, new_bank) = (read(stock, BANK), read(staged, BANK));
    let mut counts = Vec::new();
    for (field, stride, class) in [(0x68, 32, 0x80809002), (8, 4, 0x80808F48)] {
        let before = old_bank.array(field, stride, Some(class)).unwrap();
        let after = new_bank.array(field, stride, Some(class)).unwrap();
        assert!(after.len() >= before.len());
        for (a, b) in before.iter().zip(&after) {
            assert_eq!(
                old_bank.0[*a..a + stride],
                new_bank.0[*b..b + stride],
                "stock bank row changed"
            );
        }
        counts.push(json!({"field":field,"stock":before.len(),"staged":after.len()}));
    }
    json!({"stock_actions":old.len(),"staged_actions":new.len(),"bank":counts})
}

/// One staged kind-18 event. Cached-provider events resolve through the staged human tables.
pub(super) fn native(
    owner: &Payload,
    e: usize,
    staged: &PackageManager,
    stock: &PackageManager,
) -> Value {
    assert_eq!(owner.u32(e + 0x30).unwrap(), 18);
    assert_eq!(
        owner.u8(e + 0x3E).unwrap(),
        0,
        "added animation flag is set"
    );
    assert!(
        owner.0[e + 0x40..e + 0x88].iter().all(|b| *b == 0),
        "duration override is present"
    );
    let instance = usize::try_from(owner.u64(e + 8).unwrap()).unwrap();
    assert_eq!(owner.u32(instance - 4).unwrap(), 0x80803FE8);
    assert_eq!(
        owner.u64(instance + 0x40).unwrap(),
        u64::MAX,
        "cached provider is not absent"
    );
    let action = owner.u32(e + 0x38).unwrap();
    let flags = [
        owner.u8(e + 0x3C).unwrap(),
        owner.u8(e + 0x3D).unwrap(),
        owner.u8(e + 0x3F).unwrap(),
    ];
    if flags[0] != 0 {
        return json!({"route":flags[0],"lane":flags[1],"save":flags[2],"action":action});
    }
    let stock_states = Payload(stock.read_tag(TagHash(STATES)).unwrap());
    assert!(
        !names(&stock_states).contains_key(&action),
        "body event names a stock action"
    );
    let states = Payload(staged.read_tag(TagHash(STATES)).unwrap());
    let ordinal = names(&states)[&action];
    let node = states.array(24, 16, Some(0x8080342F)).unwrap()[usize::try_from(ordinal).unwrap()];
    assert_eq!(states.u64(node).unwrap(), 1, "private action is not direct");
    let bank = Payload(staged.read_tag(TagHash(BANK)).unwrap());
    let descriptors = bank.array(0x68, 32, Some(0x80809002)).unwrap();
    let clips = bank.array(8, 4, Some(0x80808F48)).unwrap();
    let choices = states
        .array(states.pointer(node + 8).unwrap(), 8, Some(0x80803439))
        .unwrap()
        .into_iter()
        .map(|row| {
            let descriptor = descriptors[usize::try_from(states.u32(row).unwrap()).unwrap()];
            let clip = bank
                .u32(clips[usize::from(bank.u16(descriptor + 24).unwrap())])
                .unwrap();
            assert!(
                stock.get_entry(TagHash(clip)).is_none(),
                "body clip is stock"
            );
            assert_eq!(
                staged.get_entry(TagHash(clip)).unwrap().reference,
                0x80808F49
            );
            let payload = Payload(staged.read_tag(TagHash(clip)).unwrap());
            json!({"weight":states.u32(row + 4).unwrap(),"motion":motion(&payload, false)})
        })
        .collect::<Vec<_>>();
    json!({"route":flags[0],"lane":flags[1],"save":flags[2],"choices":choices})
}

/// The expected staged form of one source event.
pub(super) fn expected(action: u32, flags: [u8; 3], actions: &BTreeMap<u32, Value>) -> Value {
    if flags[0] != 0 {
        json!({"route":flags[0],"lane":flags[1],"save":flags[2],"action":action})
    } else {
        json!({"route":flags[0],"lane":flags[1],"save":flags[2],"choices":actions[&action]})
    }
}

/// Stock package files by name, size and modification time.
pub(super) fn stock_files(packages: &Path) -> BTreeMap<String, (u64, std::time::SystemTime)> {
    fs::read_dir(packages)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            let metadata = entry.metadata().unwrap();
            (
                entry.file_name().to_string_lossy().into_owned(),
                (metadata.len(), metadata.modified().unwrap()),
            )
        })
        .collect()
}
