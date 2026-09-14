use super::*;

const A: u32 = 0x80F8_3B47;
const B: u32 = 0x815B_3A28;
const C: u32 = 0x80BF_9A95;

fn put32(data: &mut [u8], offset: usize, value: u32) {
    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put64(data: &mut [u8], offset: usize, value: u64) {
    data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn array(data: &mut [u8], pointer: usize, header: usize, class: u32, count: u64) {
    put64(data, pointer, (header as i64 - pointer as i64) as u64);
    put32(data, header - 4, 0x8080_9FBD);
    put64(data, header, count);
    put32(data, header + 8, class);
}

fn native(data: &[u8], class: u32) -> Result<BTreeMap<u32, usize>, String> {
    let mut registry = Registry::new()?;
    walk(data, class, |class| {
        registry.record(class, |_| Err("Unexpected generated schema".into()))
    })
}

#[test]
fn unnamed_component_reference_at_first_crash_site_is_discovered() {
    // The editable member reflection omits this tag. The native loading declaration includes it.
    let mut data = vec![0; 0x340];
    put32(&mut data, 0x1DC, A);
    assert_eq!(native(&data, 0x8080_72BD).unwrap().get(&A), Some(&0x1DC));
    data.truncate(0x33F);
    assert!(native(&data, 0x8080_72BD).is_err());
}

#[test]
fn nested_operation_array_covers_the_second_crash_path() {
    let mut data = vec![0; 0x188];
    array(&mut data, 0x48, 0x160, 0x8080_6CC8, 1);
    put32(&mut data, 0x180, A);
    put32(&mut data, 0x184, u32::MAX);
    assert_eq!(native(&data, 0x8080_6CC6).unwrap().get(&A), Some(&0x180));
    put64(&mut data, 0x160, u64::MAX);
    assert!(native(&data, 0x8080_6CC6).is_err());
}

#[test]
fn repeated_table_fields_and_backing_data_are_transitive() {
    let mut table = vec![0; 0x158];
    array(&mut table, 0x18, 0xB0, 0x8080_7378, 1);
    put32(&mut table, 0xC0, B);
    put32(&mut table, 0xC4, B);
    put32(&mut table, 0xC8, u32::MAX);
    put32(&mut table, 0xCC, 0x811C_9DC5);
    put32(&mut table, 0xD0, A); // A legitimate cycle through the same table.
    let mut reads = Vec::new();
    let result = collect([A], &mut Registry::new().unwrap(), |tag| {
        reads.push(tag);
        Ok(match tag {
            A => Resource {
                kind: 8,
                class: 0x8080_73A5,
                payload: table.clone(),
            },
            B => Resource {
                kind: 32,
                class: C,
                payload: Vec::new(),
            },
            C => Resource {
                kind: 40,
                class: u32::MAX,
                payload: Vec::new(),
            },
            _ => return Err("Unexpected reference".into()),
        })
    })
    .unwrap();
    assert_eq!(
        reads.iter().copied().collect::<BTreeSet<_>>(),
        [A, B, C].into()
    );
    assert_eq!(reads.len(), 3);
    assert!(result.iter().any(|r| r.tag == B && r.parent == A));
    assert!(
        result
            .iter()
            .any(|r| r.tag == C && r.parent == B && r.offset == usize::MAX)
    );
}

#[test]
fn gpu_headers_include_backing_resources_without_following_raw_backlinks() {
    // Native GPU headers use separate directory entries for data. Shader bytecode
    // (33 -> 41) was previously omitted even when the material and header loaded.
    // Backing entries may name their header, or another shared header, in return.
    for (header_kind, backing_kind) in [(32, 40), (33, 41), (34, 42)] {
        let mut reads = Vec::new();
        let result = collect([A], &mut Registry::new().unwrap(), |tag| {
            reads.push(tag);
            Ok(match tag {
                A => Resource {
                    kind: header_kind,
                    class: B,
                    payload: Vec::new(),
                },
                B => Resource {
                    kind: backing_kind,
                    class: C,
                    payload: Vec::new(),
                },
                _ => return Err("Raw backing metadata is not a forward dependency".into()),
            })
        })
        .unwrap();
        assert_eq!(reads, [A, B]);
        assert_eq!(
            result,
            [Reference {
                tag: B,
                parent: A,
                offset: usize::MAX,
            }]
        );
    }
}

#[test]
fn present_gpu_headers_do_not_hide_missing_backing_resources() {
    for kind in 32..=34 {
        let error = collect([A], &mut Registry::new().unwrap(), |tag| {
            if tag == A {
                Ok(Resource {
                    kind,
                    class: B,
                    payload: Vec::new(),
                })
            } else {
                Err(format!("Missing backing resource 0x{tag:08X}"))
            }
        })
        .unwrap_err();
        assert_eq!(error, format!("Missing backing resource 0x{B:08X}"));
    }
}

#[test]
fn a_missing_valid_child_is_an_error_even_when_its_parent_is_available() {
    let mut data = vec![0; 0x340];
    put32(&mut data, 0x1DC, B);
    let error = collect([A], &mut Registry::new().unwrap(), |tag| {
        if tag == A {
            Ok(Resource {
                kind: 8,
                class: 0x8080_72BD,
                payload: data.clone(),
            })
        } else {
            Err("Child resource is missing".into())
        }
    })
    .err()
    .unwrap();
    assert_eq!(error, "Child resource is missing");
}

#[test]
fn typed_pointer_headers_and_backward_arrays_are_checked() {
    let mut data = vec![0; 0xC0];
    array(&mut data, 0x18, 0xA0, 0x8080_737E, 0);
    assert!(native(&data, 0x8080_73A5).is_ok());
    put64(&mut data, 0x18, i64::MIN as u64);
    assert!(native(&data, 0x8080_73A5).is_err());
    put64(&mut data, 0x18, 0x88);
    put32(&mut data, 0x9C, 0);
    assert!(native(&data, 0x8080_73A5).is_err());

    // A typed array pointer may point backward in its owning payload.
    let mut data = vec![0; 0x40];
    array(&mut data, 0x30, 0x10, 0x8080_0014, 1);
    put32(&mut data, 0x20, A);
    let refs = walk(&data, 1, |class| {
        Ok(match class {
            1 => Record {
                size: 0x40,
                fields: vec![(0x30, 3)].into(),
            },
            0x8080_0014 => Record {
                size: 4,
                fields: vec![(0, 4)].into(),
            },
            _ => return Err("Unexpected class".into()),
        })
    })
    .unwrap();
    assert_eq!(refs.get(&A), Some(&0x20));
}

#[test]
fn canonical_upper_package_range_is_retained_and_names_are_ignored() {
    let valid = TagHash::new(0xCFF, 10).0;
    let mut data = vec![0; 0x340];
    put32(&mut data, 0x1DC, valid);
    assert!(native(&data, 0x8080_72BD).unwrap().contains_key(&valid));
    for value in [0, u32::MAX, 0x811C_9DC5, 0x8244_06DF] {
        put32(&mut data, 0x1DC, value);
        assert!(!native(&data, 0x8080_72BD).unwrap().contains_key(&value));
    }
}
