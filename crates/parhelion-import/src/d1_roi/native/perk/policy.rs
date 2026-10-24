//! Named Float32 weights keep their owner binding and weapon-selection cleanup policy.
use super::*;
use sundial::package_authoring::sandbox_perk::program::{NativeRecord, Policy};

pub(super) fn read(p: &Payload, native: Option<&mut Reader>) -> Result<Option<Policy>> {
    ensure!(
        p.bytes::<3>(0xB1)? == [0; 3],
        "Source policy has additional flags"
    );
    if p.u8(0xB0)? == 0 {
        ensure!(
            p.u64(0x78)? == 0,
            "Source default policy has a configuration"
        );
        return Ok(None);
    }
    ensure!(
        p.u8(0xB0)? == 1,
        "Source execution policy requires translation"
    );
    let at = p.pointer(0x78)?;
    ensure!(
        p.u32(at - 4)? == 0x808031C4 && p.u32(at + 4)? <= 1,
        "Source named-value policy has an unqualified configuration"
    );
    let bytes = p.bytes::<8>(at)?;
    // Original 75D280 binds on source weapon event 15 and clears on 16.
    // Native 1080EA0 performs the same work on events 14 and 15. Both
    // getters read a named Float32 output, returning zero without a binding.
    // Require an existing native policy with this key and binding mode.
    let native = native.context("Named-value policy requires a native binding witness")?;
    let mut found = false;
    for tag in native.classes(0x808040B5) {
        let stock = native.tag(tag, Some(0x808040B5))?;
        if stock.u8(0xB8)? != 1 || stock.u8(0xB9)? != 0 || stock.u64(0x78)? == 0 {
            continue;
        }
        let n = stock.pointer(0x78)?;
        if stock.u32(n - 4)? == 0x80803E07 && stock.bytes::<8>(n)? == bytes {
            found = true;
            break;
        }
    }
    ensure!(
        found,
        "Source named-value policy has no native binding witness"
    );
    Ok(Some(Policy {
        selector: 1,
        modifier: 0,
        key: 0x811C9DC5,
        configuration: Some(NativeRecord {
            class: 0x80803E07,
            bytes: bytes.to_vec(),
            hud_status: None,
        }),
    }))
}
