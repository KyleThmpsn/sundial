//! Public player values can retain a key when both builds return the same inherited property.
use super::*;

const NAMESPACE: u32 = 0x811C9DC5;
const SOURCE: u32 = 0x80A746E2;
const NATIVE: [u32; 3] = [0x80EF1439, 0x80EF62F7, 0x80C0C498];

fn lookup(p: &Payload, field: usize, name: u32, class: u32) -> Result<u32> {
    let found = p
        .array(field, 12, Some(class))?
        .into_iter()
        .filter(|&row| p.u32(row).ok() == Some(NAMESPACE) && p.u32(row + 4).ok() == Some(name))
        .collect::<Vec<_>>();
    ensure!(
        found.len() == 1,
        "Named player value lacks one default-namespace binding"
    );
    p.u32(found[0] + 8)
}

fn property(p: &Payload, class: u32, key: u32) -> Result<(u32, [u8; 32])> {
    let definition = p.pointer(24)?;
    ensure!(
        p.u32(definition - 4)? == class,
        "Named player bank class differs"
    );
    let (declaration_class, lookup_class) = if class == 0x808008F6 {
        (0x80800883, 0x8080087B)
    } else {
        (0x808097A1, 0x808097A0)
    };
    let declarations = p.array(definition + 0xD8, 112, Some(declaration_class))?;
    let index = lookup(p, definition + 0xF8, key, lookup_class)?;
    let output = *declarations
        .get(index as usize)
        .context("Named player output index")?;
    let code = p
        .array(output + 8, 1, Some(0x80800009))?
        .into_iter()
        .map(|at| p.u8(at))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        p.u32(output)? == key
            && code == [0x3C, 0, 0x3E, 0]
            && p.u64(output + 24)? == 0
            && p.u64(output + 96)? == 0,
        "Named player value is not a direct property alias"
    );
    let inputs = p.array(output + 56, 4, Some(0x80800007))?;
    ensure!(inputs.len() == 1, "Named player alias has multiple inputs");
    let index = p.u32(inputs[0])?;
    let input = *declarations
        .get(index as usize)
        .context("Named player input index")?;
    let property = p.u32(input)?;
    ensure!(
        ![0, u32::MAX, NAMESPACE].contains(&property)
            && lookup(p, definition + 0x108, property, lookup_class)? == index,
        "Named player alias does not read its inherited property"
    );
    ensure!(
        p.u64(input + 8)? == 0
            && p.u64(input + 24)? == 0
            && p.u64(input + 56)? == 0
            && p.u64(input + 96)? == 0,
        "Inherited player property has another producer"
    );
    Ok((property, p.bytes::<32>(output + 24)?))
}

pub(super) fn check(source: &Source, native: Option<&mut Reader>, key: u32) -> Result<()> {
    if key == NAMESPACE {
        return Ok(());
    }
    ensure!(![0, u32::MAX].contains(&key), "Invalid named player value");
    let original = property(&source.read(SOURCE)?, 0x808008F6, key)?;
    let native = native.context("Named player value needs native player banks")?;
    // Source 778370 and native 107E7F0 compare the first bound lane inclusively.
    // An absent nonempty binding fails in both. Each native class must keep the alias.
    for tag in NATIVE {
        let bank = native.tag(tag, Some(0x80809C36))?;
        ensure!(
            property(&bank, 0x80809790, key)? == original,
            "Named player value changes its inherited property in a native class bank"
        );
    }
    Ok(())
}
