use super::*;

#[test]
fn complete_key_tracks_contract_inputs_both_sources_and_valuation_point() {
    let generation = PricingGeneration::new(1);
    let input = input();
    let baseline = key(&input, generation, 1, 2, NOW_MS);
    let other_valuation_input = input_with_valuation(NOW_MS + 1);
    let other_valuation = key(&other_valuation_input, generation, 1, 2, NOW_MS + 1);
    let other_option_revision = key(&input, generation, 3, 2, NOW_MS);
    let other_underlying_revision = key(&input, generation, 1, 4, NOW_MS);
    let other_generation = key(&input, PricingGeneration::new(2), 1, 2, NOW_MS);
    let other_expiry_contract = key_with_provenance(
        contract("QQQ   281217C00100000"),
        &input,
        generation,
        option_provenance(generation, 1),
        underlying_provenance(generation, 2),
        NOW_MS,
    )
    .expect("different expiration key is valid");
    let changed_input = input_with_premium("6.13");
    let other_solver_input = key(&changed_input, generation, 1, 2, NOW_MS);
    let other_time_input = key(
        &input_with_assumptions(
            0.75,
            0.02,
            ExerciseStyle::European,
            ExpirationClass::NonZeroDaysToExpiry,
            DividendAssumption::NoDividends,
        ),
        generation,
        1,
        2,
        NOW_MS,
    );
    let other_rate_input = key(
        &input_with_assumptions(
            0.5,
            0.03,
            ExerciseStyle::European,
            ExpirationClass::NonZeroDaysToExpiry,
            DividendAssumption::NoDividends,
        ),
        generation,
        1,
        2,
        NOW_MS,
    );
    let other_exercise_input = key(
        &input_with_assumptions(
            0.5,
            0.02,
            ExerciseStyle::American,
            ExpirationClass::NonZeroDaysToExpiry,
            DividendAssumption::NoDividends,
        ),
        generation,
        1,
        2,
        NOW_MS,
    );
    let other_expiration_class_input = key(
        &input_with_assumptions(
            0.5,
            0.02,
            ExerciseStyle::European,
            ExpirationClass::ZeroDaysToExpiry,
            DividendAssumption::NoDividends,
        ),
        generation,
        1,
        2,
        NOW_MS,
    );
    let other_dividend_input = key(
        &input_with_assumptions(
            0.5,
            0.02,
            ExerciseStyle::European,
            ExpirationClass::NonZeroDaysToExpiry,
            DividendAssumption::ContinuousYield(0.01),
        ),
        generation,
        1,
        2,
        NOW_MS,
    );
    let other_underlying_contract = contract("IWM   271217C00100000");
    let other_underlying_input = input_with_underlying("IWM");
    let other_contract = key_with_provenance(
        other_underlying_contract,
        &other_underlying_input,
        generation,
        option_provenance(generation, 1),
        underlying_provenance(generation, 2),
        NOW_MS,
    )
    .expect("different underlying contract key is valid");

    assert_ne!(baseline, other_valuation);
    assert_ne!(baseline, other_option_revision);
    assert_ne!(baseline, other_underlying_revision);
    assert_ne!(baseline, other_generation);
    assert_ne!(baseline, other_expiry_contract);
    assert_ne!(baseline, other_solver_input);
    assert_ne!(baseline, other_time_input);
    assert_ne!(baseline, other_rate_input);
    assert_ne!(baseline, other_exercise_input);
    assert_ne!(baseline, other_expiration_class_input);
    assert_ne!(baseline, other_dividend_input);
    assert_ne!(baseline, other_contract);
    assert_eq!(
        baseline.model_version(),
        super::GreekModelVersion::BlackScholesEuropeanV2
    );
    assert_eq!(baseline.contract(), &standard_contract());
}

