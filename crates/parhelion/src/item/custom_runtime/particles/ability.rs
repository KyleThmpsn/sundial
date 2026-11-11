//! An entry's HUD changes a private copy of its prepared graph. The original group and other
//! entries keep their controllers, banks and presentation, even when they use the same root.
use super::*;
use sundial::package_authoring::{ability_hud, ability_modifier};

pub(in crate::item) fn symbol(root: &str, glyph: Option<u32>) -> String {
    glyph.map_or_else(
        || root.to_owned(),
        |glyph| format!("{root}-hud-{glyph:08X}"),
    )
}

impl ImportedParticles {
    fn hud_owner(&self, root: &Node, template: u32) -> AuthoringResult<usize> {
        let owners = root
            .patches
            .iter()
            .filter_map(|(_, name)| {
                self.nodes
                    .iter()
                    .position(|node| node.symbol == *name && node.template == template)
            })
            .collect::<BTreeSet<_>>();
        let mut owners = owners.into_iter();
        let owner = owners
            .next()
            .ok_or_else(|| invalid("Imported HUD component is not privately owned"))?;
        if owners.next().is_some() {
            return Err(invalid(
                "Imported HUD needs one privately owned component per binding",
            ));
        }
        Ok(owner)
    }

    fn hud_writes(
        &self,
        manager: &PackageManager,
        root_symbol: &str,
        keys: &[u32],
    ) -> AuthoringResult<BTreeMap<String, BTreeSet<usize>>> {
        let root = self
            .nodes
            .iter()
            .find(|node| node.symbol == root_symbol)
            .ok_or_else(|| invalid("Imported HUD root is absent"))?;
        let mut witness = root.payload.clone();
        for (offset, target) in &root.patches {
            let node = self
                .nodes
                .iter()
                .find(|node| node.symbol == *target)
                .ok_or_else(|| invalid("Imported HUD dependency is absent"))?;
            write_u32(&mut witness, *offset, node.template)?;
        }
        // Native templates qualify binding classes and offsets. Prepared bytes, not template
        // values, supply the actual controller pair and bank variant selection below.
        let site = ability_hud::glyph_site(manager, &witness)
            .map_err(invalid)?
            .ok_or_else(|| invalid("Imported ability has no qualified native HUD controller"))?;
        let controller = self.hud_owner(root, site.owner)?;
        let owner = &self.nodes[controller];
        if owner.reference.is_some() {
            return Err(invalid("Imported HUD controller must be privately owned"));
        }
        let definition = usize::try_from(read_u64(&owner.payload, site.instance + 8)?)
            .map_err(|_| invalid("Imported HUD definition exceeds the address space"))?;
        let template = manager
            .read_tag(TagHash(owner.template))
            .map_err(|error| invalid(error.to_string()))?;
        if site.instance < 4
            || definition < 4
            || read_u32(&owner.payload, site.instance - 4)?
                != read_u32(&template, site.instance - 4)?
            || read_u32(&owner.payload, definition - 4)?
                != read_u32(&template, site.definition - 4)?
            || read_u64(&owner.payload, definition + 8)? != site.instance as u64
        {
            return Err(invalid(
                "Imported HUD controller pair has an unsupported layout",
            ));
        }
        let current = read_u32(&owner.payload, site.instance + ability_hud::GLYPH_FIELD)?;
        if read_u32(&owner.payload, definition + ability_hud::GLYPH_FIELD)? != current
            || !ability_hud::glyph_keys(manager)
                .map_err(invalid)?
                .contains(&current)
        {
            return Err(invalid(
                "Imported HUD controller has no consistent native glyph",
            ));
        }
        let mut writes = BTreeMap::<String, BTreeSet<usize>>::new();
        let offsets = writes.entry(owner.symbol.clone()).or_default();
        offsets.extend([
            site.instance + ability_hud::GLYPH_FIELD,
            definition + ability_hud::GLYPH_FIELD,
        ]);
        let sealed = crate::tag_payload::relative_target(&owner.payload, 24)?;
        if sealed < 4 {
            return Err(invalid(
                "Imported HUD sealed definition has no class marker",
            ));
        }
        if sealed != definition
            && read_u32(&owner.payload, sealed - 4)? == read_u32(&owner.payload, definition - 4)?
        {
            if read_u32(&owner.payload, sealed + ability_hud::GLYPH_FIELD)? != current {
                return Err(invalid(
                    "Imported HUD sealed definition has a different glyph",
                ));
            }
            offsets.insert(sealed + ability_hud::GLYPH_FIELD);
        }
        if let Some(bank) = ability_modifier::entity_bank(&witness).map_err(invalid)? {
            let bank = &self.nodes[self.hud_owner(root, bank)?];
            if bank.reference.is_some() {
                return Err(invalid("Imported HUD bank must be privately owned"));
            }
            for variant in ability_hud::variant_glyphs(&bank.payload, keys).map_err(invalid)? {
                writes
                    .entry(bank.symbol.clone())
                    .or_default()
                    .insert(variant.offset);
            }
        }
        Ok(writes)
    }

