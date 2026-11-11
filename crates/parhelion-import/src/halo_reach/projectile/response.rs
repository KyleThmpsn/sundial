//! Source contact rules from a Reach projectile's Material Response block and their native hosts.
//!
//! `proj +0x320` holds 0x34-byte rows: material name `+0x00`, material index `+0x04`, flags
//! `+0x06`, chance `+0x08`, angle bounds `+0x0C`, velocity bounds `+0x14`, response `+0x1C`,
//! effect scaling `+0x1E`, angular and velocity noise `+0x20`/`+0x24`, overpenetration friction
//! `+0x28` and parallel and perpendicular reflection friction `+0x2C`/`+0x30`. Each material
//! appears as an ordered pair, a conditional potential row then an unconditional fallback row with
//! chance 1 and zero bounds, as the older default and potential pairs at `proj +0x314` do. A
//! row's zero angle bounds mean any angle.
use super::*;
use crate::tiger::sequence;
use assembly::Assembly;

const BOUNCE: i16 = 4;
const DUD: i16 = 5;
const RICOCHET: i16 = 6;

const ONLY_UNITS: u16 = 1;
const NEVER_UNITS: u16 = 2;
const ONLY_BIPEDS: u16 = 4;
const ONLY_VEHICLES: u16 = 8;
const ONLY_TETHERED: u16 = 0x20;
const ONLY_UNTETHERED: u16 = 0x40;

const NAMES: [&str; 9] = [
    "impact",
    "fizzle",
    "overpenetrate",
    "attach",
    "bounce",
    "bounce_dud",
    "fizzle_ricochet",
    "turn_physical",
    "airstrike",
];

struct Rule {
    material: Option<String>,
    index: i16,
    flags: u16,
    chance: f32,
    angles: [f32; 2],
    velocity: [f32; 2],
    response: i16,
    noise: f32,
}

impl Rule {
    /// The decoded receipt fields, or the raw row an older behavior receipt holds alone.
    fn read(row: &Value) -> Result<Self> {
        let raw = hex::decode(
            row["raw"]
                .as_str()
                .context("Source material response bytes")?,
        )?;
        ensure!(raw.len() == 0x34, "Source material response size differs");
        let float =
            |at: usize| f32::from_le_bytes([raw[at], raw[at + 1], raw[at + 2], raw[at + 3]]);
        let rule = Self {
            material: row["material"].as_str().map(str::to_owned),
            index: i16::from_le_bytes(raw[4..6].try_into()?),
            flags: u16::from_le_bytes(raw[6..8].try_into()?),
            chance: float(8),
            angles: [float(0xc), float(0x10)],
            velocity: [float(0x14), float(0x18)],
            response: i16::from_le_bytes(raw[0x1c..0x1e].try_into()?),
            noise: float(0x20),
        };
        ensure!(
            [rule.chance, rule.noise]
                .iter()
                .chain(&rule.angles)
                .chain(&rule.velocity)
                .all(|v| v.is_finite())
                && (0.0..=1.).contains(&rule.chance)
                && rule.noise >= 0.,
            "Source material response values are out of range"
        );
        Ok(rule)
    }

    fn default(&self) -> bool {
        self.index == 0
    }

    fn unconditional(&self) -> bool {
        self.chance >= 1. && self.angles == [0.; 2] && self.velocity == [0.; 2]
    }

    fn name(&self) -> &'static str {
        usize::try_from(self.response)
            .ok()
            .and_then(|i| NAMES.get(i))
            .copied()
            .unwrap_or("unknown")
    }
}

fn rules(projectile: &Value) -> Result<Vec<Rule>> {
    projectile["behavior"]["responses"]
        .as_array()
        .context("Source material responses")?
        .iter()
        .map(Rule::read)
        .collect()
}

/// The fraction of contacts a source angle window admits. The window is read as the angle between
/// the incoming direction and the surface, the reading under which the source's low windows make
/// shallow hits ricochet. With directions uniform over the hemisphere, the share of contacts
/// between elevations `a` and `b` is `sin b - sin a`.
fn window(angles: [f32; 2]) -> f32 {
    if angles == [0.; 2] {
        return 1.;
    }
    let [low, high] = angles.map(|a| a.clamp(0., std::f32::consts::FRAC_PI_2));
    (high.sin() - low.sin()).clamp(0., 1.)
}

const NO_DEFAULT: &str = "The source's default material does not bounce";
const MATERIAL: u32 = 0x8080_8bcb;

/// A native class of physical material and the Reach global materials whose rules it takes. The
/// stock bullet impact tables, 69 of them with the effect-only carrier's own `80C1BEDF`, give each
/// class its own impact graph: sparks and a metal bullet hole for metal, and a rock bullet hole
/// with dust and chips for stone. A filter on the anchor also takes its descendants. With
/// `category`, every other readable material naming the anchor's category word at `+0x10` joins
/// the class, as the metal records under the unreadable `80C70F91` do.
struct Class {
    name: &'static str,
    anchor: u32,
    category: bool,
    sources: &'static [&'static str],
}

const CLASSES: [Class; 2] = [
    Class {
        name: "metal",
        anchor: 0x80c7_0c96,
        category: true,
        sources: &["hard_metal_thick", "hard_metal_solid"],
    },
    Class {
        name: "stone",
        anchor: 0x80c7_0d6b,
        category: false,
        sources: &["hard_terrain"],
    },
];

/// The class's filter materials: its anchor and, with `category`, each readable material sharing
/// the anchor's category word whose parent is neither the anchor nor in the category.
fn members(reader: &mut Reader, class: &Class) -> Result<Vec<u32>> {
    let anchor = reader.tag(class.anchor, Some(MATERIAL))?;
    let mut tags = vec![class.anchor];
    if !class.category {
        return Ok(tags);
    }
    let category = anchor.u32(0x10)?;
    let mut candidates = reader.classes(MATERIAL);
    candidates.sort_unstable();
    for tag in candidates {
        if tag == class.anchor {
            continue;
        }
        // A record the packages cannot supply, as `80C70F91`, is not judged.
        let Ok(record) = reader.tag(tag, Some(MATERIAL)) else {
            continue;
        };
        if record.u32(0x10)? != category {
            continue;
        }
        // A parent in the class already covers this record, through the anchor or its own row.
        let parent = record.u32(0xc)?;
        let covered = parent == class.anchor
            || reader
                .tag(parent, Some(MATERIAL))
                .is_ok_and(|next| next.u32(0x10).ok() == Some(category));
        if !covered {
            tags.push(tag);
        }
    }
    Ok(tags)
}

