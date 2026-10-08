use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use domain::{
    ContractCurrency, ContractMultiplier, Deliverable, DeliverableComponent, ExactDecimal,
    MarketDataProviderId, MetadataSource, OptionContract, OptionSymbol, Price,
    ProviderMetadataKind, ProviderMetadataRef, ProviderRecordId, Strike, Underlying,
};

use crate::{
    DividendAssumption, DividendCoverageKind, DividendWindowEvidence, ExchangeInstant,
    ExerciseStyle, ExpirationClass, ExpirationContext, ModelUnavailable, OptionKind, PoolError,
    SolverInput, SolverOutcome,
};

use super::{
    GreekModelVersion, GreekPricingOutcome, GreekPricingRequest, GreekSingleflightError,
    GreekSolverSingleflight, MAX_SINGLEFLIGHT_WAITERS_PER_KEY, OptionPriceInput, PricingGeneration,
    PricingInputError, PricingInputFence, PricingInputKey, PricingQuoteProvenance,
    PricingQuoteQuality, PricingSourceId, QuoteRevision, UnderlyingPriceInput,
};

const NOW_MS: i64 = 10_000;

#[derive(Clone, Copy)]
struct InputDetails<'a> {
    underlying: &'a str,
    spot: Price,
    premium: &'a str,
    years_to_expiry: f64,
    risk_free_rate: f64,
    exercise_style: ExerciseStyle,
    expiration_class: ExpirationClass,
    dividend_assumption: DividendAssumption,
    valuation_at_ms: i64,
}

#[derive(Clone)]
struct EvidenceFields<'a> {
    timezone_id: &'a str,
    valuation_offset_minutes: i16,
    expiry_offset_minutes: i16,
    provider: MarketDataProviderId,
    record_id: &'a str,
    revision: u64,
    dividend_observed_at_ms: i64,
    effective_from_ms: i64,
    effective_until_ms: i64,
    dividend_assumption: DividendAssumption,
}

fn input() -> SolverInput {
    input_with_assumptions(
        0.5,
        0.02,
        ExerciseStyle::European,
        ExpirationClass::NonZeroDaysToExpiry,
        DividendAssumption::NoDividends,
    )
}

fn input_with_premium(premium: &str) -> SolverInput {
    input_with_details(InputDetails {
        underlying: "QQQ",
        spot: Price::parse_json_number("100").expect("spot"),
        premium,
        years_to_expiry: 0.5,
        risk_free_rate: 0.02,
        exercise_style: ExerciseStyle::European,
        expiration_class: ExpirationClass::NonZeroDaysToExpiry,
        dividend_assumption: DividendAssumption::NoDividends,
        valuation_at_ms: NOW_MS,
    })
}

fn input_with_spot(spot: Price) -> SolverInput {
    input_with_details(InputDetails {
        underlying: "QQQ",
        spot,
        premium: "6.12",
        years_to_expiry: 0.5,
        risk_free_rate: 0.02,
        exercise_style: ExerciseStyle::European,
        expiration_class: ExpirationClass::NonZeroDaysToExpiry,
        dividend_assumption: DividendAssumption::NoDividends,
        valuation_at_ms: NOW_MS,
    })
}

fn input_with_assumptions(
    years_to_expiry: f64,
    risk_free_rate: f64,
    exercise_style: ExerciseStyle,
    expiration_class: ExpirationClass,
    dividend_assumption: DividendAssumption,
) -> SolverInput {
    input_with_details(InputDetails {
        underlying: "QQQ",
        spot: Price::parse_json_number("100").expect("spot"),
        premium: "6.12",
        years_to_expiry,
        risk_free_rate,
        exercise_style,
        expiration_class,
        dividend_assumption,
        valuation_at_ms: NOW_MS,
    })
}

fn input_with_underlying(underlying: &str) -> SolverInput {
    input_with_details(InputDetails {
        underlying,
        spot: Price::parse_json_number("100").expect("spot"),
        premium: "6.12",
        years_to_expiry: 0.5,
        risk_free_rate: 0.02,
        exercise_style: ExerciseStyle::European,
        expiration_class: ExpirationClass::NonZeroDaysToExpiry,
        dividend_assumption: DividendAssumption::NoDividends,
        valuation_at_ms: NOW_MS,
    })
}

