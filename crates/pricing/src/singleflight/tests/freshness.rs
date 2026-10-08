use super::*;

#[tokio::test]
async fn cache_hit_and_resolve_recheck_underlying_freshness_without_renewal() {
    let pool = crate::GreekSolverPool::new(1, 1).expect("pool");
    let generation = PricingGeneration::new(1);
    let input = input();
    let key = key(&input, generation, 1, 1, NOW_MS);
    let fence = PricingInputFence::new(generation);
    let owner = GreekSolverSingleflight::new(&pool, 2, generation).expect("singleflight");

    let handle = owner
        .try_submit(request(input.clone(), key.clone(), fence.clone()), NOW_MS)
        .expect("fresh request admitted");
    assert_eq!(
        handle.resolve(11_001).await,
        GreekPricingOutcome::Unavailable {
            key: key.clone(),
            reason: ModelUnavailable::StaleInput,
        }
    );
    assert!(matches!(
        owner.try_submit(request(input, key, fence), 11_001),
        Err(GreekSingleflightError::StaleInput)
    ));
    assert_eq!(pool.shutdown().await, Ok(()));
}

#[tokio::test]
async fn cache_hit_and_resolve_recheck_option_freshness_without_renewal() {
    let pool = crate::GreekSolverPool::new(1, 1).expect("pool");
    let generation = PricingGeneration::new(1);
    let input = input_with_max_age(1_000);
    let contract = standard_contract();
    let input_quotes = quote_inputs(
        &contract,
        input.option_price(),
        input.spot(),
        PricingQuoteProvenance::new(
            PricingSourceId::new(11),
            generation,
            QuoteRevision::new(1),
            9_900,
            NOW_MS,
            1_000,
            PricingQuoteQuality::Realtime,
        ),
        PricingQuoteProvenance::new(
            PricingSourceId::new(22),
            generation,
            QuoteRevision::new(1),
            9_900,
            NOW_MS,
            3_000,
            PricingQuoteQuality::Realtime,
        ),
    );
    let key = PricingInputKey::new(
        contract,
        generation,
        input_quotes.0,
        input_quotes.1,
        NOW_MS,
        &input,
    )
    .expect("complete key");
    let fence = PricingInputFence::new(generation);
    let owner = GreekSolverSingleflight::new(&pool, 2, generation).expect("singleflight");
    let handle = owner
        .try_submit(request(input.clone(), key.clone(), fence.clone()), NOW_MS)
        .expect("fresh request admitted");
    assert_eq!(
        handle.resolve(11_001).await,
        GreekPricingOutcome::Unavailable {
            key: key.clone(),
            reason: ModelUnavailable::StaleInput,
        }
    );
    assert!(matches!(
        owner.try_submit(request(input, key, fence), 11_001),
        Err(GreekSingleflightError::StaleInput)
    ));
    assert_eq!(pool.shutdown().await, Ok(()));
}

#[tokio::test]
async fn resolve_rechecks_expiration_and_evicts_expired_completion() {
    let pool = crate::GreekSolverPool::new(1, 1).expect("pool");
    let generation = PricingGeneration::new(1);
    let input = input_with_expiry_and_max_age(1_000, 3_000, 9_900);
    let key = key_with_provenance(
        standard_contract(),
        &input,
        generation,
        PricingQuoteProvenance::new(
            PricingSourceId::new(11),
            generation,
            QuoteRevision::new(1),
            9_900,
            NOW_MS,
            3_000,
            PricingQuoteQuality::Realtime,
        ),
        PricingQuoteProvenance::new(
            PricingSourceId::new(22),
            generation,
            QuoteRevision::new(1),
            9_900,
            NOW_MS,
            3_000,
            PricingQuoteQuality::Realtime,
        ),
        NOW_MS,
    )
    .expect("complete expiry key");
    let fence = PricingInputFence::new(generation);
    let owner = GreekSolverSingleflight::new(&pool, 2, generation).expect("singleflight");
    let handle = owner
        .try_submit(request(input, key.clone(), fence), NOW_MS)
        .expect("fresh before expiry");

    assert_eq!(
        handle.resolve(NOW_MS + 1_001).await,
        GreekPricingOutcome::Unavailable {
            key,
            reason: ModelUnavailable::Expired,
        }
    );
    assert_eq!(owner.occupancy_counts(), (0, 0));
    assert_eq!(pool.shutdown().await, Ok(()));
}

