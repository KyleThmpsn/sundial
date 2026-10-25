//! Entity-visible movement providers, checked against both dispatch tables.
use super::{Movement, Relocation};
use crate::d2_mot::{
    entity::links::{Interface, Object},
    payload::Payload,
};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

pub struct Interfaces {
    pub objects: Vec<Relocation>,
    /// Source interfaces absent from Shadowkeep. A graph using one must fail.
    pub unmapped: Vec<Object>,
    /// Nonzero state on a Source-only provider remains a whole-entity obligation
    /// even when the graph has no explicit connection to that provider.
    pub active_unmapped: Vec<(Object, u64)>,
}

struct Provider {
    source: usize,
    target: usize,
    classes: [u32; 2],
    methods: &'static [(u32, u32)],
}

const MOVEMENT: &[(u32, u32)] = &[
    (0x1F, 0x1E),
    (0x20, 0x1F),
    (0x21, 0x20),
    (0x22, 0x21),
    (0x24, 0x23),
    (0x23, 0x22),
    (0x26, 0x25),
    (0x28, 0x27),
    (0x29, 0x28),
    (0x2A, 0x29),
    (0x2B, 0x2A),
    (0x2C, 0x2B),
    (0x2D, 0x2C),
    (0x2E, 0x2D),
    (0x2F, 0x2E),
    (0x30, 0x2F),
    (0x31, 0x30),
    (0x27, 0x26),
    (0x32, 0x31),
    (0x33, 0x32),
    (0x34, 0x33),
    (0x35, 0x34),
    (0x39, 0x38),
    (0x1E, 0x1D),
    (0x36, 0x35),
    (0x37, 0x36),
    (0x38, 0x37),
];

const PROVIDERS: &[Provider] = &[
    Provider {
        source: 0x390,
        target: 0x2D0,
        classes: [0x8080939D, 0x8080965D],
        methods: &[(0x3B, 0x39)],
    },
    Provider {
        source: 0x3B0,
        target: 0x2E8,
        classes: [0x8080955E, 0x8080974B],
        methods: &[(0x3C, 0x3A)],
    },
    Provider {
        source: 0x3D8,
        target: 0x308,
        classes: [0x8080955E, 0x8080974B],
        methods: &[(0x3D, 0x3B)],
    },
    Provider {
        source: 0x400,
        target: 0x328,
        classes: [0x8080955E, 0x8080974B],
        methods: &[(0x3E, 0x3C)],
    },
    Provider {
        source: 0x428,
        target: 0x348,
        classes: [0x8080955E, 0x8080974B],
        methods: &[(0x3F, 0x3D)],
    },
    Provider {
        source: 0x450,
        target: 0x368,
        classes: [0x80802A07, 0x80803802],
        methods: &[(0, 0), (1, 1)],
    },
    Provider {
        source: 0x470,
        target: 0x380,
        classes: [0x80802AA3, 0x80803891],
        methods: MOVEMENT,
    },
    Provider {
        source: 0x4B0,
        target: 0x398,
        classes: [0x808098C9, 0x80809ADA],
        methods: &[(0x25, 0x24)],
    },
    Provider {
        source: 0x4D0,
        target: 0x3B0,
        classes: [0x808098CD, 0x80809ADE],
        methods: &[(5, 5)],
    },
    Provider {
        source: 0x4F0,
        target: 0x3C8,
        classes: [0x808091A8, 0x80809494],
        methods: &[(0x1B, 0x1B)],
    },
    Provider {
        source: 0x530,
        target: 0x3E0,
        classes: [0x80809AB8, 0x80809BF0],
        methods: &[(0x1D, 0x1C)],
    },
    Provider {
        source: 0x550,
        target: 0x3F8,
        classes: [0x8080979E, 0x80809684],
        methods: &[(6, 6), (7, 7), (8, 8)],
    },
];

fn provider(
    owner: &Payload,
    root: usize,
    at: usize,
    class: u32,
    metadata: &BTreeMap<u32, Payload>,
    modern: bool,
) -> Result<(Object, Interface)> {
    let object = Object {
        owner: owner.u32(root)?,
        class,
        offset: at as u64,
    };
    let info = metadata
        .get(&owner.u32(at + 8)?)
        .with_context(|| format!("movement provider metadata missing at {at:X}"))?;
    let interface = Interface::read(owner, object, info, modern)?;
    ensure!(
        interface.definition_offset == root && (!modern || owner.u64(at + 24)? == 0),
        "movement provider parent or Source extension differs at {at:X}"
    );
    Ok((object, interface))
}