fn input_with_expiry_and_max_age(
    remaining_ms: i64,
    max_age_ms: u64,
    dividend_observed_at_ms: i64,
) -> SolverInput {
    let expiry_at_ms = NOW_MS + remaining_ms;
    let expiration_class = if expiry_at_ms.div_euclid(86_400_000) == NOW_MS.div_euclid(86_400_000) {
        ExpirationClass::ZeroDaysToExpiry
    } else {
        ExpirationClass::NonZeroDaysToExpiry
    };
    let expiration_context = ExpirationContext::new(
        expiration_class,
        "UTC",
        ExchangeInstant::new(NOW_MS, 0, NOW_MS.div_euclid(86_400_000)).expect("valuation instant"),
        ExchangeInstant::new(expiry_at_ms, 0, expiry_at_ms.div_euclid(86_400_000))
            .expect("expiry instant"),
    )
    .expect("consistent expiry context");
    let evidence = DividendWindowEvidence::new(
        Underlying::new("QQQ").expect("underlying"),
        ProviderMetadataRef::new(
            MetadataSource::MarketData(MarketDataProviderId::Alpaca),
            ProviderMetadataKind::MarketData,
            ProviderRecordId::new("synthetic-dividend-window").expect("record id"),
        ),
        1,
        dividend_observed_at_ms,
        NOW_MS - 500,
        expiry_at_ms,
        DividendCoverageKind::NoCashDividendInWindow,
    )
    .expect("dividend evidence");
    SolverInput::new(
        OptionKind::Call,
        Underlying::new("QQQ").expect("underlying"),
        Price::parse_json_number("100").expect("spot"),
        Strike::parse_json_number("100").expect("strike"),
        Price::parse_json_number("6.12").expect("premium"),
        0.02,
        ExerciseStyle::European,
        expiration_context,
        DividendAssumption::NoDividends,
        Some(evidence),
        9_900,
        NOW_MS,
        max_age_ms,
    )
    .expect("valid timed solver input")
}

fn input_with_max_age(max_age_ms: u64) -> SolverInput {
    input_with_expiry_and_max_age(182 * 86_400_000 + 43_200_000, max_age_ms, 9_900)
}

fn input_with_valuation(valuation_at_ms: i64) -> SolverInput {
    input_with_details(InputDetails {
        underlying: "QQQ",
        spot: Price::parse_json_number("100").expect("spot"),
        premium: "6.12",
        years_to_expiry: 0.5,
        risk_free_rate: 0.02,
        exercise_style: ExerciseStyle::European,
        expiration_class: ExpirationClass::NonZeroDaysToExpiry,
        dividend_assumption: DividendAssumption::NoDividends,
        valuation_at_ms,
    })
}

fn input_with_evidence_fields(fields: EvidenceFields<'_>) -> SolverInput {
    let EvidenceFields {
        timezone_id,
        valuation_offset_minutes,
        expiry_offset_minutes,
        provider,
        record_id,
        revision,
        dividend_observed_at_ms,
        effective_from_ms,
        effective_until_ms,
        dividend_assumption,
    } = fields;
    let expiry_at_ms = NOW_MS + 182 * 86_400_000 + 43_200_000;
    let valuation_day =
        (NOW_MS + i64::from(valuation_offset_minutes) * 60_000).div_euclid(86_400_000);
    let expiry_day =
        (expiry_at_ms + i64::from(expiry_offset_minutes) * 60_000).div_euclid(86_400_000);
    let expiration_class = if valuation_day == expiry_day {
        ExpirationClass::ZeroDaysToExpiry
    } else {
        ExpirationClass::NonZeroDaysToExpiry
    };
    let expiration_context = ExpirationContext::new(
        expiration_class,
        timezone_id,
        ExchangeInstant::new(NOW_MS, valuation_offset_minutes, valuation_day)
            .expect("valuation instant"),
        ExchangeInstant::new(expiry_at_ms, expiry_offset_minutes, expiry_day)
            .expect("expiry instant"),
    )
    .expect("consistent expiration evidence");
    let coverage = match dividend_assumption {
        DividendAssumption::NoDividends => DividendCoverageKind::NoCashDividendInWindow,
        DividendAssumption::ContinuousYield(value) => {
            DividendCoverageKind::ContinuousYieldApproximation(value)
        }
        DividendAssumption::DiscreteScheduleUnknown => DividendCoverageKind::Unknown,
    };
    let evidence = DividendWindowEvidence::new(
        Underlying::new("QQQ").expect("underlying"),
        ProviderMetadataRef::new(
            MetadataSource::MarketData(provider),
            ProviderMetadataKind::MarketData,
            ProviderRecordId::new(record_id).expect("record id"),
        ),
        revision,
        dividend_observed_at_ms,
        effective_from_ms,
        effective_until_ms,
        coverage,
    )
    .expect("dividend evidence");
    SolverInput::new(
        OptionKind::Call,
        Underlying::new("QQQ").expect("underlying"),
        Price::parse_json_number("100").expect("spot"),
        Strike::parse_json_number("100").expect("strike"),
        Price::parse_json_number("6.12").expect("premium"),
        0.02,
        ExerciseStyle::European,
        expiration_context,
        dividend_assumption,
        Some(evidence),
        9_900,
        NOW_MS,
        3_000,
    )
    .expect("valid evidence fingerprint input")
}

