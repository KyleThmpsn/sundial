//! Required native query action and its independently owned damage recipient.
use super::{Owner, defaults, layout};
use crate::{
    presentation::{append, put},
    tiger::{
        allocation::Row,
        entity::lifecycle,
        sequence::{object, relative},
    },
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};

pub(super) struct Prepared {
    pub mutable_end: usize,
    pub allocation: Row,
    pub evidence: Value,
}

pub(super) fn write(owner: &mut Owner) -> Result<Prepared> {
    let helper = owner.runtime + 0xCF0;
    let settings = owner.definition + 0x7F0;
    for (class, size) in [
        (0x80804244, 0x30),
        (0x80804245, 0x18),
        (0x80803C7D, 0xF0),
        (0x80803C7E, 0x118),
        (0x80803B36, 0xD0),
        (0x80803B28, 0x100),
        (0x80803B2C, 20),
    ] {
        ensure!(
            layout(class)?.0 == size,
            "Native query action declaration differs"
        );
    }
    ensure!(
        owner.payload.u64(helper + 0x20)? == 0 && owner.payload.u64(settings + 0x160)? == 0,
        "Retained query already contains actions"
    );
    append(
        &mut owner.payload.0,
        helper + 0x20,
        0x80804244,
        &[0; 48],
        48,
    )?;
    append(
        &mut owner.payload.0,
        settings + 0x160,
        0x80804245,
        &[0; 24],
        24,
    )?;
    let wrapper = owner.payload.array(helper + 0x20, 48, Some(0x80804244))?[0];
    let wrapper_definition = owner
        .payload
        .array(settings + 0x160, 24, Some(0x80804245))?[0];
    let action = object(&mut owner.payload, 0x80803C7D, 0xF0)?;
    let definition = object(&mut owner.payload, 0x80803C7E, 0x118)?;
    put(
        &mut owner.payload.0,
        action,
        &defaults::runtime_query_action(),
    )?;
    put(
        &mut owner.payload.0,
        definition,
        &defaults::definition_query_action(),
    )?;
    relative(&mut owner.payload, wrapper + 32, action)?;
    relative(&mut owner.payload, wrapper_definition + 16, definition)?;
    let recipient = action + 0x20;
    let recipient_definition = definition + 0x18;
    for (at, twin, classes, parent) in [
        (
            wrapper,
            wrapper_definition,
            (0x80804244, 0x80804245),
            helper,
        ),
        (action, definition, (0x80803C7D, 0x80803C7E), wrapper),
        (
            recipient,
            recipient_definition,
            (0x80803B36, 0x80803B28),
            action,
        ),
        (
            recipient + 0x50,
            recipient_definition + 0x78,
            (0x80808518, 0x80808519),
            recipient,
        ),
        (
            recipient + 0xA0,
            recipient_definition + 0x18,
            (0x808089F5, 0x808089F6),
            recipient,
        ),
    ] {
        owner.pair(at, twin, classes, Some(parent))?;
    }
    // Native F70A70 always finds 3C7D and updates its embedded 3B36, including when the
    // selected damage resource is absent. Presentation-only mode still requires this object.
    // Keep declaration-default resources absent rather than importing unrelated donor damage.
    // Stock 3C7D owners allocate sixteen tracking rows. 3B36:0 initializes their handles,
    // and 3B36:1 retires them. Register this lifecycle before the containing helper.
    const CAPACITY: usize = 16;
    append(
        &mut owner.payload.0,
        recipient + 0x88,
        0x80803B2C,
        &[0; CAPACITY * 20],
        20,
    )?;
    owner.initializers.push(lifecycle::Method {
        target: recipient,
        implementation: 0x80803B36,
        constructor: 0,
        cleanup: 1,
    });
    let allocation = Row::named(
        0xC3BA2CF7,
        u32::MAX,
        0,
        vec![Row::named(
            0xDF161D4E,
            0x80804244,
            1,
            vec![Row::named(
                0x811C9DC5,
                u32::MAX,
                0,
                vec![Row::named(
                    0xD69238E4,
                    0x80803C7D,
                    1,
                    vec![Row::named(
                        0x811C9DC5,
                        u32::MAX,
                        0,
                        vec![Row::named(
                            0x0AB82CDE,
                            u32::MAX,
                            0,
                            vec![Row::named(0xF32171C3, 0x80803B2C, CAPACITY, vec![])],
                        )],
                    )],
                )],
            )],
        )],
    );
    owner.size()?;
    Ok(Prepared {
        mutable_end: owner.payload.0.len(),
        allocation,
        evidence: json!({"runtime_class":0x80803C7Du32,"definition_class":0x80803C7Eu32,
            "action":action,"recipient":recipient,"tracking_capacity":CAPACITY,
            "constructor":0,"cleanup":1,"damage_resource":null}),
    })
}
