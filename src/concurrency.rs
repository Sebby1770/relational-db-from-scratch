use std::collections::{HashMap, HashSet};

use crate::error::{DbError, Result};
use crate::transaction::TransactionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockMode {
    Shared,
    Exclusive,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct LockState {
    shared_holders: HashSet<TransactionId>,
    exclusive_holder: Option<TransactionId>,
}

#[derive(Debug, Default)]
pub struct LockManager {
    locks: HashMap<String, LockState>,
}

impl LockManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn acquire(
        &mut self,
        transaction_id: TransactionId,
        resource: impl Into<String>,
        mode: LockMode,
    ) -> Result<()> {
        let resource = resource.into();
        let state = self.locks.entry(resource.clone()).or_default();

        match mode {
            LockMode::Shared => {
                if state
                    .exclusive_holder
                    .is_some_and(|holder| holder != transaction_id)
                {
                    return Err(DbError::Transaction(format!(
                        "resource {resource} is exclusively locked"
                    )));
                }

                state.shared_holders.insert(transaction_id);
                Ok(())
            }
            LockMode::Exclusive => {
                let other_shared_holders = state
                    .shared_holders
                    .iter()
                    .any(|holder| *holder != transaction_id);
                let other_exclusive_holder = state
                    .exclusive_holder
                    .is_some_and(|holder| holder != transaction_id);

                if other_shared_holders || other_exclusive_holder {
                    return Err(DbError::Transaction(format!(
                        "resource {resource} has conflicting locks"
                    )));
                }

                state.shared_holders.remove(&transaction_id);
                state.exclusive_holder = Some(transaction_id);
                Ok(())
            }
        }
    }

    pub fn release_all(&mut self, transaction_id: TransactionId) {
        for state in self.locks.values_mut() {
            state.shared_holders.remove(&transaction_id);

            if state.exclusive_holder == Some(transaction_id) {
                state.exclusive_holder = None;
            }
        }

        self.locks.retain(|_, state| {
            !state.shared_holders.is_empty() || state.exclusive_holder.is_some()
        });
    }
}
