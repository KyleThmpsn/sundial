//! Evaluate the emitted color route from its consumers, using the native icon and decision
//! contracts. A stored donor color must not defeat subclass inheritance, an explicit child must
//! win, a Super's own color must stay separate from its theme, and stock selections must retain
//! their original theme. This checks selected values rather than agreement with the writer.
use super::*;

#[derive(Clone, Copy, Debug)]
enum Value {
    Boolean(bool),
    Color([u8; 4]),
}

struct Route<'a> {
    bytes: &'a [u8],
    bindings: Vec<usize>,
    classes: BTreeMap<u16, u64>,
    icons: BTreeMap<u16, &'a [u8]>,
    trace: Vec<serde_json::Value>,
}

impl Route<'_> {
    fn input(&mut self, component: u16, property: u32, depth: usize) -> Value {
        assert!(depth < 16, "the color route must terminate");
        let incoming = self
            .bindings
            .iter()
            .copied()
            .filter(|&at| {
                cui_field(self.bytes, at + 24) as u16 == component
                    && cui_field(self.bytes, at + 40) == property
                    && cui_path(self.bytes, at + 32).is_empty()
            })
            .collect::<Vec<_>>();
        assert_eq!(incoming.len(), 1, "each input has one effective source");
        let at = incoming[0];
        let source = cui_field(self.bytes, at) as u16;
        let selector = cui_field(self.bytes, at + 16);
        let path = cui_path(self.bytes, at + 8);
        let value = if path == [0x600, 0] {
            let row = self.icons[&source];
            let lane = (selector >> 16) as usize;
            assert!(lane < 4, "native icon data exposes four colors");
            match selector & 0xFFFF {
                7 => Value::Boolean(row[0x60] & (1 << lane) != 0),
                6 => Value::Color(std::array::from_fn(|channel| {
                    let start = 0x20 + lane * 16 + channel * 4;
                    let value = f32::from_le_bytes(row[start..start + 4].try_into().unwrap());
                    let value = if channel == 3 {
                        f64::from(value)
                    } else {
                        f64::from(value).powf(1.0 / 2.2)
                    };
                    (value * 255.0).round() as u8
                })),
                _ => panic!("unsupported icon property {selector:08X}"),
            }
        } else {
            assert!(path.is_empty());
            assert_eq!(selector, 0x205, "read the decision's result");
            assert_eq!(self.classes[&source], 0xFBB44B29);
            let Value::Boolean(condition) = self.input(source, 0x202, depth + 1) else {
                panic!("a decision needs a native Boolean");
            };
            self.input(source, if condition { 0x203 } else { 0x204 }, depth + 1)
        };
        self.trace.push(serde_json::json!({
            "from":source,"path":path,"selector":selector,
            "to":component,"property":property,"value":format!("{value:?}")
        }));
        value
    }
}

pub(super) fn check(
    manager: &PackageManager,
    rows: &GlyphRows,
    selected_super: u32,
    ability: u32,
    expected: [u8; 3],
) -> serde_json::Value {
    let mut observations = Vec::new();
    for widget in [0x80BC6F57, 0x80BC6FB5, 0x80BC7261] {
        let bytes = manager.read_tag(TagHash(widget)).unwrap();
        let mut route = Route {
            bindings: cui_array(&bytes, 0x58, 56, 0x808046D8),
            classes: cui_array(&bytes, 0x38, 16, 0x80804622)
                .into_iter()
                .map(|at| {
                    (
                        (cui_field(&bytes, at + 8) >> 16) as u16,
                        u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap()),
                    )
                })
                .collect(),
            icons: BTreeMap::from([
                (0x2A1, rows[&ability].as_slice()),
                (0x2B5, rows[&selected_super].as_slice()),
            ]),
            bytes: &bytes,
            trace: Vec::new(),
        };
        for target in [0x29A, 0x29B] {
            let Value::Color(color) = route.input(target, 0x203, 0) else {
                panic!("the tile consumes an RGBA color");
            };
            assert_eq!(color[..3], expected, "HUD {widget:08X}, tile {target:X}");
            assert_eq!(color[3], 255);
            observations.push(serde_json::json!({
                "widget":format!("{widget:08X}"),"target":target,"rgba":color,
                "trace":std::mem::take(&mut route.trace)
            }));
        }
    }
    serde_json::json!({
        "super_glyph":format!("{selected_super:08X}"),
        "ability_glyph":format!("{ability:08X}"),"selected":observations
    })
}
