//! Validation shared by account aggregates.

use crate::{AccountError, AccountResult, DefinitionHash};

pub(crate) fn is_no_definition_hash(hash: DefinitionHash) -> bool {
    hash == crate::NO_DEFINITION_HASH
}

pub(crate) fn validate_authored_definition_hash(hash: DefinitionHash) -> AccountResult<()> {
    if is_no_definition_hash(hash) {
        Err(AccountError::InvalidDefinitionHash)
    } else {
        Ok(())
    }
}

pub(crate) fn validate_positive_quantity(quantity: i32) -> AccountResult<()> {
    if quantity > 0 {
        Ok(())
    } else {
        Err(AccountError::InvalidQuantity)
    }
}
