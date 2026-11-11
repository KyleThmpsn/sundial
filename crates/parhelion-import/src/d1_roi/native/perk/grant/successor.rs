//! A source script parameter row that no native bank row can carry lowers to a Named Property write
//! on its ability's slot, when the Shadowkeep version of the ability still declares the parameter.
//!
//! Native setter `00576010` writes every component of the resolved ability that declares the name
//! and creates none, so the write reaches only abilities whose script still reads the variable. The
//! ability is the source ability's own native entity, or for a source ability Shadowkeep lacks, the
//! entity the script parameter witness of `counterpart` names.
//!
//! Arc Blade (`95A5C27D`) has no Shadowkeep ability. Unstable Current's key `1C5D0E0E` sets its
//! script parameter `FF78BA8F` from 1 to 1.3, replacing it, and only Arc Staff's script owner
//! `80BC3BFF` declares that name. Recklessness's key `08B0D8EB` sets `C49F5B23` from 0 to 1 on Fist
//! of Havoc, whose Shadowkeep bank `80BC3CB0` lists no such parameter while its script owner
//! `80BC3CD0` and effect owner `80BC4700` declare it. Neither write is a port Bungie shipped.
use super::*;
use sundial::package_authoring::ability_bank::property_rows;

pub(super) fn lower(
    native: &mut Reader,
    data: &Payload,
    rows: &[usize],
    row: usize,
    entity: Option<u32>,
) -> Result<Option<(Action, Value)>> {
    let Some(parameters) = witness::script_parameters(data, rows, row)? else {
        return Ok(None);
    };
    let [(name, reset, value, operation)] = parameters.as_slice() else {
        return Ok(None);
    };
    // Add and replace agree from the reset while this row is the parameter's only writer.
    let writers = rows
        .iter()
        .map(|&other| witness::script_parameters(data, rows, other))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .filter(|parameters| parameters.iter().any(|parameter| parameter.0 == *name))
        .count();
    if writers != 1 {
        return Ok(None);
    }
    let value = if *operation == 1 {
        (f32::from_bits(*reset) + f32::from_bits(*value)).to_bits()
    } else {
        *value
    };
    let found = match entity {
        Some(entity) => counterpart::slot(native, entity)?.map(|slot| (entity, slot)),
        None => counterpart::by_parameter(native, data, rows)?,
    };
    let Some((entity, slot)) = found else {
        return Ok(None);
    };
    let owners = counterpart::declaring_owners(native, entity, *name)?;
    if owners.is_empty() {
        return Ok(None);
    }
    // A bank row that sets the parameter would carry it instead.
    let graph = native.tag(entity, Some(0x80809C0F))?;
    if let Some(bank) = sundial::package_authoring::ability_modifier::entity_bank(&graph.0)
        .map_err(anyhow::Error::msg)?
    {
        let payload = native.tag(bank, Some(0x80809C36))?;
        if property_rows(&payload.0)
            .map_err(anyhow::Error::msg)?
            .iter()
            .any(|row| {
                row.parameters
                    .iter()
                    .any(|parameter| parameter.name == *name)
            })
        {
            return Ok(None);
        }
    }
    let mask = 1u32 << slot.byte();
    Ok(Some((
        Action::Property {
            key: *name,
            target: 0,
            operation_byte: 0,
            removal: 1,
            value_bits: value,
            restore_bits: *reset,
            ability_mask: mask,
            input: 0,
            flag: 1,
        },
        json!({"parameter":name,"reset":reset,"value":value,"ability_mask":mask,
            "native_entity":entity,"native_declarations":owners,
            "route":"declared_parameter_write"}),
    )))
}