type Key = (i16, u32, [u32; 2], [u32; 2], u32);

/// A rule's bounce parameters: response, chance, angle and velocity windows and angular noise.
fn key(rule: &Rule) -> Key {
    (
        rule.response,
        rule.chance.to_bits(),
        rule.angles.map(f32::to_bits),
        rule.velocity.map(f32::to_bits),
        rule.noise.to_bits(),
    )
}

/// The named source material's first bounce rule, unless an unconditional rule for every contact
/// ends its list first.
fn material_bounce<'a>(rules: &'a [Rule], name: &str) -> Option<&'a Rule> {
    for rule in rules
        .iter()
        .filter(|rule| rule.material.as_deref() == Some(name))
    {
        if matches!(rule.response, BOUNCE | DUD | RICOCHET) {
            return Some(rule);
        }
        if rule.unconditional()
            && rule.flags & (ONLY_UNITS | ONLY_BIPEDS | ONLY_VEHICLES | NEVER_UNITS) == 0
        {
            return None;
        }
    }
    None
}

/// A named-material rule and the native classes that take it, with their filter materials.
struct Named {
    bounce: Bounce,
    filters: Vec<u32>,
    classes: Vec<usize>,
}

/// The named-material rules the native classes can take, most preferred first, with a report row
/// for every class. Classes holding the same rule share it. A material group hosts one rule, so
/// a rule shared by more classes comes first, then one covering more source materials, then the
/// higher chance.
fn named(reader: &mut Reader, rules: &[Rule]) -> Result<(Vec<Named>, Vec<Value>)> {
    let mut usable: Vec<(usize, Key, &Rule, usize)> = Vec::new();
    let mut report = Vec::new();
    for (index, class) in CLASSES.iter().enumerate() {
        let found = class
            .sources
            .iter()
            .filter_map(|name| material_bounce(rules, name).map(|rule| (*name, rule)))
            .collect::<Vec<_>>();
        let mut row = json!({"class":class.name,"anchor":format!("{:08X}", class.anchor),
            "sources":class.sources,"hosted":false,
            "rules":found.iter().map(|(name, rule)| json!({"material":name,"response":rule.name(),
                "chance":rule.chance,"angle_radians":rule.angles,"flags":rule.flags})).collect::<Vec<_>>()});
        match found.first() {
            None => row["reason"] = json!("The source has no bounce rule for these materials"),
            Some((_, first)) if found.iter().any(|(_, rule)| key(rule) != key(first)) => {
                row["reason"] = json!(
                    "The source rules for these materials differ, while the native class is one"
                );
            }
            Some((_, first)) if first.flags & (ONLY_UNITS | ONLY_BIPEDS | ONLY_VEHICLES) != 0 => {
                row["reason"] = json!(
                    "The source bounce applies only to units, which the native bounce rejects"
                );
            }
            Some((_, first)) => usable.push((index, key(first), *first, found.len())),
        }
        report.push(row);
    }
    let mut keys: Vec<(usize, usize, f32, Key)> = Vec::new();
    for &(_, rule_key, rule, _) in &usable {
        if keys.iter().any(|known| known.3 == rule_key) {
            continue;
        }
        let sharing = usable.iter().filter(|other| other.1 == rule_key);
        let classes = sharing.clone().count();
        let sources = sharing.map(|other| other.3).sum::<usize>();
        keys.push((classes, sources, rule.chance, rule_key));
    }
    keys.sort_by(|a, b| (b.0, b.1).cmp(&(a.0, a.1)).then(b.2.total_cmp(&a.2)));
    let mut out = Vec::new();
    for (.., chosen) in keys {
        let mut filters = Vec::new();
        let mut classes = Vec::new();
        let mut representative = None;
        for &(index, rule_key, rule, _) in &usable {
            if rule_key == chosen {
                filters.extend(members(reader, &CLASSES[index])?);
                classes.push(index);
                representative = Some(rule);
            }
        }
        let rule = representative.context("Named-material rule")?;
        filters.sort_unstable();
        filters.dedup();
        let mut limits = vec![
            "Native responses have no angle condition. The source angle window becomes a share of contacts, assuming incoming directions are uniform over the hemisphere.",
            "The rule reaches the native classes that stock bullet impact tables establish. Native metal does not separate thin from thick metal, so thin metal also ricochets, and source materials of other classes keep the default contact.",
            "The source friction values are not carried. The stock parameter's speed and friction programs stay.",
        ];
        if rule.flags & NEVER_UNITS == 0 {
            limits.push("The native bounce rejects unit contacts, while the source rule also applies to vehicles and other units of these materials.");
        }
        if rule.velocity != [0.; 2] {
            limits.push("The source velocity window is not carried.");
        }
        out.push(Named {
            bounce: Bounce {
                chance: rule.chance * window(rule.angles),
                source_chance: rule.chance,
                angles: rule.angles,
                noise: rule.noise,
                response: rule.name(),
                limits,
            },
            filters,
            classes,
        });
    }
    Ok((out, report))
}

struct Bounce {
    chance: f32,
    source_chance: f32,
    angles: [f32; 2],
    noise: f32,
    response: &'static str,
    limits: Vec<&'static str>,
}

