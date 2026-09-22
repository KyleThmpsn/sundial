//! Checked payload access. The app's package_payload module is private; this
//! standalone experiment keeps the same bounds/pointer contracts without changing it.
use anyhow::{Result, bail, ensure};
#[derive(Clone)]
pub struct Payload(pub Vec<u8>);
impl Payload {
    pub fn bytes<const N: usize>(&self, o: usize) -> Result<[u8; N]> {
        let end = o
            .checked_add(N)
            .ok_or_else(|| anyhow::anyhow!("offset overflow"))?;
        Ok(self
            .0
            .get(o..end)
            .ok_or_else(|| anyhow::anyhow!("read outside payload at {o:X}"))?
            .try_into()?)
    }
    pub fn u8(&self, o: usize) -> Result<u8> {
        Ok(self.bytes::<1>(o)?[0])
    }
    pub fn u16(&self, o: usize) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(o)?))
    }
    pub fn i16(&self, o: usize) -> Result<i16> {
        Ok(i16::from_le_bytes(self.bytes(o)?))
    }
    pub fn u32(&self, o: usize) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(o)?))
    }
    pub fn u64(&self, o: usize) -> Result<u64> {
        Ok(u64::from_le_bytes(self.bytes(o)?))
    }
    pub fn f32(&self, o: usize) -> Result<f32> {
        let v = f32::from_le_bytes(self.bytes(o)?);
        ensure!(v.is_finite(), "nonfinite transform");
        Ok(v)
    }
    pub fn pointer(&self, o: usize) -> Result<usize> {
        let delta = i64::from_le_bytes(self.bytes(o)?);
        let target = (o as i128) + (delta as i128);
        ensure!(
            target >= 0 && target <= self.0.len() as i128,
            "relative pointer outside payload"
        );
        Ok(target as usize)
    }
    pub fn array(&self, o: usize, stride: usize, class: Option<u32>) -> Result<Vec<usize>> {
        Ok(self
            .array_range(o, stride, class)?
            .step_by(stride.max(1))
            .collect())
    }
    pub fn array_range(
        &self,
        o: usize,
        stride: usize,
        class: Option<u32>,
    ) -> Result<std::ops::Range<usize>> {
        let count = self.u64(o)?;
        if count == 0 {
            return Ok(0..0);
        }
        ensure!(count <= 1_000_000 && stride > 0, "invalid array size");
        let header = self.pointer(o + 8)?;
        ensure!(
            self.u64(header)? == count,
            "array count mismatch at {o:X}: descriptor {count}, header {} at {header:X}, payload bytes {}",
            self.u64(header)?,
            self.0.len()
        );
        if let Some(c) = class {
            ensure!(self.u32(header + 8)? == c, "array class mismatch")
        }
        let start = header + 16;
        let end = start
            .checked_add(
                (count as usize)
                    .checked_mul(stride)
                    .ok_or_else(|| anyhow::anyhow!("array overflow"))?,
            )
            .ok_or_else(|| anyhow::anyhow!("array overflow"))?;
        if end > self.0.len() {
            bail!("array exceeds payload")
        }
        Ok(start..end)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_bounds_and_negative_pointer() {
        let p = Payload(vec![255; 16]);
        assert!(p.u32(usize::MAX).is_err());
        assert!(p.pointer(0).is_err());
    }
    #[test]
    fn checks_array_extent() {
        let mut p = Payload(vec![0; 40]);
        p.0[0..8].copy_from_slice(&2u64.to_le_bytes());
        p.0[8..16].copy_from_slice(&8i64.to_le_bytes());
        p.0[16..24].copy_from_slice(&2u64.to_le_bytes());
        assert!(p.array(0, 8, None).is_err());
        assert_eq!(p.array(0, 4, None).unwrap(), vec![32, 36]);
    }
}
