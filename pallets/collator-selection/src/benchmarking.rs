// Copyright 2020-2021 Manta Network.
// This file is part of Manta.
//
// Manta is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Manta is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with Manta.  If not, see <http://www.gnu.org/licenses/>.

//! Benchmarking setup for manta-collator-selection

use super::*;

#[allow(unused)]
use crate::Pallet as CollatorSelection;
use frame_benchmarking::{account, benchmarks, impl_benchmark_test_suite, whitelisted_caller};
use frame_support::{
	assert_ok,
	codec::Decode,
	traits::{Currency, EnsureOrigin, Get},
};
use frame_system::{EventRecord, RawOrigin};
use pallet_authorship::EventHandler;
use pallet_session::{self as session, SessionManager};
use sp_std::prelude::*;

pub type BalanceOf<T> =
	<<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

const SEED: u32 = 0;

// TODO: remove if this is given in substrate commit.
macro_rules! whitelist {
	($acc:ident) => {
		frame_benchmarking::benchmarking::add_to_whitelist(
			frame_system::Account::<T>::hashed_key_for(&$acc).into(),
		);
	};
}

fn assert_last_event<T: Config>(generic_event: <T as Config>::Event) {
	let events = frame_system::Pallet::<T>::events();
	let system_event: <T as frame_system::Config>::Event = generic_event.into();
	// compare to the last event record
	let EventRecord { event, .. } = &events[events.len() - 1];
	assert_eq!(event, &system_event);
}

fn create_funded_user<T: Config>(
	string: &'static str,
	n: u32,
	balance_factor: u32,
) -> T::AccountId {
	let user = account(string, n, SEED);
	let balance = T::Currency::minimum_balance() * balance_factor.into();
	let _ = T::Currency::make_free_balance_be(&user, balance);
	user
}

fn keys<T: Config + session::Config>(c: u32) -> <T as session::Config>::Keys {
	use rand::{RngCore, SeedableRng};

	let keys = {
		let mut keys = [0u8; 128];

		if c > 0 {
			let mut rng = rand::rngs::StdRng::seed_from_u64(c as u64);
			rng.fill_bytes(&mut keys);
		}

		keys
	};

	Decode::decode(&mut &keys[..]).unwrap()
}

fn validator<T: Config + session::Config>(c: u32) -> (T::AccountId, <T as session::Config>::Keys) {
	(create_funded_user::<T>("candidate", c, 1000), keys::<T>(c))
}

fn register_validators<T: Config + session::Config>(count: u32) {
	let validators = (0..count).map(|c| validator::<T>(c)).collect::<Vec<_>>();

	for (who, keys) in validators {
		<session::Pallet<T>>::set_keys(RawOrigin::Signed(who).into(), keys, Vec::new()).unwrap();
	}
}

fn register_candidates<T: Config>(count: u32) {
	let candidates = (0..count)
		.map(|c| account("candidate", c, SEED))
		.collect::<Vec<_>>();
	assert!(
		<CandidacyBond<T>>::get() > 0u32.into(),
		"Bond cannot be zero!"
	);

	for who in candidates {
		T::Currency::make_free_balance_be(&who, <CandidacyBond<T>>::get() * 2u32.into());
		<CollatorSelection<T>>::register_as_candidate(RawOrigin::Signed(who).into()).unwrap();
	}
}

