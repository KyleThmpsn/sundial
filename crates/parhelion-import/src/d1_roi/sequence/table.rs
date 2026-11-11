//! Grouped presentation events play one group of a presentation table.
//!
//! Rise of Iron `56578.17.04.12.1251.evil` kind 27, class `8080237E`, corresponds to Shadowkeep
//! `86657.20.08.23.1800.d2_rc___release` kind 30, class `80804AB4`. Thirteen event names occur in
//! both builds with the same timing, group kind, bounds and first two flag bytes, and native
//! sequence `80BC57DB` ports the Incendiary Rounds explosion sequence with the same values:
//!
//! | Role | Source | Native |
//! | --- | --- | --- |
//! | Group kind | `+20` | `+40` |
//! | Table | `+30` | `+50` |
//! | Bounds | `+38`, `+3C` | `+70`, `+74` |
//! | Locator | `+40`, `-1` for none | `+78` |
//! | Flags | `+44`, two bytes | `+7C`, with byte 2 set |
//!
//! Every paired native event sets flag byte 2, which no source event does. A source event without
//! a locator plays at the owner origin. Native events always name one, and the port adds an
//! empty-named kind-0 locator for the event to name, so the owner writer adds the same one.
//! Tables keep their bytes. Every ported table differs only in its array markers, group and row
//! classes and leaf tags.
use super::*;
use crate::d1_roi::{Source, dependency};

const SOURCE_MARKER: u32 = 0x80800184;
const NATIVE_MARKER: u32 = 0x80809FBD;
const GROUP: (u32, u32) = (0x808006C6, 0x80808BD5);
const ROW: (u32, u32) = (0x80800665, 0x80808BD7);
const LEAF: u32 = 0x80800014;
const CUE: (u32, u32) = (0x8080080A, 0x80809802);
const GRAPH: (u32, u32) = (0x80800734, 0x80809C0F);

pub struct Table {
    offset: usize,
    table: u32,
    group: u32,
    bounds: [u8; 8],
    /// Source locator ordinal. A source event without one names the origin locator the owner
    /// writer appends after the source locators.
    locator: u8,
    origin: bool,
    flags: [u8; 2],
    /// The native table with every leaf slot unset.
    payload: Payload,
    /// Leaf slots with their source resource and its source class.
    leaves: Vec<(usize, u32, u32)>,
    /// The sequence's sound became delays, so cue leaves stay empty too.
    silent: bool,
}

/// Array extents already read, so aliased or overlapping arrays and unread bytes refuse.
struct Spans<'a> {
    p: &'a Payload,
    spans: BTreeMap<usize, usize>,
    markers: Vec<(usize, u32)>,
}

impl Spans<'_> {
    fn array(&mut self, descriptor: usize, stride: usize, class: (u32, u32)) -> Result<Vec<usize>> {
        let rows = self.p.array(descriptor, stride, Some(class.0))?;
        if rows.is_empty() {
            ensure!(
                self.p.u64(descriptor + 8)? == 0,
                "Source presentation table empty array has a pointer"
            );
            return Ok(rows);
        }
        let header = self.p.pointer(descriptor + 8)?;
        ensure!(
            header % 16 == 0
                && self.p.u32(header - 4)? == SOURCE_MARKER
                && self.p.u32(header + 12)? == 0,
            "Source presentation table array envelope differs"
        );
        let (start, end) = (header - 4, header + 16 + rows.len() * stride);
        ensure!(
            self.spans
                .range(..=start)
                .next_back()
                .is_none_or(|(_, previous)| *previous <= start)
                && self
                    .spans
                    .range(start..)
                    .next()
                    .is_none_or(|(next, _)| *next >= end),
            "Source presentation table arrays overlap"
        );
        self.spans.insert(start, end);
        self.markers.push((header, class.1));
        Ok(rows)
    }

    fn finish(&self) -> Result<()> {
        let mut end = 0xC8;
        for (&start, &next) in &self.spans {
            ensure!(
                start >= end && self.p.0[end..start].iter().all(|&byte| byte == 0),
                "Source presentation table has unread data"
            );
            end = next;
        }
        ensure!(
            end == self.p.0.len(),
            "Source presentation table has an unread trailer"
        );
        Ok(())
    }
}

