//! Numeric grants require a matching native property declaration in the corresponding bank. A
//! gated row, as an upgrade's rows gate on their base ability's key, keeps its own name,
//! priority and the gate the caller maps for it.
use super::*;

pub(super) fn lower(
    source: &Source,
    data: &Payload,
    row: usize,
    native: &Payload,
    gate: Option<u32>,
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
    // The source gate's own key, which a same-key native declaration gates on too.
    let source_gate = if data.u64(row + 24)? == 0 {
        0
    } else {
        data.u32(data.pointer(row + 24)?)?
    };
    ensure!(
        (source_gate == 0) == gate.is_none(),
        "Source numeric property gate has no mapped key"
    );
    let rows = native.array(native.pointer(24)? + 0x88, 56, Some(0x8080453B))?;
    let gate_of = |r: usize| -> Result<u32> {
        if native.u64(r + 40)? == 0 {
            return Ok(0);
        }
        let at = native.pointer(r + 40)?;
        ensure!(
            native.u32(at - 4)? == 0x8080454A,
            "Native numeric gate has another class"
        );
        native.u32(at)
    };
    let mut anchors = Vec::new();
    for &r in &rows {
        let m = native.pointer(r + 48)?;
        if native.u32(r + 16)? == data.u32(row)? && native.u32(m + 4)? == 0x8080451B {
            ensure!(
                gate_of(r)? == source_gate && native.u32(r + 32)? == data.u32(row + 16)?,
                "Matching native numeric property has a different gate or mode"
            );
            anchors.push(m);
        }
    }
    // A gated row whose key has no native declaration, such as Brimstone's, is anchored by
    // any declaration that reads the same inputs with the same operations. The handler and
    // input storage are the bank's, so they witness the row as well as a same-key one.
    let shared = anchors.is_empty() && gate.is_some();
    if shared {
        let wanted = declarations
            .iter()
            .map(|&d| Ok((data.u8(d)?, data.u8(d + 1)?)))
            .collect::<Result<Vec<_>>>()?;
        for &r in &rows {
            let m = native.pointer(r + 48)?;
            if native.u32(m + 4)? != 0x8080451B || native.u32(r + 32)? != data.u32(row + 16)? {
                continue;
            }
            let children = native.array(m + 16, 24, Some(0x8080451F))?;
            let read = children
                .iter()
                .map(|&c| Ok((native.u8(c + 16)?, native.u8(c + 17)?)))
                .collect::<Result<Vec<_>>>()?;
            if read == wanted {
                anchors.push(m);
                break;
            }
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
    // An ungated wildcard row keeps the selection-free identity it always had.
    let priority = u8::try_from(data.u32(row + 8)?)?;
    let selection = (gate.is_some() || priority != 255).then(|| {
        sundial::package_authoring::ability_bank::FixedSelection {
            name: data.u32(row + 4).unwrap_or(0),
            priority,
            required_key: gate.unwrap_or(0),
        }
    });
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
                selection,
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
