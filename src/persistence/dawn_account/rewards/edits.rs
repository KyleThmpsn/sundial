use super::*;
use crate::catalog::InventoryMetadata;

fn validate_currency(quantity: i32, metadata: &InventoryMetadata) -> Result<(), String> {
    if !supports_currency(metadata) {
        return Err("Dawn reward debts support single-slot profile currencies only. Weapons, armor, shaders and consumables are not supported.".into());
    }
    if quantity <= 0 || quantity as u32 > metadata.max_stack_size.unwrap_or(0) {
        return Err("The reward quantity must fit the currency's stack cap".into());
    }
    Ok(())
}

impl DawnAccountDocument {
    pub(crate) fn queue_currency(
        &mut self,
        character: usize,
        hash: u32,
        quantity: i32,
        metadata: &InventoryMetadata,
    ) -> Result<(), String> {
        validate_currency(quantity, metadata)?;
        if matches!(hash, 0 | 0x811C9DC5 | u32::MAX) {
            return Err("Invalid currency definition".into());
        }
        let character_soid = self
            .characters()
            .characters()
            .get(character)
            .and_then(|character| character.soid)
            .ok_or("Select a valid Dawn character")?
            .get();
        if self
            .reward_debts
            .iter()
            .filter(|debt| !debt.delivered)
            .count()
            >= 10_000
        {
            return Err("The reward queue is full".into());
        }
        let id = self
            .reward_sequence
            .checked_add(1)
            .filter(|id| *id > 0)
            .ok_or("The reward identity space is exhausted")?;
        let debt = RewardDebt {
            id,
            account_soid: self.primary_soid().get(),
            character_soid,
            runtime_epoch: self.metadata.reward_epoch as u64,
            session_id: EDITOR_SESSION,
            run_id: id as u64,
            mission_hash: EDITOR_MISSION,
            definition_hash: hash,
            quantity,
            credited: 0,
            delivered: false,
        };
        if self.reward_debts.iter().any(|before| {
            before.account_soid == debt.account_soid
                && before.runtime_epoch == debt.runtime_epoch
                && before.session_id == debt.session_id
                && before.run_id == debt.run_id
                && before.definition_hash == debt.definition_hash
        }) {
            return Err(
                "A reward already uses this delivery identity. Reload before queuing.".into(),
            );
        }
        self.reward_sequence = id;
        self.reward_debts.push(debt);
        Ok(())
    }

    pub(crate) fn set_debt_quantity(
        &mut self,
        id: i64,
        quantity: i32,
        metadata: &InventoryMetadata,
    ) -> Result<(), String> {
        validate_currency(quantity, metadata)?;
        self.pending_debt_mut(id)?.quantity = quantity;
        Ok(())
    }

    pub(crate) fn cancel_debt(&mut self, id: i64) -> Result<(), String> {
        self.pending_debt_mut(id)?.delivered = true;
        Ok(())
    }

    fn pending_debt_mut(&mut self, id: i64) -> Result<&mut RewardDebt, String> {
        let account = self.primary_soid().get();
        self.reward_debts
            .iter_mut()
            .find(|debt| {
                debt.id == id
                    && debt.account_soid == account
                    && !debt.delivered
                    && debt.credited == 0
            })
            .ok_or_else(|| {
                "Only uncredited pending rewards for this Dawn account can be changed".into()
            })
    }
}
