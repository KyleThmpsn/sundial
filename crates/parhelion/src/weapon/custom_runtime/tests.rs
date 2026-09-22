use super::*;
use sundial::package_authoring::sandbox_perk::{action, program::NativeProgram};

#[test]
fn damage_callbacks_follow_each_authored_record_after_relocation() {
    let mut program = NativeProgram::empty();
    program
        .graph
        .create_target(0, 0x40, action::EFFECT_ROW_CLASS, true)
        .unwrap();
    let rows = program.graph.blocks[0].links[&0x40];
    program.graph.resize_array(rows, 3).unwrap();
    for (index, class) in [0x8080_3E3F, 0x8080_3E3C, 0x8080_3E3C]
        .into_iter()
        .enumerate()
    {
        program
            .graph
            .create_target(rows, index * 8, class, false)
            .unwrap();
        if class == 0x8080_3E3C {
            let node = program.graph.blocks[rows].links[&(index * 8)];
            program.graph.blocks[node].bytes[4..8]
                .copy_from_slice(&(0.25 * index as f32).to_le_bytes());
            program.graph.blocks[node].bytes[9..12].copy_from_slice(&[0xAB, 0xCD, 0xEF]);
        }
    }
    let mut payload = program.graph.emit().unwrap();
    let source = payload.clone();
    let sites = action::self_references(&source).unwrap();
    assert_eq!(sites.len(), 2);
    let owner = TagHash::new(0x04FC, 123);
    rebind_program_callbacks(&mut payload, owner).unwrap();
    let mut expected = source.clone();
    for site in &sites {
        let at = site.reference_offset;
        expected[at..at + 4].copy_from_slice(&owner.0.to_le_bytes());
        expected[at + 8..at + 16].copy_from_slice(&(site.target_offset as u64).to_le_bytes());
        assert_eq!(read_u32(&payload, at).unwrap(), owner.0);
        assert_eq!(read_u32(&payload, at + 4).unwrap(), 0x8080_3E3C);
        assert_eq!(
            read_u64(&payload, at + 8).unwrap(),
            site.target_offset as u64
        );
    }
    assert_eq!(payload, expected, "only owner and offset may change");
    rebind_program_callbacks(&mut payload, owner).unwrap();
    assert_eq!(payload, expected, "rebinding is idempotent");

    // Validate every descriptor before mutating any reference.
    let mut invalid = source;
    invalid[sites[1].reference_offset + 4] ^= 1;
    let before = invalid.clone();
    assert!(rebind_program_callbacks(&mut invalid, owner).is_err());
    assert_eq!(invalid, before);
}
