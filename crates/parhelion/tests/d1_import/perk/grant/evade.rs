//! Follow the emitted selection through the movement controller: a private profile that plays
//! the source evade's converted human actions, or the untouched native profile.
use super::*;
use sundial::package_authoring::ability_movement::evade;

/// The shared human action table the private actions join.
const HUMAN_STATES: u32 = 0x80BB2C46;

pub(super) fn check(
    stock: &mut Reader,
    native: &mut Reader,
    source: &Payload,
    row: usize,
    report: &Value,
) -> CheckResult<Value> {
    assert_eq!(source.bytes::<4>(source.pointer(row + 32)?)?, [0, 1, 0, 0]);
    if report["route"] == "private_air_evade" {
        return private(stock, native, report);
    }
    assert_eq!(report["route"], "native_air_evade");
    assert_eq!(number(&report["slot"])?, 4);
    let bank = number(&report["native_bank"])?;
    let key = number(&report["property_key"])?;
    let before = stock.tag(bank, Some(0x80809C36))?;
    let emitted = native.tag(bank, Some(0x80809C36))?;
    assert_eq!(
        before.0, emitted.0,
        "Air evade must not mutate the shared movement bank"
    );
    let properties = emitted.array(emitted.pointer(24)? + 0x88, 56, Some(0x8080453B))?;
    let selected = properties
        .into_iter()
        .filter(|&r| emitted.u32(r + 16).ok() == Some(key))
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 1);
    let r = selected[0];
    assert_eq!(emitted.u32(r + 24)?, 255);
    assert_eq!(emitted.u32(r + 32)?, 255);
    assert_eq!(emitted.u64(r + 40)?, 0);
    let modifier = emitted.pointer(r + 48)?;
    assert_eq!(emitted.u32(modifier + 4)?, 0x80804121);
    let profile_key = emitted.u32(modifier + 16)?;
    let graph = stock.tag(number(&report["native_graph"])?, Some(0x80809C0F))?;
    let mut profiles = Vec::new();
    for component in graph.array(16, 12, None)? {
        let owner = graph.u32(component)?;
        let data = stock.tag(owner, Some(0x80809C36))?;
        let settings = data.pointer(24)?;
        if data.u32(settings - 4)? != 0x808044E0 {
            continue;
        }
        assert_eq!(native.tag(owner, Some(0x80809C36))?.0, data.0);
        let config = data.pointer(settings + 0x800)?;
        assert_eq!(data.u32(config - 4)?, 0x8080410F);
        for p in data.array(config + 16, 128, Some(0x80804106))? {
            if data.u32(p + 16)? != profile_key {
                continue;
            }
            assert_eq!(data.u8(p + 0x58)?, 2, "Air-only movement gate");
            assert!(
                data.array(p + 0x30, 8, None)?.is_empty(),
                "Unexpected profile conditions"
            );
            assert_eq!(data.u32(p + 0x70)?, 0x476D0367, "Air-evade input identity");
            assert_eq!(data.u32(p + 0x14)?, 0.5f32.to_bits());
            assert_eq!(data.u32(p + 0x18)?, 5.0f32.to_bits());
            assert_eq!(data.u32(p + 0x40)?, 0.5f32.to_bits());
            assert_eq!(data.u32(p + 0x44)?, 1.0f32.to_bits());
            stock.tag(data.u32(p + 0x50)?, Some(0x80809C0F))?;
            profiles.push(json!({"owner":owner,"key":profile_key,"offset":p}));
        }
    }
    assert_eq!(
        profiles.len(),
        1,
        "Selected air-evade profile is missing or ambiguous"
    );
    Ok(json!({"bank":bank,"key":key,"profile":profiles[0],"gameplay_verified":false}))
}

