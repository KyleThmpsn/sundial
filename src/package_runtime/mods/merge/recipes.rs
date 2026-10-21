//! The installed tables whose rows Sunrise merges, and the fields that name their rows, as
//! Sunrise Studio 007dd7a6 describes them (`TableMergeRecipes.cs`). A sidecar's ops and sites
//! carry these layouts, so mods built by different editors agree on them.
use super::sidecar::{FieldKind, OpKind};

/// A typed-array writer stops at this many rows.
pub const EDITOR_ROW_LIMIT: u32 = 1_048_576;
/// Native readers sign-extend a 16-bit row number.
pub const SIGNED_ROW_LIMIT: u32 = 0x8000;
/// These tables stop at the largest signed 16-bit value.
pub const SHORT_ROW_LIMIT: u32 = 0x7FFF;
/// An item display picks its stat group with a signed byte.
pub const STAT_GROUP_LIMIT: u32 = 128;

pub const ITEM_INDEX: u32 = 0x8132_7CCB;
pub const ITEM_DISPLAY_INDEX: u32 = 0x8161_3CF1;
pub const ICON_INDEX: u32 = 0x81A2_91C2;
pub const ITEM_ART: u32 = 0x81A2_8B23;
pub const COLLECTIBLES: u32 = 0x8132_7CDE;
pub const COLLECTIBLE_DISPLAYS: u32 = 0x8132_7D67;
pub const PRESENTATION_NODES: u32 = 0x8131_933F;
pub const PERK_DEFINITIONS: u32 = 0x8131_9327;
pub const PERK_DISPLAYS: u32 = 0x8161_3D28;
pub const SOCKET_TYPES: u32 = 0x8131_932F;
pub const SOCKET_TYPE_DISPLAYS: u32 = 0x8161_3D06;
pub const LORE_TEXT: u32 = 0x8161_3CF2;
pub const LORE_ROWS: u32 = 0x8131_9348;
pub const STAT_GROUPS: u32 = 0x8161_3D08;
pub const GEAR: u32 = 0x8161_3D27;
pub const GEAR_DEFINITIONS: u32 = 0x8131_9326;
pub const ARRANGEMENTS: u32 = 0x8161_3D23;
pub const ARRANGEMENT_DEFINITIONS: u32 = 0x8131_9329;
pub const VENDORS: u32 = 0x8131_931D;
pub const VENDOR_DISPLAYS: u32 = 0x81A2_926D;
pub const STRING_BANK_INDEX: u32 = 0x81A2_7211;
pub const RUNTIME_NAMES: u32 = 0x80EC_3F60;
pub const ART_NAMES: u32 = 0x80EC_3F61;
pub const HUD_ICONS: u32 = 0x80EF_C07B;
pub const PLUG_SETS: u32 = 0x8131_9349;
pub const NODE_DISPLAYS: u32 = 0x8161_3CF8;

pub const ITEM_CLASS: u32 = 0x8080_7BEA;
pub const ITEM_DISPLAY_CLASS: u32 = 0x8080_5CE1;
pub const VENDOR_CLASS: u32 = 0x8080_7850;
pub const VENDOR_DISPLAY_CLASS: u32 = 0x8080_58EE;
/// Loading manifests, whose group entries the loader unions.
pub const MANIFEST_CLASS: u32 = 0x8080_9EF9;

/// The descriptor of a table's main rows.
const ROWS: usize = 8;

/// A row field that holds a self-relative pointer.
#[derive(Clone, Copy, Debug)]
pub struct Field {
    pub offset: usize,
    pub kind: FieldKind,
    /// Element layout of an owned array, `None` when its elements hold no pointers.
    pub element: Option<&'static Shape>,
    /// Element size of an owned array, 0 when it isn't known.
    pub element_stride: usize,
    /// Descriptor offset of the array a row pointer points into.
    pub target_array: usize,
    /// A stock row whose array here grew gets a nested op.
    pub nestable: bool,
}

/// An object an element owns, at a self-relative pointer. Both must sit in the mod's added bytes.
#[derive(Clone, Copy, Debug)]
pub struct Local {
    pub offset: usize,
    pub target: &'static Shape,
}

/// Every pointer a row of this size holds.
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    pub stride: usize,
    pub fields: &'static [Field],
    pub local: &'static [Local],
}