/// The default material's first bounce rule, when the native bounce, which rejects units, can host
/// it. Earlier unconditional default rows can already take units, as the Needle Rifle's
/// biped-only attachment does before its bounce.
fn bounce(rules: &[Rule]) -> std::result::Result<Bounce, &'static str> {
    let mut units = false;
    let mut bipeds = false;
    for rule in rules.iter().filter(|r| r.default()) {
        if matches!(rule.response, BOUNCE | DUD | RICOCHET) {
            let mut limits = vec![
                "Native responses have no angle condition. The source angle window becomes a share of contacts, assuming incoming directions are uniform over the hemisphere.",
                "Other materials' rules and the source friction values are not carried. The stock parameter's speed and friction programs stay.",
            ];
            if rule.flags & (ONLY_UNITS | ONLY_BIPEDS | ONLY_VEHICLES) != 0 {
                return Err(
                    "The source bounce applies only to units, which the native bounce rejects",
                );
            }
            if rule.flags & NEVER_UNITS == 0 && !units {
                if !bipeds {
                    return Err(
                        "The source bounce also applies to units, which the native bounce mask rejects",
                    );
                }
                limits.push("Vehicles do not bounce, while the source bounces off them once bipeds are taken by the earlier rule.");
            }
            if rule.velocity != [0.; 2] {
                limits.push("The source velocity window is not carried.");
            }
            return Ok(Bounce {
                chance: rule.chance * window(rule.angles),
                source_chance: rule.chance,
                angles: rule.angles,
                noise: rule.noise,
                response: rule.name(),
                limits,
            });
        }
        if rule.unconditional() {
            if rule.flags & ONLY_UNITS != 0 {
                units = true;
            } else if rule.flags & ONLY_BIPEDS != 0 {
                bipeds = true;
            } else if rule.flags & (NEVER_UNITS | ONLY_VEHICLES) == 0 {
                // An unconditional rule for every contact ends the default material's list.
                break;
            }
        }
    }
    Err(NO_DEFAULT)
}

/// Hosts the source's bounce on the movement's free stop response, the shard movement's response
/// of mask 0 with a stop and a damage action and otherwise unused. The native bounce is selector 2
/// with a `808037EA` parameter, as Fighting Lion's movement `815282E7` uses on nodes of mask
/// `0x101`, which reject unit contacts. Its parameter, with the code and constant arrays its
/// relative descriptors name, is copied as one block at its 16-byte phase for each hosted rule,
/// and its angle variation at `+0x04` takes the rule's angular noise. Responses are tried from the
/// last group to the first, so a failed draw falls through to the movement's own surface response.
/// A movement without a free response hosts it on a native bounce just before its surface
/// attachment or, as the effect-only carrier's movement `815B39AC` does, in material groups
/// scanned before a group covering every material. A default-material bounce takes every
/// material. Named-material bounces take one material group per rule, filtered to the rule's
/// native classes, and the movement gains groups copied from its last one when it has too few.
/// `allocations` holds the private allocation edits that such groups need. The carrier's bounce
/// limit must be a multiple of the copied increment.
pub(super) fn ricochet(
    reader: &mut Reader,
    owners: &mut BTreeMap<u32, Payload>,
    allocations: &mut BTreeMap<u32, Payload>,
    projectile: &Value,
) -> Result<Value> {
    let (hosted, mut report) = match plan_bounce(reader, projectile)? {
        BouncePlan::Hosted(hosted, report) => (hosted, report),
        BouncePlan::Retained(receipt) => return Ok(receipt),
    };
    let filtered = report.is_some();
    let first_bounce = &hosted[0].bounce;
    let (source_chance, source_angles) = (first_bounce.source_chance, first_bounce.angles);
    let (phase, increment, block) = bounce_template(reader)?;
    // A movement whose allocation another component shares cannot gain groups privately.
    let allocation = owners
        .values()
        .find(|p| Assembly::class(p).ok() == Some(0x8080388f))
        .map(|p| p.u32(0x44))
        .transpose()?;
    let shared = allocation.is_some_and(|tag| {
        owners
            .values()
            .filter(|p| p.u32(0x44).ok() == Some(tag))
            .count()
            > 1
    });
    let mut movements = owners
        .values_mut()
        .filter(|p| Assembly::class(p).ok() == Some(0x8080388f))
        .collect::<Vec<_>>();
    let [p] = movements.as_mut_slice() else {
        anyhow::bail!("Projectile movement is absent or ambiguous")
    };
    let inserted = add_material_hosts(
        reader,
        p,
        allocations,
        (allocation, shared),
        hosted.len(),
        filtered,
    )?;
    // Find the hosts before writing any rule. Added groups stay inert until a rule takes them.
    let definition = p.pointer(24)?;
    let views = BTreeSet::from([definition, usize::try_from(p.u64(p.pointer(16)? + 8)?)?]);
    let mut hosts = Vec::new();
    let mut limits = Vec::new();
    for &view in &views {
        let found = if filtered {
            let groups = p.array(view + 0x218, 88, Some(0x8080_37cd))?;
            material_groups(p, &groups)?
        } else {
            host(p, view)?.into_iter().collect()
        };
        if found.is_empty() {
            return Ok(json!({"status":"native",
                "reason":if filtered {
                    "Named-material rules need a material group to filter, which this carrier movement lacks"
                } else {
                    "The carrier movement has no single free stop response, no native bounce response just before its surface attachment and no single-node material group before a group covering every material to host the source bounce"
                },
                "materials":report,"source_chance":source_chance,"source_angle_radians":source_angles}));
        }
        hosts.push(found);
        // Definition `+78` and `+79` become the contact and bounce limits. A nonzero bounce
        // limit refuses a bounce once the count reaches it, so it must be a multiple of the
        // increment or one bounce can pass it.
        let (contact, bounces) = (p.u8(view + 0x78)?, p.u8(view + 0x79)?);
        if bounces != 0 && (increment <= 0 || i32::from(bounces) % increment != 0) {
            return Ok(json!({"status":"native",
                "reason":"The carrier's bounce limit is not a multiple of the native bounce count increment, so one bounce could pass the limit",
                "bounce_limit":bounces,"bounce_increment":increment,
                "source_chance":source_chance,"source_angle_radians":source_angles}));
        }
        limits.push(json!({"contact":contact,"bounce":bounces}));
    }
    limits.dedup();
    // Every view hosts the same rules, as many as its fewest material groups allow.
    let count = hosts
        .iter()
        .map(Vec::len)
        .min()
        .unwrap_or_default()
        .min(hosted.len());
    let mut blocks = Vec::new();
    for rule in &hosted[..count] {
        let start = p.0.len().div_ceil(16) * 16 + phase;
        p.0.resize(start, 0);
        p.0.extend_from_slice(&block);
        let size = p.0.len().div_ceil(16) * 16;
        p.0.resize(size, 0);
        let length = p.0.len() as u64;
        put(&mut p.0, 0, &length.to_le_bytes())?;
        put(&mut p.0, start + 8, &rule.bounce.noise.to_le_bytes())?;
        blocks.push(start + 4);
    }
    let kinds = write_bounces(p, &hosts, &hosted, &blocks, filtered)?;
    if let Some(rows) = report.as_mut() {
        for (index, rule) in hosted.iter().enumerate() {
            for &class in &rule.classes {
                if index < count {
                    rows[class]["hosted"] = json!(true);
                } else {
                    rows[class]["reason"] = json!(
                        "The movement has no further material group for this rule, which another rule's group could not share"
                    );
                }
            }
        }
    }
    let count_limits = limits;
    let lead = &hosted[0].bounce;
    let mut limits = lead.limits.clone();
    if kinds.contains("split_attachment") {
        limits.push("On a carrier whose native bounce sits just before its surface attachment, the bounce takes the attachment's mask and its gate program stays, and the attachment draws first at one minus the bounce chance.");
    }
    if kinds.contains("material_group") {
        limits.push(if filtered {
            "Each hosted rule's material group filters to its native classes in place of the carrier's own material, and a failed draw or another material falls through to the default group's contact."
        } else {
            "On a carrier whose bounce sits in a material group scanned before its default group, that group covers every material and drops the carrier's own response to its material, and a failed draw falls through to the default group's contact."
        });
    }
    if !inserted.is_null() {
        limits.push("Groups added to the movement are copies of its last material group, and sealing the movement adds a second definition view that every later writer must keep in step.");
    }
    limits.push("The source has no bounce count limit, and a carrier limit of zero applies none, so a projectile can bounce until its range or lifetime ends.");
    let rules_hosted = hosted[..count]
        .iter()
        .map(|rule| {
            json!({"response":rule.bounce.response,"chance":rule.bounce.chance,
                "source_chance":rule.bounce.source_chance,"source_angle_radians":rule.bounce.angles,
                "angular_noise_radians":rule.bounce.noise,
                "classes":rule.classes.iter().map(|&class| CLASSES[class].name).collect::<Vec<_>>(),
                "filters":rule.filters.iter().map(|tag| format!("{tag:08X}")).collect::<Vec<_>>()})
        })
        .collect::<Vec<_>>();
    Ok(
        json!({"status":"translated","response":lead.response,"chance":lead.chance,
        "source_chance":lead.source_chance,"source_angle_radians":lead.angles,
        "angular_noise_radians":lead.noise,"native_parameter":"815282E7 selector 2",
        "bounce_increment":increment,"count_limits":count_limits,"materials":report,
        "rules":if filtered { json!(rules_hosted) } else { Value::Null },"inserted_groups":inserted,
        "hosts":kinds,"hosted_views":hosts.len(),"gameplay_verified":false,"limits":limits}),
    )
}

