use super::*;
use sha2::{Digest, Sha256};

/// Consume the reopened table using the native vendor service's independent wire contract.
/// This deliberately does not use Parhelion's numeric-expression parser.
pub(super) fn verify(manager: &PackageManager, output: &Path) -> serde_json::Value {
    let globals = manager
        .read_tag(resolve_live_named_tag(manager, "investment_globals", None).unwrap())
        .unwrap();
    let root = manager
        .read_tag(globals_child_tag(&globals, 0).unwrap())
        .unwrap();
    let tag = root_child_tag(&root, ROOT_OBJECTIVE_DEFINITION_TABLE_SLOT).unwrap();
    let bytes = manager.read_tag(tag).unwrap();
    fs::create_dir_all(output).unwrap();
    fs::write(output.join("native-objectives.bin"), &bytes).unwrap();
    let (count, rows) = consume_array(&bytes, 8, 160, 0x8080_775F);
    assert!(count > 0 && count <= 16_384);
    let mut expressions = 0;
    let mut flags = 0;
    let mut markers = BTreeMap::<String, usize>::new();
    for index in 0..count {
        let row = rows + index * 160;
        read_i32(&bytes, row + 48).unwrap();
        let (ops, _) = consume_array(&bytes, row + 8, 8, 0x8080_7D31);
        let (supplied, _) = consume_array(&bytes, row + 72, 2, 0x8080_7D4B);
        assert!(ops <= 256 && supplied <= 256, "objective row {index}");
        expressions += ops;
        flags += supplied;
        if ops > 0 {
            let header = relative_target(&bytes, row + 16).unwrap();
            let marker = read_u32(&bytes, header - 4).unwrap();
            *markers.entry(format!("{marker:08X}")).or_default() += 1;
        }
    }
    let report = serde_json::json!({
        "objective_tag": format!("{:08X}", tag.0),
        "payload_sha256": format!("{:x}", Sha256::digest(&bytes)),
        "objective_rows": count,
        "expression_instructions": expressions,
        "supplied_flags": flags,
        "expression_markers": markers,
        "native_objective_consumption": true,
        "contract": "Dawn vendor service array, expression and objective publication",
        "gameplay_verified": false
    });
    fs::write(
        output.join("native-objective-consumption.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    report
}

fn consume_array(bytes: &[u8], descriptor: usize, stride: usize, class: u32) -> (usize, usize) {
    let count = usize::try_from(read_u64(bytes, descriptor).unwrap()).unwrap();
    assert!(count <= 65_535, "array at 0x{descriptor:X}");
    if count == 0 {
        return (0, 0);
    }
    let header = relative_target(bytes, descriptor + 8).unwrap();
    assert!(header >= 4 && header <= bytes.len().saturating_sub(16));
    assert_eq!(
        read_u32(bytes, header - 4).unwrap() >> 16,
        0x8080,
        "native array marker at 0x{descriptor:X}, header 0x{header:X}"
    );
    assert_eq!(read_u64(bytes, header).unwrap(), count as u64);
    assert_eq!(read_u32(bytes, header + 8).unwrap(), class);
    let rows = header + 16;
    assert!(count <= (bytes.len() - rows) / stride);
    (count, rows)
}