/// One typed array the merge rebuilds, at the `{count, relative}` descriptor in the entry.
#[derive(Clone, Copy, Debug)]
pub struct Array {
    pub name: &'static str,
    pub descriptor: usize,
    pub kind: OpKind,
    pub row: &'static Shape,
    pub pair_group: Option<&'static str>,
    pub limit: u32,
    /// The u32 key's offset in a row.
    pub key_offset: Option<usize>,
    /// Keys must be unique across the merge. Set only when the stock keys are.
    pub unique: bool,
    /// Keys must stay strictly ascending. Set only when the stock keys are.
    pub ordered_when_stock_ascending: bool,
}

/// Every array of one installed table that the merge rebuilds.
#[derive(Clone, Copy, Debug)]
pub struct Recipe {
    pub tag: u32,
    pub name: &'static str,
    pub arrays: &'static [Array],
}

/// One step from the entry root toward a site field.
#[derive(Clone, Copy, Debug)]
pub enum Step {
    /// Follows the i64 at `offset`, keeping a target whose class word is `marker`, or any target
    /// when `marker` is 0.
    Pointer { offset: usize, marker: u32 },
    /// Visits each row of the array whose descriptor is at `offset`.
    Each { offset: usize, stride: usize },
    /// Visits rows `[first, first + count)` of the root array at `offset`. The first row is an i32
    /// at node `+first` and the count a byte at node `+count`.
    Pool {
        offset: usize,
        stride: usize,
        first: usize,
        count: usize,
    },
    /// Keeps a node whose byte at `offset` is `value`.
    Is { offset: usize, value: u8 },
}

/// A field whose value is a row number in a target array.
#[derive(Clone, Copy, Debug)]
pub struct Rule {
    pub name: &'static str,
    pub path: &'static [Step],
    pub field: usize,
    pub width: u8,
    pub signed: bool,
    pub table: u32,
    pub array: usize,
    /// The target is an array of the same entry.
    pub op_local: bool,
}

const fn plain(stride: usize) -> Shape {
    Shape {
        stride,
        fields: &[],
        local: &[],
    }
}

const fn owned(offset: usize, element_stride: usize, element: Option<&'static Shape>) -> Field {
    Field {
        offset,
        kind: FieldKind::OwnedArray,
        element,
        element_stride,
        target_array: 0,
        nestable: false,
    }
}

const fn append(
    name: &'static str,
    descriptor: usize,
    row: &'static Shape,
    pair_group: Option<&'static str>,
    limit: u32,
) -> Array {
    Array {
        name,
        descriptor,
        kind: OpKind::Append,
        row,
        pair_group,
        limit,
        key_offset: None,
        unique: false,
        ordered_when_stock_ascending: false,
    }
}

const fn unique_keys(array: Array) -> Array {
    Array {
        key_offset: Some(0),
        unique: true,
        ..array
    }
}

const fn sorted(name: &'static str, row: &'static Shape) -> Array {
    Array {
        name,
        descriptor: ROWS,
        kind: OpKind::SortedMap,
        row,
        pair_group: None,
        limit: EDITOR_ROW_LIMIT,
        key_offset: Some(0),
        unique: false,
        ordered_when_stock_ascending: false,
    }
}

