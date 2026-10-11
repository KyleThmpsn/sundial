//! One private identity space for an effect's start, stop and transition banks.
use super::*;

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
