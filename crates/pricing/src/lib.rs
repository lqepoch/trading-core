#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

//! Exact, broker-independent pricing calculations and bounded model estimates.
//!
//! Callers provide quote provenance, feed-quality evidence, and contract
//! qualification. This crate checks those inputs for freshness and internal
//! consistency, but does not authenticate providers or qualify contracts. It
//! accepts exact prices and never converts an authoritative value to binary
//! floating point.
//!
//! ## 简体中文
//!
//! 本 crate 提供与券商无关的定价计算和有界模型估算。调用方提供行情来源、质量和合约资格证据；本 crate 检查输入的新鲜度及内部一致性，但不认证供应商或核验合约资格。精确价格保持精确类型，不转为二进制浮点数。

mod greeks;
mod singleflight;
mod solver;
mod vertical;
mod worker_pool;

pub use greeks::{
    AggregatedGreek, GreekKind, GreekPosition, LongMinusShortIvDifference, MAX_AGGREGATE_UNITS,
    MAX_METRIC_AGE_MS, MAX_PROVIDER_METRIC_ABS, MAX_SIGNED_CONTRACTS, MetricKind,
    MetricObservation, MetricQuality, MetricSource, MetricUnit, MetricsError, ObservationWindow,
    OptionMetrics, SignedContractPosition, aggregate_greek,
};
pub use singleflight::{
    GreekModelVersion, GreekPricingJobHandle, GreekPricingOutcome, GreekPricingRequest,
    GreekSingleflightError, GreekSolverSingleflight, MAX_SINGLEFLIGHT_ENTRIES, OptionPriceInput,
    PricingFenceRevision, PricingGeneration, PricingInputError, PricingInputFence, PricingInputKey,
    PricingQuoteProvenance, PricingQuoteQuality, PricingSourceId, UnderlyingPriceInput,
};
pub use solver::{
    DividendAssumption, DividendCoverageKind, DividendEvidenceError, DividendWindowEvidence,
    ExchangeInstant, ExerciseStyle, ExpirationClass, ExpirationContext, ExpirationContextError,
    ModelUnavailable, OptionKind, SolverInput, SolverInputError, SolverOutcome, solve_american_crr,
    solve_european_black_scholes, solve_option_model,
};
pub use worker_pool::{
    GreekJobHandle, GreekJobOutcome, GreekSolverPool, GreekSolverRequest, PoolError, QuoteRevision,
    QuoteRevisionFence,
};

pub use vertical::{
    LegQuote, QuoteSide, SyntheticVerticalQuote, VerticalLeg, synthetic_debit_vertical,
};