    fn hud_copies(
        &self,
        root_symbol: &str,
        writes: &BTreeMap<String, BTreeSet<usize>>,
        glyph: u32,
    ) -> AuthoringResult<(BTreeMap<String, String>, Vec<Node>)> {
        let mut reachable = BTreeSet::new();
        let mut pending = vec![root_symbol.to_owned()];
        while let Some(name) = pending.pop() {
            if !reachable.insert(name.clone()) {
                continue;
            }
            let node = self
                .nodes
                .iter()
                .find(|node| node.symbol == name)
                .ok_or_else(|| invalid("Imported HUD closure is incomplete"))?;
            pending.extend(node.patches.iter().map(|(_, target)| target.clone()));
            pending.extend(node.reference.iter().cloned());
        }
        // Linked audio banks have namespace-wide identities. Keep immutable bank and media nodes
        // shared, including metadata-only media references, while copies of cue owners bind normally.
        let names = self
            .nodes
            .iter()
            .filter(|node| reachable.contains(&node.symbol))
            .filter(|node| {
                !matches!(
                    node.storage,
                    crate::NewTagStorageMode::AudioBank | crate::NewTagStorageMode::AudioMedia
                )
            })
            .map(|node| (node.symbol.clone(), symbol(&node.symbol, Some(glyph))))
            .collect::<BTreeMap<_, _>>();
        if names
            .values()
            .any(|name| self.nodes.iter().any(|node| node.symbol == *name))
        {
            return Err(invalid("Imported HUD copy duplicates an existing symbol"));
        }
        let mut copies = Vec::new();
        for source in self
            .nodes
            .iter()
            .filter(|node| names.contains_key(&node.symbol))
        {
            let mut copy = source.clone();
            if let Some(offsets) = writes.get(&source.symbol) {
                for &offset in offsets {
                    if source
                        .patches
                        .iter()
                        .any(|(at, _)| *at < offset + 4 && at.saturating_add(4) > offset)
                    {
                        return Err(invalid("Imported HUD glyph overlaps a resource reference"));
                    }
                    write_u32(&mut copy.payload, offset, glyph)?;
                }
            }
            copy.symbol = names[&source.symbol].clone();
            for (_, target) in &mut copy.patches {
                if let Some(name) = names.get(target) {
                    *target = name.clone();
                }
            }
            copy.reference = copy
                .reference
                .map(|name| names.get(&name).cloned().unwrap_or(name));
            copies.push(copy);
        }
        Ok((names, copies))
    }

    pub(in crate::item) fn ability_hud(
        &mut self,
        manager: &PackageManager,
        root_symbol: &str,
        keys: &[u32],
        glyph: u32,
    ) -> AuthoringResult<()> {
        let writes = self.hud_writes(manager, root_symbol, keys)?;
        let (names, copies) = self.hud_copies(root_symbol, &writes, glyph)?;
        let statuses = self
            .hud
            .iter()
            .filter_map(|binding| binding.remap(&names))
            .collect::<Vec<_>>();
        self.nodes.extend(copies);
        self.hud.extend(statuses);
        self.attachments.insert(symbol(root_symbol, Some(glyph)));
        Ok(())
    }
}
