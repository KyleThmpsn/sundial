use super::*;

type Controls = Option<(f32, f32, u8)>;

pub(super) fn output(code: &mut Vec<u32>, controls: Controls) {
    let Some((_, _, mode)) = controls else {
        return;
    };
    if mode == 1 {
        // A live branch in W must preserve the independently supported emission slice.
        code.extend(instruction(31 | 1 << 18, &[&source(1, 3, 0)]));
    }
    code.extend(instruction(54, &[&register(2, 2, 8), &constant(1, 0)]));
    if mode == 1 {
        code.extend(instruction(21, &[]));
    }
}

pub(super) fn constants(material: &mut Vec<u8>, controls: Controls) {
    let Some((w, _, mode)) = controls else {
        return;
    };
    let offset = i64::from_le_bytes(material[0x320..0x328].try_into().unwrap());
    let row = (0x320_i64 + offset) as usize + 32;
    floats(material, row, &[w, 0.0, 0.0, 0.0]);
    if mode == 4 {
        let mut values = [0; 16];
        floats(&mut values, 0, &[1.0; 4]);
        array(material, 0x2F8, 0x8080_0090, &values, 16);
        array(
            material,
            0x2E8,
            0x8080_0009,
            &[0x3C, 1, 0, 0x34, 0, 0x03, 0x43, 1],
            1,
        );
    }
}

pub(super) fn channels(package: &mut Package, controls: Controls) {
    let Some((_, exponent, mode)) = controls else {
        return;
    };
    let mut table = vec![0; 0x28];
    // The fixture moves this named channel to index zero instead of shipped index 82.
    let name = crate::hash::fnv1_name_hash(if mode == 2 {
        "fixture.unrelated_channel"
    } else {
        "ao_ambient_weight"
    });
    array(&mut table, 8, 0x8080_0070, &name.to_le_bytes(), 4);
    let mut value = [0; 16];
    floats(&mut value, 0, &[exponent, 0.0, 0.0, 0.0]);
    array(&mut table, 0x18, 0x8080_0090, &value, 16);
    if mode == 6 {
        // Missing numeric storage cannot stand in for the named exponent.
        put(&mut table, 0x18, &0_u64.to_le_bytes());
    }
    if mode == 5 {
        // Conflicting tables must not choose whichever package was visited first.
        let mut conflict = table.clone();
        let offset = i64::from_le_bytes(conflict[0x20..0x28].try_into().unwrap());
        let row = (0x20_i64 + offset) as usize + 16;
        floats(&mut conflict, row, &[exponent + 0.5, 0.0, 0.0, 0.0]);
        package.add(0x8080_858D, conflict);
    }
    package.add(0x8080_858D, table);
}
