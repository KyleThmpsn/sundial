//! Catalog of the native perk action nodes registered by the supported client.
//!
//! Build 86657.20.08.23. The counts are occurrences across the 1,632 action resources
//! recovered from the installed packages, not a count of distinct perks. A name here
//! records a traced operation. It is not a claim that every field of a node is mapped
//! or that a given combination works in game. See `docs/perk-runtime-map-2026-09-10.md`
//! for the recovery method and the remaining semantic gaps.

/// How far Parhelion supports one native node kind today.
#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    /// The action compiler can emit this node from an authored program.
    Authorable,
    /// The decoder reads this node's mapped fields, but the compiler cannot emit it.
    Readable,
    /// The node's class and size are known. Its individual fields are not mapped.
    Structural,
    /// The client registers this kind, but no surveyed action uses it.
    Unobserved,
}

impl Support {
    /// Short label for a UI badge.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Authorable => "Authorable",
            Self::Readable => "Readable",
            Self::Structural => "Structure Only",
            Self::Unobserved => "Not Observed",
        }
    }

    /// Sentence-case explanation of what the level means for authoring.
    #[must_use]
    pub const fn detail(self) -> &'static str {
        match self {
            Self::Authorable => {
                "Parhelion can build a supported configuration of this node into a custom effect."
            }
            Self::Readable => {
                "Parhelion can read mapped fields of this node, but cannot author it yet."
            }
            Self::Structural => "Only the node type and size are known. Its fields are not mapped.",
            Self::Unobserved => {
                "No surveyed action uses this registered kind. Its native layout has not been recovered."
            }
        }
    }
}

/// One registered condition or effect kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NodeKind {
    /// Dispatch index used by the node header byte.
    pub kind: u8,
    /// Native structure class, or zero when the kind was never observed.
    pub class: u32,
    /// Native structure size in bytes, or zero when the kind was never observed.
    pub struct_size: u32,
    /// Occurrences across the surveyed action resources.
    pub occurrences: u32,
    /// Title-case display name for the traced operation.
    pub name: &'static str,
    /// Sentence-case explanation for a workbench reader.
    pub summary: &'static str,
    /// Traced role and its recorded limits.
    pub evidence: &'static str,
    /// Authoring support level.
    pub support: Support,
}

impl NodeKind {
    /// Whether the surveyed packages contain this kind at all.
    #[must_use]
    pub const fn observed(&self) -> bool {
        self.occurrences != 0
    }
}

/// One action execution policy selected by the action root.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActionPolicy {
    /// Policy index stored in the action root.
    pub kind: u8,
    /// Title-case display name.
    pub name: &'static str,
    /// Sentence-case explanation for a workbench reader.
    pub summary: &'static str,
    /// Typed configuration class, or zero when the policy carries no configuration.
    pub configuration_class: u32,
    /// Surveyed action resources that select this policy.
    pub actions: u32,
}

