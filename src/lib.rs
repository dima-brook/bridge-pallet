#![cfg_attr(not(feature = "std"), no_std)]

pub mod weights;
mod actions;

pub use pallet::*;
use actions::*;

use sp_std::vec::Vec;
use frame_support::traits::Currency;


pub type ActionId = u128;
pub(crate) type Balance<T> =
	<<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

#[frame_support::pallet]
pub mod pallet {
    use super::*;
	use frame_support::{
        pallet_prelude::*,
        traits::{WithdrawReasons, ExistenceRequirement}
    };
    use sp_runtime::traits::StaticLookup;
    use sp_std::vec::Vec;
	use frame_system::pallet_prelude::*;
    use weights::WeightInfo;

	#[pallet::config]
	pub trait Config: frame_system::Config {
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;

        type Currency: Currency<Self::AccountId>;

        type WeightInfo: WeightInfo;
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

    /// Set of Validators
    #[pallet::storage]
    pub type Validators<T: Config> = StorageMap<_, Blake2_128Concat, T::AccountId, ()>;

    /// Validator Cnt
    #[pallet::storage]
    pub type ValidatorCnt<T: Config> = StorageValue<_, u64>;

    /// Id of the last emitted action id
    #[pallet::storage]
    pub type LastActionId<T: Config> = StorageValue<_, ActionId>;

    /// Only validators can add new actions
    /// action_id: action_info
    #[pallet::storage]
    pub type Actions<T: Config> = StorageMap<_, Blake2_128Concat, Vec<u8>, ActionInfo<T>>;

	#[pallet::event]
	#[pallet::metadata(T::AccountId = "AccountId")]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
        /// Send currency from local chain to foreign chain
        /// action_id, target address, currency ammount
        Transfer(ActionId, Vec<u8>, Balance<T>),

        /// Call a smart contract on foreign chain
        /// action_id, contract address, call endpoint identifier, raw arguments
        ScCall(ActionId, Vec<u8>, Vec<u8>, Vec<Vec<u8>>),

