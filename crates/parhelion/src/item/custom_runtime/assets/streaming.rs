//! Stream the two leading mips of large private textures, as streamed stock textures do.
//!
//! A streamed Shadowkeep 2D texture keeps its header and mip tail resident and names a type 48:1
//! large buffer at header `+0x24` that holds the two leading mips. The header's byte count still
//! covers the whole chain, and its storage bytes `+0x18` and `+0x1C` are 2 and 1 instead of 0 and
//! 0. The large buffer's entry reference names the header. Stock loading indexes list the header
//! and the tail but never a large buffer, which the client streams on demand. Stock keeps little
//! pixel data loaded from boot: most of the texture data its investment index lists are tails.
//!
//! Imported model graphs and private runtime groups, such as imported effects and the models they
//! spawn, take the same split. A runtime group's entries are otherwise all loaded from boot, so
//! a large effect texture would stay whole in memory in every world.
use super::*;

/// The smallest top mip edge that streams. Smaller textures stay whole in their data entry.
const STREAMED_EDGE: u16 = 512;
/// Leading mips held by a large buffer, as every stock streamed texture with at least three has.
const STREAMED_MIPS: u8 = 2;
/// Large-buffer words of a resident header.
const UNSET: [u32; 3] = [0, u32::MAX, 0x811C_9DC5];
/// Header offset of the large-buffer tag.
const LARGE_BUFFER: usize = 0x24;

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
pub(in crate::item) fn leading_bytes(header: &[u8], data: usize) -> AuthoringResult<Option<usize>> {
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
        || !UNSET.contains(&read_u32(header, LARGE_BUFFER)?)
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

/// The large-buffer field of a streamed 2D header, as an (offset, tag) reference, or `None` for a
/// resident header or another payload.
pub(in crate::item) fn large_buffer_field(header: &[u8]) -> Option<(usize, u32)> {
    if header.len() != 40 || read_u16(header, 12).ok()? != 0xCAFE || header[28] != 1 {
        return None;
    }
    let tag = read_u32(header, LARGE_BUFFER).ok()?;
    (!UNSET.contains(&tag)).then_some((LARGE_BUFFER, tag))
}

/// The lowest stock type 48:1 tag, whose entry metadata every large buffer copies.
pub(in crate::item) fn large_template(manager: &PackageManager) -> AuthoringResult<u32> {
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

fn kind(manager: &PackageManager, template: u32) -> Option<(u8, u8)> {
    manager
        .get_entry(TagHash(template))
        .map(|entry| (entry.file_type, entry.file_subtype))
}

type Streamed<'a, M> = (std::borrow::Cow<'a, [Node<M>]>, BTreeSet<String>);

/// `nodes` with each large texture's two leading mips moved out of its data node into a new
/// large-buffer node, and the symbols of those buffers. Nothing that loads the group may list a
/// large buffer. A group with no texture to stream is returned as it is.
pub(super) fn split<'a, M: Clone + Default>(
    nodes: &'a [Node<M>],
    manager: &PackageManager,
) -> AuthoringResult<Streamed<'a, M>> {
    let positions: BTreeMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.symbol.as_str(), index))
        .collect();
    // A data node named by more than one node keeps its whole chain, since every header that
    // names it reads the same bytes.
    let mut uses = BTreeMap::<&str, usize>::new();
    for node in nodes {
        let patched = node.patches.iter().map(|(_, target)| target.as_str());
        for target in node.reference.as_deref().into_iter().chain(patched) {
            *uses.entry(target).or_default() += 1;
        }
    }
    let mut planned = Vec::new();
    for (index, header) in nodes.iter().enumerate() {
        if header.storage != crate::NewTagStorageMode::InheritTemplate
            || kind(manager, header.template) != Some((32, 1))
        {
            continue;
        }
        let Some(&data_index) = header
            .reference
            .as_deref()
            .and_then(|symbol| positions.get(symbol))
        else {
            continue;
        };
        let data = &nodes[data_index];
        if data.storage != crate::NewTagStorageMode::InheritTemplate
            || uses.get(data.symbol.as_str()).copied().unwrap_or(0) > 1
            || !data.patches.is_empty()
            || kind(manager, data.template) != Some((40, 1))
        {
            continue;
        }
        if let Some(leading) = leading_bytes(&header.payload, data.payload.len())? {
            planned.push((index, data_index, leading));
        }
    }
    if planned.is_empty() {
        return Ok((std::borrow::Cow::Borrowed(nodes), BTreeSet::new()));
    }
    let template = large_template(manager)?;
    let mut nodes = nodes.to_vec();
    let mut streamed = BTreeSet::new();
    for (index, data_index, leading) in planned {
        let symbol = format!("{}-streamed", nodes[data_index].symbol);
        if positions.contains_key(symbol.as_str()) || streamed.contains(&symbol) {
            return Err(invalid(format!(
                "Streamed texture symbol {symbol} is taken"
            )));
        }
        let payload = nodes[data_index].payload.drain(..leading).collect();
        let header = &mut nodes[index];
        header.payload[24] = 2;
        header.payload[28] = 1;
        // A placeholder the group's allocation fills with the buffer's tag.
        header.payload[LARGE_BUFFER..LARGE_BUFFER + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        header.patches.push((LARGE_BUFFER, symbol.clone()));
        let reference = Some(header.symbol.clone());
        nodes.push(Node {
            symbol: symbol.clone(),
            template,
            payload,
            reference,
            storage: crate::NewTagStorageMode::InheritTemplate,
            patches: Vec::new(),
            metadata: M::default(),
        });
        streamed.insert(symbol);
    }
    Ok((std::borrow::Cow::Owned(nodes), streamed))
}