/// Every condition kind the client registers, indexed by kind.
pub const CONDITIONS: [NodeKind; 45] = [
    NodeKind {
        kind: 0,
        class: 0x80803E03,
        struct_size: 8,
        occurrences: 909,
        name: "Unconditional Check",
        summary: "Always passes. The action still obeys its probability roll and event routing.",
        evidence: "The specific checker returns true. The common probability and event-routing checks still apply. The compiler emits it as the activation of an always-active program and carries it verbatim as an ending condition, where 41 stock actions use it to end at once.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 1,
        class: 0x80803DCD,
        struct_size: 12,
        occurrences: 527,
        name: "Timer",
        summary: "Waits for a fixed number of seconds. Used for both effect duration and cooldown.",
        evidence: "Uses timer enumeration and expiry callbacks. The specific boolean checker itself returns true.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 2,
        class: 0x80803DE7,
        struct_size: 344,
        occurrences: 396,
        name: "Kill Event",
        summary: "Fires on a kill. Can require the owning weapon and a label such as precision.",
        evidence: "Filters weapon ownership, ability labels, target properties and optional restrictions. Supplies activation target context.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 3,
        class: 0x80803DE5,
        struct_size: 288,
        occurrences: 12,
        name: "Label and Value Event Filter",
        summary: "Fires on an event that carries labels and a number, and checks both against a range.",
        evidence: "Reads both source label filters, the inclusive value range at +110, the owning-weapon flag at +118 and the optional weapon key at +11C. The event source still needs identification.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 4,
        class: 0x80802F5F,
        struct_size: 208,
        occurrences: 163,
        name: "Object and Numeric Event Filter",
        summary: "Fires on an event that references an object, then checks that object and several numbers.",
        evidence: "Reads the source label filter at +8, value threshold +A0, optional named key +9C, object filter selector +A8, source mask +C1 and whether the stateful +C8 predicate is present. That predicate keeps per target counts or fractions between evaluations. The event source and some field names remain unresolved.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 5,
        class: 0x80803DDC,
        struct_size: 200,
        occurrences: 20,
        name: "Distance and Numeric Event Filter",
        summary: "Fires on an event that references an object, then checks distance and a numeric threshold.",
        evidence: "Reads the source label filter at +8, threshold +98, optional named key +A0, optional maximum distance +A4 and the event masks at +B8 and +B9. Distance calculation reads protected position values, and the event source is unresolved.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 6,
        class: 0x80803DFB,
        struct_size: 12,
        occurrences: 48,
        name: "Two Event Flag Masks",
        summary: "Requires the event to carry both of two flag masks.",
        evidence: "Requires an intersection between node +8 and event byte 0. Node +9 either disables the second check or must intersect event byte 1.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 7,
        class: 0x80803DFD,
        struct_size: 32,
        occurrences: 25,
        name: "Slot and Player Filter",
        summary: "Checks the event equipment slot and the acting player.",
        evidence: "Reads the slot mask at +8 and the player filter selector at +10, which the checker applies to the event slot and acting player. The event source still needs identification.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 8,
        class: 0x80803E01,
        struct_size: 32,
        occurrences: 62,
        name: "Slot and Player Filter",
        summary: "Checks the event equipment slot and the acting player.",
        evidence: "Reads the slot mask at +8 and the player filter selector at +10, which the checker applies to the event slot and acting player. The event source still needs identification.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 9,
        class: 0x80803E00,
        struct_size: 12,
        occurrences: 16,
        name: "Event Slot Mask",
        summary: "Selects a slot bit from an event integer and requires it in the node mask.",
        evidence: "Tests node +8 against the bit selected by the event integer at +4.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 10,
        class: 0x80803DFF,
        struct_size: 24,
        occurrences: 5,
        name: "Event Key at Node +10",
        summary: "Requires the event key to equal the key stored on this node.",
        evidence: "Requires equality between the node integer at +10 and the first event integer.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 11,
        class: 0x80803DFE,
        struct_size: 24,
        occurrences: 8,
        name: "Event Key at Node +10",
        summary: "Requires the event key to equal the key stored on this node.",
        evidence: "Requires equality between the node integer at +10 and the first event integer.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 12,
        class: 0x80803DEA,
        struct_size: 16,
        occurrences: 31,
        name: "Key and Context Lookup",
        summary: "Matches an event value, then resolves a named key against the event context.",
        evidence: "Requires node +8 to match event +10, then resolves node key +C against the event context through 504150.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 13,
        class: 0x80803DDE,
        struct_size: 112,
        occurrences: 8,
        name: "Weapon Event Filter",
        summary: "Fires on a weapon event and checks the weapon labels. The source event is unnamed.",
        evidence: "Reads the owning-weapon flag at +8, the slot mask at +B and the source label filter at +10, which the checker compiles into its object-label test. Handles an invalid event slot explicitly. The source event is unnamed.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 14,
        class: 0x80803DFA,
        struct_size: 112,
        occurrences: 89,
        name: "Attach Event",
        summary: "Fires when the weapon is attached to the character.",
        evidence: "Observed in the Wave Frame activation program. Target and host restrictions still apply.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 15,
        class: 0x80803DF7,
        struct_size: 112,
        occurrences: 89,
        name: "Detach Event",
        summary: "Fires when the weapon is detached from the character.",
        evidence: "Paired with the Wave Frame attach event for removal.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 16,
        class: 0x80803DF5,
        struct_size: 112,
        occurrences: 95,
        name: "Draw Event",
        summary: "Fires when the weapon is drawn.",
        evidence: "Observed in the Cluster Bomb activation program. Shares a checker with holster and related weapon events.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 17,
        class: 0x80803DDB,
        struct_size: 112,
        occurrences: 107,
        name: "Holster Event",
        summary: "Fires when the weapon is holstered.",
        evidence: "Observed in Cluster Bomb and Outlaw removal programs.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 18,
        class: 0x80803DDD,
        struct_size: 112,
        occurrences: 19,
        name: "Weapon Event Filter",
        summary: "Fires on an unnamed weapon event and checks the weapon labels.",
        evidence: "Reads the owning-weapon flag at +8, the slot mask at +B and the source label filter at +10, sharing the checker used by attach, detach, draw and holster events. Its source event is not yet named.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 19,
        class: 0x80803DE2,
        struct_size: 112,
        occurrences: 46,
        name: "Weapon Event with Time Restriction",
        summary: "Fires on an unnamed weapon event with an additional time restriction.",
        evidence: "Checks optional owning weapon +8, slot mask +B, time restriction +C and object labels +60. Event byte +8 selects whether node byte +9 or +A must be set.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 20,
        class: 0x80803DCE,
        struct_size: 256,
        occurrences: 293,
        name: "General Predicate",
        summary: "A general predicate over player, object, ability and value restrictions, optionally inverted.",
        evidence: "Combines state, object, ability and value restrictions, with optional final inversion. Individual fields are only partly mapped.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 21,
        class: 0x80803DFC,
        struct_size: 352,
        occurrences: 1,
        name: "Two Objects and Ability Filter",
        summary: "Checks two event objects and an ability filter.",
        evidence: "Checks valid event objects against the compiled predicates at +58 and +B8, applies ability filter +138 and optionally scans related player state when +158 is set. The reader reports that flag. The source event is unresolved.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 22,
        class: 0x80803DF9,
        struct_size: 12,
        occurrences: 6,
        name: "Event Byte Match",
        summary: "Requires an event byte to equal the byte on this node.",
        evidence: "Requires equality between node byte +8 and event byte 0.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 23,
        class: 0x80803DDA,
        struct_size: 12,
        occurrences: 26,
        name: "Event Byte with Host Restrictions",
        summary: "Requires an event byte to match, and can require the owning weapon.",
        evidence: "Matches node byte +8 with event byte +4. Node +A can require the owning weapon. Node +9 excludes host component states 1 and 2.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 24,
        class: 0x80803DE1,
        struct_size: 12,
        occurrences: 1,
        name: "Event Byte Match",
        summary: "Requires an event byte to equal the byte on this node.",
        evidence: "Requires equality between node byte +8 and event byte 0.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 25,
        class: 0x80803DDF,
        struct_size: 12,
        occurrences: 2,
        name: "Event Byte Match",
        summary: "Requires an event byte to equal the byte on this node.",
        evidence: "Requires equality between node byte +8 and event byte 0.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 26,
        class: 0x80803E30,
        struct_size: 56,
        occurrences: 142,
        name: "Accumulator",
        summary: "Counts toward a threshold. Child rows add, replace or multiply the stored value.",
        evidence: "Applies success or failure arithmetic for nested conditions, clamps the value and checks a threshold. Includes scheduled processing.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 27,
        class: 0x808029E0,
        struct_size: 152,
        occurrences: 43,
        name: "Two Objects and Event State",
        summary: "Checks two objects and the state carried by the event.",
        evidence: "Reads the owning-weapon flag +8, slot mask +9, event bytes +C and +D, the source label filter at +10, the object filter selector at +80 and mode +90, which selects restrictions on those event bytes. The source event remains unresolved.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 28,
        class: 0x80803DE0,
        struct_size: 12,
        occurrences: 2,
        name: "Event Mask",
        summary: "Selects a bit from an event integer and requires it in the node mask.",
        evidence: "Tests node +8 against the bit selected by the first event integer.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 29,
        class: 0x80803DEC,
        struct_size: 12,
        occurrences: 29,
        name: "Event Key Match",
        summary: "Requires the event context value to equal the key on this node.",
        evidence: "Compares the 32-bit event context value with the key stored at node +8.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 30,
        class: 0x80803DEB,
        struct_size: 12,
        occurrences: 449,
        name: "Event Key Match",
        summary: "Requires the event context value to equal the key on this node.",
        evidence: "Compares the 32-bit event context value with the key stored at node +8. The compiler emits it as the ending condition of an always-active program, the shape 216 stock actions share.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 31,
        class: 0x80803E04,
        struct_size: 24,
        occurrences: 10,
        name: "All Subgroups",
        summary: "Requires every subgroup to pass. Conditions inside one subgroup are alternatives.",
        evidence: "Requires each subgroup to pass or retain positive hold state. Conditions within each subgroup use OR semantics.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 32,
        class: 0x80803DF8,
        struct_size: 104,
        occurrences: 34,
        name: "Event Label Filter",
        summary: "Checks the labels carried by the object the event references.",
        evidence: "Applies the predicate at node +58 to the label set four bytes into the event-referenced object.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 33,
        class: 0x00000000,
        struct_size: 0,
        occurrences: 0,
        name: "Event Byte Differs",
        summary: "Requires an event byte to differ from the byte on this node.",
        evidence: "Requires node byte +8 to differ from event byte 0. No package occurrence was found.",
        support: Support::Unobserved,
    },
    NodeKind {
        kind: 34,
        class: 0x80803DE4,
        struct_size: 40,
        occurrences: 4,
        name: "Key and Two Numeric Ranges",
        summary: "Matches an optional key and requires two event numbers to fall inside stored ranges.",
        evidence: "The key at +10 may be FFFFFFFF to skip key matching. The two event floats must lie within node ranges +18..+1C and +20..+24.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 35,
        class: 0x80803DCC,
        struct_size: 264,
        occurrences: 244,
        name: "Predicate with Nested Condition",
        summary: "Runs the general predicate and then a second nested condition.",
        evidence: "Runs the kind 20 general predicate and then the nested condition at +100. Maintains separate state routing.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 36,
        class: 0x80803DF6,
        struct_size: 12,
        occurrences: 8,
        name: "Event Byte Match",
        summary: "Requires an event byte to equal the byte on this node.",
        evidence: "Requires equality between node byte +8 and event byte 0.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 37,
        class: 0x80802D02,
        struct_size: 12,
        occurrences: 1,
        name: "Nonempty Event Key Match",
        summary: "Requires a nonempty key on this node to equal the event key.",
        evidence: "Requires node +8 to be nonempty and equal the first event integer. Empty uses the FNV offset-basis value 811C9DC5.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 38,
        class: 0x80802D01,
        struct_size: 16,
        occurrences: 2,
        name: "Named Target Beyond Distance",
        summary: "Requires a named target to be farther away than the stored distance.",
        evidence: "Requires a matching nonempty key, resolves its target and compares squared distance with the square of node float +C.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 39,
        class: 0x00000000,
        struct_size: 0,
        occurrences: 0,
        name: "Event Integer Match",
        summary: "Requires an event integer to equal the integer on this node.",
        evidence: "Requires equality between node integer +8 and event integer 0. No package occurrence was found.",
        support: Support::Unobserved,
    },
    NodeKind {
        kind: 40,
        class: 0x80802D00,
        struct_size: 144,
        occurrences: 1,
        name: "Named Event with Four Object Filters",
        summary: "Matches an optional named event and applies four object filters.",
        evidence: "Reads the optional key at +8. The checker matches it, then applies the filters at +10, +30, +50 and +70 to three event weak handles and the controller player.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 41,
        class: 0x808029E1,
        struct_size: 40,
        occurrences: 12,
        name: "Object and Slot Filter",
        summary: "Checks an event slot mask and then an object filter against the event object and weapon.",
        evidence: "Reads the four-slot event mask at +8 and the object filter selector at +18. The checker applies that filter to the event object and owning weapon.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 42,
        class: 0x808029E6,
        struct_size: 12,
        occurrences: 14,
        name: "Event Byte at +8 Match",
        summary: "Requires the event byte at offset 8 to equal the byte on this node.",
        evidence: "Requires node byte +8 to equal event byte +8.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 43,
        class: 0x00000000,
        struct_size: 0,
        occurrences: 0,
        name: "Event Object Filter",
        summary: "Applies an object filter to the event object.",
        evidence: "Rejects an invalid event object, then evaluates node filter +8 against it and the controller player. No package occurrence was found.",
        support: Support::Unobserved,
    },
    NodeKind {
        kind: 44,
        class: 0x00000000,
        struct_size: 0,
        occurrences: 0,
        name: "Event Object Filter",
        summary: "Applies an object filter to the event object.",
        evidence: "Rejects an invalid event object, then evaluates node filter +8 against it and the controller player. No package occurrence was found.",
        support: Support::Unobserved,
    },
];

