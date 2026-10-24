//! Retain source palette metadata. D1 material programs use their own source colors.
use super::*;

impl Assembly<'_> {
    pub(super) fn armor_dyes(
        &mut self,
        source: &[Value],
        donor: u32,
        item: u32,
    ) -> Result<(Vec<Value>, Vec<Value>)> {
        let tag = crate::tiger::shadowkeep::item_tag(&mut self.native, donor)?;
        let templates = crate::tiger::dye::inspect(&mut self.native, tag, false)?;
        let mut effective = BTreeMap::new();
        for dye in source {
            let channel = dye["channel"]
                .as_u64()
                .context("Source armor dye channel")?;
            ensure!(
                channel < 3,
                "Source armor dye channel is outside the armor bank"
            );
            effective.insert(channel, dye);
        }
        let mut dyes = Vec::new();
        let mut rows = Vec::new();
        for (channel, source) in effective {
            let template = templates
                .as_array()
                .context("Native armor dye templates")?
                .iter()
                .find(|row| row["channel"] == channel)
                .context("Armor donor dye channel is missing")?;
            let found = template["found"]
                .as_array()
                .filter(|rows| rows.len() == 1)
                .context("Armor donor dye template is ambiguous")?;
            let template = &found[0];
            let tag = |name: &str| -> Result<u32> {
                Ok(u32::from_str_radix(
                    template[name].as_str().context("Native dye resource")?,
                    16,
                )?)
            };
            let binding = self.globals.dyes[channel as usize];
            let original = self.native.tag(binding.tag, Some(0x808071F3))?;
            let mut scope = original.0.clone();
            let vectors = original.array(0x88, 16, Some(0x80800090))?;
            ensure!(
                vectors.len() == 27,
                "Native armor default dye vectors differ"
            );
            let mut constants = vectors
                .iter()
                .flat_map(|at| original.0[*at..*at + 16].iter().copied())
                .collect::<Vec<_>>();
            for (name, destinations) in [("primary", [9, 17]), ("secondary", [13, 21])] {
                let color = source[name]
                    .as_array()
                    .filter(|values| values.len() == 4)
                    .context("Source armor dye color")?;
                for row in destinations {
                    for (lane, value) in color.iter().enumerate() {
                        let value = value
                            .as_f64()
                            .context("Source armor dye color is not numeric")?
                            as f32;
                        ensure!(value.is_finite(), "Source armor dye color is not finite");
                        put(&mut constants, row * 16 + lane * 4, &value.to_le_bytes())?;
                    }
                }
            }
            for (index, at) in vectors.into_iter().enumerate() {
                put(&mut scope, at, &constants[index * 16..index * 16 + 16])?;
            }
            for descriptor in [0x40, 0x58, 0x68, 0x78] {
                put(&mut scope, descriptor, &[0; 16])?;
            }
            put(&mut scope, 0x10, &binding.index.to_le_bytes())?;
            put(&mut scope, 0xAC, &0u32.to_le_bytes())?;
            put(&mut scope, 0xB8, &binding.slot.to_le_bytes())?;
            put(&mut scope, 0xBC, &u32::MAX.to_le_bytes())?;
            let symbol = |name: &str| format!("dye-{channel}-{name}");
            let header = tag("buffer_header")?;
            let raw = self.native.reference(header)?;
            ensure!(
                self.native.tag(raw, None)?.0.len() == constants.len(),
                "Native dye buffer size differs"
            );
            self.node(
                &symbol("constants"),
                raw,
                &constants,
                Some(&symbol("buffer")),
                &[],
            )?;
            let header_data = self.native.tag(header, None)?.0.clone();
            self.node(
                &symbol("buffer"),
                header,
                &header_data,
                Some(&symbol("constants")),
                &[],
            )?;
            self.node(
                &symbol("scope"),
                binding.tag,
                &scope,
                None,
                &[(0xBC, symbol("buffer"))],
            )?;
            let definition_tag = tag("dye")?;
            let mut definition = self.native.tag(definition_tag, Some(0x808071CD))?.0.clone();
            put(&mut definition, 12, &u32::MAX.to_le_bytes())?;
            self.node(
                &symbol("definition"),
                definition_tag,
                &definition,
                None,
                &[(12, symbol("scope"))],
            )?;
            let parent_tag = tag("relation")?;
            let mut parent = self.native.tag(parent_tag, Some(0x8080744A))?.0.clone();
            put(&mut parent, 16, &u32::MAX.to_le_bytes())?;
            self.node(
                &symbol("parent"),
                parent_tag,
                &parent,
                None,
                &[(16, symbol("definition"))],
            )?;
            let companion = 0x81A662DE;
            let bytes = self.native.tag(companion, None)?.0.clone();
            self.node(&symbol("companion"), companion, &bytes, None, &[])?;
            let node = self.nodes.last_mut().context("Armor dye companion")?;
            node["shared_owner"] = json!(symbol("parent"));
            node["source_parent"] = json!(parent_tag);
            let manifest = format!("d1-roi-{item:08X}-dye-{channel}")
                .bytes()
                .fold(0x811C9DC5u32, |h, b| {
                    h.wrapping_mul(0x01000193) ^ u32::from(b)
                });
            rows.push(json!({"channel":channel,"dye":dyes.len()}));
            dyes.push(json!({"channel":channel,"manifest":manifest,"parent":symbol("parent")}));
        }
        Ok((dyes, rows))
    }
}
