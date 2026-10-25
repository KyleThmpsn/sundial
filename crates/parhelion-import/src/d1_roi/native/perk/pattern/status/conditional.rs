//! An inherited scalar channel supplies the status count through a retained connection.
use super::*;

pub(super) struct Companion {
    pub owner: u32,
    pub threshold: [u8; 16],
    name: u32,
    code: Vec<u8>,
}

pub(super) fn read(
    source: &Source,
    graph: &Payload,
    status: &Payload,
    status_tag: u32,
) -> Result<Companion> {
    let owners = graph.array(32, 12, Some(0x80800715))?;
    ensure!(
        owners.len() == 2 && graph.u32(owners[0])? == status_tag,
        "Conditional status owner order differs"
    );
    let owner = graph.u32(owners[1])?;
    ensure!(
        source.reference(owner)? == 0x80800861,
        "Conditional channel owner class differs"
    );
    let p = source.read(owner)?;
    let (s, live) = (p.pointer(24)?, p.pointer(16)?);
    ensure!(
        p.u32(s - 4)? == 0x808008F6 && p.u32(live - 4)? == 0x80800594,
        "Conditional status companion is not a channel bank"
    );
    pair(&p, owner, live, s, 0x80800594, 0x808008F6)?;
    for (offset, value) in [
        (0x18, 0),
        (0x20, u32::MAX as u64),
        (0x28, 0),
        (0x30, u32::MAX as u64),
        (0x38, u32::MAX as u64),
        (0x40, 0),
        (0x48, u32::MAX as u64),
        (0x50, 0),
        (0x58, 0),
    ] {
        ensure!(
            p.u64(s + offset)? == value,
            "Conditional channel has additional host state"
        );
    }
    let rows = p.array(s + 0xD8, 112, Some(0x80800883))?;
    let inherited = p.array(s + 0x108, 12, Some(0x8080087B))?;
    ensure!(
        rows.len() == 1
            && inherited.len() == 1
            && p.u64(s + 0xE8)? == 0
            && p.u64(s + 0xF8)? == 0
            && p.u32(s + 0x128)? == 0,
        "Conditional channel needs additional declarations"
    );
    let (row, parent) = (rows[0], inherited[0]);
    let name = p.u32(row)?;
    // This built-in declaration occurs in both original executable dictionaries.
    // Other inherited names need their own native producer correspondence.
    ensure!(
        name == 0x58A9CB99
            && p.u32(row + 4)? == 0
            && p.0[row + 8..row + 80].iter().all(|&v| v == 0)
            && p.u64(row + 96)? == 0
            && p.bytes::<8>(row + 104)? == [255, 254, 255, 255, 0, 0, 0, 0]
            && p.u32(parent)? == 0x811C9DC5
            && p.u32(parent + 4)? == name
            && p.u32(parent + 8)? == 0,
        "Conditional channel has an unqualified source or procedure"
    );
    let dependencies = p.array(row + 80, 4, Some(0x8080000B))?;
    ensure!(
        dependencies.len() == 1 && p.u32(dependencies[0])? == 1,
        "Conditional channel dependency differs"
    );
    for (offset, stride, class, bytes) in [
        (0x40, 4, 0x8080000B, vec![0; 4]),
        (0x50, 16, 0x80800090, vec![0; 16]),
        (0x60, 16, 0x80800090, vec![0; 16]),
        (0xA0, 16, 0x808008DC, vec![255; 16]),
        (0xB0, 4, 0x8080000B, vec![0; 4]),
    ] {
        let array = p.array(live + offset, stride, Some(class))?;
        ensure!(
            array.len() == 1 && p.0.get(array[0]..array[0] + stride) == Some(bytes.as_slice()),
            "Conditional channel initialization differs"
        );
    }
    ensure!(
        p.u64(live + 0x70)? == 0 && p.u64(live + 0x80)? == 0 && p.u64(live + 0x90)? == 0,
        "Conditional channel has additional history or receivers"
    );

    let exports = graph.array(0x68, 40, Some(0x80800370))?;
    let references = graph.array(0x78, 24, Some(0x80800790))?;
    ensure!(
        exports.len() == references.len(),
        "Conditional export references differ"
    );
    let expected: [(usize, u32, u64, u32); 4] = [
        (0x60, 0x80AAD526, 0u64, 0),
        (0x78, 0x80AAD525, 0xFFF7, 4),
        (0x90, 0x80AAD523, 12, 0),
        (0xA8, 0x80AAD524, 0, 0),
    ];
    let mut seen = BTreeSet::new();
    for (&at, &reference) in exports.iter().zip(&references) {
        if graph.u32(at + 16)? != owner {
            continue;
        }
        let parameter = usize::try_from(graph.u64(at + 24)?)?;
        let &(offset, declaration, selector, output) = expected
            .iter()
            .find(|&&(offset, _, _, _)| s + offset == parameter)
            .context("Unknown conditional channel export")?;
        ensure!(
            p.pointer(parameter)? == s
                && p.u64(parameter + 8)? == declaration as u64
                && p.u64(parameter + 16)? == selector
                && graph.u32(at)? == output << 16
                && graph.u32(at + 4)? == 0
                && graph.u32(at + 8)? == (selector as u16 as i16 as i32) as u32
                && graph.u32(at + 12)? == 0x80800594
                && graph.u32(at + 20)? == 0
                && graph.u64(at + 32)? == declaration as u64
                && graph.u64(reference)? == owner as u64
                && graph.u64(reference + 8)? == live as u64
                && graph.u64(reference + 16)? == 1
                && seen.insert(offset),
            "Conditional channel export or instance binding differs"
        );
    }
    ensure!(
        seen.len() == expected.len(),
        "Incomplete conditional channel exports"
    );

    let root = status.pointer(24)?;
    let expression = root + 0x190;
    let code = status
        .array(expression + 24, 1, Some(0x80800009))?
        .into_iter()
        .map(|a| status.u8(a))
        .collect::<Result<Vec<_>>>()?;
    let constants = status.array(expression + 40, 16, Some(0x80800090))?;
    let inputs = status.array(expression + 72, 48, Some(0x80800927))?;
    ensure!(
        code == [0x3C, 1, 0x34, 0, 0x02, 0x23, 0x3E, 0]
            && constants.len() == 1
            && inputs.len() == 1
            && status.u64(expression + 56)? == 2
            && status.u64(expression + 64)? == 1
            && status.bytes::<16>(expression + 88)? == [0; 16],
        "Conditional status count needs another expression or input contract"
    );
    let threshold = status.bytes::<16>(constants[0])?;
    let scalar = status.f32(constants[0])?;
    ensure!(
        scalar.is_finite()
            && (0.0..=127.0).contains(&scalar)
            && (1..4)
                .all(|lane| status.u32(constants[0] + lane * 4).ok() == Some(scalar.to_bits())),
        "Conditional status threshold is not a bounded scalar"
    );
    let input = inputs[0];
    pair(
        status,
        status_tag,
        status.pointer(16)? + 0x1F0,
        input,
        0x8080077D,
        0x80800927,
    )?;
    ensure!(
        status.u64(input + 24)? == 0
            && status.u64(input + 32)? == 0x808003E8
            && status.u64(input + 40)? == name as u64,
        "Conditional status input is not a direct named channel"
    );
    let outputs = graph.array(0x30, 88, Some(0x8080054E))?;
    ensure!(
        outputs.len() == 2,
        "Conditional status connection count differs"
    );
    let at = outputs[1];
    ensure!(
        graph.u64(at)? == 0x811C9DC5
            && graph.u64(at + 8)? == status_tag as u64
            && graph.u64(at + 16)? == input as u64
            && graph.u64(at + 24)? == 0x80800927
            && graph.u64(at + 32)? == 0
            && graph.u64(at + 40)? == 0x811C9DC5
            && graph.u64(at + 48)? == owner as u64
            && graph.u64(at + 56)? == (s + 0x60) as u64
            && graph.u64(at + 64)? == 0x808005AB
            && graph.u64(at + 72)? == 1
            && graph.u64(at + 80)? == 0,
        "Conditional status input connects to a different receiver"
    );
    Ok(Companion {
        owner,
        threshold,
        name,
        code,
    })
}

