//! One private identity space for an effect's start, stop and transition banks.
use super::*;

#[derive(PartialEq, Eq)]
struct Definition<'a> {
    kind: u8,
    body: &'a [u8],
    references: Vec<(usize, Kind, u32)>,
}

/// A bank's top-level objects, and each identity it owns inside one of them, by the object that
/// holds it.
type Definitions<'a> = (BTreeMap<u32, Definition<'a>>, BTreeMap<u32, u32>);

/// The Wwise 113 hierarchy type of a layer container, the one object that holds identities of
/// its own, its layer controllers.
const LAYER_CONTAINER: u8 = 9;

/// Banks have already been normalized to Wwise 113. Compare object-local bytes
/// and typed relocations, excluding unrelated bank headers and physical offsets.
fn definitions(bank: &Bank) -> Result<Definitions<'_>> {
    let mut chunks = Read {
        bytes: &bank.bytes,
        at: 0,
    };
    let mut hierarchy = None;
    let mut header = false;
    while chunks.at < chunks.bytes.len() {
        let kind = chunks.take(4)?;
        let count = chunks.u32()? as usize;
        let base = chunks.at;
        let body = chunks.take(count)?;
        match kind {
            b"BKHD" => {
                let mut reader = Read { bytes: body, at: 0 };
                ensure!(
                    !header && reader.u32()? == 113 && reader.u32()? == bank.bank_id,
                    "linked audio bank header differs"
                );
                header = true;
            }
            b"HIRC" => ensure!(
                hierarchy.replace((base, body)).is_none(),
                "duplicate linked hierarchy"
            ),
            _ => {}
        }
    }
    ensure!(header, "linked audio bank header missing");
    let (base, bytes) = hierarchy.context("linked audio hierarchy missing")?;
    let mut reader = Read { bytes, at: 0 };
    let count = reader.count()?;
    let mut result = BTreeMap::new();
    for _ in 0..count {
        let kind = reader.u8()?;
        let size = reader.u32()? as usize;
        let offset = base + reader.at;
        let body = reader.take(size)?;
        let id = u32::from_le_bytes(
            body.get(..4)
                .context("linked object identity")?
                .try_into()?,
        );
        let mut references = Vec::new();
        for reference in bank
            .refs
            .iter()
            .filter(|r| (offset..offset + size).contains(&r.offset))
        {
            let at = reference.offset - offset;
            ensure!(
                body.get(at..at + 4) == Some(reference.value.to_le_bytes().as_slice()),
                "linked audio object relocation differs from its bytes"
            );
            references.push((at, reference.kind, reference.value));
        }
        references.sort_by_key(|r| r.0);
        ensure!(
            references.windows(2).all(|w| w[0].0 + 4 <= w[1].0),
            "linked audio object relocations overlap"
        );
        ensure!(
            result
                .insert(
                    id,
                    Definition {
                        kind,
                        body,
                        references
                    }
                )
                .is_none(),
            "duplicate linked audio object {id:08X}"
        );
    }
    reader.end()?;
    let top = result.keys().copied().collect::<BTreeSet<_>>();
    ensure!(
        top.is_subset(&bank.objects),
        "linked audio object inventory differs from its hierarchy"
    );
    // A layer controller's identity lives in its layer container's body, relocated there as an
    // object reference, rather than as an object of its own. Every other identity the bank
    // owns is a top-level object.
    let mut embedded = BTreeMap::new();
    for &id in bank.objects.difference(&top) {
        let owners = result
            .iter()
            .filter(|(_, definition)| {
                definition
                    .references
                    .iter()
                    .any(|(_, kind, value)| matches!(kind, Kind::Object) && *value == id)
            })
            .map(|(owner, definition)| (*owner, definition.kind))
            .collect::<Vec<_>>();
        ensure!(
            matches!(owners.as_slice(), [(_, LAYER_CONTAINER)]),
            "linked audio object {id:08X} is neither a hierarchy object nor held by one layer container"
        );
        embedded.insert(id, owners[0].0);
    }
    Ok((result, embedded))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Namespace {
    objects: BTreeMap<u32, u32>,
    banks: BTreeMap<u32, u32>,
}

