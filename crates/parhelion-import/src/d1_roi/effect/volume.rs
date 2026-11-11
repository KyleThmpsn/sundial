//! Rise of Iron condition volume scripts as Shadowkeep's.
//!
//! `c_condition_vol_component` (`bs\abilities\ability_components.bs`) is a script on the common
//! component base with no code of its own. Its embedded query helper selects the entities inside
//! a sphere around its host, and the helper's recipient gives each a graph while it stays inside.
//! A Timer retires the volume. Shadowkeep ships the same script, path, class names and fields, on
//! its own component base. Source helper `80802168` sits at definition `+70`, runtime `+30`, its
//! recipient `80802226` at helper `+280`, the Timer at `+4B0` and the render flag at `+60`.
//! Native helper `8080422E` sits at definition `+50`, its Timer at `+280` and the flag at `+48`.
use super::{
    owner::{expression, finish, lifetime},
    *,
};
use crate::d1_roi::{
    ability::controller::native::selection::Selection, native::damage_profile::NATIVE_DICTIONARY,
};

const NAME: &[u8] = b"abilities.ability_components.c_condition_vol_component_definition\0";

/// Whether a dynamic definition schema is the condition volume script on component base `base`,
/// source `80802361` or native `80803C57`. Many scripts share the base, so the class name decides.
pub(super) fn is_script(schema: &Payload, base: u32) -> bool {
    schema.u32(16).ok() == Some(base) && schema.0.windows(NAME.len()).any(|window| window == NAME)
}

/// The source recipient: its definition, runtime and the graph it gives each tracked entity.
fn recipient(p: &Payload) -> Result<(usize, u32)> {
    let (live, definition) = (p.pointer(16)?, p.pointer(24)?);
    let owner = p.u32(definition)?;
    let child = definition + 0x70 + 0x280;
    pattern::embedded(p, owner, (live + 0x50, child), (0x8080234A, 0x80802226))?;
    Ok((child, p.u32(child + 0xC0)?))
}

/// The graph a source condition volume owner gives the entities inside it.
pub(super) fn graph(p: &Payload) -> Result<u32> {
    Ok(recipient(p)?.1)
}

/// Write a source condition volume into a retargeted copy of a native template volume. The query
/// takes the source sphere, ally predicate, required component, vertical offset and scale
/// program, as the Radiance controller's query does. The recipient keeps its stock action,
/// tracking and lifecycle and takes the source's constant program, and the caller names the
/// translated recipient graph at the returned offset. The Timer takes the source flag and
/// lifetime, and the render flag carries.
pub(super) fn write(
    source: &Source,
    native: &mut Reader,
    target: &mut Payload,
    p: &Payload,
) -> Result<(Value, usize)> {
    let (live, definition) = (p.pointer(16)?, p.pointer(24)?);
    let owner = p.u32(definition)?;
    let registry = native.tag(NATIVE_DICTIONARY, None)?;
    let query = Selection::query(source, p, &registry, definition + 0x70, live + 0x30)?;

    // The recipient's constant program at `+20` and its `8080075B` record at `+88`. The program
    // ends at `+80` with the empty name, which the native recipient keeps at `+70`.
    let (child, recipient_graph) = recipient(p)?;
    pattern::embedded(
        p,
        owner,
        (live + 0x218, child + 0x20),
        (0x808008AC, 0x8080038D),
    )?;
    pattern::embedded(
        p,
        owner,
        (live + 0x98, child + 0x88),
        (0x80800555, 0x8080075B),
    )?;
    ensure!(
        p.u64(child + 0x20 + 88)? == 0 && p.u64(child + 0x80)? == 0x811C9DC5,
        "Source condition volume recipient program has flags or a key"
    );
    let (code, constants, input) = pattern::program(p, child + 0x20)?;
    ensure!(
        code == [0x34, 0, 0x3E, 0] && constants.len() == 1 && input.is_none(),
        "Source condition volume recipient program is not a constant"
    );
    let (flag, timer) = pattern::timer(p, owner, live + 0x280, definition + 0x4B0)?;
    let render = p.bytes::<16>(definition + 0x60)?;
    ensure!(
        render[0] <= 1 && render[1..].iter().all(|&byte| byte == 0),
        "Source condition volume render flag differs"
    );

    let (ni, nd) = pair(target)?;
    let id = target.u32(ni)?;
    let helper = nd + 0x50;
    let geometry = target.pointer(helper + 0x20)?;
    let actions = target.array(helper + 0x160, 0x20, Some(0x80804245))?;
    let [action] = actions.as_slice() else {
        anyhow::bail!("Native condition volume has no single recipient action");
    };
    let action = target.pointer(action + 0x10)?;
    let receiver = action + 0x18;
    let timer_at = nd + 0x280;
    ensure!(
        target.u32(helper)? == id
            && target.u32(helper + 4)? == 0x8080422D
            && target.u32(geometry - 4)? == 0x808092B1
            && target.u32(geometry)? == 2
            && target.u64(geometry + 8)? == 0
            && target.u32(action - 4)? == 0x80803C7E
            && target.u32(receiver)? == id
            && target.u32(receiver + 4)? == 0x80803B36
            && target.u64(receiver + 0x70)? == 0x811C9DC5
            && target.u32(receiver + 0x78)? == id
            && target.u32(receiver + 0x7C)? == 0x80808518
            && target.u32(timer_at)? == id
            && target.u32(timer_at + 4)? == 0x80803C50
            && target.bytes::<8>(nd + 0x48)?[1..]
                .iter()
                .all(|&byte| byte == 0),
        "Native condition volume template differs"
    );
    let (native_code, values) = expression(target, receiver + 0x18)?;
    ensure!(
        native_code == [0x34, 0, 0x3E, 0] && values.len() == 1,
        "Native condition volume recipient program is not a constant"
    );
    put(&mut target.0, values[0], &constants[0])?;
    put(&mut target.0, timer_at + 0x70, &flag)?;
    let timer_evidence = lifetime(target, id, timer_at + 0x78, &timer)?;
    put(&mut target.0, nd + 0x48, &[render[0]])?;
    let selection = query.write_at(target, helper, geometry)?;
    finish(target)?;
    Ok((
        json!({"query":selection,"recipient":{"graph":recipient_graph,
            "program":hex::encode(constants[0]),
            "source_capacity_words":[p.u32(child + 0x18)?, p.u32(child + 0x1C)?],
            "native_capacity":target.u64(receiver + 0x10)?},
            "timer":{"flag":hex::encode(flag),"lifetime":timer_evidence},
            "render_flag":render[0]}),
        receiver + 0xA0,
    ))
}
