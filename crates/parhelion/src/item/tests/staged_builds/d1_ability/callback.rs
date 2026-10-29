//! Follow loader reference slots in persisted owners, independently of the callback writer.
use super::*;

#[allow(clippy::cognitive_complexity)]
pub(super) fn readback(owner: &Payload, tag: u32, bank: bool) -> serde_json::Value {
    let root = owner.pointer(16).unwrap();
    let boundary = owner.pointer(24).unwrap();
    let mut rows = Vec::new();
    let mut occupied = BTreeSet::new();
    let mut embedded = BTreeSet::new();
    for row in owner.array(0x20, 8, Some(0x8080919F)).unwrap() {
        // Native loader entries address the object reference after implementation and method.
        let reference = owner.pointer(row).unwrap();
        let record = reference.checked_sub(8).unwrap();
        assert!(record >= root && record + 24 <= boundary);
        for byte in record..record + 24 {
            assert!(occupied.insert(byte), "overlapping callback storage");
        }
        let implementation = owner.u32(record).unwrap();
        let method = owner.u32(record + 4).unwrap();
        assert_eq!(owner.u32(reference).unwrap(), tag);
        let class = owner.u32(reference + 4).unwrap();
        let target = usize::try_from(owner.u64(reference + 8).unwrap()).unwrap();
        assert!(target >= root && target + 16 <= boundary);
        let definition = usize::try_from(owner.u64(target + 8).unwrap()).unwrap();
        assert_eq!(owner.u32(target).unwrap(), tag);
        assert_eq!(owner.u32(definition).unwrap(), tag);
        assert_eq!(owner.u32(definition + 4).unwrap(), class);
        assert_eq!(owner.u64(definition + 8).unwrap(), target as u64);
        if target != root {
            embedded.insert(target);
        }
        rows.push(
            json!({"record":record,"reference":reference,"implementation":implementation,
            "method":method,"class":class,"target":target}),
        );
    }
    assert!(!rows.is_empty(), "required callback corpus is empty");
    if bank {
        // Original retained bank method registration and native common-bank witness.
        assert_eq!(rows.len(), 3);
        for (row, (offset, implementation, method)) in rows.iter().zip([
            (0x30, 0x8080417Au32, 0),
            (0x48, 0x8080417A, 1),
            (0x60, 0x808044FC, 12),
        ]) {
            assert_eq!(row["record"], json!(root + offset));
            assert_eq!(row["target"], json!(root));
            assert_eq!(row["class"], json!(0x8080417Au32));
            assert_eq!(row["implementation"], json!(implementation));
            assert_eq!(row["method"], json!(method));
        }
    } else {
        assert!(
            !embedded.is_empty(),
            "controller lost its embedded callback targets"
        );
    }
    json!({"owner":tag,"sha256":sha(&owner.0),"callbacks":rows})
}
