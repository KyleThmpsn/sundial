//! Stream the two largest mips of large imported textures, as streamed stock textures do.
//!
//! A streamed Shadowkeep 2D texture keeps its header and mip tail resident and names a type 48:1
//! large buffer at header `+0x24` that holds the two leading mips. The header's byte count still
//! covers the whole chain, and its storage bytes `+0x18` and `+0x1C` are 2 and 1 instead of 0 and
//! 0. The large buffer's entry reference names the header. Stock loading indexes list the header
//! and the tail but never a large buffer, which the client streams on demand.
use super::*;

/// The smallest top mip edge that streams. Smaller textures stay whole in their data entry.
const STREAMED_EDGE: u16 = 512;
/// Leading mips held by a large buffer, as every stock streamed texture with at least three has.
const STREAMED_MIPS: u8 = 2;
const UNSET: [u32; 3] = [0, u32::MAX, 0x811C_9DC5];

/// Bytes in one mip surface, or `None` for a format this split does not size.
fn surface(format: u32, width: u32, height: u32) -> Option<usize> {
    let (width, height) = (width.max(1) as usize, height.max(1) as usize);
    let blocks = width.div_ceil(4) * height.div_ceil(4);
    Some(match format {
        70..=72 | 79..=81 => blocks * 8,
        73..=78 | 82..=84 | 94..=99 => blocks * 16,
        61 => width * height,
        28 | 29 | 87 | 88 | 91 | 93 => width * height * 4,
        10 => width * height * 8,
        2 => width * height * 16,
        _ => return None,
    })
}

/// The leading-mip byte count of a resident 2D header that should stream, after checking that
/// the header's byte count and its data node's length both equal the whole mip chain.
fn leading_bytes(header: &[u8], data: usize) -> AuthoringResult<Option<usize>> {
    if header.len() != 40 || read_u16(header, 12)? != 0xCAFE {
        return Ok(None);
    }
    let width = read_u16(header, 14)?;
    let height = read_u16(header, 16)?;
    let mips = header[23];
    if read_u16(header, 18)? != 1
        || read_u16(header, 20)? != 1
        || mips <= STREAMED_MIPS
        || width.max(height) < STREAMED_EDGE
        || header[24] != 0
        || header[28] != 0
        || !UNSET.contains(&read_u32(header, 36)?)
    {
        return Ok(None);
    }
    let format = read_u32(header, 4)?;
    let mut chain = 0;
    let mut leading = 0;
    for mip in 0..mips {
        let Some(size) = surface(format, u32::from(width) >> mip, u32::from(height) >> mip) else {
            return Ok(None);
        };
        chain += size;
        if mip < STREAMED_MIPS {
            leading += size;
        }
    }
    if chain != data || read_u32(header, 0)? as usize != chain {
        return Ok(None);
    }
    Ok(Some(leading))
}

/// The lowest stock type 48:1 tag, whose entry metadata every large buffer copies.
fn large_template(manager: &sundial::package_authoring::PackageManager) -> AuthoringResult<u32> {
    manager
        .lookup
        .tag32_entries_by_pkg
        .iter()
        .flat_map(|(&package, entries)| {
            entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.file_type == 48 && entry.file_subtype == 1)
                .map(move |(index, _)| TagHash::new(package, index as u16).0)
        })
        .min()
        .ok_or_else(|| invalid("The installed packages have no large texture buffer template"))
}

/// Moves each large texture's two leading mips out of its data node into a new streamed large
/// buffer node, and points its header at that buffer.
pub(super) fn split(
    nodes: &mut Vec<linking::Node>,
    manager: &sundial::package_authoring::PackageManager,
) -> AuthoringResult<()> {
    let positions: BTreeMap<String, usize> = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.symbol.clone(), index))
        .collect();
    // A data node named by more than one node keeps its whole chain, since every header that
    // names it reads the same bytes.
    let mut uses = BTreeMap::<&str, usize>::new();
    for node in nodes.iter() {
        let patched = node.patches.iter().map(|(_, target)| target.as_str());
        for target in node.reference.as_deref().into_iter().chain(patched) {
            *uses.entry(target).or_default() += 1;
        }
    }
    let shared: BTreeSet<String> = uses
        .into_iter()
        .filter(|&(_, count)| count > 1)
        .map(|(symbol, _)| symbol.to_owned())
        .collect();
    let mut template = None;
    let mut buffers = Vec::new();
    for index in 0..nodes.len() {
        let header = &nodes[index];
        if header.storage != crate::NewTagStorageMode::InheritTemplate
            || manager
                .get_entry(header.template)
                .is_none_or(|entry| entry.file_type != 32 || entry.file_subtype != 1)
        {
            continue;
        }
        let Some(&data_index) = header.reference.as_deref().and_then(|s| positions.get(s)) else {
            continue;
        };
        let data = &nodes[data_index];
        if data.storage != crate::NewTagStorageMode::InheritTemplate
            || shared.contains(&data.symbol)
            || !data.patches.is_empty()
            || manager
                .get_entry(data.template)
                .is_none_or(|entry| entry.file_type != 40 || entry.file_subtype != 1)
        {
            continue;
        }
        let Some(leading) = leading_bytes(&header.payload, data.payload.len())? else {
            continue;
        };
        let symbol = format!("{}-streamed", data.symbol);
        if positions.contains_key(&symbol) {
            return Err(invalid(format!(
                "Streamed texture symbol {symbol} is taken"
            )));
        }
        let template = match template {
            Some(tag) => tag,
            None => *template.insert(large_template(manager)?),
        };
        let header_symbol = header.symbol.clone();
        let mut buffer = linking::Node::new(
            symbol.clone(),
            template,
            nodes[data_index].payload.drain(..leading).collect(),
        );
        buffer.reference = Some(header_symbol);
        buffer.streamed = true;
        buffers.push(buffer);
        let header = &mut nodes[index];
        header.payload[24] = 2;
        header.payload[28] = 1;
        header.patch(36, symbol)?;
    }
    nodes.extend(buffers);
    Ok(())
}