fn shift_nonzero(p: &mut Payload, at: usize, by: i64) -> Result<()> {
    let value = i64::from_le_bytes(p.bytes::<8>(at)?);
    if value != 0 {
        put(&mut p.0, at, &(value + by).to_le_bytes())?;
    }
    Ok(())
}

/// Appends `bytes` as a native array of `class` named by the descriptor at `descriptor` and
/// returns its first row.
fn append_rows(
    p: &mut Payload,
    descriptor: usize,
    class: u32,
    bytes: &[u8],
    stride: usize,
) -> Result<usize> {
    ensure!(
        stride > 0 && !bytes.is_empty() && bytes.len().is_multiple_of(stride),
        "Projectile response array extent differs"
    );
    crate::tiger::payload::write_array(&mut p.0, descriptor, class, bytes.len() / stride, bytes)?;
    Ok(p.pointer(descriptor + 8)? + 16)
}

/// Gives an unsealed movement `extra` response groups copied from its last material group, as the
/// Rise of Iron importer's rebuild does. Groups (`808037CD`, `58`) and their instance twins
/// (`808037CC`, `30`) move to new arrays in the same order and name each other by absolute offset
/// at `+08`. Moved rows keep their filter (`+20`) and node (`+50`) arrays, and moved twins their
/// instance root (`+10`) and node twins (`+28`), whose `+10` names the moved twin. A copied group
/// takes one copied node (`808037D0`, `188`) and node twin (`808037CF`, `70`): the node keeps the
/// template's program arrays, gets its own action rows naming the template's parameters, and names
/// its twin at `+08`, `+20` and `+F8` while the twin names it at `+08`, `+28` and `+48`. The
/// allocation's member `5680393D` gains a group wrapper per copy with one node list and no node
/// storage. The caller seals the movement afterwards, since the twins lie past its schema.
fn insert_groups(p: &mut Payload, allocation: &mut Payload, extra: usize) -> Result<()> {
    let definition = p.pointer(24)?;
    let instance = p.pointer(16)?;
    let groups = p.array(definition + 0x218, 0x58, Some(0x8080_37cd))?;
    let twins = p.array(instance + 0x30, 0x30, Some(0x8080_37cc))?;
    ensure!(
        !groups.is_empty() && groups.len() == twins.len(),
        "Projectile group twins differ from their groups"
    );
    let owner = p.u32(groups[0])?;
    for (&group, &twin) in groups.iter().zip(&twins) {
        ensure!(
            p.u32(group)? == owner
                && p.u32(twin)? == owner
                && p.u64(group + 8)? == twin as u64
                && p.u64(twin + 8)? == group as u64
                && p.pointer(twin + 0x10)? == instance,
            "Projectile group and twin are not reciprocal"
        );
        for node_twin in p.array(twin + 0x20, 0x70, None)? {
            ensure!(
                p.pointer(node_twin + 0x10)? == twin,
                "Projectile node twin does not name its group twin"
            );
        }
    }
    // Only the reciprocal fields address these rows by offset.
    for at in (0..p.0.len().saturating_sub(15)).step_by(4) {
        if p.u32(at)? == owner && matches!(p.u32(at + 4)?, 0x8080_37cc | 0x8080_37cd) {
            ensure!(
                groups.contains(&at) || twins.contains(&at),
                "Another record addresses a projectile response group"
            );
        }
    }
    let template = groups.len() - 1;
    ensure!(
        material_groups(p, &groups)?.first().is_some_and(
            |host| matches!(host, Host::Group { group, .. } if *group == groups[template])
        ),
        "The movement's last group is not a material group to copy"
    );
    let (template_group, template_twin) = (groups[template], twins[template]);
    let nodes = p.array(template_group + 0x48, 0x188, Some(0x8080_37d0))?;
    let node_twins = p.array(template_twin + 0x20, 0x70, None)?;
    let ([template_node], [template_node_twin]) = (nodes.as_slice(), node_twins.as_slice()) else {
        anyhow::bail!("The material group to copy has another node count");
    };
    let (template_node, template_node_twin) = (*template_node, *template_node_twin);
    ensure!(
        p.u64(template_node + 0x130)? == 0 && p.u64(template_node_twin + 0x60)? == 0,
        "The material group to copy reads named inputs"
    );
    let node_twin_class = p.u32(p.pointer(template_twin + 0x28)? + 8)?;
    let total = groups.len() + extra;
    let mut group_rows = Vec::new();
    let mut twin_rows = Vec::new();
    for index in 0..total {
        let from = index.min(template);
        group_rows.extend_from_slice(&p.0[groups[from]..groups[from] + 0x58]);
        twin_rows.extend_from_slice(&p.0[twins[from]..twins[from] + 0x30]);
    }
    let first_group = append_rows(p, definition + 0x218, 0x8080_37cd, &group_rows, 0x58)?;
    let first_twin = append_rows(p, instance + 0x30, 0x8080_37cc, &twin_rows, 0x30)?;
    for index in 0..total {
        let from = index.min(template);
        let (group, twin) = (first_group + index * 0x58, first_twin + index * 0x30);
        let (old_group, old_twin) = (groups[from], twins[from]);
        shift_nonzero(p, group + 0x20, old_group as i64 - group as i64)?;
        shift_nonzero(p, group + 0x50, old_group as i64 - group as i64)?;
        shift_nonzero(p, twin + 0x10, old_twin as i64 - twin as i64)?;
        shift_nonzero(p, twin + 0x28, old_twin as i64 - twin as i64)?;
        put(&mut p.0, group + 8, &(twin as u64).to_le_bytes())?;
        put(&mut p.0, twin + 8, &(group as u64).to_le_bytes())?;
        if index < groups.len() {
            for node_twin in p.array(twin + 0x20, 0x70, None)? {
                put(
                    &mut p.0,
                    node_twin + 0x10,
                    &(twin as i64 - (node_twin + 0x10) as i64).to_le_bytes(),
                )?;
            }
            continue;
        }
        let node_row = p.0[template_node..template_node + 0x188].to_vec();
        let node_twin_row = p.0[template_node_twin..template_node_twin + 0x70].to_vec();
        let node = append_rows(p, group + 0x48, 0x8080_37d0, &node_row, 0x188)?;
        let node_twin = append_rows(p, twin + 0x20, node_twin_class, &node_twin_row, 0x70)?;
        // The copied node's program and input arrays stay the template's.
        for field in [0xc8, 0xd8, 0x108, 0x118, 0x138, 0x180] {
            shift_nonzero(p, node + field, template_node as i64 - node as i64)?;
        }
        shift_nonzero(
            p,
            node_twin + 0x68,
            template_node_twin as i64 - node_twin as i64,
        )?;
        put(&mut p.0, node + 8, &(node_twin as u64).to_le_bytes())?;
        put(
            &mut p.0,
            node + 0x20,
            &((node_twin + 0x20) as u64).to_le_bytes(),
        )?;
        put(
            &mut p.0,
            node + 0xf8,
            &((node_twin + 0x40) as u64).to_le_bytes(),
        )?;
        put(&mut p.0, node_twin + 8, &(node as u64).to_le_bytes())?;
        put(
            &mut p.0,
            node_twin + 0x10,
            &(twin as i64 - (node_twin + 0x10) as i64).to_le_bytes(),
        )?;
        put(
            &mut p.0,
            node_twin + 0x28,
            &((node + 0x18) as u64).to_le_bytes(),
        )?;
        put(
            &mut p.0,
            node_twin + 0x48,
            &((node + 0xf0) as u64).to_le_bytes(),
        )?;
        // Its own action rows, so a host can rewrite them without touching the template's.
        let actions = p.array(template_node + 0x178, 16, Some(0x8080_37f7))?;
        let mut action_rows = Vec::new();
        let mut targets = Vec::new();
        for &action in &actions {
            action_rows.extend_from_slice(&p.0[action..action + 16]);
            targets.push(p.pointer(action)?);
        }
        let first_action = append_rows(p, node + 0x178, 0x8080_37f7, &action_rows, 16)?;
        for (row, &target) in targets.iter().enumerate() {
            sequence::relative(p, first_action + row * 16, target)?;
        }
    }
    let length = p.0.len() as u64;
    put(&mut p.0, 0, &length.to_le_bytes())?;
    insert_allocation(allocation, groups.len(), extra)
}