/// Index rows: the hash at `+0`, the tag at `+16`.
const INDEX_ROW: Shape = plain(24);
/// A stat row's curve is a typed array of 8-byte points at `+8`.
const STAT_ROW: Shape = Shape {
    stride: 24,
    fields: &[owned(8, 8, None)],
    local: &[],
};
/// An arrangement holder points at a 24-byte group whose art hashes are a typed array at `+8`.
const HOLDER_GROUP: Shape = Shape {
    stride: 24,
    fields: &[owned(8, 4, None)],
    local: &[],
};
const HOLDER: Shape = Shape {
    stride: 8,
    fields: &[],
    local: &[Local {
        offset: 0,
        target: &HOLDER_GROUP,
    }],
};
const COLLECTIBLE_ROW: Shape = Shape {
    stride: 184,
    fields: &[
        owned(24, 2, None),
        owned(48, 0, None),
        owned(64, 0, None),
        owned(80, 0, None),
        owned(96, 0, None),
        owned(112, 0, None),
        owned(136, 0, None),
        owned(168, 0, None),
    ],
    local: &[],
};
/// A node's child-node row: the child's index, then the condition program that shows it, a typed
/// array at `+8`.
const CHILD_NODE_ROW: Shape = Shape {
    stride: 24,
    fields: &[owned(8, 0, None)],
    local: &[],
};
const NODE_ROW: Shape = Shape {
    stride: 168,
    fields: &[
        owned(24, 0, None),
        owned(48, 0, None),
        owned(64, 0, None),
        owned(88, 0, None),
        // Parhelion adds its badge and its pages after a stock node's own children. Studio's
        // recipe leaves the array fixed, and a mod that keeps its count merges with one that
        // grows it.
        Field {
            nestable: true,
            ..owned(104, 24, Some(&CHILD_NODE_ROW))
        },
        Field {
            nestable: true,
            ..owned(120, 4, None)
        },
        owned(136, 4, None),
        owned(152, 4, None),
    ],
    local: &[],
};
const PERK_DISPLAY_ROW: Shape = Shape {
    stride: 24,
    fields: &[Field {
        offset: 8,
        kind: FieldKind::RowPointer,
        element: None,
        element_stride: 0,
        target_array: 24,
        nestable: false,
    }],
    local: &[],
};
const SOCKET_TYPE_ROW: Shape = Shape {
    stride: 96,
    fields: &[
        owned(16, 0, None),
        owned(40, 8, None),
        owned(56, 0, None),
        owned(80, 0, None),
    ],
    local: &[],
};
const STAT_GROUP_ROW: Shape = Shape {
    stride: 56,
    fields: &[owned(16, 24, Some(&STAT_ROW)), owned(32, 0, None)],
    local: &[],
};
/// A gear row's `+8` points at an object, not an array.
const GEAR_ROW: Shape = Shape {
    stride: 48,
    fields: &[
        Field {
            offset: 8,
            kind: FieldKind::ObjectPointer,
            element: None,
            element_stride: 0,
            target_array: 0,
            nestable: false,
        },
        owned(32, 0, None),
    ],
    local: &[],
};
/// The `+24` group pool stays in place, since stock holders point into it.
const ARRANGEMENT_ROW: Shape = Shape {
    stride: 32,
    fields: &[owned(16, 8, Some(&HOLDER))],
    local: &[],
};

const PLAIN_4: Shape = plain(4);
const PLAIN_8: Shape = plain(8);
const PLAIN_16: Shape = plain(16);
const PLAIN_28: Shape = plain(28);
const PLAIN_40: Shape = plain(40);
const PLAIN_112: Shape = plain(112);
const COLLECTIBLE_DISPLAY_ROW: Shape = Shape {
    stride: 96,
    fields: &[owned(72, 0, None)],
    local: &[],
};

