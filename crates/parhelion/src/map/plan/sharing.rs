//! Prove which allocated texture slots are inactive aliases from the final linked bytes.
use super::*;

struct Linked<'a> {
    tags: &'a [NewTagSpec],
    peers: Vec<Option<usize>>,
    kinds: Vec<Option<(u8, u8)>>,
}

impl Linked<'_> {
    fn alias(&self, index: usize) -> Option<usize> {
        let peer = self.peers[index]?;
        let canonical = self.peers[peer]?;
        if canonical == index
            || self.peers[canonical] != Some(peer)
            || self.kinds[canonical] != self.kinds[index]
            || self.tags[index].template_tag != self.tags[canonical].template_tag
            || self.tags[index].payload != self.tags[canonical].payload
        {
            return None;
        }
        let (header, data) = match (self.kinds[index], self.kinds[peer]) {
            (Some((32, 1)), Some((40, 1))) => (canonical, peer),
            (Some((40, 1)), Some((32, 1))) => (peer, canonical),
            _ => return None,
        };
        let bytes = &self.tags[header].payload;
        if bytes.len() != 40
            || read_u32(bytes, 36).ok()? != u32::MAX
            || read_u32(bytes, 0).ok()? as usize != self.tags[data].payload.len()
        {
            return None;
        }
        Some(canonical)
    }
}

pub(super) fn inactive(
    addresses: &[TagHash],
    tags: &[NewTagSpec],
    references: &[crate::NewTagReferenceOverride],
    manager: &PackageManager,
) -> AuthoringResult<BTreeSet<TagHash>> {
    let by_tag = addresses
        .iter()
        .enumerate()
        .map(|(index, &tag)| (tag, index))
        .collect::<BTreeMap<_, _>>();
    if by_tag.len() != addresses.len() || addresses.len() != tags.len() {
        return Err(invalid("Map texture allocation is inconsistent"));
    }
    let modes = crate::extend::resolve_reference_modes(tags.len(), references)?;
    let peers = modes
        .iter()
        .map(|mode| match mode {
            crate::NewTagReference::Tag(tag) => by_tag.get(tag).copied(),
            crate::NewTagReference::Appended(index) => Some(*index),
            crate::NewTagReference::Template => None,
        })
        .collect();
    let kinds = tags
        .iter()
        .map(|spec| {
            if spec.storage != crate::NewTagStorageMode::InheritTemplate {
                return None;
            }
            manager
                .get_entry(spec.template_tag)
                .map(|entry| (entry.file_type, entry.file_subtype))
        })
        .collect();
    let linked = Linked { tags, peers, kinds };
    Ok((0..tags.len())
        .filter(|&index| linked.alias(index).is_some())
        .map(|index| addresses[index])
        .collect())
}
