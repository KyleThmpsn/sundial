//! Checked component expression storage. Only exact interpreter shapes expose constants.
use super::*;

pub(super) struct Data<'a> {
    pub bytes: &'a [u8],
    pub owner: u32,
    registry: Registry,
}

#[derive(Clone)]
pub(super) struct Program {
    pub at: usize,
    pub code: Vec<u8>,
    pub constants: Vec<usize>,
    pub providers: Vec<usize>,
}

impl<'a> Data<'a> {
    pub fn new(bytes: &'a [u8], owner: u32) -> Result<Self, String> {
        if u64_at(bytes, 0)? != bytes.len() as u64 {
            return Err("Expression owner has an invalid envelope".into());
        }
        Ok(Self {
            bytes,
            owner,
            registry: Registry::new()?,
        })
    }

    pub fn extent(&mut self, at: usize, class: u32) -> Result<usize, String> {
        let size = self
            .registry
            .record(class, |_| Err("Expression requires a native class".into()))?
            .size;
        rows_fit(self.bytes, at, 1, size)?;
        Ok(size)
    }

    pub fn pair(&mut self, at: usize, definition: u32, source: u32) -> Result<usize, String> {
        self.extent(at, definition)?;
        let twin = usize::try_from(u64_at(self.bytes, at + 8)?)
            .map_err(|_| "Expression pair overflows")?;
        self.extent(twin, source)?;
        if u32_at(self.bytes, at)? != self.owner
            || u32_at(self.bytes, twin)? != self.owner
            || u32_at(self.bytes, at + 4)? != source
            || u32_at(self.bytes, twin + 4)? != definition
            || u64_at(self.bytes, twin + 8)? != at as u64
        {
            return Err("Expression pair is not reciprocal".into());
        }
        Ok(twin)
    }

    pub fn array(&mut self, at: usize, class: u32) -> Result<Vec<usize>, String> {
        bytes_at::<16>(self.bytes, at)?;
        if u64_at(self.bytes, at)? == 0 {
            return Ok(Vec::new());
        }
        let (count, header, rows, actual) = native_array_at(self.bytes, at)?;
        if actual != class || u32_at(self.bytes, header + 12)? != 0 || count > 8192 {
            return Err("Expression array has an incompatible type or count".into());
        }
        let stride = self.extent(rows, class)?;
        if stride == 0 {
            return Err("Expression array has an empty row type".into());
        }
        rows_fit(self.bytes, rows, count, stride)?;
        Ok((0..count).map(|index| rows + stride * index).collect())
    }

    pub fn program(&mut self, at: usize) -> Result<Option<Program>, String> {
        let source = self.pair(at, 0x8080_89F8, 0x8080_89F7)?;
        let code = self
            .array(at + 16, 0x8080_0009)?
            .into_iter()
            .map(|position| self.bytes[position])
            .collect::<Vec<_>>();
        let (inputs, constant_count) = match code.as_slice() {
            LITERAL => (1, 1),
            AFFINE | PRODUCT => (if code == PRODUCT { 3 } else { 2 }, 2),
            ADD | MULTIPLY | SUBTRACT => (2, 1),
            _ => return Ok(None),
        };
        let words = (0..4)
            .map(|index| u32_at(self.bytes, at + 48 + index * 4))
            .collect::<Result<Vec<_>, _>>()?;
        let constants = self.array(at + 32, 0x8080_0090)?;
        if words != [inputs, 0, 1, 0] || constants.len() != constant_count {
            return Ok(None);
        }
        let providers = self.array(at + 64, 0x8080_9789)?;
        let states = self.array(source + 32, 0x8080_9788)?;
        if providers.len() != inputs as usize - 1 || providers.len() != states.len() {
            return Err("Expression provider arrays disagree".into());
        }
        for (&provider, state) in providers.iter().zip(states) {
            if self.pair(provider, 0x8080_9789, 0x8080_9788)? != state
                || relative_offset(state, 16, i64_at(self.bytes, state + 16)?)? != source
            {
                return Err("Expression provider belongs to another program".into());
            }
        }
        Ok(Some(Program {
            at,
            code,
            constants,
            providers,
        }))
    }

    /// Includes valid paired expressions outside the selected root. Sharing a constant with any
    /// other expression would change that expression too, even if it has no named control.
    pub fn constant_users(&mut self) -> BTreeMap<usize, BTreeSet<usize>> {
        let mut users = BTreeMap::<usize, BTreeSet<usize>>::new();
        for at in (0..self.bytes.len().saturating_sub(80)).step_by(8) {
            if u32_at(self.bytes, at).ok() != Some(self.owner) {
                continue;
            }
            let source = match u32_at(self.bytes, at + 4).ok() {
                Some(0x8080_89F7) => 0x8080_89F7,
                Some(0x8080_89F5) => 0x8080_89F5,
                _ => continue,
            };
            if self.pair(at, source + 1, source).is_err() {
                continue;
            }
            if let Ok(constants) = self.array(at + 32, 0x8080_0090) {
                for constant in constants {
                    users.entry(constant).or_default().insert(at);
                }
            }
        }
        users
    }
}