/// The movement allocation keeps one wrapper per group twin under member `5680393D`, each with a
/// node list (`5B2D36EB`) counting its node twins. Copies take the last wrapper's form with an
/// empty node list of one node.
fn insert_allocation(allocation: &mut Payload, groups: usize, extra: usize) -> Result<()> {
    let members = allocation
        .array(0x20, 40, Some(0x8080_8852))?
        .into_iter()
        .filter(|&row| allocation.u32(row).ok() == Some(0x5680_393d))
        .collect::<Vec<_>>();
    let [member] = members.as_slice() else {
        anyhow::bail!("Projectile movement allocation has no single group member");
    };
    let member = *member;
    let wrappers = allocation.array(member + 24, 40, None)?;
    ensure!(
        allocation.u32(member + 16)? == 0x8080_37cc
            && usize::try_from(allocation.u32(member + 20)?)? == groups
            && wrappers.len() == groups,
        "Projectile movement allocation differs from its groups"
    );
    let wrapper_class = allocation.u32(allocation.pointer(member + 32)? + 8)?;
    let template = wrappers[groups - 1];
    let lists = allocation.array(template + 24, 40, None)?;
    let [list] = lists.as_slice() else {
        anyhow::bail!("Copied group allocation has no single node list");
    };
    ensure!(
        allocation.u32(*list)? == 0x5b2d_36eb
            && allocation.u32(*list + 20)? == 1
            && allocation.u64(*list + 24)? == 0,
        "Copied group allocation has another node list"
    );
    let list_class = allocation.u32(allocation.pointer(template + 32)? + 8)?;
    let list_row = allocation.0[*list..*list + 40].to_vec();
    let mut wrapper_rows = Vec::new();
    for index in 0..groups + extra {
        let from = wrappers[index.min(groups - 1)];
        wrapper_rows.extend_from_slice(&allocation.0[from..from + 40]);
    }
    let first = append_rows(allocation, member + 24, wrapper_class, &wrapper_rows, 40)?;
    for (index, &from) in wrappers.iter().enumerate() {
        let wrapper = first + index * 40;
        shift_nonzero(allocation, wrapper + 32, from as i64 - wrapper as i64)?;
    }
    for index in groups..groups + extra {
        let wrapper = first + index * 40;
        append_rows(allocation, wrapper + 24, list_class, &list_row, 40)?;
    }
    put(
        &mut allocation.0,
        member + 20,
        &u32::try_from(groups + extra)?.to_le_bytes(),
    )?;
    let length = allocation.0.len() as u64;
    put(&mut allocation.0, 0, &length.to_le_bytes())
}