impl Table {
    pub fn read(source: &Source, owner: &Payload, at: usize, silent: bool) -> Result<Self> {
        ensure!(
            owner.u32(at.checked_sub(4).context("Source table event marker")?)? == 0x8080237E
                && owner.u32(at + 0x1C)? == 27,
            "Source table event class or kind differs"
        );
        ensure!(
            owner.bytes::<12>(at + 0x24)? == [0; 12] && owner.u32(at + 0x34)? == 0,
            "Source table event reserved fields differ"
        );
        let bounds = owner.bytes::<8>(at + 0x38)?;
        let (lower, upper) = (owner.f32(at + 0x38)?, owner.f32(at + 0x3C)?);
        ensure!(
            lower.is_finite() && upper.is_finite() && lower <= upper,
            "Source table event bounds are invalid"
        );
        let flags = owner.bytes::<4>(at + 0x44)?;
        ensure!(
            flags[..2].iter().all(|&flag| flag <= 1) && flags[2..] == [0, 0],
            "Source table event flags need translation"
        );
        let declared = owner
            .array(owner.pointer(24)? + 0x1B0, 5, Some(0x80800429))?
            .len();
        let (locator, origin) = match owner.u32(at + 0x40)? {
            u32::MAX => (declared, true),
            locator => {
                ensure!(
                    (locator as usize) < declared,
                    "Source table event locator is undeclared"
                );
                (locator as usize, false)
            }
        };
        let table = owner.u32(at + 0x30)?;
        ensure!(
            source.reference(table)? == 0x808005F7,
            "Source presentation table class differs"
        );
        let data = source.read(table)?;
        ensure!(
            data.0.len() >= 0xC8 && data.u64(0)? == data.0.len() as u64,
            "Source presentation table size differs"
        );
        let mut spans = Spans {
            p: &data,
            spans: BTreeMap::new(),
            markers: Vec::new(),
        };
        let mut leaves = Vec::new();
        for descriptor in [0x80, 0x98, 0xB0] {
            for group in spans.array(descriptor, 24, GROUP)? {
                for row in spans.array(group + 8, 32, ROW)? {
                    for leaf in spans.array(row + 8, 4, (LEAF, LEAF))? {
                        let tag = data.u32(leaf)?;
                        if tag == u32::MAX {
                            continue;
                        }
                        let class = source.reference(tag)?;
                        ensure!(
                            class == CUE.0 || class == GRAPH.0,
                            "Source presentation table leaf class {class:08X} needs translation"
                        );
                        leaves.push((leaf, tag, class));
                    }
                }
            }
        }
        spans.finish()?;
        let mut payload = data.clone();
        for &(header, class) in &spans.markers {
            put(&mut payload.0, header - 4, &NATIVE_MARKER.to_le_bytes())?;
            put(&mut payload.0, header + 8, &class.to_le_bytes())?;
        }
        for &(leaf, _, _) in &leaves {
            put(&mut payload.0, leaf, &u32::MAX.to_le_bytes())?;
        }
        Ok(Self {
            offset: at,
            table,
            group: owner.u32(at + 0x20)?,
            bounds,
            locator: u8::try_from(locator).context("Source table event locator ordinal")?,
            origin,
            flags: [flags[0], flags[1]],
            payload,
            leaves,
            silent,
        })
    }

    /// The private table, separate when cue leaves stay empty.
    pub fn symbol(&self) -> String {
        let silent = if self.silent { "-silent" } else { "" };
        format!("presentation-table-{:08X}{silent}", self.table)
    }

    /// The native locator ordinal this event names, the appended origin when it has none.
    pub fn locator(&self) -> u8 {
        self.locator
    }

    /// The locator ordinal the owner writer must append as the origin, if this event needs it.
    pub fn origin(&self) -> Option<u8> {
        self.origin.then_some(self.locator)
    }

