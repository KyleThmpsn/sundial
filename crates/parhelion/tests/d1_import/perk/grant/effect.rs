//! Source contact-effect operands select the corresponding complete native effect graph.
use super::*;

pub(super) fn check(
    stock: &mut Reader,
    native: &mut Reader,
    source: &Payload,
    row: usize,
    grant: &Value,
) -> CheckResult<Value> {
    assert_eq!(grant["route"], "native_melee_effect");
    assert_eq!(grant["balance"], "native");
    let key = source.u32(row)?;
    assert_eq!(number(&grant["property_key"])?, key);
    let bank = number(&grant["native_bank"])?;
    let original = stock.tag(bank, Some(0x80809C36))?;
    let emitted = native.tag(bank, Some(0x80809C36))?;
    let sm = source.pointer(row + 32)?;
    let mut graphs = Vec::new();
    for p in [&original, &emitted] {
        let matches = p
            .array(p.pointer(24)? + 0x88, 56, Some(0x8080453B))?
            .into_iter()
            .filter(|&r| p.u32(r + 16).ok() == Some(key))
            .collect::<Vec<_>>();
        assert_eq!(matches.len(), 1);
        let r = matches[0];
        assert_eq!(source.bytes::<8>(row + 4)?, p.bytes::<8>(r + 20)?);
        assert_eq!(source.u32(row + 16)?, p.u32(r + 32)?);
        let gate = p.pointer(r + 40)?;
        assert_eq!(p.u32(gate - 4)?, 0x8080454A);
        assert_eq!(source.u32(source.pointer(row + 24)?)?, p.u32(gate)?);
        let m = p.pointer(r + 48)?;
        assert_eq!(p.u32(m + 4)?, 0x8080442C);
        assert_eq!(source.bytes::<8>(sm)?, p.bytes::<8>(m + 16)?);
        graphs.push(p.u32(m + 32)?);
    }
    assert_eq!(graphs[0], graphs[1]);
    assert_eq!(
        stock.tag(graphs[0], Some(0x80809C0F))?.0,
        native.tag(graphs[1], Some(0x80809C0F))?.0
    );
    Ok(
        json!({"bank":bank,"key":key,"native_effect":graphs[0],"balance":"native","gameplay_verified":false}),
    )
}