fn methods(interface: &Interface, class: u32, expected: impl Iterator<Item = u32>) -> Result<()> {
    let expected: Vec<_> = expected.collect();
    ensure!(
        interface.methods.len() == expected.len(),
        "movement provider method count differs"
    );
    for (entry, index) in interface.methods.iter().zip(expected) {
        ensure!(
            entry.implementation_class == class
                && entry.index == index
                && entry.arguments == [0, 0],
            "movement provider dispatch contract differs"
        );
    }
    Ok(())
}

impl Movement {
    /// Link each supported provider through its independently serialized native
    /// interface. Method ordinals remain those of the native metadata. The
    /// caller reads all referenced metadata from the corresponding package set.
    pub fn interfaces(
        &self,
        source: &Payload,
        metadata: &BTreeMap<u32, Payload>,
    ) -> Result<Interfaces> {
        let settings = self
            .objects
            .iter()
            .find(|row| row.source.class == 0x80809A9F)
            .context("movement settings relocation is absent")?;
        let source_settings = usize::try_from(settings.source.offset)?;
        let native_settings = usize::try_from(settings.target.offset)?;
        ensure!(
            settings.target.class == 0x80809BD9,
            "movement settings relocation class differs"
        );
        let sd = source.pointer(24)?;
        let nd = self.owner.pointer(24)?;
        let mut objects = Vec::new();
        for row in 0..18 {
            let (from, si) = provider(
                source,
                sd,
                source_settings + 0xC0 + row * 0x28,
                0x808098D2,
                metadata,
                true,
            )?;
            let (to, ni) = provider(
                &self.owner,
                nd,
                native_settings + 0x90 + row * 0x20,
                0x80809AE1,
                metadata,
                false,
            )?;
            ensure!(
                si.instance_class == 0x808098D3 && ni.instance_class == 0x80809AE2,
                "movement scalar provider instance class differs"
            );
            methods(&si, 0x80802DCC, std::iter::once(9 + row as u32))?;
            methods(&ni, 0x80803B73, std::iter::once(9 + row as u32))?;
            ensure!(
                source.u64(from.offset as usize + 32)?
                    == self.owner.u64(to.offset as usize + 24)?,
                "movement provider binding name differs"
            );
            ensure!(
                source.u64(from.offset as usize + 16)?
                    == self.owner.u64(to.offset as usize + 16)?,
                "movement provider state differs"
            );
            objects.push(Relocation {
                source: from,
                target: to,
            });
        }
        for layout in PROVIDERS {
            let (from, si) = provider(
                source,
                sd,
                source_settings + layout.source,
                layout.classes[0],
                metadata,
                true,
            )?;
            let (to, ni) = provider(
                &self.owner,
                nd,
                native_settings + layout.target,
                layout.classes[1],
                metadata,
                false,
            )?;
            // These interface pairs are adjacent instance and definition ids.
            ensure!(
                si.instance_class + 1 == layout.classes[0]
                    && ni.instance_class + 1 == layout.classes[1],
                "movement provider instance class differs"
            );
            methods(&si, 0x80802DCC, layout.methods.iter().map(|pair| pair.0))?;
            methods(&ni, 0x80803B73, layout.methods.iter().map(|pair| pair.1))?;
            ensure!(
                source.u64(from.offset as usize + 16)?
                    == self.owner.u64(to.offset as usize + 16)?,
                "movement provider state differs"
            );
            objects.push(Relocation {
                source: from,
                target: to,
            });
        }
        let mut unmapped = Vec::new();
        let mut active_unmapped = Vec::new();
        for (offset, class, instance_class, index) in [
            (0x490, 0x80802284, 0x80802283, 0x3A),
            (0x510, 0x8080BD49, 0x8080BD48, 0x1C),
        ] {
            let (object, interface) =
                provider(source, sd, source_settings + offset, class, metadata, true)?;
            ensure!(
                interface.instance_class == instance_class,
                "source-only movement interface class differs"
            );
            methods(&interface, 0x80802DCC, std::iter::once(index))?;
            let state = source.u64(object.offset as usize + 16)?;
            if state != 0 {
                active_unmapped.push((object, state));
            }
            unmapped.push(object);
        }
        Ok(Interfaces {
            objects,
            unmapped,
            active_unmapped,
        })
    }
}