pub(super) fn patch(
    native: &mut Reader,
    graph: &Payload,
    status: &Payload,
    status_tag: u32,
    companion: &Companion,
) -> Result<(Vec<NativeAssetResourcePatch>, NativeAssetResourceAppend)> {
    let owners = graph.array(16, 12, Some(0x80809C04))?;
    ensure!(
        owners.len() == 2 && graph.u32(owners[0])? == status_tag,
        "Native conditional owner order differs"
    );
    let channel_tag = graph.u32(owners[1])?;
    let channel = native.tag(channel_tag, Some(0x80809C36))?;
    let c = channel.pointer(24)?;
    let live = channel.pointer(16)?;
    ensure!(
        channel.u32(c - 4)? == 0x80809790
            && channel.u32(live - 4)? == 0x8080979F
            && channel.u32(c)? == channel_tag
            && channel.u32(c + 4)? == 0x8080979F
            && channel.u64(c + 8)? == live as u64
            && channel.u32(live)? == channel_tag
            && channel.u32(live + 4)? == 0x80809790
            && channel.u64(live + 8)? == c as u64
            && channel.u64(c + 0x48)? == (-(0x48i64)) as u64
            && channel.u64(c + 0x58)? == 0
            && channel.u64(c + 0x70)? == 0
            && channel.u64(c + 0x88)? == 0xFFF7
            && channel.u64(c + 0xA0)? == 14
            && channel.u64(c + 0xB8)? == 0
            && channel.u64(c + 0xE8)? == 0
            && channel.u64(c + 0xF8)? == 0
            && channel.u64(c + 0x118)? == 0,
        "Native conditional channel has additional behavior"
    );
    ensure!(
        channel.u32(c + 0x128)? == u32::MAX
            && channel.u32(c + 0x134)? == 1
            && channel.u32(c + 0x13C)? == 0
            && channel.u32(c + 0x140)? == 1,
        "Native conditional inherited lookup range or allocation differs"
    );
    let rows = channel.array(c + 0xD8, 112, Some(0x808097A1))?;
    let parents = channel.array(c + 0x108, 12, Some(0x808097A0))?;
    ensure!(
        rows.len() == 1 && parents.len() == 1,
        "Native conditional channel count differs"
    );
    let row = rows[0];
    ensure!(
        channel.0[row + 4..row + 80].iter().all(|&v| v == 0)
            && channel.u64(row + 96)? == 0
            && channel.bytes::<8>(row + 104)? == [255, 254, 255, 255, 0, 0, 0, 0]
            && channel.u32(parents[0] + 8)? == 0,
        "Native conditional channel has a procedure"
    );
    let dependencies = channel.array(row + 80, 4, Some(0x8080000B))?;
    ensure!(
        dependencies.len() == 1 && channel.u32(dependencies[0])? == 1,
        "Native conditional dependency differs"
    );
    for (offset, stride, class, byte) in [
        (0x40, 4, 0x8080000B, 0),
        (0x50, 16, 0x80800090, 0),
        (0x90, 16, 0x8080979C, 255),
        (0xA0, 4, 0x8080000B, 0),
    ] {
        let rows = channel.array(live + offset, stride, Some(class))?;
        ensure!(
            rows.len() == 1
                && channel.0[rows[0]..rows[0] + stride]
                    .iter()
                    .all(|&v| v == byte),
            "Native conditional channel initialization differs"
        );
    }
    ensure!(
        channel.u64(live + 0x60)? == 0
            && channel.u64(live + 0x70)? == 0
            && channel.u64(live + 0x80)? == 0,
        "Native conditional channel has local receivers"
    );
    let h = status.pointer(24)? + 0xE0;
    let expression = h + 0x98;
    let inputs = status.array(expression + 64, 40, Some(0x80809789))?;
    ensure!(
        inputs.len() == 1
            && status.u64(expression + 48)? == 2
            && status.u64(expression + 56)? == 1
            && status.u64(expression + 80)? == 0x811C9DC5
            && status.u64(expression + 88)? == 0,
        "Native conditional input metadata differs"
    );
    let input = inputs[0];
    ensure!(
        status.u32(input)? == status_tag
            && status.u32(input + 4)? == 0x80809788
            && status.u64(input + 16)? == 0
            && status.u64(input + 24)? == 0x808097C1,
        "Native conditional input is not a named channel"
    );
    let outputs = graph.array(32, 72, Some(0x80809BC9))?;
    let mut connected = 0;
    for at in outputs {
        if graph.u32(at + 40)? == u32::MAX {
            continue;
        }
        ensure!(
            graph.u64(at)? == 0x811C9DC5
                && graph.u32(at + 8)? == status_tag
                && graph.u32(at + 12)? == 0x80809789
                && graph.u64(at + 16)? == input as u64
                && graph.u64(at + 24)? == 0
                && graph.u64(at + 32)? == 0x811C9DC5
                && graph.u32(at + 40)? == channel_tag
                && graph.u32(at + 44)? == 0x808097C2
                && graph.u64(at + 48)? == (c + 0x60) as u64
                && graph.u64(at + 56)? == 1
                && graph.u64(at + 64)? == 0,
            "Native conditional connection differs"
        );
        connected += 1;
    }
    ensure!(connected == 1, "Native conditional channel is unconnected");
    let status_binding =
        weapon_component_bindings(&graph.0, 0x9E50D3EB).map_err(anyhow::Error::msg)?;
    let channel_binding =
        weapon_component_bindings(&graph.0, 0xE3F4E04C).map_err(anyhow::Error::msg)?;
    ensure!(
        status_binding.len() == 1
            && channel_binding.len() == 1
            && status_binding[0].owner_tag == status_tag
            && channel_binding[0].owner_tag == channel_tag
            && channel_binding[0].concrete_class == 0x8080979F,
        "Native conditional private bindings differ"
    );
    let mut patches = Vec::new();
    let mut replace = |p: &Payload,
                       binding: &sundial::package_authoring::entity::WeaponComponentBinding,
                       at: usize,
                       bytes: Vec<u8>|
     -> Result<()> {
        patches.push(NativeAssetResourcePatch {
            binding_hash: binding.binding_hash,
            resource_index: u16::try_from(binding.resource_index)?,
            offset: u32::try_from(
                at.checked_sub(usize::try_from(binding.resource_offset)?)
                    .context("Conditional field precedes instance")?,
            )?,
            expected: p
                .0
                .get(at..at + bytes.len())
                .context("Truncated conditional field")?
                .to_vec(),
            bytes,
            imported_particle: None,
        });
        Ok(())
    };
    replace(
        &channel,
        &channel_binding[0],
        row,
        companion.name.to_le_bytes().to_vec(),
    )?;
    // The carrier inherits a named namespace. The source inherits the sole
    // unscoped parent entry, so publish its inclusive lookup range as [0, 0].
    replace(
        &channel,
        &channel_binding[0],
        c + 0x128,
        0u32.to_le_bytes().to_vec(),
    )?;
    replace(
        &channel,
        &channel_binding[0],
        parents[0],
        [
            0x811C9DC5u32.to_le_bytes(),
            companion.name.to_le_bytes(),
            0u32.to_le_bytes(),
        ]
        .concat(),
    )?;
    replace(
        status,
        &status_binding[0],
        input + 32,
        companion.name.to_le_bytes().to_vec(),
    )?;
    // Append a typed code array. The owner clone keeps the existing one-input
    // allocation and event connection, while its definition can grow privately.
    let start = status.0.len();
    let mut bytes = vec![0; (start + 4).next_multiple_of(16) - start - 4];
    bytes.extend(0x80809FBDu32.to_le_bytes());
    let header = start + bytes.len();
    bytes.extend((companion.code.len() as u64).to_le_bytes());
    bytes.extend(0x80800009u32.to_le_bytes());
    bytes.extend([0; 4]);
    bytes.extend(&companion.code);
    replace(
        status,
        &status_binding[0],
        expression + 16,
        [
            (companion.code.len() as u64).to_le_bytes(),
            (i64::try_from(header)? - i64::try_from(expression + 24)?).to_le_bytes(),
        ]
        .concat(),
    )?;
    let append = NativeAssetResourceAppend {
        binding_hash: status_binding[0].binding_hash,
        resource_index: u16::try_from(status_binding[0].resource_index)?,
        expected_owner_size: u32::try_from(start)?,
        bytes,
    };
    Ok((patches, append))
}