/// Every table whose rows the loader merges.
pub const TABLES: &[Recipe] = &[
    Recipe {
        tag: ITEM_INDEX,
        name: "item index",
        arrays: &[unique_keys(append(
            "rows",
            ROWS,
            &INDEX_ROW,
            Some("item"),
            SIGNED_ROW_LIMIT,
        ))],
    },
    Recipe {
        tag: ITEM_DISPLAY_INDEX,
        name: "item display index",
        arrays: &[unique_keys(append(
            "rows",
            ROWS,
            &INDEX_ROW,
            Some("item"),
            SIGNED_ROW_LIMIT,
        ))],
    },
    Recipe {
        tag: ICON_INDEX,
        name: "icon index",
        arrays: &[append("rows", ROWS, &INDEX_ROW, None, SIGNED_ROW_LIMIT)],
    },
    Recipe {
        tag: ITEM_ART,
        name: "item art",
        arrays: &[
            append("handles", 8, &PLAIN_4, None, EDITOR_ROW_LIMIT),
            append("integers", 24, &PLAIN_4, None, EDITOR_ROW_LIMIT),
            append("fields", 40, &PLAIN_8, None, EDITOR_ROW_LIMIT),
            append("items", 56, &PLAIN_16, Some("item"), SIGNED_ROW_LIMIT),
            // One presentation row per socket entry list, which the inspect screen reads by the
            // list's index. Studio's recipe stops at the items, as Studio adds no lists. A build
            // that adds a list grows this array, and a mod that doesn't keeps its stock count,
            // which the merge accepts.
            append("lists", 72, &PLAIN_8, None, EDITOR_ROW_LIMIT),
        ],
    },
    Recipe {
        tag: COLLECTIBLES,
        name: "collectibles",
        arrays: &[Array {
            key_offset: Some(40),
            unique: true,
            ..append(
                "rows",
                ROWS,
                &COLLECTIBLE_ROW,
                Some("collectible"),
                SIGNED_ROW_LIMIT,
            )
        }],
    },
    Recipe {
        tag: COLLECTIBLE_DISPLAYS,
        name: "collectible displays",
        arrays: &[append(
            "rows",
            ROWS,
            &COLLECTIBLE_DISPLAY_ROW,
            Some("collectible"),
            SIGNED_ROW_LIMIT,
        )],
    },
    Recipe {
        tag: PRESENTATION_NODES,
        name: "presentation nodes",
        arrays: &[append("rows", ROWS, &NODE_ROW, None, SIGNED_ROW_LIMIT)],
    },
    Recipe {
        tag: PERK_DEFINITIONS,
        name: "sandbox perk definitions",
        arrays: &[append(
            "rows",
            ROWS,
            &PLAIN_8,
            Some("perk"),
            SIGNED_ROW_LIMIT,
        )],
    },
    Recipe {
        tag: PERK_DISPLAYS,
        name: "sandbox perk displays",
        arrays: &[
            append(
                "rows",
                ROWS,
                &PERK_DISPLAY_ROW,
                Some("perk"),
                SIGNED_ROW_LIMIT,
            ),
            append("text", 24, &PLAIN_28, None, EDITOR_ROW_LIMIT),
        ],
    },
    Recipe {
        tag: SOCKET_TYPES,
        name: "socket types",
        arrays: &[append(
            "rows",
            ROWS,
            &SOCKET_TYPE_ROW,
            Some("socket_type"),
            SHORT_ROW_LIMIT,
        )],
    },
    Recipe {
        tag: SOCKET_TYPE_DISPLAYS,
        name: "socket type displays",
        arrays: &[append(
            "rows",
            ROWS,
            &PLAIN_40,
            Some("socket_type"),
            SHORT_ROW_LIMIT,
        )],
    },
    Recipe {
        tag: LORE_TEXT,
        name: "lore text",
        arrays: &[append(
            "rows",
            ROWS,
            &PLAIN_40,
            Some("lore"),
            SHORT_ROW_LIMIT,
        )],
    },
    Recipe {
        tag: LORE_ROWS,
        name: "lore rows",
        arrays: &[append(
            "rows",
            ROWS,
            &PLAIN_16,
            Some("lore"),
            SHORT_ROW_LIMIT,
        )],
    },
    Recipe {
        tag: STAT_GROUPS,
        name: "stat groups",
        arrays: &[append(
            "rows",
            ROWS,
            &STAT_GROUP_ROW,
            None,
            STAT_GROUP_LIMIT,
        )],
    },
    Recipe {
        tag: GEAR,
        name: "gear",
        arrays: &[append(
            "rows",
            ROWS,
            &GEAR_ROW,
            Some("gear"),
            SIGNED_ROW_LIMIT,
        )],
    },
    Recipe {
        tag: GEAR_DEFINITIONS,
        name: "gear definitions",
        arrays: &[append(
            "rows",
            ROWS,
            &PLAIN_8,
            Some("gear"),
            SIGNED_ROW_LIMIT,
        )],
    },
    Recipe {
        tag: ARRANGEMENTS,
        name: "arrangements",
        arrays: &[append(
            "rows",
            ROWS,
            &ARRANGEMENT_ROW,
            Some("arrangement"),
            SIGNED_ROW_LIMIT,
        )],
    },
    Recipe {
        tag: ARRANGEMENT_DEFINITIONS,
        name: "arrangement definitions",
        arrays: &[append(
            "rows",
            ROWS,
            &PLAIN_4,
            Some("arrangement"),
            SIGNED_ROW_LIMIT,
        )],
    },
    Recipe {
        tag: VENDORS,
        name: "vendor index",
        arrays: &[append(
            "rows",
            ROWS,
            &INDEX_ROW,
            Some("vendor"),
            SHORT_ROW_LIMIT,
        )],
    },
    Recipe {
        tag: VENDOR_DISPLAYS,
        name: "vendor display index",
        arrays: &[append(
            "rows",
            ROWS,
            &INDEX_ROW,
            Some("vendor"),
            SHORT_ROW_LIMIT,
        )],
    },
    Recipe {
        tag: STRING_BANK_INDEX,
        name: "string bank index",
        arrays: &[Array {
            ordered_when_stock_ascending: true,
            ..unique_keys(append("rows", ROWS, &PLAIN_8, None, SIGNED_ROW_LIMIT))
        }],
    },
    // The runtime name map's `+24` array isn't rebuilt. Studio never changes it.
    Recipe {
        tag: RUNTIME_NAMES,
        name: "runtime name map",
        arrays: &[sorted("rows", &PLAIN_8)],
    },
    Recipe {
        tag: ART_NAMES,
        name: "art name map",
        arrays: &[sorted("rows", &PLAIN_8)],
    },
    Recipe {
        tag: HUD_ICONS,
        name: "HUD icon library",
        arrays: &[sorted("rows", &PLAIN_112)],
    },
];