impl Namespace {
    /// Preserve cross-bank references while keeping native global objects shared.
    /// Supply the complete bank closure, including banks that contain only actions.
    pub fn new(name: &str, banks: &[Bank]) -> Result<Self> {
        ensure!(
            !name.is_empty() && !banks.is_empty(),
            "empty audio namespace"
        );
        let mut shared = BTreeMap::new();
        // An identity held inside an object is shared only with the same holder, which the
        // holder's own comparison then covers.
        let mut held = BTreeMap::new();
        for bank in banks {
            let (objects, embedded) = definitions(bank)?;
            for (id, definition) in objects {
                if let Some((owner, existing)) = shared.get(&id) {
                    ensure!(
                        *existing == definition,
                        "conflicting linked audio object {id:08X} in banks {owner:08X} and {:08X}",
                        bank.bank_id
                    );
                } else {
                    shared.insert(id, (bank.bank_id, definition));
                }
            }
            for (id, holder) in embedded {
                if let Some(&(first, existing)) = held.get(&id) {
                    ensure!(
                        existing == holder,
                        "conflicting linked audio object {id:08X} in banks {first:08X} and {:08X}",
                        bank.bank_id
                    );
                } else {
                    held.insert(id, (bank.bank_id, holder));
                }
            }
        }
        // A top-level object in one bank cannot also be held inside another's object.
        for (id, (owner, _)) in &shared {
            let held_by = held.get(id).map(|&(bank, _)| bank);
            ensure!(
                held_by.is_none(),
                "conflicting linked audio object {id:08X} in banks {owner:08X} and {:08X}",
                held_by.unwrap_or_default()
            );
        }
        let owned = banks
            .iter()
            .flat_map(|bank| bank.objects.iter().copied())
            .collect::<BTreeSet<_>>();
        let mut unique = banks
            .iter()
            .flat_map(|bank| &bank.refs)
            .filter(|reference| {
                matches!(reference.kind, Kind::Object) && !owned.contains(&reference.value)
            })
            .map(|reference| reference.value)
            .collect::<BTreeSet<_>>();
        unique.insert(0);
        let mut result = Self {
            objects: BTreeMap::new(),
            banks: BTreeMap::new(),
        };
        for id in owned {
            let private = hash(&format!("{name}/object/{id:08x}"));
            ensure!(
                unique.insert(private),
                "linked audio object identity collision"
            );
            result.objects.insert(id, private);
        }
        for bank in banks {
            ensure!(
                !result.banks.contains_key(&bank.bank_id),
                "duplicate linked audio bank"
            );
            let private = hash(&format!("{name}/bank/{:08x}", bank.bank_id));
            ensure!(
                unique.insert(private),
                "linked audio bank identity collision"
            );
            result.banks.insert(bank.bank_id, private);
        }
        for reference in banks.iter().flat_map(|bank| &bank.refs) {
            if matches!(reference.kind, Kind::Bank) {
                ensure!(
                    result.banks.contains_key(&reference.value),
                    "linked audio bank dependency {:08X} is absent",
                    reference.value
                );
            }
        }
        Ok(result)
    }

    pub fn object(&self, source: u32) -> Result<u32> {
        self.objects
            .get(&source)
            .copied()
            .context("object is outside the audio namespace")
    }

    pub fn bank(&self, source: u32) -> Result<u32> {
        self.banks
            .get(&source)
            .copied()
            .context("bank is outside the audio namespace")
    }

    /// Media IDs may be shared across banks. Their mapping and byte sizes come
    /// from the complete converted media closure, not from a donor sound.
    pub fn instantiate(
        &self,
        bank: &Bank,
        media: &BTreeMap<u32, u32>,
        sizes: &BTreeMap<u32, u32>,
    ) -> Result<Vec<u8>> {
        self.bank(bank.bank_id)?;
        for id in &bank.objects {
            self.object(*id)?;
        }
        let mut bytes = bank.bytes.clone();
        for reference in &bank.refs {
            let value = match reference.kind {
                Kind::Object => self
                    .objects
                    .get(&reference.value)
                    .copied()
                    .unwrap_or(reference.value),
                Kind::Bank => self.bank(reference.value)?,
                Kind::Media => *media
                    .get(&reference.value)
                    .context("linked medium was not converted")?,
                Kind::MediaSize => *sizes
                    .get(&reference.value)
                    .context("linked medium size is missing")?,
            };
            bytes
                .get_mut(
                    reference.offset
                        ..reference
                            .offset
                            .checked_add(4)
                            .context("audio relocation overflow")?,
                )
                .context("linked audio relocation is outside bank")?
                .copy_from_slice(&value.to_le_bytes());
        }
        Ok(bytes)
    }
}