#[test]
fn complete_key_tracks_all_expiry_and_dividend_evidence_fields() {
    let generation = PricingGeneration::new(1);
    let expiry_at_ms = NOW_MS + 182 * 86_400_000 + 43_200_000;
    let baseline_fields = EvidenceFields {
        timezone_id: "UTC",
        valuation_offset_minutes: 0,
        expiry_offset_minutes: 0,
        provider: MarketDataProviderId::Alpaca,
        record_id: "synthetic-dividend-window",
        revision: 1,
        dividend_observed_at_ms: 9_900,
        effective_from_ms: 9_500,
        effective_until_ms: expiry_at_ms,
        dividend_assumption: DividendAssumption::NoDividends,
    };
    let baseline_input = input_with_evidence_fields(baseline_fields.clone());
    let baseline = key(&baseline_input, generation, 1, 2, NOW_MS);
    let variants = [
        input_with_evidence_fields(EvidenceFields {
            timezone_id: "UTC-fixture",
            ..baseline_fields.clone()
        }),
        input_with_evidence_fields(EvidenceFields {
            valuation_offset_minutes: 60,
            ..baseline_fields.clone()
        }),
        input_with_evidence_fields(EvidenceFields {
            expiry_offset_minutes: 60,
            ..baseline_fields.clone()
        }),
        input_with_evidence_fields(EvidenceFields {
            provider: MarketDataProviderId::Schwab,
            ..baseline_fields.clone()
        }),
        input_with_evidence_fields(EvidenceFields {
            record_id: "synthetic-dividend-window-v2",
            ..baseline_fields.clone()
        }),
        input_with_evidence_fields(EvidenceFields {
            revision: 2,
            ..baseline_fields.clone()
        }),
        input_with_evidence_fields(EvidenceFields {
            dividend_observed_at_ms: 9_800,
            ..baseline_fields.clone()
        }),
        input_with_evidence_fields(EvidenceFields {
            effective_from_ms: 9_400,
            ..baseline_fields.clone()
        }),
        input_with_evidence_fields(EvidenceFields {
            effective_until_ms: expiry_at_ms + 1,
            ..baseline_fields.clone()
        }),
        input_with_evidence_fields(EvidenceFields {
            dividend_assumption: DividendAssumption::ContinuousYield(0.01),
            ..baseline_fields.clone()
        }),
    ];
    for variant in variants {
        assert_ne!(baseline, key(&variant, generation, 1, 2, NOW_MS));
    }
}

#[test]
fn rejects_pricing_key_valuation_that_differs_from_solver_valuation() {
    let generation = PricingGeneration::new(1);
    let input = input();
    let result = key_with_provenance(
        standard_contract(),
        &input,
        generation,
        option_provenance(generation, 1),
        underlying_provenance(generation, 2),
        NOW_MS + 1,
    );
    assert_eq!(result, Err(PricingInputError::ValuationInstantMismatch));
}

#[test]
fn rejects_solver_underlying_that_differs_from_option_contract() {
    let generation = PricingGeneration::new(1);
    let input = input_with_underlying("IWM");
    let result = key_with_provenance(
        standard_contract(),
        &input,
        generation,
        option_provenance(generation, 1),
        underlying_provenance(generation, 2),
        NOW_MS,
    );
    assert_eq!(result, Err(PricingInputError::ContractSolverMismatch));
}

#[test]
fn rejects_contract_solver_mismatch_and_quote_generation_mismatch() {
    let input = input();
    let generation = PricingGeneration::new(1);
    let mismatch = key_with_provenance(
        contract("QQQ   271217P00100000"),
        &input,
        generation,
        option_provenance(generation, 1),
        underlying_provenance(generation, 1),
        NOW_MS,
    );
    assert_eq!(mismatch, Err(PricingInputError::ContractSolverMismatch));

    let wrong_generation = key_with_provenance(
        standard_contract(),
        &input,
        generation,
        option_provenance(PricingGeneration::new(2), 1),
        underlying_provenance(generation, 1),
        NOW_MS,
    );
    assert_eq!(wrong_generation, Err(PricingInputError::GenerationMismatch));
}

#[test]
fn rejects_underlying_quote_price_mismatched_with_solver_spot() {
    let generation = PricingGeneration::new(1);
    let underlying_quote_price = Price::parse_json_number("100").expect("quote price");
    let solver_spot = Price::parse_json_number("101").expect("solver spot");
    assert_ne!(underlying_quote_price, solver_spot);
    let input = input_with_spot(solver_spot);
    let contract = standard_contract();

    // The single underlying quote binding must match both the contract
    // identity and the exact spot passed to the solver.
    let (option_input, underlying_input) = quote_inputs(
        &contract,
        input.option_price(),
        underlying_quote_price,
        option_provenance(generation, 1),
        underlying_provenance(generation, 1),
    );
    let result = PricingInputKey::new(
        contract,
        generation,
        option_input,
        underlying_input,
        NOW_MS,
        &input,
    );
    assert!(
        matches!(result, Err(PricingInputError::UnderlyingPriceMismatch)),
        "accepted solver spot {solver_spot:?} for underlying quote price {underlying_quote_price:?}"
    );
}

#[test]
fn rejects_option_quote_price_mismatched_with_solver_premium() {
    let generation = PricingGeneration::new(1);
    let option_quote_price = Price::parse_json_number("6.12").expect("quote price");
    let solver_premium = Price::parse_json_number("6.13").expect("solver premium");
    assert_ne!(option_quote_price, solver_premium);
    let input = input_with_premium("6.13");
    let contract = standard_contract();
    let (option_input, underlying_input) = quote_inputs(
        &contract,
        option_quote_price,
        input.spot(),
        option_provenance(generation, 1),
        underlying_provenance(generation, 1),
    );

    let result = PricingInputKey::new(
        contract,
        generation,
        option_input,
        underlying_input,
        NOW_MS,
        &input,
    );
    assert!(
        matches!(result, Err(PricingInputError::OptionPriceMismatch)),
        "accepted solver premium {solver_premium:?} for option quote price {option_quote_price:?}"
    );
}

