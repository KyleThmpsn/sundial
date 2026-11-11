//! Named Float32 weights keep their owner binding and weapon-selection cleanup policy.
use super::*;
use sundial::package_authoring::sandbox_perk::program::{NativeRecord, Policy};

pub(super) fn read(
    source: &Source,
    p: &Payload,
    native: Option<&mut Reader>,
) -> Result<Option<Policy>> {
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
    if p.u8(0xB0)? == 3 {
        return stacking(source, p, native);
    }
    if p.u8(0xB0)? == 4 {
        return burst(p).map(Some);
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

/// Policy 4 runs the action for one burst and lets misses earn bonus rounds. Source vtable
/// `01251400` and native `01C31B50` hold matching bodies, the native one shifted by the slot both
/// builds' policy tables insert at `+20`. Their shot handlers (`75CBB0`, `D391B0`) count each
/// event 27 shot, add one owed round per miss within the first `config[0]` shots up to
/// `config[1]`, activate on the first shot and remove after `config[0]` plus the owed rounds.
/// Source tests the producer's miss flag (`+08` from `004F9F50`) and native the inverse hit flag
/// (`+0C` from `CFE490`), so both count misses. Activation (`75CA40`, `D377A0` to `D36050`) adds
/// `config[1]` rounds to the magazine and removal (`75CAC0`, `D37820` to `D330B0`) takes back the
/// unearned ones. No stock action selects policy 4. Native `80803E08`, the only two-byte policy
/// configuration class, is inferred as the one both handlers read through action `+78`.
fn burst(p: &Payload) -> Result<Policy> {
    let at = p.pointer(0x78)?;
    ensure!(
        p.u32(at - 4)? == 0x80802FBB && p.bytes::<2>(at + 2)? == [0; 2],
        "Source burst policy has an unqualified configuration"
    );
    let [shots, extra] = p.bytes::<2>(at)?;
    // Both handlers compare signed bytes, and the shot count must reach shots plus extra.
    ensure!(
        (1..=0x7F).contains(&shots) && u16::from(shots) + u16::from(extra) <= 0x7F,
        "Source burst policy counts exceed the signed byte range"
    );
    Ok(Policy {
        selector: 4,
        modifier: 0,
        key: 0x811C9DC5,
        configuration: Some(NativeRecord {
            class: 0x80803E08,
            bytes: vec![shots, extra],
            hud_status: None,
        }),
    })
}

/// Policy 3 counts stacks between zero and a maximum and keeps a modifier pattern attached while
/// any stack is held, weighted by the count. Source vtable `012513A0` and native `01C31AF0` hold
/// the same handlers: a hit adds stacks, a hit of the configured class spends them on a magazine
/// refill of `ceil(capacity * stacks / maximum)`, and a disqualified hit, a reload (event 19) or
/// a missed shot (event 27) removes them. Source config `8080231B` and native `80803E09` store the
/// maximum, the amounts added and removed, a path string and the pattern. The setters `0075C750`
/// and `010815A0` clamp and create or destroy the pattern alike. A stock action with the same
/// counts and a pattern holding exactly the source pattern's rows witnesses the configuration,
/// which carries whole, naming that stock pattern.
fn stacking(source: &Source, p: &Payload, native: Option<&mut Reader>) -> Result<Option<Policy>> {
    let at = p.pointer(0x78)?;
    ensure!(
        p.u32(at - 4)? == 0x8080231B,
        "Source stacking policy has an unqualified configuration"
    );
    let counts = p.bytes::<16>(at)?;
    let pattern = p.u32(at + 0x18)?;
    let native = native.context("Stacking policy requires a native witness")?;
    for tag in native.classes(0x808040B5) {
        let stock = native.tag(tag, Some(0x808040B5))?;
        if stock.u8(0xB8)? != 3 || stock.u8(0xB9)? != 0 || stock.u64(0x78)? == 0 {
            continue;
        }
        let n = stock.pointer(0x78)?;
        if stock.u32(n - 4)? != 0x80803E09 || stock.bytes::<16>(n)? != counts {
            continue;
        }
        let stock_pattern = stock.u32(n + 0x18)?;
        let bytes = stock.0.clone();
        if super::pattern::stock_modifier_graph_in(source, native, pattern, &[stock_pattern])
            .is_err()
        {
            continue;
        }
        let decoded = sundial::package_authoring::sandbox_perk::action::decode(&bytes)
            .map_err(anyhow::Error::msg)?;
        let configuration = decoded
            .policy_configuration
            .context("Native stacking witness has no configuration")?;
        return Ok(Some(Policy {
            selector: 3,
            modifier: 0,
            key: 0x811C9DC5,
            configuration: Some(NativeRecord::from(&configuration)),
        }));
    }
    anyhow::bail!("Source stacking policy has no native witness")
}
