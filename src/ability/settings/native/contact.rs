use super::*;

impl Reader<'_> {
    pub(super) fn projectile(&mut self) -> Result<(), String> {
        let root = self.root.owner_offset as usize;
        self.pair(root, 0x8080_388F, 0x8080_3B73)?;
        self.put(
            Property::LaunchSpeedWeight,
            0x8080_388F,
            root,
            0x8C,
            &[],
            Codec::Float,
        )?;
        self.put(
            Property::CollisionLimit,
            0x8080_388F,
            root,
            0x7A,
            &[],
            Codec::Byte,
        )?;
        for (group_index, group) in self
            .array(root + 0x218, 0x8080_37CD)?
            .into_iter()
            .enumerate()
        {
            self.pair(group, 0x8080_37CD, 0x8080_37CC)?;
            for (node_index, node) in self
                .array(group + 0x48, 0x8080_37D0)?
                .into_iter()
                .enumerate()
            {
                self.pair(node, 0x8080_37D0, 0x8080_37CF)?;
                for (action_index, action) in self
                    .array(node + 0x178, 0x8080_37F7)?
                    .into_iter()
                    .enumerate()
                {
                    let selector = bytes_at::<1>(self.data, action + 8)?[0];
                    if !matches!(selector, 0 | 1 | 2 | 6 | 17) {
                        continue;
                    }
                    let relative = i64_at(self.data, action)?;
                    if relative == 0 {
                        continue;
                    }
                    let body = relative_offset(action, 0, relative)?;
                    if body < 4 {
                        return Err("Contact action has no parameter marker".into());
                    }
                    let class = u32_at(self.data, body - 4)?;
                    self.extent(body, class)?;
                    let route = [group_index as u32, node_index as u32, action_index as u32];
                    match (selector, class) {
                        (0, 0x8080_37E1) => self.put(
                            Property::CleanupOnContact,
                            class,
                            body,
                            0,
                            &route,
                            Codec::UnsignedByte,
                        )?,
                        (1, 0x8080_37E2) => self.put(
                            Property::SurfacePlacement,
                            class,
                            body,
                            8,
                            &route,
                            Codec::UnsignedByte,
                        )?,
                        (2 | 17, _) if crate::runtime::native_type_inherits(class, 0x8080_37EA) => {
                            for (offset, property, codec) in [
                                (0, Property::BounceCountIncrement, Codec::Integer),
                                (4, Property::BounceAngleVariation, Codec::Float),
                                (8, Property::BounceSpeedVariation, Codec::Float),
                                (0x10, Property::BounceSurfaceRadius, Codec::Float),
                            ] {
                                self.put(property, class, body, offset, &route, codec)?;
                            }
                        }
                        (6, 0x8080_37DD) => {
                            if !self.seconds(body + 12)? {
                                continue;
                            }
                            for (offset, property) in [
                                (0, Property::ContactSpeed),
                                (4, Property::ContactFinalSpeed),
                                (8, Property::ContactFinalGravity),
                                (12, Property::ContactCurveStart),
                                (16, Property::ContactCurveEnd),
                            ] {
                                self.put(property, class, body, offset, &route, Codec::Float)?;
                            }
                        }
                        // Other legitimate actions retain their own parameter classes.
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }
}