fn exact_expiry_input() -> SolverInput {
    let expiration_context = ExpirationContext::new(
        ExpirationClass::ZeroDaysToExpiry,
        "UTC",
        ExchangeInstant::new(NOW_MS, 0, 0).expect("valuation instant"),
        ExchangeInstant::new(NOW_MS, 0, 0).expect("expiry instant"),
    )
    .expect("exact-expiry context");
    SolverInput::new(
        OptionKind::Call,
        Underlying::new("QQQ").expect("underlying"),
        Price::parse_json_number("100").expect("spot"),
        Strike::parse_json_number("100").expect("strike"),
        Price::parse_json_number("0").expect("premium"),
        0.02,
        ExerciseStyle::European,
        expiration_context,
        DividendAssumption::NoDividends,
        None,
        9_900,
        NOW_MS,
        3_000,
    )
    .expect("exact-expiry input")
}

fn input_with_details(details: InputDetails<'_>) -> SolverInput {
    let InputDetails {
        underlying,
        spot,
        premium,
        years_to_expiry,
        risk_free_rate,
        exercise_style,
        expiration_class,
        dividend_assumption,
        valuation_at_ms,
    } = details;
    let remaining_ms = if expiration_class == ExpirationClass::ZeroDaysToExpiry {
        60 * 60 * 1_000
    } else {
        (years_to_expiry * 365.0 * 86_400_000.0).round() as i64
    };
    let expiry_at_ms = valuation_at_ms + remaining_ms;
    let expiration_context = ExpirationContext::new(
        expiration_class,
        "UTC",
        ExchangeInstant::new(valuation_at_ms, 0, valuation_at_ms.div_euclid(86_400_000))
            .expect("valuation instant"),
        ExchangeInstant::new(expiry_at_ms, 0, expiry_at_ms.div_euclid(86_400_000))
            .expect("expiry instant"),
    )
    .expect("consistent expiry context");
    let coverage = match dividend_assumption {
        DividendAssumption::NoDividends => DividendCoverageKind::NoCashDividendInWindow,
        DividendAssumption::ContinuousYield(value) => {
            DividendCoverageKind::ContinuousYieldApproximation(value)
        }
        DividendAssumption::DiscreteScheduleUnknown => DividendCoverageKind::Unknown,
    };
    let dividend_evidence = DividendWindowEvidence::new(
        Underlying::new(underlying).expect("synthetic underlying"),
        ProviderMetadataRef::new(
            MetadataSource::MarketData(MarketDataProviderId::Alpaca),
            ProviderMetadataKind::MarketData,
            ProviderRecordId::new("synthetic-dividend-window").expect("record id"),
        ),
        1,
        9_900,
        9_500,
        expiry_at_ms,
        coverage,
    )
    .expect("synthetic dividend evidence");
    SolverInput::new(
        OptionKind::Call,
        Underlying::new(underlying).expect("synthetic underlying"),
        spot,
        Strike::parse_json_number("100").expect("strike"),
        Price::parse_json_number(premium).expect("premium"),
        risk_free_rate,
        exercise_style,
        expiration_context,
        dividend_assumption,
        Some(dividend_evidence),
        9_900,
        valuation_at_ms,
        3_000,
    )
    .expect("valid synthetic solver assumptions")
}

fn contract(symbol: &str) -> OptionContract {
    let symbol = OptionSymbol::parse(symbol).expect("valid synthetic OCC symbol");
    let deliverable = Deliverable::new(vec![
        DeliverableComponent::equity(symbol.underlying().clone(), ExactDecimal::from_integer(100))
            .expect("positive synthetic deliverable"),
    ])
    .expect("one synthetic equity deliverable");
    OptionContract::new(
        symbol,
        ContractMultiplier::new(100).expect("positive multiplier"),
        ContractCurrency::new("USD").expect("valid currency"),
        deliverable,
    )
}

fn standard_contract() -> OptionContract {
    contract("QQQ   271217C00100000")
}

fn option_provenance(generation: PricingGeneration, revision: u64) -> PricingQuoteProvenance {
    PricingQuoteProvenance::new(
        PricingSourceId::new(11),
        generation,
        QuoteRevision::new(revision),
        9_900,
        NOW_MS,
        3_000,
        PricingQuoteQuality::Realtime,
    )
}

