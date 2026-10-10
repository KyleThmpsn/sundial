//! Permanent controls applied to the final private Barrel, before projectile capacity fitting.
pub use sundial::package_authoring::entity::spread::{Pattern, Ring};

#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pellets: Option<u16>,
    /// Multiplier relative to the final Barrel's native spread scale, with 1 as unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spread_scale_bits: Option<u32>,
    /// Absolute ring geometry. None keeps the selected Barrel's shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rings: Option<Vec<Ring>>,
}

impl Edits {
    pub fn is_empty(&self) -> bool {
        self.pellets.is_none() && self.spread_scale_bits.is_none() && self.rings.is_none()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self
            .pellets
            .is_some_and(|count| !(1..=0x7FFF).contains(&count))
        {
            return Err("Pellets per shot must be between 1 and 32767".into());
        }
        let scale = f32::from_bits(self.spread_scale_bits.unwrap_or(1.0_f32.to_bits()));
        if !scale.is_finite() || scale < 0.0 {
            return Err("Spread must be a finite, nonnegative percentage".into());
        }
        if let Some(rings) = &self.rings {
            let count = Pattern {
                rings: rings.clone(),
                scale_bits: 1.0_f32.to_bits(),
            }
            .validate()?;
            if self.pellets.is_some_and(|pellets| pellets != count) {
                return Err("The pattern's rings must add up to Pellets per Shot".into());
            }
        }
        Ok(())
    }

    pub fn resolve(&self, inherited: Option<&Pattern>) -> Result<Pattern, String> {
        self.validate()?;
        let mut pattern = inherited
            .cloned()
            .unwrap_or_else(|| Pattern::circular(self.pellets.unwrap_or(1)));
        if let Some(rings) = &self.rings {
            pattern.rings = rings.clone();
        } else if let Some(pellets) = self.pellets {
            redistribute(&mut pattern.rings, pellets)?;
        }
        pattern.scale_bits = (f32::from_bits(pattern.scale_bits)
            * f32::from_bits(self.spread_scale_bits.unwrap_or(1.0_f32.to_bits())))
        .to_bits();
        pattern.validate()?;
        Ok(pattern)
    }

    /// The total control retains a custom pattern's proportions and updates its saved total.
    pub fn set_pellets(&mut self, pellets: u16) -> Result<(), String> {
        if !(1..=0x7FFF).contains(&pellets) {
            return Err("Pellets per shot must be between 1 and 32767".into());
        }
        if let Some(rings) = &mut self.rings {
            redistribute(rings, pellets)?;
        }
        self.pellets = Some(pellets);
        Ok(())
    }
}

/// Largest-remainder allocation, reserving the inherited single center pellet where possible.
pub fn redistribute(rings: &mut [Ring], pellets: u16) -> Result<(), String> {
    if rings.is_empty() || !(1..=0x7FFF).contains(&pellets) {
        return Err("A pattern needs rings and between 1 and 32767 pellets".into());
    }
    let center = rings.iter().position(|ring| {
        ring.pellets == 1
            && f32::from_bits(ring.inner_radius_bits) == 0.0
            && f32::from_bits(ring.outer_radius_bits) == 0.0
    });
    let remaining = u64::from(pellets) - center.is_some() as u64;
    let weights = rings
        .iter()
        .enumerate()
        .map(|(index, ring)| u64::from(ring.pellets) - (center == Some(index)) as u64)
        .collect::<Vec<_>>();
    let total = weights.iter().sum::<u64>();
    if total == 0 {
        for ring in rings.iter_mut() {
            ring.pellets = 0;
        }
        rings[center.unwrap_or(0)].pellets = pellets;
        return Ok(());
    }
    let mut allocated = 0;
    let mut remainders = Vec::with_capacity(rings.len());
    for (index, (ring, weight)) in rings.iter_mut().zip(weights).enumerate() {
        let share = remaining * weight;
        ring.pellets = (share / total) as u16 + (center == Some(index)) as u16;
        allocated += u64::from(ring.pellets);
        remainders.push((index, share % total));
    }
    remainders.sort_by(|(ai, a), (bi, b)| b.cmp(a).then_with(|| ai.cmp(bi)));
    for (index, _) in remainders
        .into_iter()
        .take((u64::from(pellets) - allocated) as usize)
    {
        rings[index].pellets += 1;
    }
    Ok(())
}
