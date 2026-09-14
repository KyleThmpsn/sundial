use super::*;

fn one_node(entry: &nodes::NodeKind, condition: bool) -> (Builder, usize) {
    let mut out = Builder::new();
    let at = if condition {
        out.condition(entry.class, entry.kind, entry.struct_size as usize)
    } else {
        let at = out.node(entry.class, entry.struct_size as usize);
        out.bytes[at] = entry.kind;
        at
    };
    out.pointer_list(
        PRIMARY_GROUP
            + if condition {
                GROUP_ACTIVATION
            } else {
                GROUP_EFFECTS
            },
        if condition {
            CONDITION_ROW_CLASS
        } else {
            EFFECT_ROW_CLASS
        },
        &[at],
    );
    (out, at)
}

#[test]
fn every_observed_catalog_type_decodes_and_validates_its_class_and_extent() {
    for (entries, condition) in [(&nodes::CONDITIONS[..], true), (&nodes::EFFECTS[..], false)] {
        for entry in entries.iter().filter(|entry| entry.observed()) {
            let (out, at) = one_node(entry, condition);
            let payload = out.finish();
            let decoded = decode(&payload)
                .unwrap_or_else(|error| panic!("{} {}: {error}", condition, entry.kind));
            if condition {
                assert_eq!(decoded.conditions()[0].kind, entry.kind);
            } else {
                assert_eq!(decoded.effects().next().unwrap().kind, entry.kind);
            }
            let mut wrong_class = payload.clone();
            wrong_class[at - 4..at].copy_from_slice(&0x8080_40B5_u32.to_le_bytes());
            assert!(decode(&wrong_class).unwrap_err().contains("class"));

            // Put the pointed node last so a truncated node cannot borrow list bytes.
            let mut truncated = Builder::new();
            let rows = truncated.rows(
                PRIMARY_GROUP
                    + if condition {
                        GROUP_ACTIVATION
                    } else {
                        GROUP_EFFECTS
                    },
                if condition {
                    CONDITION_ROW_CLASS
                } else {
                    EFFECT_ROW_CLASS
                },
                1,
                8,
            );
            let end_node = truncated.node(entry.class, entry.struct_size as usize);
            truncated.bytes[end_node..end_node + entry.struct_size as usize]
                .copy_from_slice(&payload[at..at + entry.struct_size as usize]);
            truncated.pointer(rows, end_node);
            truncated.bytes.pop();
            assert!(
                decode(&truncated.finish()).is_err(),
                "{} {}",
                condition,
                entry.kind
            );
        }
    }
}

#[test]
fn every_unrecovered_or_unregistered_kind_is_rejected_explicitly() {
    for condition in [false, true] {
        for kind in 0..=u8::MAX {
            let entry = if condition {
                nodes::condition(kind)
            } else {
                nodes::effect(kind)
            };
            if entry.is_some_and(nodes::NodeKind::observed) {
                continue;
            }
            let entry = nodes::NodeKind {
                kind,
                class: 0x1234_5678,
                struct_size: 8,
                occurrences: 0,
                name: "Unknown",
                summary: "Unknown",
                evidence: "Unknown",
                support: Support::Unobserved,
            };
            let (out, _) = one_node(&entry, condition);
            let error = decode(&out.finish()).unwrap_err();
            assert!(
                error.contains("no recovered") || error.contains("not registered"),
                "{error}"
            );
        }
    }
}

#[test]
fn nested_predicate_cycles_hit_the_depth_guard() {
    let entry = nodes::condition(35).unwrap();
    let (mut out, at) = one_node(entry, true);
    out.pointer(at + NESTED_PREDICATE_POINTER, at);
    assert!(
        decode(&out.finish())
            .unwrap_err()
            .contains("nest too deeply")
    );
}

#[test]
fn policy_and_program_shape_are_part_of_authoring_support() {
    let payload = drawn_pattern_action();
    let mut action = decode(&payload).unwrap();
    assert_eq!(action.support(), Support::Authorable);
    action.policy = 1;
    assert_eq!(action.support(), Support::Readable);
    action.policy = 0;
    action.groups.push(action.groups[0].clone());
    assert_eq!(action.support(), Support::Readable);
    action.groups.pop();
    action.groups[0].effects[0].kind = 255;
    assert_eq!(action.support(), Support::Unobserved);
}

#[test]
fn signed_ammunition_counts_are_read_without_float_rounding() {
    for value in [16_777_217_i32, -16_777_217, i32::MIN, i32::MAX] {
        let (mut out, at) = one_node(nodes::effect(14).unwrap(), false);
        out.u32(at + 0x6C, value as u32);
        let decoded = decode(&out.finish()).unwrap();
        let fact = decoded
            .effects()
            .next()
            .unwrap()
            .facts
            .iter()
            .find(|fact| fact.label == "Owning Slot Amount")
            .unwrap();
        assert_eq!(fact.value, FactValue::Integer(value));
        assert_eq!(fact.value.render(), value.to_string());
    }
}