/// The recipe of an installed table, if the loader merges its rows.
pub fn table(tag: u32) -> Option<&'static Recipe> {
    TABLES.iter().find(|recipe| recipe.tag == tag)
}

const fn rule(
    name: &'static str,
    path: &'static [Step],
    field: usize,
    width: u8,
    table: u32,
) -> Rule {
    Rule {
        name,
        path,
        field,
        width,
        signed: false,
        table,
        array: ROWS,
        op_local: false,
    }
}

const SOCKET_ROWS: &[Step] = &[
    Step::Pointer {
        offset: 104,
        marker: 0x8080_77C1,
    },
    Step::Each {
        offset: 0,
        stride: 80,
    },
];
const SOCKET_PLUGS: &[Step] = &[
    Step::Pointer {
        offset: 104,
        marker: 0x8080_77C1,
    },
    Step::Each {
        offset: 0,
        stride: 80,
    },
    Step::Each {
        offset: 64,
        stride: 32,
    },
];
const ROOT: &[Step] = &[];
const TRANSLATION: &[Step] = &[Step::Pointer {
    offset: 136,
    marker: 0,
}];
const ART_ROWS: &[Step] = &[
    Step::Pointer {
        offset: 136,
        marker: 0,
    },
    Step::Each {
        offset: 0,
        stride: 4,
    },
];

const ITEM_RULES: &[Rule] = &[
    rule(
        "lore row",
        &[Step::Pointer {
            offset: 40,
            marker: 0x8080_77ED,
        }],
        0,
        2,
        LORE_TEXT,
    ),
    rule("socket type", SOCKET_ROWS, 0, 2, SOCKET_TYPES),
    rule("initial plug", SOCKET_ROWS, 2, 2, ITEM_INDEX),
    rule("socket plug", SOCKET_PLUGS, 0, 4, ITEM_INDEX),
    rule(
        "sandbox perk",
        &[
            Step::Pointer {
                offset: 112,
                marker: 0x8080_77B9,
            },
            Step::Each {
                offset: 16,
                stride: 24,
            },
        ],
        0,
        2,
        PERK_DEFINITIONS,
    ),
    rule("gear row", TRANSLATION, 88, 2, GEAR),
    rule("arrangement row", ART_ROWS, 2, 2, ARRANGEMENTS),
];

const ITEM_DISPLAY_RULES: &[Rule] = &[
    rule("icon row", ROOT, 128, 2, ICON_INDEX),
    rule("name bank", ROOT, 132, 4, STRING_BANK_INDEX),
    rule("type bank", ROOT, 144, 4, STRING_BANK_INDEX),
    rule("flavor bank", ROOT, 152, 4, STRING_BANK_INDEX),
    Rule {
        signed: true,
        ..rule(
            "stat group",
            &[Step::Pointer {
                offset: 112,
                marker: 0x8080_5CF1,
            }],
            20,
            1,
            STAT_GROUPS,
        )
    },
];

const SALES: &[Step] = &[Step::Each {
    offset: 48,
    stride: 184,
}];
const VENDOR_RULES: &[Rule] = &[
    rule("sale item", SALES, 70, 2, ITEM_INDEX),
    rule(
        "cost item",
        &[
            Step::Each {
                offset: 48,
                stride: 184,
            },
            Step::Each {
                offset: 32,
                stride: 48,
            },
        ],
        0,
        2,
        ITEM_INDEX,
    ),
];

const VENDOR_DISPLAY_RULES: &[Rule] = &[
    rule("name bank", ROOT, 64, 2, STRING_BANK_INDEX),
    rule("subtitle bank", ROOT, 72, 2, STRING_BANK_INDEX),
];

/// Site rules for single-record entries, by entry class.
pub fn class_rules(class: u32) -> &'static [Rule] {
    match class {
        ITEM_CLASS => ITEM_RULES,
        ITEM_DISPLAY_CLASS => ITEM_DISPLAY_RULES,
        VENDOR_CLASS => VENDOR_RULES,
        VENDOR_DISPLAY_CLASS => VENDOR_DISPLAY_RULES,
        _ => &[],
    }
}

/// Whether entries of a class can name rows a mod adds, so a mod's new package keeps them where
/// the loader reads them.
pub fn has_class_rules(class: u32) -> bool {
    !class_rules(class).is_empty()
}

