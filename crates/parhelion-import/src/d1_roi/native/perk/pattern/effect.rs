//! Component readers shared with connected ability effect graphs.
//!
//! Perk patterns patch a whole native carrier graph. Ability effect graphs instead assemble
//! complete private owners, so these entries return source values and edit owner payloads.
pub(crate) use super::status::{Connected, Lifetime, host::Host};
use super::*;

/// Check a generated source network against the native graph's network owner.
pub(crate) fn network(
    source: &Source,
    native: &mut Reader,
    graph: &Payload,
    ordinal: usize,
    native_graph: u32,
    native_ordinal: usize,
    exclusion: u32,
) -> Result<u32> {
    network::effect(
        source,
        native,
        graph,
        ordinal,
        native_graph,
        native_ordinal,
        exclusion,
    )
}

/// A status host whose lifetime sequence is another owner of its graph.
pub(crate) fn status(
    graph: &Payload,
    p: &Payload,
    owner: u32,
    ordinal: usize,
) -> Result<Connected> {
    status::connected(graph, p, owner, ordinal)
}

/// A source scalar program (`8080038D`) at `at`: its code, constants and its one named direct
/// channel input with that input's row, if it reads one. Slot zero is the builtin clock.
#[allow(clippy::type_complexity)]
pub(crate) fn program(
    p: &Payload,
    at: usize,
) -> Result<(Vec<u8>, Vec<[u8; 16]>, Option<(u32, usize)>)> {
    status::program(p, at)
}

/// Whether a source dynamic definition schema is the player status icon script.
pub(crate) fn is_icon(schema: &Payload) -> bool {
    status::is_icon(schema)
}

/// A source player status icon, as a status without a host.
pub(crate) fn icon(p: &Payload, owner: u32) -> Result<Connected> {
    status::icon(p, owner)
}

/// A source Timer's flag word and lifetime.
pub(crate) fn timer(
    p: &Payload,
    owner: u32,
    live: usize,
    definition: usize,
) -> Result<([u8; 8], Lifetime)> {
    status::timer(p, owner, live, definition)
}

/// Check an embedded source runtime and definition pair that name each other.
pub(crate) fn embedded(
    p: &Payload,
    owner: u32,
    (instance, definition): (usize, usize),
    (ic, dc): (u32, u32),
) -> Result<()> {
    status::pair(p, owner, instance, definition, ic, dc)
}

/// Evidence that a native template's nested controller `stock_owner` owns the native form of the
/// child action source nested controller `owner` registers ([`nested::equivalent`]).
pub(crate) fn nested(
    source: &Source,
    native: &mut Reader,
    (owner, p): (u32, &Payload),
    (stock_owner, stock): (u32, &Payload),
) -> Result<Value> {
    let (_, _, child) = nested::wrapper(p, owner)?;
    ensure!(
        source.reference(child)? == 0x80802711,
        "Nested reference is not a source controller"
    );
    let (_, _, stock_child) = nested::native_wrapper(stock, stock_owner)?;
    nested::equivalent(source, native, child, stock_child)
}

/// A standalone Duration host whose lifetime input, if any, is a channel of its graph's bank.
pub(crate) fn host(graph: &Payload, p: &Payload, owner: u32, ordinal: usize) -> Result<Host> {
    super::status::host::read(graph, p, owner, ordinal)
}

/// Every constant modifier row of a source bank owner, in order.
pub(crate) struct Modifiers(Vec<Modifier>);

pub(crate) fn modifiers(source: &Source, graph: &Payload, owner: u32) -> Result<Modifiers> {
    Ok(Modifiers(modifier_owner(source, graph, owner)?))
}

impl Modifiers {
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    /// A standalone native graph whose single modifier bank has exactly this many rows.
    pub(crate) fn envelope(&self) -> Result<u32> {
        envelope(self.0.len())
    }

    /// Replace every native row of a bank owner whose owner word is `owner`, keeping the
    /// native pairs, weights and enable state.
    pub(crate) fn write(&self, target: &mut Payload, owner: u32) -> Result<Value> {
        let settings = target.pointer(24)?;
        ensure!(
            target.u32(settings - 4)? == 0x80803AFE
                && target.u64(settings + 0x78)? == 0
                && target.u64(settings + 0x90)? == 0
                && target.u64(settings + 0x98)? == 1,
            "Native modifier bank default enable differs"
        );
        let rows = target.array(settings + 88, 88, Some(0x80803B06))?;
        ensure!(
            rows.len() == self.0.len(),
            "Native modifier bank has a different row count"
        );
        let mut evidence = Vec::new();
        for (row, to) in self.0.iter().zip(rows) {
            let instance = usize::try_from(target.u64(to + 8)?)?;
            ensure!(
                target.u32(to)? == owner
                    && target.u32(to + 4)? == 0x80803B05
                    && target.u64(instance + 8)? == to as u64
                    && target.u64(to + 32)? == 0
                    && target.u64(to + 64)? == 1
                    && target.f32(instance + 64)? == 1.0
                    && target.u8(instance + 68)? == 1,
                "Native modifier row has a different pair or weight"
            );
            put(&mut target.0, to + 40, &row.amount.to_le_bytes())?;
            target.0[to + 44] = row.operation;
            put(&mut target.0, to + 72, &row.ability.to_le_bytes())?;
            put(&mut target.0, to + 74, &row.input.to_le_bytes())?;
            target.0[to + 76] = row.component;
            evidence.push(json!({"component":row.component,"ability":row.ability,
                "input":row.input,"operation":row.operation,"amount":row.amount,
                "source_change":row.change}));
        }
        Ok(Value::Array(evidence))
    }
}
