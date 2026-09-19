//! Authored unlock flags, independent of where an account is stored.
//!
//! An unlock definition is backed by a flag in one of four durable banks, addressed by a bank
//! code and a slot within it. Every runtime stores those banks somewhere different, so the bank
//! codes and their limits live here and each persistence adapter translates them at its own
//! boundary.

/// The durable bank an unlock definition's flag belongs to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnlockScope {
    /// Held once for the account.
    Account,
    /// Held once for the profile.
    Profile,
    /// Held per character.
    Character,
    /// Held per character, on the character's own replicated object.
    CharacterObject,
}

/// A set flag is stored as its biased two-bit true value, not as one.
pub const FLAG_SET: u8 = 2;
/// A clear flag is stored as zero.
pub const FLAG_CLEAR: u8 = 0;

impl UnlockScope {
    /// The bank an unlock-map code addresses. The codes are the engine's own, and a code with
    /// no bank here is one no runtime stores a flag for.
    #[must_use]
    pub const fn for_bank(bank: u8) -> Option<Self> {
        match bank {
            1 => Some(Self::Account),
            2 => Some(Self::Profile),
            3 => Some(Self::CharacterObject),
            6 => Some(Self::Character),
            _ => None,
        }
    }

    /// The unlock-map code this bank is addressed by.
    #[must_use]
    pub const fn bank(self) -> u8 {
        match self {
            Self::Account => 1,
            Self::Profile => 2,
            Self::CharacterObject => 3,
            Self::Character => 6,
        }
    }

    /// How many flags the bank holds. A slot at or past this is outside the region the engine
    /// reserves for it and would land in whatever follows.
    #[must_use]
    pub const fn capacity(self) -> usize {
        match self {
            Self::Account => 12_300,
            Self::Profile => 512,
            Self::Character => 256,
            Self::CharacterObject => 4_096,
        }
    }

    /// Whether every character owns a separate copy of this bank, so an account-wide unlock has
    /// to be written once for each of them.
    #[must_use]
    pub const fn per_character(self) -> bool {
        matches!(self, Self::Character | Self::CharacterObject)
    }

    /// The name a reader knows this bank by.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Account => "account",
            Self::Profile => "profile",
            Self::Character => "character",
            Self::CharacterObject => "character object",
        }
    }
}

/// One authored collection unlock, as the definition index it belongs to and the flag backing it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthoredUnlock {
    pub definition_index: u16,
    pub bank: u8,
    pub slot: u16,
}

impl AuthoredUnlock {
    /// The bank and slot this unlock sets, refusing a bank no runtime stores and a slot outside
    /// the bank's region.
    pub fn target(self) -> Result<(UnlockScope, usize), String> {
        let scope = UnlockScope::for_bank(self.bank).ok_or_else(|| {
            format!(
                "Authored unlock definition {} uses unsupported bank {}",
                self.definition_index, self.bank
            )
        })?;
        let slot = usize::from(self.slot);
        if slot >= scope.capacity() {
            return Err(format!(
                "Authored unlock definition {} uses {} slot {slot}, beyond the {} flags that bank holds",
                self.definition_index,
                scope.label(),
                scope.capacity()
            ));
        }
        Ok((scope, slot))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bank_code_round_trips_through_its_scope() {
        for scope in [
            UnlockScope::Account,
            UnlockScope::Profile,
            UnlockScope::Character,
            UnlockScope::CharacterObject,
        ] {
            assert_eq!(UnlockScope::for_bank(scope.bank()), Some(scope));
        }
        // Codes 4 and 5 are not flag banks, so an unlock naming one is refused rather than
        // written to whichever bank happened to sort next.
        for bank in [0, 4, 5, 7, 255] {
            assert_eq!(UnlockScope::for_bank(bank), None);
        }
    }

    #[test]
    fn a_slot_outside_its_bank_is_refused_by_name() {
        let unlock = AuthoredUnlock {
            definition_index: 12,
            bank: UnlockScope::Character.bank(),
            slot: 256,
        };
        let error = unlock.target().unwrap_err();
        assert!(error.contains("character slot 256"), "{error}");
        let inside = AuthoredUnlock {
            slot: 255,
            ..unlock
        };
        assert_eq!(inside.target(), Ok((UnlockScope::Character, 255)));
    }

    #[test]
    fn an_unsupported_bank_names_the_definition_that_used_it() {
        let error = AuthoredUnlock {
            definition_index: 7,
            bank: 9,
            slot: 0,
        }
        .target()
        .unwrap_err();
        assert!(error.contains("definition 7"), "{error}");
        assert!(error.contains("bank 9"), "{error}");
    }
}