/// Every effect kind the client registers, indexed by kind.
pub const EFFECTS: [NodeKind; 55] = [
    NodeKind {
        kind: 0,
        class: 0x80803E4C,
        struct_size: 2,
        occurrences: 1,
        name: "Target Component Gate",
        summary: "Opens a gate on the target component while the action is active.",
        evidence: "Resolves an activation target, retains its weak handle and sets gate 1 through CA4910. Cleanup clears that gate. CA4910 combines two gate inputs into host byte +A48. The node stores no fields beyond its header. The downstream feature remains unresolved.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 1,
        class: 0x80803E45,
        struct_size: 64,
        occurrences: 1011,
        name: "Create Entity",
        summary: "Creates a referenced entity and keeps it until the action ends.",
        evidence: "Creates a referenced entity graph and retains state needed for removal. The entity can provide further behavior.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 2,
        class: 0x80803E44,
        struct_size: 88,
        occurrences: 95,
        name: "Create Entity with Dynamic Value",
        summary: "Creates a referenced entity and drives one of its numbers from a value program.",
        evidence: "Creates the referenced entity, retains its handle and applies the program at +20 to its numeric component state. Slot 1 recomputes the value using input selector +50 and normalization flag +51.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 3,
        class: 0x80803E43,
        struct_size: 24,
        occurrences: 42,
        name: "Spawn Entity at Selected Transform",
        summary: "Spawns a referenced entity once at a selected position. It then owns its own lifetime.",
        evidence: "Creates the entity resource at +10, chooses its position and attachment using bytes +2 through +4 and activation context, then submits it through 56D990. No retained cleanup callback is registered.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 4,
        class: 0x80803E46,
        struct_size: 24,
        occurrences: 8,
        name: "Apply Referenced Resource to Target",
        summary: "Applies a referenced resource to a selected target.",
        evidence: "Target selector +2 resolves the target. Resource +10 is consumed through either CDA100 and DD9C60 or the entity creation path, depending on its resource type.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 5,
        class: 0x80803E42,
        struct_size: 32,
        occurrences: 8,
        name: "Generate Orbs of Light",
        summary: "Generates Orbs of Light for player recipients, with a count, value and location. Publishes the generation event. Use a spawn action for health pickups and other objects.",
        evidence: "Callback 1089AF0 uses EC0FC0 to select the owner position (0) or activation event position (1), then passes count +4, orb value +8 and a resource reference at +18 to FBC3E0. Masterwork Weapon 453 and Trinity Ghoul Catalyst 2010 use entity 80EFAE02. Striking Light 1803 and Light of the Fire 1960 use its default entity. The downstream helper accepts an entity argument and selects player recipients, but that does not establish that this perk action honors arbitrary pickup overrides. Stock sources establish orb generation. The resource reference's effect on pickup type remains unresolved.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 6,
        class: 0x80803E41,
        struct_size: 4,
        occurrences: 9,
        name: "Set Host Mode",
        summary: "Sets a mode byte on the host and restores it on removal unless persistence is requested.",
        evidence: "Writes byte +2 through the selected ability interface or DAB210. Cleanup resets the mode unless byte +3 requests persistence. Mode 0 is Kinetic, 1 Solar, 2 Arc and 3 Void, established by the stock plugs that set each value: Solar, Arc and Void Damage Mod name their element, The Fundamentals sets all three and is described in game as changing the weapon's damage type, and Play with Your Prey sets 1 and 3 while describing a Solar and a Void rocket.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 7,
        class: 0x80803E1D,
        struct_size: 12,
        occurrences: 81,
        name: "Ability Property",
        summary: "Changes a named property inside an ability bank.",
        evidence: "Targets a named property in an ability bank. Previously traced with sandbox perk 166.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 8,
        class: 0x80803E4D,
        struct_size: 80,
        occurrences: 179,
        name: "Component Value Adjustment",
        summary: "Scales a selected component value by a value program, with an optional limit.",
        evidence: "Reads the selector bytes at +2 through +4, the scale at +8, the limit at +C, the input selector at +48 and the value program at +18. The callback multiplies the scale by the evaluated program and optionally limits movement toward the limit. The exact component slot names remain unresolved.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 9,
        class: 0x00000000,
        struct_size: 0,
        occurrences: 0,
        name: "Unobserved Kind",
        summary: "Registered by the client. No surveyed action uses it, and its behavior is unclassified.",
        evidence: "Individual gameplay semantics remain unclassified.",
        support: Support::Unobserved,
    },
    NodeKind {
        kind: 10,
        class: 0x808029ED,
        struct_size: 80,
        occurrences: 203,
        name: "Named Property",
        summary: "Changes a named property on the weapon or an ability, with an explicit restore policy.",
        evidence: "Evaluates a value, chooses native or ability targets and applies arithmetic to a named key. Has explicit cleanup policies. The compiler emits the constant value program that 200 of the 203 stock nodes use.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 11,
        class: 0x80803E35,
        struct_size: 48,
        occurrences: 5,
        name: "Add Host Numeric Modifiers",
        summary: "Adds shared and per slot numbers into a host component, and subtracts them on removal.",
        evidence: "Reads the two shared floats at +4 and +8 and the three slot-specific triples from +C through +2C, which the callback adds into a host component. Cleanup subtracts the same values. Downstream property names remain unresolved.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 12,
        class: 0x00000000,
        struct_size: 0,
        occurrences: 0,
        name: "Unobserved Kind",
        summary: "Registered by the client. No surveyed action uses it, and its behavior is unclassified.",
        evidence: "Individual gameplay semantics remain unclassified.",
        support: Support::Unobserved,
    },
    NodeKind {
        kind: 13,
        class: 0x80803E47,
        struct_size: 64,
        occurrences: 12,
        name: "Weighted Spawn Operation",
        summary: "Picks one of three weighted categories and spawns from the sampled range.",
        evidence: "Chooses one of three categories using weights +20, +2C and +38, then samples its configured range. Flags +3 and +4 control owner and related-player paths. The optional resource at +10 is attached to the spawned result. The categories are the ammo types, established by the stock perks that weight exactly one: Snapload Finisher (Primary ammo) the first, Special Finisher, Extra Reserves and Swift Charge (Special ammo) the second, Heavy Finisher, Giving Hand and the Voltaic Ammo Collectors (Heavy ammo) the third.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 14,
        class: 0x80803E3F,
        struct_size: 136,
        occurrences: 37,
        name: "Fixed Ammunition Adjustment",
        summary: "Adds a fixed number of rounds to the magazine or reserves of a weapon slot or ammo type.",
        evidence: "Reads the source label filter at +8, the storage path, overflow, unit-scaling and action-value bytes at +68 through +6B and the seven signed contributions at +6C through +84 for the owning slot, three weapon slots and three ammunition categories. Stock use reads storage path 1 as the magazine (Triple Tap and Fourth Time's the Charm return 1 and 2 rounds through it) and path 0 as reserves (the ammo pickup perks add to the ammo types through it). The compiler emits a label-free node with one amount, the shape 30 of the 37 stock nodes take.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 15,
        class: 0x80803E3E,
        struct_size: 136,
        occurrences: 35,
        name: "Proportional Ammunition Adjustment",
        summary: "Adds a share of a capacity, such as half a magazine, to the magazine or reserves.",
        evidence: "Reads the source label filter at +8, the destination, overflow, capacity-source and action-value bytes at +68 through +6B and the seven float contributions at +6C through +84. The callback sums them, multiplies by the selected capacity and rounds before applying. Its entry passes through protected code. Stock kill perks pair destination 1 with capacity source 1 (a kill from this weapon refills half or all of the magazine) and the ammo pickup perks pair 0 with 0, so the compiler reads 1 as the magazine and 0 as reserves. It emits a label-free node with one amount, the shape 40 of the 48 stock nodes take.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 16,
        class: 0x808029EC,
        struct_size: 392,
        occurrences: 37,
        name: "Transfer Reserve Ammunition",
        summary: "Moves ammunition from reserves into the magazine.",
        evidence: "Reads the source label filter at +8, the overflow, capacity-basis, event, input-selector and normalization bytes at +68 through +6C and the five value programs at +78, +B0, +E8, +120 and +158. The callback sums the applicable programs, rounds to an ammunition unit, caps by reserve availability and applies the amount to the magazine, then sends the remaining reserve to CC7BB0. Both its entry and that setter pass through protected code.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 17,
        class: 0x00000000,
        struct_size: 0,
        occurrences: 0,
        name: "Unobserved Kind",
        summary: "Registered by the client. No surveyed action uses it, and its behavior is unclassified.",
        evidence: "Individual gameplay semantics remain unclassified.",
        support: Support::Unobserved,
    },
    NodeKind {
        kind: 18,
        class: 0x80803E23,
        struct_size: 16,
        occurrences: 2,
        name: "Three Host Float Overrides",
        summary: "Replaces three host floats and restores them on removal.",
        evidence: "Reads the three values at +4, +8 and +C. Nonnegative values replace host floats through D9A310 selectors 1, 2 and 3, and previous values are retained for cleanup. Property names remain unresolved.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 19,
        class: 0x00000000,
        struct_size: 0,
        occurrences: 0,
        name: "Unobserved Kind",
        summary: "Registered by the client. No surveyed action uses it, and its behavior is unclassified.",
        evidence: "Individual gameplay semantics remain unclassified.",
        support: Support::Unobserved,
    },
    NodeKind {
        kind: 20,
        class: 0x80803E24,
        struct_size: 3,
        occurrences: 2,
        name: "Host Reference Count",
        summary: "Increments or decrements a host counter, and reverses it on removal.",
        evidence: "Byte +2 increments or decrements a host counter, clamped at zero, through D8EBF0. Cleanup applies the inverse operation. The downstream feature remains unresolved.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 21,
        class: 0x00000000,
        struct_size: 0,
        occurrences: 0,
        name: "Unobserved Kind",
        summary: "Registered by the client. No surveyed action uses it, and its behavior is unclassified.",
        evidence: "Individual gameplay semantics remain unclassified.",
        support: Support::Unobserved,
    },
    NodeKind {
        kind: 22,
        class: 0x80803E0F,
        struct_size: 5,
        occurrences: 1,
        name: "Set Ability Enum",
        summary: "Writes an enum byte on the selected ability and restores it on removal.",
        evidence: "Selects the owning slot and explicit slot mask, writes byte +4 through interface slot +1F8 and retains previous values for cleanup. Enum names remain unresolved.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 23,
        class: 0x00000000,
        struct_size: 0,
        occurrences: 0,
        name: "Adjust Ability Enum",
        summary: "Adds to the enum byte on the selected ability, clamped to its range.",
        evidence: "Shares kind 22 handling but adds signed byte +4 to the current value and clamps to 0 through 3. No package occurrence was found.",
        support: Support::Unobserved,
    },
    NodeKind {
        kind: 24,
        class: 0x80803E14,
        struct_size: 2,
        occurrences: 1,
        name: "Magazine Component Flag",
        summary: "Sets a flag byte on the magazine component and clears it on removal.",
        evidence: "Sets the selected magazine component byte +238. Cleanup clears it. The node stores no fields beyond its header. The consumer of this flag remains unresolved.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 25,
        class: 0x80803E13,
        struct_size: 8,
        occurrences: 5,
        name: "Pattern Component Key Override",
        summary: "Overrides a key on the pattern component and clears it on removal.",
        evidence: "Writes the key at +4 into pattern component +64 through CA4A60. Slot 1 reapplies it and cleanup clears it. The key consumer remains unresolved.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 26,
        class: 0x80803E12,
        struct_size: 24,
        occurrences: 30,
        name: "Pattern Override",
        summary: "Replaces the projectile pattern used by the owning weapon while the action is active.",
        evidence: "Writes a pattern tag into the owning weapon component. Cleanup clears the override. Host compatibility remains separate.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 27,
        class: 0x808029EB,
        struct_size: 2,
        occurrences: 7,
        name: "Host Activation Counter",
        summary: "Activates a counter on the owning ability or host, and reverses it on removal.",
        evidence: "Resolves an owning ability or fallback host component and activates its counter. The fallback increments host byte +236. Cleanup reverses the operation. The gameplay feature remains unresolved.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 28,
        class: 0x80803E0C,
        struct_size: 24,
        occurrences: 8,
        name: "Two Ability State Overrides",
        summary: "Replaces two ability state bytes and restores them on removal.",
        evidence: "Selects the owning ability, retains two bytes, then writes node bytes +2 and +3 through two interface setters. Slot 1 reapplies them and cleanup restores the retained values.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 29,
        class: 0x80803E0B,
        struct_size: 16,
        occurrences: 10,
        name: "Three Weapon Float Overrides",
        summary: "Replaces three weapon floats and restores them on removal.",
        evidence: "Retains weapon values +6A0, +6A4 and +6A8, then replaces them from node +4, +8 and +C. Slot 1 reapplies the override and cleanup restores the values. Their downstream consumers remain unresolved.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 30,
        class: 0x80803E0D,
        struct_size: 3,
        occurrences: 3,
        name: "Weapon Reference Count",
        summary: "Adds a reference count on the weapon, and removes it on removal.",
        evidence: "Byte +2 increments or decrements weapon byte +AC0 through CC9350. Crossing zero updates other weapon state. Cleanup applies the inverse operation, with retained state used to gate removal.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 31,
        class: 0x00000000,
        struct_size: 0,
        occurrences: 0,
        name: "Unobserved Kind",
        summary: "Registered by the client. No surveyed action uses it, and its behavior is unclassified.",
        evidence: "Individual gameplay semantics remain unclassified.",
        support: Support::Unobserved,
    },
    NodeKind {
        kind: 32,
        class: 0x80803E3B,
        struct_size: 40,
        occurrences: 30,
        name: "Extend Timers",
        summary: "Extends the timers that are already running, up to a cap, when its conditions match.",
        evidence: "The controller evaluates its nested conditions and extends eligible active timers by an amount up to a cap. The compiler nests the program's own kill trigger, as Outlaw does.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 33,
        class: 0x80803E3C,
        struct_size: 184,
        occurrences: 43,
        name: "Register Host Modifier",
        summary: "Registers a compiled modifier on a host component and unregisters it on removal.",
        evidence: "Reads the modifier value at +4, the input selector at +8, the optional limit at +C and the target predicate's label lists at +10. The callback registers the compiled descriptor at +A0 through CD3020 and cleanup unregisters it through CDDDD0. Descriptor semantics remain partly mapped.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 34,
        class: 0x00000000,
        struct_size: 0,
        occurrences: 0,
        name: "Unobserved Kind",
        summary: "Registered by the client. No surveyed action uses it, and its behavior is unclassified.",
        evidence: "Individual gameplay semantics remain unclassified.",
        support: Support::Unobserved,
    },
    NodeKind {
        kind: 35,
        class: 0x80803E1C,
        struct_size: 12,
        occurrences: 5,
        name: "Override Host Key",
        summary: "Replaces a named key on a host property and restores it on removal.",
        evidence: "Target selector +2 and interface selector +3 choose a host property. Key +4 replaces its stored key through F67620 selectors 3 or 14. Cleanup restores the retained key. Flag +8 can apply the same operation to the controller player.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 36,
        class: 0x80803E19,
        struct_size: 16,
        occurrences: 1,
        name: "Register Host Record",
        summary: "Registers a typed record on the host and removes it on removal.",
        evidence: "Resolves host component +3E0 and registers the optional typed record at +8 through D6F7A0 using the action identity. Cleanup removes the record.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 37,
        class: 0x80803E1A,
        struct_size: 208,
        occurrences: 14,
        name: "Add Event Labels After Ability Filter",
        summary: "Adds labels to an event after an ability filter passes.",
        evidence: "Reads the source label array at +98, whose expansion reproduces the compiled set at +A8. The callback evaluates ability filter +78, then unions that set into the event through B7C590 in event callback slot 1.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 38,
        class: 0x00000000,
        struct_size: 0,
        occurrences: 0,
        name: "Unobserved Kind",
        summary: "Registered by the client. No surveyed action uses it, and its behavior is unclassified.",
        evidence: "Individual gameplay semantics remain unclassified.",
        support: Support::Unobserved,
    },
    NodeKind {
        kind: 39,
        class: 0x00000000,
        struct_size: 0,
        occurrences: 0,
        name: "Unobserved Kind",
        summary: "Registered by the client. No surveyed action uses it, and its behavior is unclassified.",
        evidence: "Individual gameplay semantics remain unclassified.",
        support: Support::Unobserved,
    },
    NodeKind {
        kind: 40,
        class: 0x80802F16,
        struct_size: 336,
        occurrences: 175,
        name: "Event Numeric Modifier",
        summary: "Changes the numbers carried by an event after object, ability and label filters pass.",
        evidence: "Reads the object filter lists at +28, the source label filter at +C0, the assignment rows at +120, the multiplication rows at +130 and whether the scalar expression pointer at +140 is present. A row stores an event slot index with a literal or a native stat selector. The callback applies the rows after object, ability and label filters, and the expression multiplies the event scalar by one plus its value. The upstream event contract remains partly mapped.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 41,
        class: 0x80803E39,
        struct_size: 8,
        occurrences: 7,
        name: "Named Host Reference Counts",
        summary: "Increments named counters on the host and decrements them on removal.",
        evidence: "Selects owner and slot targets, increments key +4 in each host named counter through DA6BC0 and retains the affected handles. Cleanup decrements the same key and removes entries whose count reaches zero.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 42,
        class: 0x80803E2F,
        struct_size: 8,
        occurrences: 147,
        name: "Accumulator Update",
        summary: "Writes the action accumulator value.",
        evidence: "Calls the common action accumulator setter. The stored mode selects the supplied value.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 43,
        class: 0x80803E1E,
        struct_size: 8,
        occurrences: 8,
        name: "Publish Named Player Event",
        summary: "Publishes a named player event.",
        evidence: "If key +4 is nonempty and the owner is valid and authoritative, publishes that key through the event-message machinery. Receiver semantics remain unresolved.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 44,
        class: 0x00000000,
        struct_size: 0,
        occurrences: 0,
        name: "Unobserved Kind",
        summary: "Registered by the client. No surveyed action uses it, and its behavior is unclassified.",
        evidence: "Individual gameplay semantics remain unclassified.",
        support: Support::Unobserved,
    },
    NodeKind {
        kind: 45,
        class: 0x00000000,
        struct_size: 0,
        occurrences: 0,
        name: "Unobserved Kind",
        summary: "Registered by the client. No surveyed action uses it, and its behavior is unclassified.",
        evidence: "Individual gameplay semantics remain unclassified.",
        support: Support::Unobserved,
    },
    NodeKind {
        kind: 46,
        class: 0x00000000,
        struct_size: 0,
        occurrences: 0,
        name: "Unobserved Kind",
        summary: "Registered by the client. No surveyed action uses it, and its behavior is unclassified.",
        evidence: "Individual gameplay semantics remain unclassified.",
        support: Support::Unobserved,
    },
    NodeKind {
        kind: 47,
        class: 0x80803E2E,
        struct_size: 8,
        occurrences: 104,
        name: "Transmat Context, Consumer Unresolved",
        summary: "Carries a transmat key. No consumer for that key has been established.",
        evidence: "All registered callbacks are defaults. Its 104 action occurrences serve 112 perk indices. Every index has a cached Transmat Effect item reference, providing presentation context without proving the consumer of key +4. No substantive consumer was established in the inspected controller region.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 48,
        class: 0x80802D0A,
        struct_size: 24,
        occurrences: 51,
        name: "Referenced Runtime Operation",
        summary: "Runs a referenced runtime operation against three resolved targets.",
        evidence: "Stores three target selectors at +2 through +4 and the referenced runtime resource tag at +10. The native declaration marks +8 as a relative string pointer, and stock records resolve it to a resource path. It is not an operation value. The downstream operation still needs semantic identification.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 49,
        class: 0x80802D0D,
        struct_size: 8,
        occurrences: 2,
        name: "Bind Named Target",
        summary: "Binds a named target on the host. The host keeps at most four bindings.",
        evidence: "Resolves target selector +2 and adds or replaces key +4 in the host named-target list through EBCB90. The list holds up to four key, weak-target and time records.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 50,
        class: 0x00000000,
        struct_size: 0,
        occurrences: 0,
        name: "Unobserved Kind",
        summary: "Registered by the client. No surveyed action uses it, and its behavior is unclassified.",
        evidence: "Individual gameplay semantics remain unclassified.",
        support: Support::Unobserved,
    },
    NodeKind {
        kind: 51,
        class: 0x80802D03,
        struct_size: 3,
        occurrences: 1,
        name: "Append Weapon Enum",
        summary: "Appends an enum byte to a weapon list, capped at four entries.",
        evidence: "Appends byte +2 to a selected weapon list at +BC4, capped at four entries. Cleanup removes a matching entry. The enum consumer remains unresolved.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 52,
        class: 0x808029F1,
        struct_size: 12,
        occurrences: 2,
        name: "Add Named Player Float",
        summary: "Adds a float under a named key in a player table, and subtracts it on removal.",
        evidence: "Adds float +8 under key +4 in a player table through BF6310 and BF2C80. Cleanup adds the negated float. The table holds at most four keys, and the consuming player system remains unresolved.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 53,
        class: 0x808029F2,
        struct_size: 96,
        occurrences: 2,
        name: "Adjust Named Component Values",
        summary: "Adds, replaces or multiplies matching named component values.",
        evidence: "Reads the component name array at +8, the upper cap at +18, the value program at +28 and the operation byte at +5A. The callback adds, replaces or multiplies each matching component value according to that byte.",
        support: Support::Authorable,
    },
    NodeKind {
        kind: 54,
        class: 0x8080281C,
        struct_size: 168,
        occurrences: 9,
        name: "Add Event Labels After Object Filter",
        summary: "Adds labels to an event after an object filter passes.",
        evidence: "Reads the source label array at +68, whose expansion reproduces the compiled set at +78, and the event flag at +A0. The callback checks the event object predicate at +58 and that flag, then unions the set into the event through B7C590.",
        support: Support::Authorable,
    },
];