#[derive(Clone, Copy)]
enum Host {
    /// A response of mask 0 with only a stop and a damage action, otherwise unused.
    Free { node: usize, action: usize },
    /// A native bounce response `node` directly before the surface attachment `stick` in the same
    /// group. Responses are scanned from last to first, so the attachment is tried first.
    Split {
        stick: usize,
        node: usize,
        action: usize,
    },
    /// A trailing group `group`, filtered to named materials, whose single node of mask 0 bounces
    /// (selector 2 or 17) and deals damage, scanned before the group `from` that has no filter
    /// rows and a fallback material and so covers every material. `CEB5D0` scans groups from
    /// last to first and passes to the next group when no node of one is taken.
    Group {
        group: usize,
        from: usize,
        node: usize,
        action: usize,
    },
}

/// The view's single free stop response, or else its last surface attachment (a placement,
/// selector 1) with a native bounce response just before it in the same group, or else the last
/// bounce group before a group covering every material.
fn host(p: &Payload, view: usize) -> Result<Option<Host>> {
    let mut free = Vec::new();
    let mut split = None;
    let groups = p.array(view + 0x218, 88, Some(0x8080_37cd))?;
    for &group in &groups {
        let nodes = p.array(group + 0x48, 392, Some(0x8080_37d0))?;
        let mut selectors = Vec::new();
        for &node in &nodes {
            let actions = p.array(node + 0x178, 16, Some(0x8080_37f7))?;
            let kinds = actions
                .iter()
                .map(|&action| p.u8(action + 8))
                .collect::<Result<Vec<_>>>()?;
            if p.u64(node + 0x28)? == 0 && kinds == [10, 9] {
                free.push(Host::Free {
                    node,
                    action: actions[0],
                });
            }
            selectors.push((actions, kinds));
        }
        let Some(stick) = selectors.iter().rposition(|(_, kinds)| kinds.contains(&1)) else {
            continue;
        };
        if stick == 0 {
            continue;
        }
        let (actions, kinds) = &selectors[stick - 1];
        if let Some(at) = kinds.iter().position(|&kind| kind == 2) {
            ensure!(
                p.u32(p.pointer(actions[at])? - 4)? == 0x8080_37ea,
                "Native bounce parameter class differs"
            );
            split = Some(Host::Split {
                stick: nodes[stick],
                node: nodes[stick - 1],
                action: actions[at],
            });
        }
    }
    if free.len() == 1 {
        return Ok(free.pop());
    }
    if split.is_some() {
        return Ok(split);
    }
    Ok(material_groups(p, &groups)?.into_iter().next())
}

/// A group gate at `+40` of `FFFFFFFF`, no filter rows at `+18`, no fallback path at `+28` and a
/// fallback material at `+30`. `CF3BB0` then takes every contact material whose ancestry reaches
/// the fallback, the whole tree when it is the root.
fn covers(p: &Payload, group: usize) -> Result<bool> {
    Ok(p.u32(group + 0x40)? == u32::MAX
        && p.u64(group + 0x18)? == 0
        && p.u64(group + 0x28)? == 0
        && p.u32(group + 0x30)? != u32::MAX)
}

/// The single node and bounce action of a group that is not covering and has no gate, when its
/// node has mask 0 and only a bounce (selector 2 or 17) and damage actions.
fn bounce_group(p: &Payload, group: usize) -> Result<Option<(usize, usize)>> {
    if covers(p, group)? || p.u32(group + 0x40)? != u32::MAX {
        return Ok(None);
    }
    let nodes = p.array(group + 0x48, 392, Some(0x8080_37d0))?;
    let [node] = nodes.as_slice() else {
        return Ok(None);
    };
    if p.u64(node + 0x28)? != 0 {
        return Ok(None);
    }
    let mut bounce = None;
    for action in p.array(node + 0x178, 16, Some(0x8080_37f7))? {
        let class = p.u32(p.pointer(action)? - 4)?;
        match (p.u8(action + 8)?, class) {
            (2, 0x8080_37ea) | (17, 0x8080_37e9) if bounce.is_none() => bounce = Some(action),
            (9, 0x8080_37f3) => {}
            _ => return Ok(None),
        }
    }
    Ok(bounce.map(|action| (*node, action)))
}