const fn each_row(stride: usize) -> Step {
    Step::Each {
        offset: ROWS,
        stride,
    }
}

const COLLECTIBLE_RULES: &[Rule] = &[rule("item row", &[each_row(184)], 44, 2, ITEM_INDEX)];
const COLLECTIBLE_DISPLAY_RULES: &[Rule] = &[
    rule("icon row", &[each_row(96)], 4, 4, ICON_INDEX),
    rule("name bank", &[each_row(96)], 8, 4, STRING_BANK_INDEX),
    rule("flavor bank", &[each_row(96)], 16, 4, STRING_BANK_INDEX),
    rule("source bank", &[each_row(96)], 24, 4, STRING_BANK_INDEX),
];
const NODE_RULES: &[Rule] = &[rule(
    "collectible row",
    &[
        each_row(168),
        Step::Each {
            offset: 120,
            stride: 4,
        },
    ],
    0,
    2,
    COLLECTIBLES,
)];
const NODE_DISPLAY_RULES: &[Rule] = &[rule("name bank", &[each_row(44)], 8, 4, STRING_BANK_INDEX)];
const PERK_TEXT: &[Step] = &[Step::Each {
    offset: 24,
    stride: 28,
}];
const PERK_DISPLAY_RULES: &[Rule] = &[
    rule("icon row", PERK_TEXT, 16, 2, ICON_INDEX),
    rule("name bank", PERK_TEXT, 0, 4, STRING_BANK_INDEX),
    rule("description bank", PERK_TEXT, 8, 4, STRING_BANK_INDEX),
];
const LORE_TEXT_RULES: &[Rule] = &[
    rule("title bank", &[each_row(40)], 12, 2, STRING_BANK_INDEX),
    rule("subtitle bank", &[each_row(40)], 20, 2, STRING_BANK_INDEX),
    rule("body bank", &[each_row(40)], 28, 2, STRING_BANK_INDEX),
];
const PLUG_SET_RULES: &[Rule] = &[rule(
    "plug item",
    &[
        each_row(24),
        Step::Each {
            offset: 8,
            stride: 32,
        },
    ],
    0,
    4,
    ITEM_INDEX,
)];
const ART_FIELD: Step = Step::Each {
    offset: 40,
    stride: 8,
};
const fn art_rule(name: &'static str, path: &'static [Step], field: usize, array: usize) -> Rule {
    Rule {
        name,
        path,
        field,
        width: 4,
        signed: true,
        table: ITEM_ART,
        array,
        op_local: true,
    }
}
const ITEM_ART_RULES: &[Rule] = &[
    art_rule(
        "first field",
        &[Step::Each {
            offset: 56,
            stride: 16,
        }],
        8,
        40,
    ),
    art_rule(
        "first handle",
        &[
            ART_FIELD,
            Step::Is {
                offset: 4,
                value: 1,
            },
        ],
        0,
        8,
    ),
    art_rule(
        "first integer",
        &[
            ART_FIELD,
            Step::Is {
                offset: 4,
                value: 5,
            },
        ],
        0,
        24,
    ),
    rule(
        "sandbox perk",
        &[
            ART_FIELD,
            Step::Is {
                offset: 4,
                value: 5,
            },
            Step::Pool {
                offset: 24,
                stride: 4,
                first: 0,
                count: 5,
            },
        ],
        0,
        4,
        PERK_DEFINITIONS,
    ),
];

/// Site rules for the rows of installed tables, by table tag.
pub fn table_rules(tag: u32) -> &'static [Rule] {
    match tag {
        COLLECTIBLES => COLLECTIBLE_RULES,
        COLLECTIBLE_DISPLAYS => COLLECTIBLE_DISPLAY_RULES,
        PRESENTATION_NODES => NODE_RULES,
        NODE_DISPLAYS => NODE_DISPLAY_RULES,
        PERK_DISPLAYS => PERK_DISPLAY_RULES,
        LORE_TEXT => LORE_TEXT_RULES,
        PLUG_SETS => PLUG_SET_RULES,
        ITEM_ART => ITEM_ART_RULES,
        _ => &[],
    }
}

/// A pair group's id: FNV-1 over its lowercase ASCII name, multiplying before each XOR.
pub fn pair_group_id(name: &str) -> u32 {
    name.bytes().fold(0x811C_9DC5, |hash: u32, byte| {
        hash.wrapping_mul(0x0100_0193) ^ u32::from(byte.to_ascii_lowercase())
    })
}