        /// Send foreign currency back
        /// action_id, target address, currency ammount
        UnfreezeWrapped(ActionId, Vec<u8>, Balance<T>),
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T> {
        /// Invalid Value
        InvalidValue,
        /// Not enough funds
        OutOfFunds,
	    /// Invalid Destination address
        InvalidDestination,
        Unauthorized,
        DuplicateValidation
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
        // TODO: proper weight
        #[pallet::weight(10000 + T::DbWeight::get().writes(1) + T::DbWeight::get().reads(2))]
        pub fn send(
            origin: OriginFor<T>,
            dest: Vec<u8>,
            #[pallet::compact] value: Balance<T> 
        ) -> DispatchResultWithPostInfo {
            let who = ensure_signed(origin)?;
            //bech32::decode(&dest).map_err(|_| Error::<T>::InvalidDestination)?;
            if value == 0u32.into() {
                return Err(Error::<T>::InvalidValue.into());
            }
            if value > T::Currency::free_balance(&who) {
                return Err(Error::<T>::OutOfFunds.into());
            }

            // TODO: Deduct some as txn fees
            T::Currency::withdraw(&who, value, WithdrawReasons::RESERVE, ExistenceRequirement::KeepAlive)?; // TODO: Separate reserve storage for acc
            let action = <LastActionId<T>>::get().expect("Invalid genesis config?!");
            Self::deposit_event(Event::Transfer(action, dest, value));
            <LastActionId<T>>::put(action+1);

            Ok(().into())
        }

        // TODO: proper weight
        #[pallet::weight(10000 + T::DbWeight::get().writes(1) + T::DbWeight::get().reads(1))]
        pub fn send_sc_call(
            origin: OriginFor<T>,
            dest: Vec<u8>,
            endpoint: Vec<u8>,
            args: Vec<Vec<u8>>
        ) -> DispatchResultWithPostInfo {
            ensure_signed(origin)?;

            //bech32::decode(&dest).map_err(|_| Error::<T>::InvalidDestination)?;

            // TODO: Deduct some balance as txn fees
            let action = <LastActionId<T>>::get().expect("Invalid genesis config?!");
            Self::deposit_event(Event::ScCall(action, dest, endpoint, args));
            <LastActionId<T>>::put(action+1);

            Ok(().into())
        }

        // TODO: Proper weight
        #[pallet::weight(10000 + T::WeightInfo::verify_action())]
        pub fn unfreeze_verify(
            validator: OriginFor<T>,
            action_id: Vec<u8>,
            to: T::AccountId,
            #[pallet::compact] value: Balance<T>
        ) -> DispatchResultWithPostInfo {
            let acc = ensure_signed(validator)?;

            if Self::verify_action(acc, action_id, LocalAction::<T>::Unfreeze { to: to.clone(), value })? {
                T::Currency::deposit_creating(&to, value);
            }

            Ok(().into())
        }

        #[pallet::weight(10000 + T::WeightInfo::verify_action())]
        pub fn sc_call_verify(
            validator: OriginFor<T>,
            action_id: Vec<u8>,
            contract: <T::Lookup as StaticLookup>::Source,
            raw_call_data: Vec<u8>
        ) -> DispatchResultWithPostInfo {
            let acc = ensure_signed(validator)?;

            let contract = T::Lookup::lookup(contract)?;
            if Self::verify_action(acc, action_id, LocalAction::<T>::RpcCall {
                contract: contract.clone(),
                call_data: raw_call_data.clone()
            })? {
               // TODO: execute contract
            }

            Ok(().into())
        }

        #[pallet::weight(10000 + T::WeightInfo::verify_action())]
        pub fn transfer_wrapped_verify(
            validator: OriginFor<T>,
            action_id: Vec<u8>,
            to: T::AccountId,
            value: Balance<T>
        ) -> DispatchResultWithPostInfo {
            let acc = ensure_signed(validator)?;

            if Self::verify_action(acc, action_id, LocalAction::<T>::TransferWrapped {
                to,
                value
            })? {
                // TODO: Mint Wrapped token (use balance as an erc20?!)
            }

            Ok(().into())
        }
	}

    /// Genesis config
    /// initial_validators should be a list of initial validators that are trusted
    #[pallet::genesis_config]
    pub struct GenesisConfig<T: Config> {
        pub initial_validators: Vec<T::AccountId>,
    }

    #[cfg(feature = "std")]
    impl<T: Config> Default for GenesisConfig<T> {
        fn default() -> Self {
            Self {
                initial_validators: Vec::new(),
            }
        }
    }

    #[cfg(feature = "std")]
    #[pallet::genesis_build]
    impl<T: Config> GenesisBuild<T> for GenesisConfig<T> {
        fn build(&self) {
            <LastActionId<T>>::put(0);
            for validator in &self.initial_validators {
                <Validators<T>>::insert(validator.clone(), ());
            }
            <ValidatorCnt<T>>::put(self.initial_validators.len() as u64);
        }
    }
}

impl<T: Config> pallet::Pallet<T> {
    /// verify an action,
    /// return true if the action is ready to be executed
    fn verify_action(
        validator: T::AccountId,
        action_id: Vec<u8>,
        action: LocalAction<T>,
    ) -> Result<bool, Error<T>> {
        if !Validators::<T>::contains_key(validator.clone()) {
            return Err(Error::<T>::Unauthorized)
        }

        let mut action = Actions::<T>::try_get(action_id.clone())
            .unwrap_or_else(|_| ActionInfo::<T>::new(action));

        if action.validators.contains(&validator) {
            // TODO: validator misbehaviour
            return Err(Error::<T>::DuplicateValidation);
        }
        // TODO: check if matches current data
        action.validators.insert(validator);

        let mut ret = Ok(false);

        let validator_cnt = ValidatorCnt::<T>::get().expect("invalid genesis?!");
        if action.validators.len() == ((2./3.)*(validator_cnt as f64)) as usize + 1 {
            ret = Ok(true);
        }

        if action.validators.len() == validator_cnt as usize {
            Actions::<T>::remove(action_id);
        }

        return ret;
    }
}
