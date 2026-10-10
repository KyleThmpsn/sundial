//! Compiled particle parameter program. The eight bytecode spans and the 56 scalar routes are
//! stored in the definition header, independently of the emitter mesh and material.
use super::*;
mod coverage;
#[cfg(test)]
mod verification;
mod vm;
pub(crate) use vm::{Registers, Runtime, Sources};

pub(crate) struct Program {
    pub sections: [u16; 8],
    pub routes: [Option<Route>; 56],
    pub defaults: Vec<[f32; 4]>,
    pub bytecode: Vec<u8>,
    pub constants: Vec<[f32; 4]>,
    pub lifetime_ceiling: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Route {
    pub bank: u8,
    pub scalar: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RegisterWrite {
    pub section: usize,
    pub bank: u8,
    pub slot: u8,
    pub selector: u8,
}

impl Program {
    pub fn read(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 0x150 || u64_at(bytes, 0)? != bytes.len() as u64 {
            return Err("Particle definition has an invalid native envelope".into());
        }
        let defaults = float4_array(bytes, 0x08, 256)?;
        let constants = float4_array(bytes, 0x60, 4096)?;
        let bytecode = if u64_at(bytes, 0x50)? == 0 && i64_at(bytes, 0x58)? == 0 {
            Vec::new()
        } else {
            let (code_len, code_at, code_class) = array_at(bytes, 0x50)?;
            if code_class != 0x8080_0009 || code_len > 65_536 {
                return Err("Particle bytecode has an unsupported array".into());
            }
            let code_end = code_at
                .checked_add(code_len)
                .ok_or("Particle bytecode length overflow")?;
            bytes
                .get(code_at..code_end)
                .ok_or("Particle bytecode exceeds its definition")?
                .to_vec()
        };
        let sections = std::array::from_fn(|index| {
            u16::from_le_bytes(
                bytes[0x70 + index * 2..0x72 + index * 2]
                    .try_into()
                    .unwrap(),
            )
        });
        if sections
            .iter()
            .map(|&length| usize::from(length))
            .sum::<usize>()
            != bytecode.len()
        {
            return Err("Particle bytecode sections do not cover the program".into());
        }
        let routes = std::array::from_fn(|index| {
            let bank = bytes[0x80 + index * 2];
            (bank != 0xFF).then_some(Route {
                bank,
                scalar: bytes[0x81 + index * 2],
            })
        });
        for route in routes.iter().flatten() {
            if route.bank > 6
                || (route.bank == 6 && usize::from(route.scalar) >= defaults.len() * 4)
            {
                return Err("Particle output route points outside its value bank".into());
            }
        }
        let lifetime_ceiling = f32::from_bits(u32_at(bytes, 0x120)?);
        if !lifetime_ceiling.is_finite() || lifetime_ceiling < 0.0 {
            return Err("Particle lifetime ceiling is invalid".into());
        }
        Ok(Self {
            sections,
            routes,
            defaults,
            bytecode,
            constants,
            lifetime_ceiling,
        })
    }

    pub fn default_for(&self, output: usize) -> Option<f32> {
        let route = self.routes.get(output)?.as_ref()?;
        if route.bank != 6 {
            return None;
        }
        let scalar = usize::from(route.scalar);
        Some(self.defaults[scalar / 4][scalar % 4])
    }

    /// Native definitions with a stored route 5 value set the compiled lifetime ceiling to
    /// that value plus 0.05 seconds. Only expose the default when this relationship holds.
    pub fn lifetime_default(&self) -> Option<f32> {
        let value = self.default_for(5)?;
        (self.lifetime_ceiling == value + 0.05).then_some(value)
    }

    /// The captured native caller publishes position XYZ through route 8 and direction XYZ
    /// through route 9 before section 3, preserving each vector's fourth component. This
    /// bridge recognizes adjacent bank-2 inputs and bank-1 outputs in stored programs.
    /// Original Air Weak 2 vertex execution consumes this position vector as the center
    /// and the direction vector's fourth component as an age cutoff. Native upload and
    /// attachment producers remain separate from the witnessed shader consumer.
    pub fn state_routes(&self) -> Option<[(Route, Route); 2]> {
        let first = (self.routes[9]?, self.routes[7]?);
        let second = (self.routes[8]?, self.routes[6]?);
        if first.0.bank != 2
            || second.0.bank != 2
            || first.1.bank != 1
            || second.1.bank != 1
            || first.0.scalar % 4 != 0
            || first.1.scalar % 4 != 0
            || second.0.scalar != first.0.scalar.checked_add(4)?
            || second.1.scalar != first.1.scalar.checked_add(4)?
        {
            return None;
        }
        Some([first, second])
    }

    pub fn section(&self, index: usize) -> Option<&[u8]> {
        let start = self
            .sections
            .get(..index)?
            .iter()
            .map(|&length| usize::from(length))
            .sum::<usize>();
        let end = start.checked_add(usize::from(*self.sections.get(index)?))?;
        self.bytecode.get(start..end)
    }

    /// Recover write instructions without assigning engine-level meanings to their registers.
    /// The final byte is retained as an opaque selector. In particular, a write to the vector
    /// containing a routed scalar does not prove that scalar is updated by the instruction.
    pub fn register_writes(&self) -> Result<Vec<RegisterWrite>, String> {
        let mut writes = Vec::new();
        for section in 0..self.sections.len() {
            let code = self
                .section(section)
                .ok_or("Particle section exceeds bytecode")?;
            let mut at = 0;
            while at < code.len() {
                let opcode = code[at];
                let length = coverage::instruction_len(opcode)
                    .ok_or_else(|| format!("Unknown particle opcode 0x{opcode:02X}"))?;
                if at + length > code.len() {
                    return Err("Particle instruction crosses a section boundary".into());
                }
                if opcode == 0x3F {
                    writes.push(RegisterWrite {
                        section,
                        bank: code[at + 1],
                        slot: code[at + 2],
                        selector: code[at + 3],
                    });
                }
                at += length;
            }
        }
        Ok(writes)
    }
}

fn float4_array(bytes: &[u8], descriptor: usize, limit: usize) -> Result<Vec<[f32; 4]>, String> {
    // Native definitions may omit the header entirely for an empty constant array.
    if u64_at(bytes, descriptor)? == 0 && i64_at(bytes, descriptor + 8)? == 0 {
        return Ok(Vec::new());
    }
    let (count, rows, class) = array_at(bytes, descriptor)?;
    if class != 0x8080_0090 || count > limit {
        return Err("Particle float4 table has an unsupported array".into());
    }
    let end = rows
        .checked_add(
            count
                .checked_mul(16)
                .ok_or("Particle float4 count overflow")?,
        )
        .ok_or("Particle float4 offset overflow")?;
    let table = bytes
        .get(rows..end)
        .ok_or("Particle float4 table exceeds its definition")?;
    Ok(table
        .chunks_exact(16)
        .map(|row| {
            std::array::from_fn(|lane| {
                f32::from_le_bytes(row[lane * 4..lane * 4 + 4].try_into().unwrap())
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{Program, RegisterWrite, float4_array};

    #[test]
    fn omitted_empty_float_table_does_not_require_an_array_header() {
        let mut bytes = vec![0_u8; 0x20];
        assert_eq!(float4_array(&bytes, 0, 256), Ok(Vec::new()));
        bytes[0] = 1;
        assert!(float4_array(&bytes, 0, 256).is_err());
    }

    #[test]
    fn routes_are_bounded_by_the_declared_default_vectors() {
        let mut bytes = vec![0_u8; 0x1A0];
        bytes[..8].copy_from_slice(&0x1A0_u64.to_le_bytes());
        bytes[0x08..0x10].copy_from_slice(&1_u64.to_le_bytes());
        bytes[0x10..0x18].copy_from_slice(&0x150_u64.to_le_bytes());
        bytes[0x58..0x60].copy_from_slice(&0x128_u64.to_le_bytes());
        bytes[0x68..0x70].copy_from_slice(&0x128_u64.to_le_bytes());
        for index in 0..56 {
            bytes[0x80 + index * 2] = 0xFF;
        }
        bytes[0x80 + 5 * 2] = 6;
        bytes[0x81 + 5 * 2] = 2;
        bytes[0x120..0x124].copy_from_slice(&(0.85_f32 + 0.05).to_le_bytes());
        bytes[0x160..0x168].copy_from_slice(&1_u64.to_le_bytes());
        bytes[0x168..0x16C].copy_from_slice(&0x8080_0090_u32.to_le_bytes());
        bytes[0x178..0x17C].copy_from_slice(&0.85_f32.to_le_bytes());
        bytes[0x188..0x18C].copy_from_slice(&0x8080_0009_u32.to_le_bytes());
        bytes[0x198..0x19C].copy_from_slice(&0x8080_0090_u32.to_le_bytes());

        let program = Program::read(&bytes).unwrap();
        assert_eq!(program.default_for(5), Some(0.85));
        assert_eq!(program.lifetime_default(), Some(0.85));
        assert_eq!(program.section(7), Some(&[][..]));
        bytes[0x81 + 5 * 2] = 4;
        assert!(Program::read(&bytes).is_err());
        bytes[0x81 + 5 * 2] = 2;
        bytes[0x70] = 1;
        assert!(Program::read(&bytes).is_err());
    }

    #[test]
    fn register_writes_respect_section_boundaries() {
        let mut program = Program {
            sections: [6, 6, 0, 0, 0, 0, 0, 0],
            routes: [None; 56],
            defaults: Vec::new(),
            bytecode: vec![0x34, 0x03, 0x3F, 5, 1, 2, 0x40, 2, 0x3F, 1, 3, 7],
            constants: Vec::new(),
            lifetime_ceiling: 0.0,
        };
        assert_eq!(
            program.register_writes(),
            Ok(vec![
                RegisterWrite {
                    section: 0,
                    bank: 5,
                    slot: 1,
                    selector: 2
                },
                RegisterWrite {
                    section: 1,
                    bank: 1,
                    slot: 3,
                    selector: 7
                },
            ])
        );
        program.sections[1] = 1;
        assert!(program.register_writes().is_err());
    }
}