/// The view's material groups, last first: the trailing bounce groups, all scanned before a
/// covering group. None when no covering group precedes them.
fn material_groups(p: &Payload, groups: &[usize]) -> Result<Vec<Host>> {
    let mut found = Vec::new();
    let mut index = groups.len();
    while index > 0 {
        let Some((node, action)) = bounce_group(p, groups[index - 1])? else {
            break;
        };
        found.push((groups[index - 1], node, action));
        index -= 1;
    }
    let mut from = None;
    for &group in groups[..index].iter().rev() {
        if covers(p, group)? {
            from = Some(group);
            break;
        }
    }
    let Some(from) = from else {
        return Ok(Vec::new());
    };
    Ok(found
        .into_iter()
        .map(|(group, node, action)| Host::Group {
            group,
            from,
            node,
            action,
        })
        .collect())
}

/// What became of each source response kind, for the conversion receipt. `ricochet` is the
/// ricochet host's own receipt when the carrier ran one, and `strike` the strike host's.
pub(super) fn summary(
    projectile: &Value,
    ricochet: Option<&Value>,
    strike: &Value,
) -> Result<Value> {
    let rules = rules(projectile)?;
    let mut kinds = BTreeMap::<&str, Vec<Value>>::new();
    for rule in &rules {
        let mut row = json!({
            "material":rule.material,"material_index":rule.index,"chance":rule.chance,
            "angle_radians":rule.angles,"flags":rule.flags});
        // Flags 5 and 6 restrict a row to tethered or untethered projectiles, which the
        // remote detonation receipt answers.
        match (
            rule.flags & ONLY_TETHERED != 0,
            rule.flags & ONLY_UNTETHERED != 0,
        ) {
            (true, false) => row["when"] = json!("tethered"),
            (false, true) => row["when"] = json!("untethered"),
            (true, true) => row["when"] = json!("never"),
            (false, false) => {}
        }
        kinds.entry(rule.name()).or_default().push(row);
    }
    let hosted = ricochet.is_some_and(|r| r["status"] == "translated");
    let struck = strike["status"] == "partial";
    let out = kinds
        .into_iter()
        .map(|(kind, rows)| {
            let (status, reason) = match kind {
                "bounce" | "bounce_dud" | "fizzle_ricochet" if hosted => (
                    "partial",
                    "Source bounce rules are hosted on the native bounce, the default material's on every surface or each named material's on its native class. Other rows are not mapped.",
                ),
                "attach" | "impact" => (
                    "native",
                    "The carrier's own responses decide sticking and detonation on contact. Source per-material chances are not applied.",
                ),
                "overpenetrate" => (
                    "omitted",
                    "No native response is established that carries a projectile through a surface. Selector 10 counts a contact against the contact limit without a stop request in its qualified slice, but whether the trajectory then continues is not traced, and the source rows name materials with no known native counterpart.",
                ),
                "turn_physical" => (
                    "omitted",
                    "No native response is established that hands a projectile to rigid-body physics. The carrier's own bounce, placement and finish responses answer these contacts.",
                ),
                "airstrike" if struck => (
                    "partial",
                    "The contact finishes through the strike host, which creates the converted bombs over the contact point, as the strike receipt records.",
                ),
                "airstrike" => (
                    "omitted",
                    "The contact calls the scenario's airstrike, and the strike receipt records why no strike host carries it.",
                ),
                _ => (
                    "omitted",
                    "Native physical materials are matched by hashed names with no known Reach counterpart, and the carrier hosts no matching response.",
                ),
            };
            (kind.to_owned(), json!({"status":status,"reason":reason,"rows":rows}))
        })
        .collect::<serde_json::Map<_, _>>();
    Ok(json!({"kinds":out,"ricochet":ricochet,"gameplay_verified":false}))
}

fn bounce_template(reader: &mut Reader) -> Result<(usize, i32, Vec<u8>)> {
    // The stock bounce parameter and the arrays it names, as one relative block.
    let stock = reader.tag(0x8152_82e7, Some(0x8080_9c36))?;
    let mut parameters = Vec::new();
    let sd = stock.pointer(24)?;
    for group in stock.array(sd + 0x218, 88, Some(0x8080_37cd))? {
        for node in stock.array(group + 0x48, 392, Some(0x8080_37d0))? {
            if stock.u64(node + 0x28)? != 0x101 {
                continue;
            }
            for action in stock.array(node + 0x178, 16, Some(0x8080_37f7))? {
                if stock.u8(action + 8)? == 2 {
                    parameters.push(stock.pointer(action)?);
                }
            }
        }
    }
    let first = *parameters.first().context("Stock bounce parameter")?;
    ensure!(
        stock.u32(first - 4)? == 0x8080_37ea,
        "Stock bounce parameter class differs"
    );
    // Parameter `+00` is the signed count each bounce adds to trajectory `+118`.
    let increment = i32::from_le_bytes(stock.bytes::<4>(first)?);
    let mut end = first + 0xd0;
    for at in (first..first + 0xd0 - 8).step_by(8) {
        let count = stock.u64(at)?;
        if !(1..=64).contains(&count) {
            continue;
        }
        let Ok(target) = stock.pointer(at + 8) else {
            continue;
        };
        if target <= first || target > first + 0x400 || stock.u64(target)? != count {
            continue;
        }
        let stride = match stock.u32(target + 8)? {
            0x8080_0009 => 1,
            0x8080_0090 => 16,
            _ => continue,
        };
        end = end.max(target + 16 + usize::try_from(count)? * stride);
    }
    let block = stock.0[first - 4..end].to_vec();
    Ok(((first - 4) % 16, increment, block))
}

