use super::*;

impl Reader<'_> {
    pub(super) fn controller(&mut self) -> Result<(), String> {
        let root = self.root.owner_offset as usize;
        self.root_pair()?;
        if !crate::runtime::native_type_inherits(u32_at(self.data, root + 4)?, 0x8080_3BB7) {
            return Err("Numeric inputs require a paired ability controller".into());
        }
        for (input, property) in [
            (0, Property::RechargeScale),
            (1, Property::ActivationCostScale),
            (2, Property::ActiveEnergyScale),
            (4, Property::ActivationLockout),
        ] {
            self.numeric_input(root, input, property)?;
        }
        if matches!(self.root.schema, 0x8080_4404 | 0x8080_41B2) {
            // The selected-profile interface's row 5 must still dispatch the query factor.
            let descriptor = root + 0x7C0;
            if relative_offset(descriptor, 0, i64_at(self.data, descriptor)?)? != root {
                return Err("Targeting profile descriptor selects another controller".into());
            }
            let tag = u32_at(self.data, descriptor + 8)?;
            let interface = self
                .manager
                .read_tag(TagHash(tag))
                .map_err(|error| error.to_string())?;
            if u64_at(&interface, 0)? != interface.len() as u64
                || u32_at(&interface, 8)? != 0x8080_4457
                || u32_at(&interface, 12)? != 0x8080_4573
            {
                return Err("Targeting profile has an incompatible interface".into());
            }
            let (count, _, rows, class) = native_array_at(&interface, 16)?;
            rows_fit(&interface, rows, count, 24)?;
            if count != 6
                || class != 0x8080_9C56
                || u32_at(&interface, rows + 5 * 24)? != 0x8080_4403
                || u32_at(&interface, rows + 5 * 24 + 4)? != 20
            {
                return Err("Targeting profile does not dispatch the verified query".into());
            }
            self.numeric_input(root, 6, Property::TargetingRange)?;
        }
        Ok(())
    }

    fn numeric_input(
        &mut self,
        root: usize,
        input: usize,
        property: Property,
    ) -> Result<(), String> {
        let record = root + 0x338 + input * 32;
        bytes_at::<32>(self.data, record)?;
        let relative = i64_at(self.data, record)?;
        let (body, offset, codec) = if relative == 0 {
            (record, 12, Codec::Float)
        } else {
            let literal = relative_offset(record, 0, relative)?;
            // F3F8F0 selects the referenced literal before the fallback. The type byte is
            // native numeric-source storage, unrelated to the encoded network codec.
            match bytes_at::<1>(self.data, literal)?[0] {
                0 => (literal, 4, Codec::Float),
                1 => (literal, 4, Codec::Integer),
                2 => (literal, 1, Codec::Byte),
                // Other source kinds are evaluated at runtime. Their fallback is not the
                // effective value and must not become an editable literal.
                _ => return Ok(()),
            }
        };
        self.put(
            property,
            self.root.schema,
            body,
            offset,
            &[input as u32],
            codec,
        )
    }

    pub(super) fn health(&mut self) -> Result<(), String> {
        let root = self.root.owner_offset as usize;
        let source = self.pair(root, 0x8080_4B8A, 0x8080_4BEE)?;
        let regions = self.array(root + 0x368, 0x8080_4C5F)?;
        let states = self.array(source + 0x340, 0x8080_4C5E)?;
        if regions.len() != states.len() {
            return Err("Health region arrays disagree".into());
        }
        let mut used = std::collections::BTreeSet::new();
        for region in self.array(root + 0x1B0, 0x8080_4C11)? {
            let group = u32_at(self.data, region + 0xD0)?;
            if group == u32::MAX {
                continue;
            }
            if group as usize >= regions.len() {
                return Err("Damage region selects a missing health group".into());
            }
            used.insert(group as usize);
        }
        for (index, (region, state)) in regions.into_iter().zip(states).enumerate() {
            if self.pair(region, 0x8080_4C5F, 0x8080_4C5E)? != state {
                return Err("Health region selects a different source row".into());
            }
            if used.contains(&index) {
                self.put(
                    Property::StartingHealth,
                    0x8080_4C5F,
                    region,
                    0x18,
                    &[index as u32],
                    Codec::Float,
                )?;
            }
        }
        Ok(())
    }

    pub(super) fn tracking(&mut self) -> Result<(), String> {
        let root = self.root.owner_offset as usize;
        self.root_pair()?;
        if let Some(speed) = self.optional(root + 0x158, 0x8080_3775)? {
            self.pair(speed, 0x8080_3775, 0x8080_3774)?;
            self.put(
                Property::TrackingSpeedChange,
                0x8080_3775,
                speed,
                0x78,
                &[],
                Codec::Float,
            )?;
        }
        Ok(())
    }

    pub(super) fn spawner(&mut self) -> Result<(), String> {
        let root = self.root.owner_offset as usize;
        let source = self.pair(root, 0x8080_37A9, 0x8080_37A1)?;
        let definition = self.optional(root + 0x160, 0x8080_37A8)?;
        let state = self.optional(source + 0xB0, 0x8080_37A7)?;
        let (definition, state) = match (definition, state) {
            (None, None) => return Ok(()),
            (Some(definition), Some(state)) => (definition, state),
            _ => return Err("Spawn settings and source disagree about the optional rows".into()),
        };
        if self.pair(definition, 0x8080_37A8, 0x8080_37A7)? != state {
            return Err("Spawn settings select another source".into());
        }
        for (index, row) in self
            .array(definition + 0x70, 0x8080_37D8)?
            .into_iter()
            .enumerate()
        {
            if !self.seconds(row + 0x68)? {
                continue;
            }
            for (offset, property) in [
                (0x68, Property::MinimumSpawnDelay),
                (0x6C, Property::MaximumSpawnDelay),
            ] {
                self.put(
                    property,
                    0x8080_37D8,
                    row,
                    offset,
                    &[index as u32],
                    Codec::Float,
                )?;
            }
        }
        Ok(())
    }

    pub(super) fn status(&mut self) -> Result<(), String> {
        let root = self.root.owner_offset as usize;
        self.root_pair()?;
        let sequence = root + 0x48;
        self.pair(sequence, 0x8080_3A61, 0x8080_3A60)?;
        if bytes_at::<1>(self.data, sequence + 0x10)?[0] > 15 {
            return Ok(());
        }
        self.put(
            Property::EffectGroup,
            0x8080_3A61,
            sequence,
            0x10,
            &[],
            Codec::UnsignedByte,
        )?;
        // The editor marks priority inactive until the current draft selects a conflict group.
        self.put(
            Property::EffectPriority,
            0x8080_3A61,
            sequence,
            0x14,
            &[],
            Codec::Float,
        )
    }
}