/// The private row selects a profile of its own in the staged controller, copied from the
/// template with only its identity and actions changed, and every private action resolves in the
/// staged human action table while every stock action keeps its ordinal.
fn private(stock: &mut Reader, native: &mut Reader, report: &Value) -> CheckResult<Value> {
    assert_eq!(number(&report["slot"])?, 4);
    let bank = number(&report["native_bank"])?;
    let key = number(&report["property_key"])?;
    let detail = &report["evade"];
    let controller = number(&detail["native_controller"])?;
    let template = number(&detail["template_profile"])?;
    let actions = detail["private_actions"]
        .as_array()
        .ok_or("Private evade actions")?
        .iter()
        .map(|value| number(value).map_err(Into::into))
        .collect::<CheckResult<Vec<_>>>()?;
    assert_eq!(actions.len(), 4);
    let emitted = native.tag(bank, Some(0x80809C36))?;
    let rows = emitted.array(emitted.pointer(24)? + 0x88, 56, Some(0x8080453B))?;
    let selected = rows
        .into_iter()
        .filter(|&r| emitted.u32(r + 16).ok() == Some(key))
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 1);
    let r = selected[0];
    assert_eq!(emitted.u32(r + 24)?, 255);
    assert_eq!(emitted.u32(r + 32)?, 255);
    assert_eq!(emitted.u64(r + 40)?, 0, "Private evade row is gated");
    let modifier = emitted.pointer(r + 48)?;
    assert_eq!(emitted.u32(modifier + 4)?, 0x80804121);
    assert_eq!(
        emitted.u32(modifier + 16)?,
        key,
        "Selector names another profile"
    );

    let before = stock.tag(controller, Some(0x80809C36))?;
    let after = native.tag(controller, Some(0x80809C36))?;
    let old = evade::identities(&before.0)?;
    let new = evade::identities(&after.0)?;
    assert!(old.contains(&template) && !old.contains(&key));
    assert_eq!(&new[..old.len()], &old[..], "Stock profiles moved");
    assert!(new.contains(&key));
    assert_eq!(evade::actions(&after.0, key)?.to_vec(), actions);
    // Every stock profile keeps its actions, and the copy differs from its template only in
    // identity, actions and the offsets that name its own rows.
    for &identity in &old {
        assert_eq!(
            evade::actions(&after.0, identity)?,
            evade::actions(&before.0, identity)?
        );
    }
    let profile = |p: &Payload, identity: u32| -> CheckResult<usize> {
        let settings = p.pointer(24)?;
        let config = p.pointer(settings + 0x800)?;
        p.array(config + 16, 128, Some(0x80804106))?
            .into_iter()
            .find(|&at| p.u32(at + 16).ok() == Some(identity))
            .ok_or_else(|| "Profile missing".into())
    };
    let (copy, original) = (profile(&after, key)?, profile(&after, template)?);
    for field in [0x14, 0x18, 0x1C, 0x40, 0x44, 0x50, 0x58, 0x5C, 0x70, 0x74] {
        assert_eq!(
            after.u32(copy + field)?,
            after.u32(original + field)?,
            "Profile field {field:X}"
        );
    }
    assert_eq!(
        after.pointer(copy + 0x78)?,
        after.pointer(original + 0x78)?,
        "The copy moves the body with another preparation"
    );

    let states = native.tag(HUMAN_STATES, None)?;
    let stock_states = stock.tag(HUMAN_STATES, None)?;
    let names = |p: &Payload| -> CheckResult<BTreeMap<u32, u32>> {
        p.array(8, 8, Some(0x8080342E))?
            .into_iter()
            .map(|at| Ok((p.u32(at)?, p.u32(at + 4)?)))
            .collect::<CheckResult<_>>()
    };
    let (old_names, new_names) = (names(&stock_states)?, names(&states)?);
    for (name, ordinal) in &old_names {
        assert_eq!(
            new_names.get(name),
            Some(ordinal),
            "Stock human action moved"
        );
    }
    for action in &actions {
        assert!(!old_names.contains_key(action) && new_names.contains_key(action));
    }
    Ok(
        json!({"bank":bank,"key":key,"controller":controller,"template":template,
        "profiles":new.len(),"actions":actions,"gameplay_verified":false}),
    )
}
