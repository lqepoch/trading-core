use super::*;

#[tokio::test]
async fn identical_complete_inputs_share_one_solver_job_and_cache_entry() {
    let calls = Arc::new(AtomicUsize::new(0));
    let gate = gate();
    let pool = crate::GreekSolverPool::new_with_solver(
        1,
        2,
        blocking_solver(Arc::clone(&gate), Arc::clone(&calls)),
    )
    .expect("pool");
    let generation = PricingGeneration::new(1);
    let input = input();
    let key = key(&input, generation, 1, 1, NOW_MS);
    let fence = PricingInputFence::new(generation);
    let owner = GreekSolverSingleflight::new(&pool, 2, generation).expect("singleflight");

    let first = owner
        .try_submit(request(input.clone(), key.clone(), fence.clone()), NOW_MS)
        .expect("first request admitted");
    assert_eq!(owner.admitted_flight_count(), 1);
    assert_eq!(owner.in_flight_join_count(), 0);
    assert_eq!(owner.cache_hit_count(), 0);
    assert_eq!(owner.saturation_count(), 0);
    assert_eq!(owner.occupancy_counts(), (1, 0));
    wait_started(&gate, 1);
    let second = owner
        .try_submit(request(input.clone(), key.clone(), fence.clone()), NOW_MS)
        .expect("same request joins existing flight");
    assert_eq!(owner.admitted_flight_count(), 1);
    assert_eq!(owner.in_flight_join_count(), 1);
    assert_eq!(owner.cache_hit_count(), 0);
    release(&gate);

    let expected = GreekPricingOutcome::Unavailable {
        key: key.clone(),
        reason: ModelUnavailable::NonConvergent,
    };
    assert_eq!(first.resolve(NOW_MS).await, expected);
    assert_eq!(second.resolve(NOW_MS).await, expected);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(owner.occupancy_counts(), (0, 1));

    let cached = owner
        .try_submit(request(input, key, fence), NOW_MS)
        .expect("completed result remains cached");
    assert_eq!(cached.resolve(NOW_MS).await, expected);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(owner.admitted_flight_count(), 1);
    assert_eq!(owner.in_flight_join_count(), 1);
    assert_eq!(owner.cache_hit_count(), 1);
    assert_eq!(owner.saturation_count(), 0);
    assert_eq!(owner.occupancy_counts(), (0, 1));
    assert_eq!(pool.shutdown().await, Ok(()));
}

