//! Recognize the source projection-mode selection before using native reciprocal depth.
use super::{instruction::Emitter, *};

impl Emitter<'_> {
    fn definition(&self, source: &str) -> Option<&str> {
        let ident = id(source).ok()?;
        self.program
            .blocks
            .values()
            .flatten()
            .find(|row| row.id == Some(ident))
            .map(|row| row.code.as_str())
    }

    fn binary<'a>(&'a self, source: &str, opcode: &str) -> Option<Vec<&'a str>> {
        let rest = self.definition(source)?.strip_prefix(opcode)?;
        let parts = arguments(rest).ok()?;
        (parts.len() == 2).then_some(parts)
    }

    fn component(&self, source: &str, buffer: u32, row: u32, lane: u32) -> Option<()> {
        let parts = self.binary(source, "extractvalue ")?;
        if number(parts[1]).ok()? != lane {
            return None;
        }
        let (_, name, args) = call(self.definition(parts[0])?).ok()?;
        (name == "cbufferLoadLegacy"
            && args.len() == 3
            && number(args[0]).ok()? == 59
            && self.handles.get(&id(args[1]).ok()?) == Some(&(2, buffer))
            && number(args[2]).ok()? == row)
            .then_some(())
    }

    fn depth_texel(&self, source: &str) -> Option<()> {
        let parts = self.binary(source, "extractvalue ")?;
        if number(parts[1]).ok()? != 0 {
            return None;
        }
        let (_, name, args) = call(self.definition(parts[0])?).ok()?;
        (name == "textureLoad"
            && args.len() >= 2
            && number(args[0]).ok()? == 66
            && self.handles.get(&id(args[1]).ok()?) == Some(&(0, 10)))
        .then_some(())
    }

    fn linear_depth(&self, source: &str) -> Option<()> {
        let sum = self.binary(source, "fadd ")?;
        for swap in [false, true] {
            let (x, product) = if swap {
                (sum[1], sum[0])
            } else {
                (sum[0], sum[1])
            };
            if self.component(x, 2, 0, 0).is_none() {
                continue;
            }
            let Some(product) = self.binary(product, "fmul ") else {
                continue;
            };
            if (self.component(product[0], 2, 0, 1).is_some()
                && self.depth_texel(product[1]).is_some())
                || (self.component(product[1], 2, 0, 1).is_some()
                    && self.depth_texel(product[0]).is_some())
            {
                return Some(());
            }
        }
        None
    }

    pub(super) fn perspective_depth(&self, operands: &[&str]) -> bool {
        self.checked_depth(operands).is_some()
    }

    fn checked_depth(&self, operands: &[&str]) -> Option<()> {
        if !self.options.perspective_depth || operands.len() != 3 {
            return None;
        }
        // The source chooses linear depth when bit 1 is clear, reciprocal depth otherwise.
        let test = self.binary(operands[0], "icmp eq ")?;
        if number(test[1]).ok()? != 0 {
            return None;
        }
        let masked = self.binary(test[0], "and ")?;
        if number(masked[1]).ok()? != 2 {
            return None;
        }
        let cast = self
            .definition(masked[0])?
            .strip_prefix("bitcast float ")?
            .strip_suffix(" to i32")?;
        self.component(cast, 12, 17, 1)?;
        self.linear_depth(operands[1])?;
        let reciprocal = self.binary(operands[2], "fdiv ")?;
        if reciprocal[0]
            .split_whitespace()
            .last()?
            .parse::<f32>()
            .ok()?
            != 1.0
            || id(reciprocal[1]).ok()? != id(operands[1]).ok()?
        {
            return None;
        }
        Some(())
    }
}