fn underlying_provenance(generation: PricingGeneration, revision: u64) -> PricingQuoteProvenance {
    PricingQuoteProvenance::new(
        PricingSourceId::new(22),
        generation,
        QuoteRevision::new(revision),
        9_900,
        NOW_MS,
        1_000,
        PricingQuoteQuality::Realtime,
    )
}

fn key(
    input: &SolverInput,
    generation: PricingGeneration,
    option_revision: u64,
    underlying_revision: u64,
    valuation_at_ms: i64,
) -> PricingInputKey {
    key_with_provenance(
        standard_contract(),
        input,
        generation,
        option_provenance(generation, option_revision),
        underlying_provenance(generation, underlying_revision),
        valuation_at_ms,
    )
    .expect("complete pricing key")
}

fn quote_inputs(
    contract: &OptionContract,
    option_price: Price,
    underlying_price: Price,
    option_provenance: PricingQuoteProvenance,
    underlying_provenance: PricingQuoteProvenance,
) -> (OptionPriceInput, UnderlyingPriceInput) {
    (
        OptionPriceInput::new(contract.clone(), option_price, option_provenance),
        UnderlyingPriceInput::new(
            contract.symbol().underlying().clone(),
            underlying_price,
            underlying_provenance,
        ),
    )
}

fn key_with_provenance(
    contract: OptionContract,
    input: &SolverInput,
    generation: PricingGeneration,
    option_provenance: PricingQuoteProvenance,
    underlying_provenance: PricingQuoteProvenance,
    valuation_at_ms: i64,
) -> Result<PricingInputKey, PricingInputError> {
    let (option_input, underlying_input) = quote_inputs(
        &contract,
        input.option_price(),
        input.spot(),
        option_provenance,
        underlying_provenance,
    );
    PricingInputKey::new(
        contract,
        generation,
        option_input,
        underlying_input,
        valuation_at_ms,
        input,
    )
}

fn request(
    input: SolverInput,
    key: PricingInputKey,
    fence: PricingInputFence,
) -> GreekPricingRequest {
    let revision = fence.accept_key(&key).expect("accept complete key");
    GreekPricingRequest::new(input, key, fence, revision).expect("valid pricing request")
}

fn gate() -> Arc<(Mutex<(usize, bool)>, Condvar)> {
    Arc::new((Mutex::new((0, false)), Condvar::new()))
}

fn blocking_solver(
    gate: Arc<(Mutex<(usize, bool)>, Condvar)>,
    calls: Arc<AtomicUsize>,
) -> Arc<dyn Fn(&SolverInput) -> SolverOutcome + Send + Sync> {
    Arc::new(move |_| {
        calls.fetch_add(1, Ordering::Relaxed);
        let (lock, condition) = &*gate;
        let mut state = lock.lock().expect("test gate lock");
        state.0 += 1;
        condition.notify_all();
        while !state.1 {
            state = condition.wait(state).expect("test gate wait");
        }
        SolverOutcome::Unavailable(ModelUnavailable::NonConvergent)
    })
}

fn block_first_solver(
    gate: Arc<(Mutex<(usize, bool)>, Condvar)>,
    calls: Arc<AtomicUsize>,
) -> Arc<dyn Fn(&SolverInput) -> SolverOutcome + Send + Sync> {
    Arc::new(move |_| {
        if calls.fetch_add(1, Ordering::Relaxed) == 0 {
            let (lock, condition) = &*gate;
            let mut state = lock.lock().expect("test gate lock");
            state.0 += 1;
            condition.notify_all();
            while !state.1 {
                state = condition.wait(state).expect("test gate wait");
            }
            SolverOutcome::Unavailable(ModelUnavailable::NonConvergent)
        } else {
            SolverOutcome::Unavailable(ModelUnavailable::PriceOutsideModelBounds)
        }
    })
}

fn wait_started(gate: &Arc<(Mutex<(usize, bool)>, Condvar)>, count: usize) {
    let (lock, condition) = &**gate;
    let state = lock.lock().expect("test gate lock");
    let (state, timeout) = condition
        .wait_timeout_while(state, Duration::from_secs(2), |state| state.0 < count)
        .expect("test gate wait");
    assert!(!timeout.timed_out(), "worker did not start");
    assert!(state.0 >= count);
}

fn release(gate: &Arc<(Mutex<(usize, bool)>, Condvar)>) {
    let (lock, condition) = &**gate;
    let mut state = lock.lock().expect("test gate lock");
    state.1 = true;
    condition.notify_all();
}

#[path = "tests/freshness.rs"]
mod freshness;
#[path = "tests/keys.rs"]
mod keys;
#[path = "tests/lifecycle.rs"]
mod lifecycle;
