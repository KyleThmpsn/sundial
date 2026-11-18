//! Artifact identity uses its own source equipment slot, independent of armor appearances.
use super::*;

#[derive(Clone, Debug, Serialize)]
pub struct Artifact {
    pub hash: u32,
    pub name: String,
    pub kind: String,
    pub class_type: u8,
    pub slot: u8,
    pub rarity: u8,
    pub definition: u32,
    pub strings: u32,
}

impl Source {
    pub fn artifacts(&self) -> Result<Vec<Artifact>> {
        if let Some(items) = self.artifacts.get() {
            return Ok(items.clone());
        }
        ensure!(
            self.report
                .selected
                .iter()
                .all(|package| package.platform == 8),
            "Only the Xbox One build of Rise of Iron is supported"
        );
        let (_, definitions) = self.table(0x80801933)?;
        let (_, strings) = self.table(0x808034C7)?;
        let rows = definitions.array(8, 24, Some(0x808016BD))?;
        let labels = strings.array(8, 16, Some(0x808033E8))?;
        ensure!(
            rows.len() == labels.len(),
            "Item and string ordinals disagree"
        );
        let mut text = super::localization::Labels::new(self)?;
        let mut result = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for (row, label) in rows.into_iter().zip(labels) {
            let hash = definitions.u32(row)?;
            let strings = strings.u32(label + 8)?;
            let display = self.read(strings)?;
            ensure!(
                self.reference(strings)? == 0x80803484,
                "Unexpected item string class"
            );
            // Artifact strings use a generic display class. Their localized type reference
            // distinguishes class-specific artifacts without relying on an item's hash or name.
            let class_type = match display.u32(0x84)? {
                0xAAE6EE71 => 0,
                0x45683BAF => 1,
                0xCAEDE0DC => 2,
                0x19EC2C8B => 3,
                _ => continue,
            };
            let definition = definitions.u32(row + 16)?;
            let data = self.read(definition)?;
            ensure!(
                self.reference(definition)? == 0x80801806 && data.u32(0x78)? == hash,
                "Source definition identity mismatch for {hash:08X}"
            );
            if data.u64(0x18)? == 0 {
                continue;
            }
            let client = data.pointer(0x18)?;
            ensure!(
                client >= 4 && data.u32(client - 4)? == 0x80801706,
                "Unexpected item client record"
            );
            ensure!(
                data.u8(client + 0x18)? == 16,
                "Artifact type and source equipment slot disagree"
            );
            ensure!(seen.insert(hash), "Duplicate artifact item {hash:08X}");
            result.push(Artifact {
                hash,
                name: text
                    .optional_text(self, &display, 0x78)?
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| format!("Unnamed Artifact ({hash:08X})")),
                kind: text.text(self, &display, 0x80)?.trim().to_owned(),
                class_type,
                slot: 16,
                rarity: data.u8(0x8A)?,
                definition,
                strings,
            });
        }
        result.sort_by(|a, b| (a.name.as_str(), a.hash).cmp(&(b.name.as_str(), b.hash)));
        self.unchanged()?;
        let _ = self.artifacts.set(result.clone());
        Ok(result)
    }
}