#[tokio::test]
async fn completion_after_all_waiters_cancelled_is_retained_for_cache_hit() {
    let calls = Arc::new(AtomicUsize::new(0));
    let gate = gate();
    let pool = crate::GreekSolverPool::new_with_solver(
        1,
        2,
        blocking_solver(Arc::clone(&gate), Arc::clone(&calls)),
    )
    .expect("pool");
    let generation = PricingGeneration::new(1);
    let input = input();
    let key = key(&input, generation, 1, 1, NOW_MS);
    let fence = PricingInputFence::new(generation);
    let owner = GreekSolverSingleflight::new(&pool, 2, generation).expect("singleflight");

    let first = owner
        .try_submit(request(input.clone(), key.clone(), fence.clone()), NOW_MS)
        .expect("first waiter admitted");
    wait_started(&gate, 1);
    let second = owner
        .try_submit(request(input.clone(), key.clone(), fence.clone()), NOW_MS)
        .expect("second waiter joins");
    drop((first, second));
    release(&gate);

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if owner.entry_counts() == (0, 1) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("worker completion reached bounded cache");

    let cached = owner
        .try_submit(request(input, key.clone(), fence), NOW_MS)
        .expect("same key cache hit after cancellation");
    let actual = tokio::time::timeout(Duration::from_secs(2), cached.resolve(NOW_MS))
        .await
        .expect("cached completion is retained without original receivers");
    assert_eq!(
        actual,
        GreekPricingOutcome::Unavailable {
            key,
            reason: ModelUnavailable::NonConvergent,
        }
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(pool.shutdown().await, Ok(()));
}

#[tokio::test]
async fn per_key_waiter_limit_saturates_and_cancelled_handle_releases_capacity() {
    const WAITER_LIMIT: usize = MAX_SINGLEFLIGHT_WAITERS_PER_KEY;

    let calls = Arc::new(AtomicUsize::new(0));
    let gate = gate();
    let pool = crate::GreekSolverPool::new_with_solver(
        1,
        2,
        blocking_solver(Arc::clone(&gate), Arc::clone(&calls)),
    )
    .expect("pool");
    let generation = PricingGeneration::new(1);
    let input = input();
    let key = key(&input, generation, 1, 1, NOW_MS);
    let fence = PricingInputFence::new(generation);
    let owner = GreekSolverSingleflight::new(&pool, 2, generation).expect("singleflight");

    let mut waiters = vec![
        owner
            .try_submit(request(input.clone(), key.clone(), fence.clone()), NOW_MS)
            .expect("first waiter admitted"),
    ];
    wait_started(&gate, 1);
    for _ in 1..WAITER_LIMIT {
        waiters.push(
            owner
                .try_submit(request(input.clone(), key.clone(), fence.clone()), NOW_MS)
                .expect("waiter below per-key limit joins the shared job"),
        );
    }

    let over_limit = owner.try_submit(request(input.clone(), key.clone(), fence.clone()), NOW_MS);
    let over_limit_was_rejected =
        matches!(&over_limit, Err(GreekSingleflightError::WaiterSaturated));
    drop(over_limit);

    drop(waiters.pop().expect("one waiter to cancel"));
    let replacement = owner.try_submit(request(input.clone(), key.clone(), fence.clone()), NOW_MS);
    let replacement_was_admitted = replacement.is_ok();
    let replacement_handle = replacement.ok();

    release(&gate);
    for waiter in waiters {
        let _ = waiter.resolve(NOW_MS).await;
    }
    if let Some(replacement) = replacement_handle {
        let _ = replacement.resolve(NOW_MS).await;
    }

    assert!(
        over_limit_was_rejected,
        "the 129th active same-key handle must be rejected with the typed waiter saturation error"
    );
    assert!(
        replacement_was_admitted,
        "dropping a handle releases one waiter slot"
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(owner.admitted_flight_count(), 1);
    assert_eq!(owner.saturation_count(), 1);
    assert_eq!(owner.occupancy_counts(), (0, 1));
    assert_eq!(pool.shutdown().await, Ok(()));
}

#[tokio::test]
async fn completed_entry_with_live_waiters_cannot_be_evicted_at_capacity() {
    const WAITER_LIMIT: usize = MAX_SINGLEFLIGHT_WAITERS_PER_KEY;

    let calls = Arc::new(AtomicUsize::new(0));
    let gate = gate();
    let pool = crate::GreekSolverPool::new_with_solver(
        1,
        2,
        blocking_solver(Arc::clone(&gate), Arc::clone(&calls)),
    )
    .expect("pool");
    let generation = PricingGeneration::new(1);
    let first_input = input();
    let first_key = key(&first_input, generation, 1, 1, NOW_MS);
    let fence = PricingInputFence::new(generation);
    let owner = GreekSolverSingleflight::new(&pool, 1, generation).expect("singleflight");

    let mut waiters = vec![
        owner
            .try_submit(
                request(first_input.clone(), first_key.clone(), fence.clone()),
                NOW_MS,
            )
            .expect("first waiter admitted"),
    ];
    wait_started(&gate, 1);
    for _ in 1..WAITER_LIMIT {
        waiters.push(
            owner
                .try_submit(
                    request(first_input.clone(), first_key.clone(), fence.clone()),
                    NOW_MS,
                )
                .expect("same-key waiter joins while the solver runs"),
        );
    }
    release(&gate);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if owner.entry_counts() == (0, 1) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("first result reaches the completed cache");

    let extra_waiter = owner.try_submit(
        request(first_input.clone(), first_key.clone(), fence.clone()),
        NOW_MS,
    );
    let cache_hit_was_rejected =
        matches!(&extra_waiter, Err(GreekSingleflightError::WaiterSaturated));
    drop(extra_waiter);

    let resolved_waiter = waiters.pop().expect("one completed waiter to resolve");
    let _ = resolved_waiter.resolve(NOW_MS).await;
    let after_resolve = owner.try_submit(
        request(first_input.clone(), first_key.clone(), fence.clone()),
        NOW_MS,
    );
    let resolve_released_capacity = after_resolve.is_ok();
    if let Ok(handle) = after_resolve {
        waiters.push(handle);
    }

    let second_input = input_with_premium("6.13");
    let second_key = key(&second_input, generation, 2, 1, NOW_MS);
    let second_attempt = owner.try_submit(
        request(second_input.clone(), second_key.clone(), fence.clone()),
        NOW_MS,
    );
    let live_waiters_prevented_eviction =
        matches!(&second_attempt, Err(GreekSingleflightError::CacheSaturated));
    let second_handle = second_attempt.ok();

    drop(waiters);
    let second_handle = match second_handle {
        Some(handle) => handle,
        None => owner
            .try_submit(request(second_input, second_key, fence), NOW_MS)
            .expect("released waiter leases make the completed entry evictable"),
    };
    let _ = second_handle.resolve(NOW_MS).await;

    assert!(
        cache_hit_was_rejected && live_waiters_prevented_eviction,
        "completed-cache waiter saturation = {cache_hit_was_rejected}, active-waiter eviction protection = {live_waiters_prevented_eviction}"
    );
    assert!(
        resolve_released_capacity,
        "resolving a completed handle releases one waiter slot"
    );
    assert_eq!(owner.admitted_flight_count(), 2);
    assert_eq!(owner.saturation_count(), 2);
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert_eq!(owner.occupancy_counts(), (0, 1));
    assert_eq!(pool.shutdown().await, Ok(()));
}

#[tokio::test]
async fn old_generation_handle_release_does_not_open_current_generation_waiter_slot() {
    const WAITER_LIMIT: usize = MAX_SINGLEFLIGHT_WAITERS_PER_KEY;

    let calls = Arc::new(AtomicUsize::new(0));
    let gate = gate();
    let pool = crate::GreekSolverPool::new_with_solver(
        2,
        2,
        blocking_solver(Arc::clone(&gate), Arc::clone(&calls)),
    )
    .expect("pool");
    let generation_one = PricingGeneration::new(1);
    let old_input = input();
    let old_key = key(&old_input, generation_one, 1, 1, NOW_MS);
    let fence = PricingInputFence::new(generation_one);
    let owner = GreekSolverSingleflight::new(&pool, 2, generation_one).expect("singleflight");
    let old = owner
        .try_submit(request(old_input, old_key.clone(), fence.clone()), NOW_MS)
        .expect("old generation job admitted");
    wait_started(&gate, 1);

    let generation_two = PricingGeneration::new(2);
    owner
        .begin_generation(generation_two)
        .expect("owner generation advances");
    fence
        .begin_generation(generation_two)
        .expect("input fence generation advances");
    let current_input = input();
    let current_key = key(&current_input, generation_two, 1, 1, NOW_MS);
    let mut current_waiters = vec![
        owner
            .try_submit(
                request(current_input.clone(), current_key.clone(), fence.clone()),
                NOW_MS,
            )
            .expect("new generation job admitted"),
    ];
    wait_started(&gate, 2);
    for _ in 1..WAITER_LIMIT {
        current_waiters.push(
            owner
                .try_submit(
                    request(current_input.clone(), current_key.clone(), fence.clone()),
                    NOW_MS,
                )
                .expect("new generation waiter joins current flight"),
        );
    }

    release(&gate);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if owner.entry_counts() == (0, 1) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("current generation result reaches cache");
    assert_eq!(
        old.resolve(NOW_MS).await,
        GreekPricingOutcome::Stale { key: old_key }
    );

    let after_old_release = owner.try_submit(
        request(current_input.clone(), current_key.clone(), fence.clone()),
        NOW_MS,
    );
    let current_generation_remained_full = matches!(
        &after_old_release,
        Err(GreekSingleflightError::WaiterSaturated)
    );
    drop(after_old_release);

    drop(current_waiters.pop().expect("one current waiter to cancel"));
    let replacement = owner.try_submit(
        request(current_input.clone(), current_key.clone(), fence.clone()),
        NOW_MS,
    );
    let replacement_was_admitted = replacement.is_ok();
    let replacement_handle = replacement.ok();
    for waiter in current_waiters {
        let _ = waiter.resolve(NOW_MS).await;
    }
    if let Some(replacement) = replacement_handle {
        let _ = replacement.resolve(NOW_MS).await;
    }

    assert!(
        current_generation_remained_full,
        "releasing an old-generation handle must not decrement the current entry's waiter count"
    );
    assert!(
        replacement_was_admitted,
        "a current-generation cancellation frees one slot"
    );
    assert_eq!(owner.admitted_flight_count(), 2);
    assert_eq!(owner.saturation_count(), 1);
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert_eq!(owner.occupancy_counts(), (0, 1));
    assert_eq!(pool.shutdown().await, Ok(()));
}

#[tokio::test]
async fn accepted_quote_update_fences_running_job_even_without_new_solver_request() {
    let calls = Arc::new(AtomicUsize::new(0));
    let gate = gate();
    let pool = crate::GreekSolverPool::new_with_solver(
        1,
        1,
        blocking_solver(Arc::clone(&gate), Arc::clone(&calls)),
    )
    .expect("pool");
    let generation = PricingGeneration::new(1);
    let input = input();
    let key = key(&input, generation, 1, 1, NOW_MS);
    let fence = PricingInputFence::new(generation);
    let owner = GreekSolverSingleflight::new(&pool, 1, generation).expect("singleflight");
    let old = owner
        .try_submit(request(input, key.clone(), fence.clone()), NOW_MS)
        .expect("job admitted");
    wait_started(&gate, 1);

    fence
        .advance()
        .expect("accepted quote update advances fence");
    release(&gate);
    assert_eq!(
        old.resolve(NOW_MS).await,
        GreekPricingOutcome::Stale { key }
    );
    assert_eq!(owner.entry_counts(), (0, 0));
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(pool.shutdown().await, Ok(()));
}

#[tokio::test]
async fn late_quote_fenced_completion_cannot_replace_new_current_cache_entry() {
    let calls = Arc::new(AtomicUsize::new(0));
    let gate = gate();
    let pool = crate::GreekSolverPool::new_with_solver(
        2,
        2,
        block_first_solver(Arc::clone(&gate), Arc::clone(&calls)),
    )
    .expect("pool");
    let generation = PricingGeneration::new(1);
    let fence = PricingInputFence::new(generation);
    let owner = GreekSolverSingleflight::new(&pool, 3, generation).expect("singleflight");
    let old_input = input();
    let old_key = key(&old_input, generation, 1, 1, NOW_MS);
    let old = owner
        .try_submit(request(old_input, old_key.clone(), fence.clone()), NOW_MS)
        .expect("old key admitted");
    wait_started(&gate, 1);

    fence
        .advance()
        .expect("accepted option update advances fence");
    let current_input = input_with_premium("6.13");
    let current_key = key(&current_input, generation, 2, 1, NOW_MS);
    let current = owner
        .try_submit(
            request(current_input.clone(), current_key.clone(), fence.clone()),
            NOW_MS,
        )
        .expect("new complete key admitted");
    let expected = GreekPricingOutcome::Unavailable {
        key: current_key.clone(),
        reason: ModelUnavailable::PriceOutsideModelBounds,
    };
    assert_eq!(current.resolve(NOW_MS).await, expected);

    release(&gate);
    assert_eq!(
        old.resolve(NOW_MS).await,
        GreekPricingOutcome::Stale { key: old_key }
    );
    assert_eq!(owner.entry_counts(), (0, 1));
    let cached_current = owner
        .try_submit(request(current_input, current_key, fence), NOW_MS)
        .expect("late old completion leaves new cache entry intact");
    assert_eq!(cached_current.resolve(NOW_MS).await, expected);
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert_eq!(pool.shutdown().await, Ok(()));
}

#[tokio::test]
async fn generation_rollover_fences_late_job_and_allows_new_generation() {
    let calls = Arc::new(AtomicUsize::new(0));
    let gate = gate();
    let pool = crate::GreekSolverPool::new_with_solver(
        1,
        2,
        blocking_solver(Arc::clone(&gate), Arc::clone(&calls)),
    )
    .expect("pool");
    let generation_one = PricingGeneration::new(1);
    let old_input = input();
    let old_key = key(&old_input, generation_one, 1, 1, NOW_MS);
    let fence = PricingInputFence::new(generation_one);
    let owner = GreekSolverSingleflight::new(&pool, 2, generation_one).expect("singleflight");
    let old = owner
        .try_submit(request(old_input, old_key.clone(), fence.clone()), NOW_MS)
        .expect("old generation job admitted");
    wait_started(&gate, 1);

    let generation_two = PricingGeneration::new(2);
    owner
        .begin_generation(generation_two)
        .expect("singleflight generation advances");
    fence
        .begin_generation(generation_two)
        .expect("input fence generation advances");
    assert_eq!(owner.entry_counts(), (0, 0));
    release(&gate);
    assert_eq!(
        old.resolve(NOW_MS).await,
        GreekPricingOutcome::Stale { key: old_key }
    );

    let current_input = input();
    let current_key = key(&current_input, generation_two, 1, 1, NOW_MS);
    let current = owner
        .try_submit(request(current_input, current_key.clone(), fence), NOW_MS)
        .expect("new generation job admitted");
    assert_eq!(
        current.resolve(NOW_MS).await,
        GreekPricingOutcome::Unavailable {
            key: current_key,
            reason: ModelUnavailable::NonConvergent,
        }
    );
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert_eq!(pool.shutdown().await, Ok(()));
}

#[tokio::test]
async fn late_old_generation_completion_cannot_replace_new_generation_cache() {
    let calls = Arc::new(AtomicUsize::new(0));
    let gate = gate();
    let pool = crate::GreekSolverPool::new_with_solver(
        2,
        2,
        block_first_solver(Arc::clone(&gate), Arc::clone(&calls)),
    )
    .expect("pool");
    let generation_one = PricingGeneration::new(1);
    let fence = PricingInputFence::new(generation_one);
    let owner = GreekSolverSingleflight::new(&pool, 3, generation_one).expect("singleflight");
    let old_input = input();
    let old_key = key(&old_input, generation_one, 1, 1, NOW_MS);
    let old = owner
        .try_submit(request(old_input, old_key.clone(), fence.clone()), NOW_MS)
        .expect("old generation key admitted");
    wait_started(&gate, 1);

    let generation_two = PricingGeneration::new(2);
    owner
        .begin_generation(generation_two)
        .expect("singleflight generation advances");
    fence
        .begin_generation(generation_two)
        .expect("input fence generation advances");
    let current_input = input();
    let current_key = key(&current_input, generation_two, 1, 1, NOW_MS);
    let current = owner
        .try_submit(
            request(current_input.clone(), current_key.clone(), fence.clone()),
            NOW_MS,
        )
        .expect("new generation key admitted");
    let expected = GreekPricingOutcome::Unavailable {
        key: current_key.clone(),
        reason: ModelUnavailable::PriceOutsideModelBounds,
    };
    assert_eq!(current.resolve(NOW_MS).await, expected);

    release(&gate);
    assert_eq!(
        old.resolve(NOW_MS).await,
        GreekPricingOutcome::Stale { key: old_key }
    );
    assert_eq!(owner.entry_counts(), (0, 1));
    let cached_current = owner
        .try_submit(request(current_input, current_key, fence), NOW_MS)
        .expect("late old generation completion leaves current cache intact");
    assert_eq!(cached_current.resolve(NOW_MS).await, expected);
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert_eq!(pool.shutdown().await, Ok(()));
}
