//! Positions in a stock subclass's socket-entry list.

/// Entries every stock list holds.
pub const ENTRY_COUNT: usize = 24;
/// The base melee target that each attunement's melee links to. It differs by class.
pub const CLASS_BASE: u8 = 0;
/// The always-selected movement foundation and translated stat contributions.
pub const BASE_MOVEMENT: u8 = 1;
pub const STAT_PASSIVES: u8 = 19;
pub const FOUNDATIONS: [u8; 2] = [BASE_MOVEMENT, STAT_PASSIVES];
pub const CLASS_ABILITIES: [u8; 2] = [2, 3];
pub const MOVEMENT: [u8; 3] = [4, 5, 6];
pub const GRENADES: [u8; 3] = [7, 8, 9];
/// The super every attunement uses unless the middle one brings its own.
pub const SUPER: u8 = 10;
/// Attunement entries: top, bottom and middle. Top and bottom lead with their melee, and the
/// middle attunement leads with its own super.
pub const ATTUNEMENTS: [[u8; 4]; 3] = [[11, 12, 13, 14], [15, 16, 17, 18], [20, 21, 22, 23]];
/// Attunement entries that hold an ability rather than a passive node: the melee that leads the
/// top and bottom paths, and the middle path's own super and melee, the pairs Dawn selects as
/// 10 with 11, 10 with 15, and 20 with 21.
pub const PATH_ABILITIES: [u8; 4] = [11, 15, 20, 21];
/// Nodes in an attunement path.
pub const PATH_NODES: u8 = 4;
/// The node that leads a path: its melee in the top and bottom paths, its super in the
/// middle one. It takes only another path's lead node.
pub const LEAD_NODE: u8 = 0;
