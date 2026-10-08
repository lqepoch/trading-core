#![forbid(unsafe_code)]

use domain::{ExactDecimal, IntentId, InventoryQuantity, MAX_SAFE_INTEGER, Revision};
use proptest::{
    prelude::*,
    test_runner::{Config, RngAlgorithm, RngSeed, TestRunner},
};
use std::{
    collections::HashSet,
    hash::{DefaultHasher, Hash, Hasher},
};

fn runner(seed: u64) -> TestRunner {
    TestRunner::new(Config {
        cases: 96,
        max_local_rejects: 128,
        max_global_rejects: 128,
        max_flat_map_regens: 128,
        failure_persistence: None,
        max_shrink_time: 0,
        max_shrink_iters: 1_024,
        max_default_size_range: 32,
        rng_algorithm: RngAlgorithm::ChaCha,
        rng_seed: RngSeed::Fixed(seed),
        verbose: 0,
        ..Config::default()
    })
}

#[test]
fn exact_decimal_display_roundtrips_normalized_values_and_ordering() {
    let strategy = (any::<i64>(), 0_u32..=12, any::<i64>(), 0_u32..=12);
    runner(0x2160_0001)
        .run(
            &strategy,
            |(left_coefficient, left_scale, right_coefficient, right_scale)| {
                let left = ExactDecimal::from_parts(i128::from(left_coefficient), left_scale)
                    .expect("bounded coefficient and scale are representable");
                let right = ExactDecimal::from_parts(i128::from(right_coefficient), right_scale)
                    .expect("bounded coefficient and scale are representable");
                let left_roundtrip = ExactDecimal::parse_json_number(&left.to_string())
                    .expect("canonical decimal display is strict JSON numeric input");
                let right_roundtrip = ExactDecimal::parse_json_number(&right.to_string())
                    .expect("canonical decimal display is strict JSON numeric input");

                prop_assert_eq!(left_roundtrip, left);
                prop_assert_eq!(right_roundtrip, right);
                let common_scale = left.scale().max(right.scale());
                let expected_ordering = left
                    .to_scaled_integer(common_scale)
                    .expect("bounded test values fit i128")
                    .cmp(
                        &right
                            .to_scaled_integer(common_scale)
                            .expect("bounded test values fit i128"),
                    );
                prop_assert_eq!(left.cmp(&right), expected_ordering);
                prop_assert_eq!(left.partial_cmp(&right), Some(left.cmp(&right)));
                Ok(())
            },
        )
        .expect("exact decimal properties hold");
}

#[test]
fn intent_id_value_identity_is_stable_for_clones_and_hash_keys() {
    let strategy = any::<u64>();
    runner(0x2160_0002)
        .run(&strategy, |suffix| {
            let text = format!("property-intent-{suffix}");
            let first = IntentId::new(text.clone()).expect("generated identity is bounded");
            let second = IntentId::new(text.clone()).expect("same identity is valid");
            let clone = first.clone();
            let mut first_hasher = DefaultHasher::new();
            first.hash(&mut first_hasher);
            let mut second_hasher = DefaultHasher::new();
            second.hash(&mut second_hasher);
            let mut ids = HashSet::new();

            prop_assert_eq!(first.as_str(), text);
            prop_assert_eq!(&first, &second);
            prop_assert_eq!(&first, &clone);
            prop_assert_eq!(first_hasher.finish(), second_hasher.finish());
            prop_assert!(ids.insert(first.clone()));
            prop_assert!(!ids.insert(second));
            Ok(())
        })
        .expect("intent identity properties hold");
}

#[test]
fn revision_ordering_and_inventory_arithmetic_respect_bounds() {
    let revisions = (any::<u64>(), any::<u64>());
    runner(0x2160_0003)
        .run(&revisions, |(left, right)| {
            let revision = Revision::new(left);
            prop_assert_eq!(revision.cmp(&Revision::new(right)), left.cmp(&right));
            match revision.checked_next() {
                Some(next) => {
                    prop_assert_eq!(next.get(), left + 1);
                    prop_assert!(next > revision);
                }
                None => prop_assert_eq!(left, u64::MAX),
            }
            Ok(())
        })
        .expect("revision ordering properties hold");

    let inventory = (0_u64..=MAX_SAFE_INTEGER, 0_u64..=MAX_SAFE_INTEGER);
    runner(0x2160_0004)
        .run(&inventory, |(left, right)| {
            let left_value = InventoryQuantity::new(left).expect("strategy is in range");
            let right_value = InventoryQuantity::new(right).expect("strategy is in range");
            let sum = u128::from(left) + u128::from(right);
            let added = left_value
                .checked_add(right_value)
                .ok()
                .map(InventoryQuantity::get);
            let expected = (sum <= u128::from(MAX_SAFE_INTEGER)).then_some(sum as u64);
            prop_assert_eq!(added, expected);

            let difference = left_value
                .checked_sub(right_value)
                .ok()
                .map(InventoryQuantity::get);
            prop_assert_eq!(difference, left.checked_sub(right));
            prop_assert_eq!(
                left_value
                    .as_order_quantity()
                    .ok()
                    .map(|quantity| quantity.get()),
                (left > 0).then_some(left)
            );
            Ok(())
        })
        .expect("bounded inventory value arithmetic properties hold");
}