benchmarks! {
	where_clause { where T: pallet_authorship::Config + session::Config }

	set_invulnerables {
		let b in 1 .. T::MaxInvulnerables::get();
		let new_invulnerables = (0..b).map(|c| account("candidate", c, SEED)).collect::<Vec<_>>();
		let origin = T::UpdateOrigin::successful_origin();
	}: {
		assert_ok!(
			<CollatorSelection<T>>::set_invulnerables(origin, new_invulnerables.clone())
		);
	}
	verify {
		assert_last_event::<T>(Event::NewInvulnerables(new_invulnerables).into());
	}

	set_desired_candidates {
		let max: u32 = 999;
		let origin = T::UpdateOrigin::successful_origin();
	}: {
		assert_ok!(
			<CollatorSelection<T>>::set_desired_candidates(origin, max.clone())
		);
	}
	verify {
		assert_last_event::<T>(Event::NewDesiredCandidates(max).into());
	}

	set_candidacy_bond {
		let bond: BalanceOf<T> = T::Currency::minimum_balance() * 10u32.into();
		let origin = T::UpdateOrigin::successful_origin();
	}: {
		assert_ok!(
			<CollatorSelection<T>>::set_candidacy_bond(origin, bond.clone())
		);
	}
	verify {
		assert_last_event::<T>(Event::NewCandidacyBond(bond).into());
	}

	// worse case is when we have all the max-candidate slots filled except one, and we fill that
	// one.
	register_as_candidate {
		let c in 1 .. T::MaxCandidates::get();

		<CandidacyBond<T>>::put(T::Currency::minimum_balance());
		<DesiredCandidates<T>>::put(c + 1);

		register_validators::<T>(c);
		register_candidates::<T>(c);

		let caller: T::AccountId = whitelisted_caller();
		let bond: BalanceOf<T> = T::Currency::minimum_balance() * 2u32.into();
		T::Currency::make_free_balance_be(&caller, bond.clone());

		<session::Pallet<T>>::set_keys(
			RawOrigin::Signed(caller.clone()).into(),
			keys::<T>(c + 1),
			Vec::new()
		).unwrap();

	}: _(RawOrigin::Signed(caller.clone()))
	verify {
		assert_last_event::<T>(Event::CandidateAdded(caller, bond / 2u32.into()).into());
	}

	// worse case is the last candidate leaving.
	leave_intent {
		let c in 1 .. T::MaxCandidates::get();
		<CandidacyBond<T>>::put(T::Currency::minimum_balance());
		<DesiredCandidates<T>>::put(c);

		register_validators::<T>(c);
		register_candidates::<T>(c);

		let leaving = <Candidates<T>>::get().last().unwrap().who.clone();
		whitelist!(leaving);
	}: _(RawOrigin::Signed(leaving.clone()))
	verify {
		assert_last_event::<T>(Event::CandidateRemoved(leaving).into());
	}

	// worse case is the last candidate leaving.
	remove_collator {
		let c in 1 .. T::MaxCandidates::get();
		<CandidacyBond<T>>::put(T::Currency::minimum_balance());
		<DesiredCandidates<T>>::put(c);

		register_validators::<T>(c);
		register_candidates::<T>(c);

		let leaving = <Candidates<T>>::get().last().unwrap().who.clone();
		whitelist!(leaving);
		let origin = T::UpdateOrigin::successful_origin();
	}: {
		assert_ok!(
			<CollatorSelection<T>>::remove_collator(origin, leaving.clone())
		);
	}
	verify {
		assert_last_event::<T>(Event::CandidateRemoved(leaving).into());
	}

	// worse case is when we have all the max-candidate slots filled except one, and we fill that
	// one.
	register_candidate {
		let c in 1 .. T::MaxCandidates::get();

		<CandidacyBond<T>>::put(T::Currency::minimum_balance());
		<DesiredCandidates<T>>::put(c + 1);

		register_validators::<T>(c);
		register_candidates::<T>(c);

		let caller: T::AccountId = whitelisted_caller();
		let bond: BalanceOf<T> = T::Currency::minimum_balance() * 2u32.into();
		T::Currency::make_free_balance_be(&caller, bond.clone());

		<session::Pallet<T>>::set_keys(
			RawOrigin::Signed(caller.clone()).into(),
			keys::<T>(c + 1),
			Vec::new()
		).unwrap();

		let origin = T::UpdateOrigin::successful_origin();
	}: {
		assert_ok!(
			<CollatorSelection<T>>::register_candidate(origin, caller.clone())
		);
	}
	verify {
		assert_last_event::<T>(Event::CandidateAdded(caller, bond / 2u32.into()).into());
	}

	// worse case is paying a non-existing candidate account.
	note_author {
		<CandidacyBond<T>>::put(T::Currency::minimum_balance());
		T::Currency::make_free_balance_be(
			&<CollatorSelection<T>>::account_id(),
			T::Currency::minimum_balance() * 4u32.into(),
		);
		let author = account("author", 0, SEED);
		let new_block: T::BlockNumber = 10u32.into();

		frame_system::Pallet::<T>::set_block_number(new_block);
		assert!(T::Currency::free_balance(&author) == 0u32.into());
	}: {
		<CollatorSelection<T> as EventHandler<_, _>>::note_author(author.clone())
	} verify {
		assert!(T::Currency::free_balance(&author) > 0u32.into());
		assert_eq!(frame_system::Pallet::<T>::block_number(), new_block);
	}

	// worst case for new session.
	new_session {
		let r in 1 .. T::MaxCandidates::get();
		let c in 1 .. T::MaxCandidates::get();

		<CandidacyBond<T>>::put(T::Currency::minimum_balance());
		<DesiredCandidates<T>>::put(c);
		frame_system::Pallet::<T>::set_block_number(0u32.into());

		register_validators::<T>(c);
		register_candidates::<T>(c);

		let new_block: T::BlockNumber = 1800u32.into();
		let zero_block: T::BlockNumber = 0u32.into();
		let candidates = <Candidates<T>>::get();

		let non_removals = c.saturating_sub(r);

		for i in 0..c {
			<LastAuthoredBlock<T>>::insert(candidates[i as usize].who.clone(), zero_block);
		}

		if non_removals > 0 {
			for i in 0..non_removals {
				<LastAuthoredBlock<T>>::insert(candidates[i as usize].who.clone(), new_block);
			}
		} else {
			for i in 0..c {
				<LastAuthoredBlock<T>>::insert(candidates[i as usize].who.clone(), new_block);
			}
		}

		let pre_length = <Candidates<T>>::get().len();

		frame_system::Pallet::<T>::set_block_number(new_block);

		assert!(<Candidates<T>>::get().len() == c as usize);
	}: {
		<CollatorSelection<T> as SessionManager<_>>::new_session(0)
	} verify {
		if c > r {
			assert!(<Candidates<T>>::get().len() < pre_length);
		} else {
			assert!(<Candidates<T>>::get().len() == pre_length);
		}
	}
}

impl_benchmark_test_suite!(
	CollatorSelection,
	crate::mock::new_test_ext(),
	crate::mock::Test,
);