    fn leaves(&self, class: u32) -> impl Iterator<Item = u32> + '_ {
        self.leaves
            .iter()
            .filter(move |leaf| leaf.2 == class)
            .map(|leaf| leaf.1)
    }

    pub fn graphs(&self) -> impl Iterator<Item = u32> + '_ {
        self.leaves(GRAPH.0)
    }

    pub fn cues(&self) -> impl Iterator<Item = u32> + '_ {
        self.leaves(CUE.0).filter(|_| !self.silent)
    }

    pub fn mapping(&self) -> (usize, NativeEvent) {
        (
            self.offset,
            NativeEvent {
                source_class: 0x8080237E,
                source_kind: 27,
                class: 0x80804AB4,
            },
        )
    }

    /// The native table, whose leaves name private cues, translated graphs or the stock graphs
    /// a native port creates instead. Cue leaves of a silent sequence stay empty.
    pub fn translate(
        &self,
        cues: &BTreeMap<u32, String>,
        graphs: &BTreeMap<u32, String>,
        stock: &BTreeMap<u32, u32>,
    ) -> Result<dependency::Asset> {
        let mut payload = self.payload.clone();
        let mut patches = Vec::new();
        for &(offset, tag, class) in &self.leaves {
            let (symbol, class) = if class == CUE.0 {
                if self.silent {
                    continue;
                }
                (cues.get(&tag), CUE.1)
            } else if let Some(stock) = stock.get(&tag) {
                put(&mut payload.0, offset, &stock.to_le_bytes())?;
                continue;
            } else {
                (graphs.get(&tag), GRAPH.1)
            };
            patches.push(dependency::Patch {
                offset,
                symbol: symbol
                    .with_context(|| format!("Presentation table resource {tag:08X} is unbound"))?
                    .clone(),
                class,
            });
        }
        Ok(dependency::Asset {
            symbol: self.symbol(),
            class: 0x80808BCD,
            payload,
            patches,
        })
    }

    pub fn write(
        &self,
        target: &mut Payload,
        event: usize,
        locators: &BTreeMap<u8, u8>,
    ) -> Result<ResourcePatch> {
        target.bytes::<0x80>(event)?;
        let instance = usize::try_from(target.u64(event + 8)?)?;
        target.bytes::<0x40>(instance)?;
        ensure!(
            target.u32(event.checked_sub(4).context("Native table event marker")?)? == 0x80804AB4
                && target.u32(event + 0x30)? == 30
                && target.u32(event + 4)? == 0x80804AB3
                && target.u32(
                    instance
                        .checked_sub(4)
                        .context("Native table runtime marker")?
                )? == 0x80804AB3
                && target.u32(instance + 4)? == 0x80804AB4
                && target.u32(instance)? == target.u32(event)?
                && target.u64(instance + 8)? == event as u64
                && instance + 0x40 <= event,
            "Native table event pair or kind differs"
        );
        let locator = *locators
            .get(&self.locator)
            .context("Table locator has no native mapping")?;
        let mut output = target.clone();
        // The common writer gave the event its timing, parent and selector. Every stock runtime
        // tail is zero.
        put(&mut output.0, instance + 0x18, &[0; 0x28])?;
        put(&mut output.0, event + 0x40, &self.group.to_le_bytes())?;
        put(&mut output.0, event + 0x44, &[0; 12])?;
        put(&mut output.0, event + 0x50, &u32::MAX.to_le_bytes())?;
        put(&mut output.0, event + 0x54, &[0; 0x1C])?;
        put(&mut output.0, event + 0x70, &self.bounds)?;
        put(
            &mut output.0,
            event + 0x78,
            &u32::from(locator).to_le_bytes(),
        )?;
        put(
            &mut output.0,
            event + 0x7C,
            &[self.flags[0], self.flags[1], 1, 0],
        )?;
        *target = output;
        Ok(ResourcePatch {
            offset: event + 0x50,
            symbol: self.symbol(),
            source: self.table,
            source_class: 0x808005F7,
            native_class: 0x80808BCD,
        })
    }
}