#[test]
fn rejects_underlying_quote_for_different_contract_underlying() {
    let generation = PricingGeneration::new(1);
    let input = input();
    let contract = standard_contract();
    let (option_input, _) = quote_inputs(
        &contract,
        input.option_price(),
        input.spot(),
        option_provenance(generation, 1),
        underlying_provenance(generation, 1),
    );
    let wrong_underlying_input = UnderlyingPriceInput::new(
        Underlying::new("SPY").expect("valid underlying"),
        input.spot(),
        underlying_provenance(generation, 1),
    );

    let result = PricingInputKey::new(
        contract,
        generation,
        option_input,
        wrong_underlying_input,
        NOW_MS,
        &input,
    );
    assert_eq!(result, Err(PricingInputError::UnderlyingIdentityMismatch));
}

#[test]
fn rejects_option_quote_with_different_option_symbol() {
    let generation = PricingGeneration::new(1);
    let input = input();
    let quoted_contract = standard_contract();
    let requested_contract = contract("QQQ   280121C00100000");
    assert_ne!(quoted_contract.symbol(), requested_contract.symbol());
    let option_input = OptionPriceInput::new(
        quoted_contract,
        input.option_price(),
        option_provenance(generation, 1),
    );
    let underlying_input = UnderlyingPriceInput::new(
        requested_contract.symbol().underlying().clone(),
        input.spot(),
        underlying_provenance(generation, 1),
    );

    let result = PricingInputKey::new(
        requested_contract,
        generation,
        option_input,
        underlying_input,
        NOW_MS,
        &input,
    );
    assert_eq!(result, Err(PricingInputError::OptionContractMismatch));
}

#[test]
fn freshness_provenance_quality_and_source_are_part_of_key() {
    let generation = PricingGeneration::new(1);
    let input = input();
    let base = key(&input, generation, 1, 2, NOW_MS);
    let altered_option = PricingQuoteProvenance::new(
        PricingSourceId::new(12),
        generation,
        QuoteRevision::new(1),
        9_900,
        NOW_MS,
        3_000,
        PricingQuoteQuality::Delayed,
    );
    let altered_underlying = PricingQuoteProvenance::new(
        PricingSourceId::new(23),
        generation,
        QuoteRevision::new(2),
        9_900,
        NOW_MS,
        1_000,
        PricingQuoteQuality::Unknown,
    );
    let option_changed = key_with_provenance(
        standard_contract(),
        &input,
        generation,
        altered_option,
        underlying_provenance(generation, 2),
        NOW_MS,
    )
    .expect("option provenance remains valid");
    let underlying_changed = key_with_provenance(
        standard_contract(),
        &input,
        generation,
        option_provenance(generation, 1),
        altered_underlying,
        NOW_MS,
    )
    .expect("underlying provenance remains valid");
    let underlying_time_changed = key_with_provenance(
        standard_contract(),
        &input,
        generation,
        option_provenance(generation, 1),
        PricingQuoteProvenance::new(
            PricingSourceId::new(22),
            generation,
            QuoteRevision::new(2),
            9_800,
            9_999,
            1_500,
            PricingQuoteQuality::Realtime,
        ),
        NOW_MS,
    )
    .expect("underlying timestamp and age policy remain valid");
    let option_received_changed = key_with_provenance(
        standard_contract(),
        &input,
        generation,
        PricingQuoteProvenance::new(
            PricingSourceId::new(11),
            generation,
            QuoteRevision::new(1),
            9_900,
            9_999,
            3_000,
            PricingQuoteQuality::Realtime,
        ),
        underlying_provenance(generation, 2),
        NOW_MS,
    )
    .expect("option receive timestamp remains valid");
    assert_ne!(base, option_changed);
    assert_ne!(base, underlying_changed);
    assert_ne!(base, underlying_time_changed);
    assert_ne!(base, option_received_changed);
}

#[test]
fn floating_point_zero_is_normalized_without_merging_other_values() {
    let generation = PricingGeneration::new(1);
    let negative_zero = input_with_assumptions(
        0.5,
        -0.0,
        ExerciseStyle::European,
        ExpirationClass::NonZeroDaysToExpiry,
        DividendAssumption::NoDividends,
    );
    let positive_zero = input_with_assumptions(
        0.5,
        0.0,
        ExerciseStyle::European,
        ExpirationClass::NonZeroDaysToExpiry,
        DividendAssumption::NoDividends,
    );
    assert_eq!(
        key(&negative_zero, generation, 1, 2, NOW_MS),
        key(&positive_zero, generation, 1, 2, NOW_MS)
    );
}
