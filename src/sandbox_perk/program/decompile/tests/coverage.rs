use super::*;
use crate::sandbox_perk::action::fixtures::{Builder, drawn_pattern_action, precision_kill_action};
use crate::sandbox_perk::action::{CONDITION_ROW_CLASS, EFFECT_ROW_CLASS};

#[test]
fn out_of_range_probability_is_preserved_and_refused_for_authoring() {
    let original = precision_kill_action();
    let at = decode(&original).unwrap().groups[0].activation[0].offset;
    for value in [2.0_f32, -1.0, f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
        let mut payload = original.clone();
        payload[at..at + 4].copy_from_slice(&value.to_le_bytes());
        let decoded = decode(&payload).unwrap();
        let Probability::Literal(read) = decoded.groups[0].activation[0].probability else {
            panic!("invalid probability was normalized to Always");
        };
        assert_eq!(read.to_bits(), value.to_bits());
        assert!(
            decompile(&decoded, "Invalid Probability", |tag| tag)
                .unwrap_err()
                .0
                .contains("probability")
        );
    }
}

#[test]
fn fidelity_detects_removed_effects_and_changes_to_every_scalar_field() {
    for layout in layout::EFFECT_LAYOUTS {
        let entry = crate::sandbox_perk::nodes::effect(layout.kind).unwrap();
        let mut builder = Builder::new();
        let node = builder.node(entry.class, entry.struct_size as usize);
        builder.bytes[node] = entry.kind;
        builder.pointer_list(0x38, EFFECT_ROW_CLASS, &[node]);
        let original = builder.finish();
        for field in layout.fields {
            let mut changed = original.clone();
            changed[node + field.offset] ^= 1;
            let differences = fidelity(&original, &changed).unwrap();
            assert!(
                !differences.is_empty(),
                "kind {} {}",
                layout.kind,
                field.label
            );
        }
        let mut removed = original.clone();
        removed[0x38..0x48].fill(0);
        assert!(
            fidelity(&original, &removed)
                .unwrap()
                .iter()
                .any(|d| d.node == "Effect List Length")
        );
    }
}

#[test]
fn fidelity_detects_timer_caps_root_state_and_label_changes() {
    let original = precision_kill_action();
    let decoded = decode(&original).unwrap();
    let effect = &decoded.groups[0].effects[0];
    let mut changed = original.clone();
    changed[effect.offset + 8..effect.offset + 12].copy_from_slice(&10.0_f32.to_le_bytes());
    assert!(!fidelity(&original, &changed).unwrap().is_empty());
    for offset in [0x88, 0x90, 0x98, 0xA0, 0xCC, 0xCD] {
        let mut changed = original.clone();
        changed[offset] ^= 1;
        assert!(
            !fidelity(&original, &changed).unwrap().is_empty(),
            "root {offset:X}"
        );
    }
    let at = decoded.groups[0].activation[0].offset + 0xD0;
    let (_, _, rows, _) = crate::package_payload::native_array_at(&original, at).unwrap();
    let mut changed = original.clone();
    changed[rows..rows + 4].copy_from_slice(&0xC20D_D425_u32.to_le_bytes());
    assert!(
        fidelity(&original, &changed)
            .unwrap()
            .iter()
            .any(|d| d.node.contains("Matches Any Label"))
    );
}

#[test]
fn unsupported_action_structure_never_gets_a_false_exact_conversion() {
    let original = drawn_pattern_action();
    // A policy selector is carried and compared, so a change is a reported difference.
    let mut changed = original.clone();
    changed[0xB8] = 1;
    assert!(!fidelity(&original, &changed).unwrap().is_empty());
    let mut builder = Builder::new();
    let activation = builder.unconditional();
    builder.pointer_list(0x20, CONDITION_ROW_CLASS, &[activation]);
    // Auxiliary records are carried verbatim, so a root list whose rows are not the pointer
    // rows every stock action uses cannot be decoded and nothing claims to convert it.
    builder.rows(0x10, 0x8080_4087, 1, 8);
    assert!(
        decode(&builder.finish())
            .unwrap_err()
            .contains("0x80804087")
    );
}
