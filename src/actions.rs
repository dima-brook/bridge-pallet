use sp_std::{collections::btree_set::BTreeSet, vec::Vec};
use crate::{Config, Balance, EgldBalance};
use sp_runtime::{RuntimeDebug};
use codec::{Encode, Decode};


/// Action to perform on this chain
#[derive(Encode, Decode, RuntimeDebug, Clone, PartialEq)]
pub enum LocalAction<T: Config> {
    /// Release local currency and send to target
    Unfreeze {
        to: T::AccountId,
        value: Balance<T>
    },
    /// Call a smart contract
    RpcCall {
        /// address of the smart contract
        contract: T::AccountId,
        /// Raw call data of the smart contract
        call_data: Vec<u8>
    },
    /// Mint foreign currency and send to target
    TransferWrapped {
        to: T::AccountId,
        value: EgldBalance<T>
    }
}

#[derive(Encode, Decode, RuntimeDebug, Clone, PartialEq)]
pub struct ActionInfo<T: Config> {
    action: LocalAction<T>,
    /// O(1) contains, removal is more desirable but we dont have a choice!
    pub(crate) validators: BTreeSet<T::AccountId>
}

impl<T: Config> ActionInfo<T> {
    pub fn new(action: LocalAction<T>) -> Self {
        Self {
            action,
            validators: BTreeSet::new(),
        }
    }
}