#[tokio::test]
async fn delayed_completion_after_expiry_is_not_a_current_cache_hit() {
    let calls = Arc::new(AtomicUsize::new(0));
    let gate = gate();
    let pool = crate::GreekSolverPool::new_with_solver(
        1,
        1,
        blocking_solver(Arc::clone(&gate), Arc::clone(&calls)),
    )
    .expect("pool");
    let generation = PricingGeneration::new(1);
    let input = input_with_expiry_and_max_age(1, 3_000, 9_900);
    let key = key(&input, generation, 1, 1, NOW_MS);
    let fence = PricingInputFence::new(generation);
    let owner = GreekSolverSingleflight::new(&pool, 2, generation).expect("singleflight");
    let handle = owner
        .try_submit(request(input.clone(), key.clone(), fence.clone()), NOW_MS)
        .expect("fresh request admitted");
    wait_started(&gate, 1);
    release(&gate);

    tokio::time::timeout(Duration::from_secs(2), async {
        while owner.occupancy_counts() != (0, 1) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("worker completed and published its bounded entry");
    assert!(matches!(
        owner.try_submit(request(input, key.clone(), fence), NOW_MS + 2),
        Err(GreekSingleflightError::StaleInput)
    ));
    assert_eq!(owner.cache_hit_count(), 0);
    assert_eq!(owner.occupancy_counts(), (0, 1));
    assert_eq!(
        handle.resolve(NOW_MS + 2).await,
        GreekPricingOutcome::Unavailable {
            key,
            reason: ModelUnavailable::Expired,
        }
    );
    assert_eq!(owner.occupancy_counts(), (0, 0));
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(pool.shutdown().await, Ok(()));
}

#[tokio::test]
async fn resolve_rechecks_dividend_age_independently_of_quote_freshness() {
    let pool = crate::GreekSolverPool::new(1, 1).expect("pool");
    let generation = PricingGeneration::new(1);
    let input = input_with_expiry_and_max_age(182 * 86_400_000, 60_000, 0);
    let key = key_with_provenance(
        standard_contract(),
        &input,
        generation,
        PricingQuoteProvenance::new(
            PricingSourceId::new(11),
            generation,
            QuoteRevision::new(1),
            9_900,
            NOW_MS,
            60_000,
            PricingQuoteQuality::Realtime,
        ),
        PricingQuoteProvenance::new(
            PricingSourceId::new(22),
            generation,
            QuoteRevision::new(1),
            9_900,
            NOW_MS,
            60_000,
            PricingQuoteQuality::Realtime,
        ),
        NOW_MS,
    )
    .expect("complete freshness key");
    let fence = PricingInputFence::new(generation);
    let owner = GreekSolverSingleflight::new(&pool, 2, generation).expect("singleflight");
    let handle = owner
        .try_submit(request(input, key.clone(), fence), NOW_MS)
        .expect("quotes and evidence fresh at admission");

    assert_eq!(
        handle.resolve(60_001).await,
        GreekPricingOutcome::Unavailable {
            key,
            reason: ModelUnavailable::DividendEvidenceStale,
        }
    );
    assert_eq!(owner.occupancy_counts(), (0, 0));
    assert_eq!(pool.shutdown().await, Ok(()));
}

#[tokio::test]
async fn delayed_completion_after_dividend_evidence_ages_out_is_not_a_current_cache_hit() {
    let calls = Arc::new(AtomicUsize::new(0));
    let gate = gate();
    let pool = crate::GreekSolverPool::new_with_solver(
        1,
        1,
        blocking_solver(Arc::clone(&gate), Arc::clone(&calls)),
    )
    .expect("pool");
    let generation = PricingGeneration::new(1);
    let input = input_with_expiry_and_max_age(182 * 86_400_000, 3_000, 7_000);
    let key = key(&input, generation, 1, 1, NOW_MS);
    let fence = PricingInputFence::new(generation);
    let owner = GreekSolverSingleflight::new(&pool, 2, generation).expect("singleflight");
    let handle = owner
        .try_submit(request(input.clone(), key.clone(), fence.clone()), NOW_MS)
        .expect("fresh request admitted at evidence age boundary");
    wait_started(&gate, 1);
    release(&gate);

    tokio::time::timeout(Duration::from_secs(2), async {
        while owner.occupancy_counts() != (0, 1) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("worker completed and published its bounded entry");
    assert!(matches!(
        owner.try_submit(request(input, key.clone(), fence), NOW_MS + 1),
        Err(GreekSingleflightError::StaleInput)
    ));
    assert_eq!(owner.cache_hit_count(), 0);
    assert_eq!(owner.occupancy_counts(), (0, 1));
    assert_eq!(
        handle.resolve(NOW_MS + 1).await,
        GreekPricingOutcome::Unavailable {
            key,
            reason: ModelUnavailable::DividendEvidenceStale,
        }
    );
    assert_eq!(owner.occupancy_counts(), (0, 0));
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(pool.shutdown().await, Ok(()));
}

#[tokio::test]
async fn exact_expiry_returns_typed_intrinsic_only_outcome() {
    let pool = crate::GreekSolverPool::new(1, 1).expect("pool");
    let generation = PricingGeneration::new(1);
    let input = exact_expiry_input();
    let key = key(&input, generation, 1, 1, NOW_MS);
    let fence = PricingInputFence::new(generation);
    let owner = GreekSolverSingleflight::new(&pool, 2, generation).expect("singleflight");
    let handle = owner
        .try_submit(request(input, key.clone(), fence), NOW_MS)
        .expect("exact-expiry job admitted");

    assert_eq!(
        handle.resolve(NOW_MS).await,
        GreekPricingOutcome::AtExpiryIntrinsic {
            key,
            value: Price::parse_json_number("0").expect("zero intrinsic"),
        }
    );
    assert_eq!(pool.shutdown().await, Ok(()));
}

#[tokio::test]
async fn available_result_keeps_local_solver_source_and_units() {
    let pool = crate::GreekSolverPool::new(1, 1).expect("pool");
    let generation = PricingGeneration::new(1);
    let input = input();
    let key = key(&input, generation, 1, 1, NOW_MS);
    let fence = PricingInputFence::new(generation);
    let owner = GreekSolverSingleflight::new(&pool, 2, generation).expect("singleflight");
    let handle = owner
        .try_submit(request(input, key, fence), NOW_MS)
        .expect("local pricing job admitted");
    let GreekPricingOutcome::Available { metrics, .. } = handle.resolve(NOW_MS).await else {
        panic!("expected a convergent local model result");
    };
    let iv = metrics.implied_volatility().expect("IV present");
    assert_eq!(iv.source(), crate::MetricSource::BlackScholesEuropeanV2);
    assert_eq!(iv.unit(), crate::MetricUnit::ImpliedVolatilityFraction);
    assert_eq!(
        metrics.delta().expect("delta").unit(),
        crate::MetricUnit::DeltaPerUnderlyingUnit
    );
    assert_eq!(pool.shutdown().await, Ok(()));
}

#[tokio::test]
async fn bounded_cache_evicts_lru_completed_results_and_recomputes() {
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_for_solver = Arc::clone(&calls);
    let solver = Arc::new(move |_: &SolverInput| {
        calls_for_solver.fetch_add(1, Ordering::Relaxed);
        SolverOutcome::Unavailable(ModelUnavailable::NonConvergent)
    });
    let solver: Arc<dyn Fn(&SolverInput) -> SolverOutcome + Send + Sync> = solver;
    let pool = crate::GreekSolverPool::new_with_solver(1, 1, solver).expect("pool");
    let generation = PricingGeneration::new(1);
    let fence = PricingInputFence::new(generation);
    let owner = GreekSolverSingleflight::new(&pool, 1, generation).expect("singleflight");

    let first_input = input();
    let first_key = key(&first_input, generation, 1, 1, NOW_MS);
    owner
        .try_submit(
            request(first_input.clone(), first_key.clone(), fence.clone()),
            NOW_MS,
        )
        .expect("first job admitted")
        .resolve(NOW_MS)
        .await;

    let second_input = input_with_premium("6.13");
    let second_key = key(&second_input, generation, 2, 1, NOW_MS);
    owner
        .try_submit(request(second_input, second_key, fence.clone()), NOW_MS)
        .expect("second key evicts completed first key")
        .resolve(NOW_MS)
        .await;

    owner
        .try_submit(request(first_input, first_key, fence), NOW_MS)
        .expect("evicted key is recomputed")
        .resolve(NOW_MS)
        .await;
    assert_eq!(owner.entry_counts(), (0, 1));
    assert_eq!(calls.load(Ordering::Relaxed), 3);
    assert_eq!(owner.capacity(), 1);
    assert_eq!(pool.shutdown().await, Ok(()));
}

#[tokio::test]
async fn saturated_inflight_capacity_rejects_without_growing_state() {
    let calls = Arc::new(AtomicUsize::new(0));
    let gate = gate();
    let pool = crate::GreekSolverPool::new_with_solver(
        1,
        1,
        blocking_solver(Arc::clone(&gate), Arc::clone(&calls)),
    )
    .expect("pool");
    let generation = PricingGeneration::new(1);
    let fence = PricingInputFence::new(generation);
    let owner = GreekSolverSingleflight::new(&pool, 1, generation).expect("singleflight");
    let first_input = input();
    let first_key = key(&first_input, generation, 1, 1, NOW_MS);
    let first = owner
        .try_submit(request(first_input, first_key, fence.clone()), NOW_MS)
        .expect("first in-flight key admitted");
    wait_started(&gate, 1);

    let second_input = input_with_premium("6.13");
    let second_key = key(&second_input, generation, 2, 1, NOW_MS);
    assert!(matches!(
        owner.try_submit(request(second_input, second_key, fence), NOW_MS),
        Err(GreekSingleflightError::CacheSaturated)
    ));
    assert_eq!(owner.saturation_count(), 1);
    assert_eq!(owner.admitted_flight_count(), 1);
    assert_eq!(owner.occupancy_counts(), (1, 0));
    assert_eq!(owner.entry_counts(), (1, 0));
    release(&gate);
    assert!(matches!(
        first.resolve(NOW_MS).await,
        GreekPricingOutcome::Stale { .. }
    ));
    assert_eq!(owner.entry_counts(), (0, 0));
    assert_eq!(pool.shutdown().await, Ok(()));
}

#[tokio::test]
async fn worker_queue_saturation_does_not_insert_unadmitted_flight() {
    let calls = Arc::new(AtomicUsize::new(0));
    let gate = gate();
    let pool = crate::GreekSolverPool::new_with_solver(
        1,
        1,
        blocking_solver(Arc::clone(&gate), Arc::clone(&calls)),
    )
    .expect("pool");
    let generation = PricingGeneration::new(1);
    let fence = PricingInputFence::new(generation);
    let owner = GreekSolverSingleflight::new(&pool, 3, generation).expect("singleflight");
    let first_input = input();
    let first_key = key(&first_input, generation, 1, 1, NOW_MS);
    let first = owner
        .try_submit(request(first_input, first_key, fence.clone()), NOW_MS)
        .expect("first job admitted");
    wait_started(&gate, 1);

    let second_input = input_with_premium("6.13");
    let second_key = key(&second_input, generation, 2, 1, NOW_MS);
    let second = owner
        .try_submit(request(second_input, second_key, fence.clone()), NOW_MS)
        .expect("one job queued");
    let third_input = input_with_premium("6.14");
    let third_key = key(&third_input, generation, 3, 1, NOW_MS);
    assert!(matches!(
        owner.try_submit(request(third_input, third_key, fence), NOW_MS),
        Err(GreekSingleflightError::Pool(PoolError::Saturated))
    ));
    assert_eq!(owner.saturation_count(), 1);
    assert_eq!(owner.admitted_flight_count(), 2);
    assert_eq!(owner.occupancy_counts(), (2, 0));
    assert_eq!(owner.entry_counts(), (2, 0));
    release(&gate);
    assert!(matches!(
        first.resolve(NOW_MS).await,
        GreekPricingOutcome::Stale { .. }
    ));
    assert!(matches!(
        second.resolve(NOW_MS).await,
        GreekPricingOutcome::Stale { .. }
    ));
    assert_eq!(owner.entry_counts(), (0, 0));
    assert_eq!(pool.shutdown().await, Ok(()));
}
