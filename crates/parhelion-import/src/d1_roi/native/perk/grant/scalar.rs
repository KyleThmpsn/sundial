//! Numeric grants require a matching native property declaration in the corresponding bank.
use super::*;

pub(super) fn lower(
    source: &Source,
    data: &Payload,
    row: usize,
    native: &Payload,
) -> Result<Vec<(AbilityRowModifier, Option<usize>)>> {
    let modifier = data.pointer(row + 32)?;
    let declarations = data.array(modifier, 8, Some(0x80802AA5))?;
    let first = usize::try_from(data.u32(modifier + 16)?)?;
    let count = usize::try_from(data.u32(modifier + 20)?)?;
    let end = first
        .checked_add(count)
        .context("Source numeric input range overflow")?;
    let inputs = data.array(data.pointer(24)? + 0xC0, 56, Some(0x808023B9))?;
    ensure!(
        count > 0 && count == declarations.len() && end <= inputs.len(),
        "Source numeric declarations and flattened input range differ"
    );
    let mut anchors = Vec::new();
    for r in native.array(native.pointer(24)? + 0x88, 56, Some(0x8080453B))? {
        let m = native.pointer(r + 48)?;
        if native.u32(r + 16)? == data.u32(row)? && native.u32(m + 4)? == 0x8080451B {
            ensure!(
                native.u64(r + 40)? == 0 && native.u32(r + 32)? == data.u32(row + 16)?,
                "Matching native numeric property has a different gate or mode"
            );
            anchors.push(m);
        }
    }
    ensure!(
        anchors.len() == 1,
        "Source numeric property has no unique native input declaration"
    );
    let children = native.array(anchors[0] + 16, 24, Some(0x8080451F))?;
    ensure!(
        children.len() == count,
        "Matching numeric property has different input arity"
    );
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    for (index, ((&declaration, &flat), &child)) in declarations
        .iter()
        .zip(&inputs[first..end])
        .zip(&children)
        .enumerate()
    {
        let input = data.u8(declaration)?;
        let operation = data.u8(declaration + 1)?;
        let change = data.f32(declaration + 4)?;
        ensure!(
            input < 7
                && operation <= 1
                && data.bytes::<2>(declaration + 2)? == [0; 2]
                && change.is_finite()
                && seen.insert(input)
                && native.u8(child + 16)? == input
                && native.u8(child + 17)? == operation,
            "Source numeric input has no corresponding native selector and operation"
        );
        validate_input(source, data, flat, change, operation)?;
        // These constant inputs use the same signed-change evaluator as component
        // modifiers (695020). The native provider F40810 consumes the final factor.
        let amount = if operation == 0 {
            change
        } else if change > 0.0 {
            1.0 + change
        } else {
            1.0 / (1.0 - change)
        };
        ensure!(
            amount.is_finite() && (operation == 0 || amount > 0.0),
            "Source numeric change has no finite native factor"
        );
        result.push((
            AbilityRowModifier::Scalar {
                input,
                value: amount.to_bits(),
                multiply: operation != 0,
            },
            Some(first + index),
        ));
    }
    Ok(result)
}

/// Both numeric declaration families bind the same source provider and input pair.
pub(super) fn validate_input(
    source: &Source,
    data: &Payload,
    flat: usize,
    change: f32,
    operation: u8,
) -> Result<()> {
    let owner = data.u32(data.pointer(16)?)?;
    let instance = usize::try_from(data.u64(flat + 8)?)?;
    ensure!(
        data.u32(flat)? == owner
            && data.u32(flat + 16)? == 0x8080284A
            && data.u32(instance)? == owner
            && data.u64(instance + 8)? == flat as u64
            && data.u32(instance + 16)? == 0x808023B9
            && data.pointer(instance + 24)? == data.pointer(16)?
            && data.pointer(flat + 24)? == flat
            && data.bytes::<12>(flat + 36)? == [0; 12]
            && data.u32(flat + 48)? == change.to_bits()
            && data.u32(flat + 52)? == u32::from(operation)
            && data.f32(instance + 64)? == 1.0
            && data.bytes::<4>(instance + 68)? == [1, 0, 0, 0],
        "Source numeric input pair, value or activation weight differs"
    );
    let provider = data.u32(flat + 32)?;
    ensure!(
        source.reference(provider)? == 0x8080058A,
        "Source numeric input provider has a different binding format"
    );
    let provider = source.read(provider)?;
    ensure!(
        provider.u32(8)? == 0x8080237A && provider.u32(12)? == 0x808025FC,
        "Source numeric input provider has a different interface"
    );
    let methods = provider.array(16, 24, Some(0x808005C0))?;
    ensure!(
        methods.len() == 9,
        "Source numeric input provider method count differs"
    );
    for (index, method) in methods.into_iter().enumerate() {
        ensure!(
            provider.u32(method)? == 0x8080284A
                && provider.u32(method + 4)? == index as u32 + 2
                && provider.bytes::<16>(method + 8)? == [0; 16],
            "Source numeric input provider has a different method binding"
        );
    }
    Ok(())
}
