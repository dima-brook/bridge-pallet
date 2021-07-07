#![cfg_attr(not(feature = "std"), no_std)]

pub mod weights;
mod erc1155_raw;
mod actions;

pub use pallet::*;
use actions::*;

use codec::Encode;
use sp_std::vec::Vec;
use frame_support::{traits::{Currency, Get}, PalletId};
use sp_runtime::{traits::AccountIdConversion, DispatchError};
use pallet_contracts::{Pallet as Contract, chain_extension::UncheckedFrom};
use pallet_contracts_primitives::ContractExecResult;

pub type TokenId = u128;
pub type ActionId = u128;

pub(crate) type Balance<T> =
	<<T as pallet_contracts::Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

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
	pub trait Config: frame_system::Config + pallet_contracts::Config {
        type PalletId: Get<PalletId>;
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;

        type FreezerWeightInfo: WeightInfo;
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

    /// Address of the erc1155 contract
    /// initialized at genesis
    #[pallet::storage]
    #[pallet::getter(fn erc1155_addr)]
    pub type Erc1155<T: Config> = StorageValue<_, T::AccountId>;

    /// Token identifier of egld
    #[pallet::storage]
    #[pallet::getter(fn egld_identifier)]
    pub type EgldTokenId<T: Config> = StorageValue<_, TokenId>;


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
        DuplicateValidation,
        /// Contract already initialized
        Initialized
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {}

	#[pallet::call]
	impl<T: Config> Pallet<T>
    where
        T::AccountId: UncheckedFrom<T::Hash> + AsRef<[u8]>
    {
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
            let action = Self::action_inc();
            Self::deposit_event(Event::Transfer(action, dest, value));

            Ok(().into())
        }

        #[pallet::weight(10000 + T::DbWeight::get().writes(1) + T::DbWeight::get().reads(2))]
        pub fn withdraw_wrapped(
            origin: OriginFor<T>,
            dest: Vec<u8>,
            #[pallet::compact] value: Balance<T>
        ) -> DispatchResultWithPostInfo {
            let acc = ensure_signed(origin)?;
            // TODO: bech32 decode check
            if value == 0u32.into() {
                return Err(Error::<T>::InvalidValue.into());
            }

            Self::erc1155_burn(acc, Self::egld_identifier().unwrap(), value).result?;
            let action = Self::action_inc();
            Self::deposit_event(Event::UnfreezeWrapped(action, dest, value));

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
            let action = <LastActionId<T>>::get().unwrap();
            Self::deposit_event(Event::ScCall(action, dest, endpoint, args));
            <LastActionId<T>>::put(action+1);

            Ok(().into())
        }

        // TODO: Proper weight
        #[pallet::weight(10000 + T::FreezerWeightInfo::verify_action())]
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

        #[pallet::weight(10000 + T::FreezerWeightInfo::verify_action())]
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

        #[pallet::weight(10000 + T::FreezerWeightInfo::verify_action())]
        pub fn transfer_wrapped_verify(
            validator: OriginFor<T>,
            action_id: Vec<u8>,
            to: T::AccountId,
            value: Balance<T>
        ) -> DispatchResultWithPostInfo {
            let acc = ensure_signed(validator)?;

            if Self::verify_action(acc.clone(), action_id, LocalAction::<T>::TransferWrapped {
                to,
                value
            })? {
                Self::erc1155_mint(acc, Self::egld_identifier().unwrap(), value).result?;
            }

            Ok(().into())
        }
	}

    /// Genesis config
    /// initial_validators should be a list of initial validators that are trusted
    /// You shouldn't use the default GenesisConfig!
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
    impl<T: Config> GenesisBuild<T> for GenesisConfig<T>
    where
        T::AccountId: UncheckedFrom<T::Hash> + AsRef<[u8]>
    {
        fn build(&self) {
            <LastActionId<T>>::put(0);
            for validator in &self.initial_validators {
                <Validators<T>>::insert(validator.clone(), ());
            }
            <ValidatorCnt<T>>::put(self.initial_validators.len() as u64);
        
            let erc1155_addr = Pallet::<T>::init_erc1155(
                erc1155_raw::CONTRACT_BYTES.to_vec()
            );
            <Erc1155<T>>::put(erc1155_addr);

            let egld_tokenid = Pallet::<T>::erc1155_create()
                .expect("Failed to create egld token!");
            <EgldTokenId<T>>::put(egld_tokenid);
        }
    }
}

impl<T: Config> Pallet<T>
where
    T::AccountId: UncheckedFrom<T::Hash> + AsRef<[u8]>
{
    fn account_id() -> T::AccountId {
        T::PalletId::get().into_account()
    }

    fn erc1155_address() -> T::AccountId {
        <Erc1155<T>>::get().unwrap()
    }

    fn erc1155_mint(account: T::AccountId, token: TokenId, value: Balance<T>) -> ContractExecResult {
        let encoded_acc = account.encode();
        let encoded_token = token.encode();
        let encoded_value = value.encode();
        let raw_data = [&erc1155_raw::MINT_SELECTOR[..], &encoded_acc, &encoded_token, &encoded_value].concat();

        Contract::<T>::bare_call(
            Self::account_id(),
            Self::erc1155_address(),
            0u32.into(),
            10E10 as u64, // TODO: Tweak
            raw_data,
            false
        )
    }

    fn erc1155_burn(account: T::AccountId, token: TokenId, value: Balance<T>) -> ContractExecResult {
        let encoded_acc = account.encode();
        let encoded_token = token.encode();
        let encoded_value = value.encode();
        let raw_data = [&erc1155_raw::BURN_SELECTOR[..], &encoded_acc, &encoded_token, &encoded_value].concat();

        Contract::<T>::bare_call(
            Self::account_id(),
            Self::erc1155_address(),
            0u32.into(),
            10E10 as u64, // TODO: Tweak
            raw_data,
            false
        )
    }

    #[cfg(feature = "std")]
    fn erc1155_create() -> Result<TokenId, DispatchError> {
        let encoded_value = Balance::<T>::from(0u32).encode();
        let raw_data = [&erc1155_raw::CREATE_SELECTOR[..], &encoded_value].concat();

        Contract::<T>::bare_call(
            Self::account_id(),
            Self::erc1155_address(),
            0u32.into(),
            10E10 as u64, // TODO: Tweak
            raw_data,
            false
        ).result.map(|_| 0u128) // TODO: Decode result and read from there inste
    }

    #[cfg(feature = "std")]
    fn init_erc1155(raw_code: Vec<u8>) -> T::AccountId {
        use pallet_contracts_primitives::Code;

        Contract::<T>::bare_instantiate(
            Self::account_id(),
            (10E16 as u32).into(),
            10E10 as u64, // TODO: Tweak
            Code::Upload(raw_code.into()),
            Vec::new(),
            Vec::new(), // TODO: proper salt
            false,
            false
        ).result.expect("Failed to initialize contract?!").account_id
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

    fn action_inc() -> ActionId {
        let action = <LastActionId<T>>::get().unwrap();
        <LastActionId<T>>::put(action+1);

        return action;
    }
}