fn write_bounces(
    p: &mut Payload,
    hosts: &[Vec<Host>],
    hosted: &[Named],
    blocks: &[usize],
    filtered: bool,
) -> Result<BTreeSet<&'static str>> {
    let mut kinds = BTreeSet::new();
    for found in hosts {
        for (rule, (host, &parameter)) in hosted.iter().zip(found.iter().zip(blocks)) {
            let chance = rule.bounce.chance;
            match *host {
                Host::Free { node, action } => {
                    kinds.insert("free_stop");
                    put(&mut p.0, node + 0x28, &0x101u64.to_le_bytes())?;
                    put(&mut p.0, action + 8, &[2])?;
                    sequence::relative(p, action, parameter)?;
                    program::constant(p, node + 0xc0, chance)?;
                    program::constant(p, node + 0x100, 1.)?;
                }
                Host::Group {
                    group,
                    from,
                    node,
                    action,
                } => {
                    // A default-material bounce takes the covering group's empty filter list and
                    // fallback material, so it sees every contact that group does and is scanned
                    // before it. A named-material bounce filters to its native classes with no
                    // fallback. Either way the group's own material and steering response give
                    // way.
                    kinds.insert("material_group");
                    if filtered {
                        let rows = rule
                            .filters
                            .iter()
                            .flat_map(|&tag| {
                                let mut row = [0u8; 16];
                                row[8..12].copy_from_slice(&tag.to_le_bytes());
                                row
                            })
                            .collect::<Vec<_>>();
                        crate::tiger::payload::write_array(
                            &mut p.0,
                            group + 0x18,
                            0x8080_9fc7,
                            rule.filters.len(),
                            &rows,
                        )?;
                        let length = p.0.len() as u64;
                        put(&mut p.0, 0, &length.to_le_bytes())?;
                        put(&mut p.0, group + 0x28, &0u64.to_le_bytes())?;
                        put(&mut p.0, group + 0x30, &u64::from(u32::MAX).to_le_bytes())?;
                    } else {
                        let fallback = (p.u64(from + 0x28)?, p.u64(from + 0x30)?);
                        put(&mut p.0, group + 0x18, &[0; 16])?;
                        put(&mut p.0, group + 0x28, &fallback.0.to_le_bytes())?;
                        put(&mut p.0, group + 0x30, &fallback.1.to_le_bytes())?;
                    }
                    put(&mut p.0, node + 0x28, &0x101u64.to_le_bytes())?;
                    put(&mut p.0, action + 8, &[2])?;
                    sequence::relative(p, action, parameter)?;
                    program::constant(p, node + 0xc0, chance)?;
                    program::constant(p, node + 0x100, 1.)?;
                }
                Host::Split {
                    stick,
                    node,
                    action,
                } => {
                    // The attachment is tried first. It keeps the contacts the bounce does not
                    // take, and the bounce node, given the attachment's mask, takes the rest.
                    kinds.insert("split_attachment");
                    let mask = p.u64(stick + 0x28)?;
                    put(&mut p.0, node + 0x28, &mask.to_le_bytes())?;
                    sequence::relative(p, action, parameter)?;
                    program::constant(p, node + 0xc0, 1.)?;
                    program::constant(p, stick + 0xc0, 1. - chance)?;
                }
            }
        }
    }
    Ok(kinds)
}

fn add_material_hosts(
    reader: &mut Reader,
    p: &mut Payload,
    allocations: &mut BTreeMap<u32, Payload>,
    (allocation, shared): (Option<u32>, bool),
    rules: usize,
    filtered: bool,
) -> Result<Value> {
    // Named rules beyond the movement's material groups get groups of their own, copied from its
    // last one before the component is sealed. A carrier that cannot take them hosts fewer rules.
    let mut inserted = Value::Null;
    if filtered && rules > 1 {
        let definition = p.pointer(24)?;
        let groups = p.array(definition + 0x218, 88, Some(0x8080_37cd))?;
        let available = material_groups(p, &groups)?.len();
        let single = usize::try_from(p.u64(p.pointer(16)? + 8)?)? == definition;
        let extra = rules.saturating_sub(available);
        inserted = if available == 0 || extra == 0 {
            Value::Null
        } else if !single {
            json!({"status":"native","reason":"The carrier movement is already sealed, so its response groups are not rebuilt"})
        } else if shared {
            json!({"status":"native","reason":"Another carrier component shares the movement's allocation"})
        } else {
            let tag = allocation.context("Projectile movement allocation")?;
            let mut edited = match allocations.get(&tag) {
                Some(payload) => payload.clone(),
                None => (*reader.tag(tag, Some(0x8080_9bbb))?).clone(),
            };
            insert_groups(p, &mut edited, extra)?;
            let end = p.0.len();
            let seal = crate::tiger::instance::seal_span(p, &mut Vec::new(), end)?;
            allocations.insert(tag, edited);
            json!({"status":"translated","groups":extra,"allocation":format!("{tag:08X}"),"seal":seal})
        };
    }
    Ok(inserted)
}

enum BouncePlan {
    Hosted(Vec<Named>, Option<Vec<Value>>),
    Retained(Value),
}

fn plan_bounce(reader: &mut Reader, projectile: &Value) -> Result<BouncePlan> {
    let rules = rules(projectile)?;
    // A source whose default material does not bounce may bounce off named materials, which
    // material groups filtered to their native classes host.
    let (hosted, report) = match bounce(&rules) {
        Ok(bounce) => (
            vec![Named {
                bounce,
                filters: Vec::new(),
                classes: Vec::new(),
            }],
            None,
        ),
        Err(reason) => {
            let (named, report) = if reason == NO_DEFAULT {
                named(reader, &rules)?
            } else {
                (Vec::new(), Vec::new())
            };
            if named.is_empty() {
                let bouncing = rules
                    .iter()
                    .filter(|rule| {
                        !rule.default() && matches!(rule.response, BOUNCE | DUD | RICOCHET)
                    })
                    .filter_map(|rule| rule.material.clone())
                    .collect::<BTreeSet<_>>();
                let mut receipt = json!({"status":"native","reason":reason});
                if !bouncing.is_empty() {
                    receipt["source_bounce_materials"] = json!(bouncing);
                    receipt["material_reason"] = json!(
                        "The source bounces only off these materials, and none of their rules has a native class to host it, as the materials report states"
                    );
                }
                if !report.is_empty() {
                    receipt["materials"] = json!(report);
                }
                return Ok(BouncePlan::Retained(receipt));
            }
            (named, Some(report))
        }
    };
    Ok(BouncePlan::Hosted(hosted, report))
}