/// Every action execution policy the client registers, indexed by kind.
pub const POLICIES: [ActionPolicy; 6] = [
    ActionPolicy {
        kind: 0,
        name: "Standard",
        summary: "The action runs from its conditions and effects alone.",
        configuration_class: 0x00000000,
        actions: 1569,
    },
    ActionPolicy {
        kind: 1,
        name: "Entity Variable Binding",
        summary: "The action reads a named numeric variable from its host entity. A host without that variable supplies zero.",
        configuration_class: 0x80803E07,
        actions: 61,
    },
    ActionPolicy {
        kind: 2,
        name: "Unobserved Policy 2",
        summary: "Registered by the client. No surveyed action uses it.",
        configuration_class: 0x00000000,
        actions: 0,
    },
    ActionPolicy {
        kind: 3,
        name: "Protected Policy 3",
        summary: "One surveyed action uses it. Its event handler enters protected code, so its contract is not established.",
        configuration_class: 0x80803E09,
        actions: 1,
    },
    ActionPolicy {
        kind: 4,
        name: "Unobserved Policy 4",
        summary: "Registered by the client. No surveyed action uses it.",
        configuration_class: 0x00000000,
        actions: 0,
    },
    ActionPolicy {
        kind: 5,
        name: "Protected Policy 5",
        summary: "One surveyed action uses it. Its event handler enters protected code, so its contract is not established.",
        configuration_class: 0x808029EA,
        actions: 1,
    },
];

/// Looks up a condition kind.
#[must_use]
pub fn condition(kind: u8) -> Option<&'static NodeKind> {
    CONDITIONS.get(kind as usize)
}

/// Looks up an effect kind.
#[must_use]
pub fn effect(kind: u8) -> Option<&'static NodeKind> {
    EFFECTS.get(kind as usize)
}

/// Looks up an action policy.
#[must_use]
pub fn policy(kind: u8) -> Option<&'static ActionPolicy> {
    POLICIES.get(kind as usize)
}

/// Title-case display name for a condition kind, falling back to its number.
#[must_use]
pub fn condition_name(kind: u8) -> String {
    condition(kind).map_or_else(|| format!("Condition {kind}"), |node| node.name.to_owned())
}

/// Title-case display name for an effect kind, falling back to its number.
#[must_use]
pub fn effect_name(kind: u8) -> String {
    effect(kind).map_or_else(|| format!("Effect {kind}"), |node| node.name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_node(node: &NodeKind, expected_kind: usize) {
        assert_eq!(usize::from(node.kind), expected_kind);
        for text in [node.name, node.summary, node.evidence] {
            assert!(!text.contains(';'), "{text}");
            assert!(!text.contains('\u{2014}'), "{text}");
            assert!(!text.is_empty());
        }
        assert_eq!(node.observed(), node.occurrences != 0);
        if node.support == Support::Unobserved {
            assert_eq!(node.occurrences, 0);
            assert_eq!(node.class, 0);
        } else {
            assert_ne!(node.class, 0);
            assert_ne!(node.struct_size, 0);
        }
    }

    #[test]
    fn every_kind_is_indexed_by_its_number_and_carries_usable_prose() {
        for (index, node) in CONDITIONS.iter().enumerate() {
            check_node(node, index);
        }
        for (index, node) in EFFECTS.iter().enumerate() {
            check_node(node, index);
        }
        for (index, entry) in POLICIES.iter().enumerate() {
            assert_eq!(usize::from(entry.kind), index);
            assert!(!entry.name.is_empty());
            assert!(!entry.summary.contains(';'));
        }
    }

    #[test]
    fn authorable_kinds_are_the_ones_the_compiler_emits() {
        let conditions = CONDITIONS
            .iter()
            .filter(|node| node.support == Support::Authorable)
            .map(|node| node.kind)
            .collect::<Vec<_>>();
        let effects = EFFECTS
            .iter()
            .filter(|node| node.support == Support::Authorable)
            .map(|node| node.kind)
            .collect::<Vec<_>>();
        assert_eq!(
            conditions,
            CONDITIONS
                .iter()
                .filter(|node| node.observed())
                .map(|node| node.kind)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            effects,
            EFFECTS
                .iter()
                .filter(|node| node.observed())
                .map(|node| node.kind)
                .collect::<Vec<_>>()
        );
        assert_eq!(conditions.len() + effects.len(), 82);
        for layout in crate::sandbox_perk::action::layout::EFFECT_LAYOUTS {
            assert!(
                effects.contains(&layout.kind),
                "effect kind {}",
                layout.kind
            );
        }
        for layout in crate::sandbox_perk::action::layout::CONDITION_LAYOUTS {
            assert!(
                conditions.contains(&layout.kind),
                "condition kind {}",
                layout.kind
            );
        }
    }
}
