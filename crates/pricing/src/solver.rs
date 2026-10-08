//! Bounded European Black-Scholes solver and an independently gated American model boundary.
//!
//! Exact domain prices are converted to floating point only inside the
//! analytical calculation; unavailable assumptions produce typed outcomes.
//!
//! ## 简体中文
//!
//! 本模块提供有界的欧式 Black-Scholes 求解器和经过独立精度门控的美式模型边界。所有模型假设都必须显式给出。精确领域价格仅在数值计算内部转换为浮点数；未验收的模型能力会返回类型化不可用结果。

#[path = "solver/american_crr.rs"]
mod american_crr;

pub use american_crr::solve_american_crr;

use domain::{
    MetadataSource, OptionContract, OptionRight, Price, ProviderMetadataKind, ProviderMetadataRef,
    Strike, Underlying,
};

use crate::greeks::{
    MetricKind, MetricObservation, MetricQuality, MetricSource, MetricUnit, MetricsError,
    ObservationWindow, OptionMetrics,
};

const MIN_VOLATILITY: f64 = 0.0001;
const MAX_VOLATILITY: f64 = 5.0;
const MAX_ITERATIONS: usize = 96;
const PRICE_TOLERANCE: f64 = 1.0e-10;
const MAX_MODEL_PRICE: f64 = 1_000_000.0;
const MAX_MODEL_TIME_YEARS: f64 = 10.0;
const MAX_MODEL_RATE_ABS: f64 = 1.0;
const MILLIS_PER_DAY: i64 = 86_400_000;
const MILLIS_PER_YEAR_ACT_365F: f64 = 365.0 * MILLIS_PER_DAY as f64;
const MAX_UTC_OFFSET_MINUTES: i16 = 14 * 60;
const MAX_ZERO_DTE_REMAINING_MS: i64 = MILLIS_PER_DAY;

/// Option contract payoff direction used by the analytical model.
/// 简体中文：解析模型采用的期权收益方向。
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum OptionKind {
    /// Call option.
    /// 简体中文：看涨期权。
    Call,
    /// Put option.
    /// 简体中文：看跌期权。
    Put,
}

/// Exercise assumption supplied by the caller. The European analytical path
/// rejects American exercise. American exercise routes to a fail-closed
/// accuracy boundary; positive-time results remain typed unavailable pending
/// independent validation.
/// 简体中文：调用方提供的行权方式。欧式解析路径拒绝美式行权；美式行权路由到 fail-closed 精度边界，独立验收通过前正剩余时间的结果保持类型化不可用。
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExerciseStyle {
    /// European exercise.
    /// 简体中文：欧式行权。
    European,
    /// American exercise; positive-time results use a fail-closed accuracy boundary.
    /// 简体中文：美式行权；正剩余时间的结果进入 fail-closed 精度边界。
    American,
}

/// Expiration class is checked against exchange-local dates in
/// [`ExpirationContext`]. The pricing crate does not qualify the contract or
/// establish the exchange calendar.
/// 简体中文：`ExpirationContext` 会根据交易所本地日期核对到期分类。定价 crate 不负责资格认证合约或确认交易所日历。
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExpirationClass {
    /// Expiration is the current trading day.
    /// 简体中文：合约在当前交易日到期。
    ZeroDaysToExpiry,
    /// Expiration's exchange-local day differs from the valuation day.
    /// Solver input separately rejects instants that are already expired.
    /// 简体中文：到期交易所本地日期与估值日期不同；求解器输入还会单独拒绝已过期时刻。
    NonZeroDaysToExpiry,
}

/// UTC instant paired with the caller's exchange-local day and UTC offset.
/// The pricing crate checks their arithmetic consistency but does not validate
/// the timezone database or the source of this evidence.
/// 简体中文：UTC 时刻及调用方提供的交易所本地日期和 UTC 偏移。定价 crate 只核对算术一致性，不验证时区数据库或证据来源。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExchangeInstant {
    utc_epoch_millis: i64,
    utc_offset_minutes: i16,
    local_day_ordinal: i64,
}

impl ExchangeInstant {
    /// Creates an instant when UTC plus its minute offset matches the supplied
    /// local-day ordinal. The ordinal counts days since 1970-01-01.
    /// 简体中文：仅当 UTC 时间加分钟偏移后与提供的本地日期序号一致时创建。日期序号自 1970-01-01 起按天计数。
    pub fn new(
        utc_epoch_millis: i64,
        utc_offset_minutes: i16,
        local_day_ordinal: i64,
    ) -> Result<Self, ExpirationContextError> {
        if !(-MAX_UTC_OFFSET_MINUTES..=MAX_UTC_OFFSET_MINUTES).contains(&utc_offset_minutes) {
            return Err(ExpirationContextError::UtcOffsetOutOfRange);
        }
        let offset_millis = i64::from(utc_offset_minutes) * 60_000;
        let local_epoch_millis = utc_epoch_millis
            .checked_add(offset_millis)
            .ok_or(ExpirationContextError::LocalInstantOutOfRange)?;
        if local_epoch_millis.div_euclid(MILLIS_PER_DAY) != local_day_ordinal {
            return Err(ExpirationContextError::LocalDateDoesNotMatchInstant);
        }
        Ok(Self {
            utc_epoch_millis,
            utc_offset_minutes,
            local_day_ordinal,
        })
    }

    /// Returns the UTC epoch-millisecond instant.
    /// 简体中文：返回 UTC Unix 毫秒时刻。
    pub const fn utc_epoch_millis(self) -> i64 {
        self.utc_epoch_millis
    }

    /// Returns the caller-supplied UTC offset in minutes.
    /// 简体中文：返回调用方提供的 UTC 分钟偏移。
    pub const fn utc_offset_minutes(self) -> i16 {
        self.utc_offset_minutes
    }

    /// Returns the exchange-local day ordinal.
    /// 简体中文：返回交易所本地日期序号。
    pub const fn local_day_ordinal(self) -> i64 {
        self.local_day_ordinal
    }
}

/// Date/time evidence used to derive a nonnegative ACT/365F year fraction.
/// `timezone_id` and offsets are retained caller evidence; this crate does not
/// assert that the ID is an IANA zone or that a broker contract was qualified.
/// 简体中文：用于派生非负 ACT/365F 年分数的日期/时间证据。`timezone_id` 和偏移作为调用方证据保留；本 crate 不声称该 ID 是已校验的 IANA 时区，也不证明券商合约已通过资格认证。
#[derive(Clone, Debug, PartialEq)]
pub struct ExpirationContext {
    classification: ExpirationClass,
    timezone_id: String,
    valuation: ExchangeInstant,
    expiration: ExchangeInstant,
}

impl ExpirationContext {
    /// Creates a context whose class matches the exchange-local dates. Local
    /// dates must be supplied in the timezone represented by `timezone_id`.
    /// `years_to_expiry` is derived later from the UTC instants using ACT/365F.
    /// 简体中文：创建与交易所本地日期分类一致的上下文。本地日期必须由调用方按 `timezone_id` 对应时区提供；之后根据 UTC 时刻按 ACT/365F 派生 `years_to_expiry`。
    pub fn new(
        classification: ExpirationClass,
        timezone_id: &str,
        valuation: ExchangeInstant,
        expiration: ExchangeInstant,
    ) -> Result<Self, ExpirationContextError> {
        if timezone_id.is_empty()
            || timezone_id.len() > 128
            || !timezone_id.is_ascii()
            || timezone_id.chars().any(char::is_control)
        {
            return Err(ExpirationContextError::InvalidTimezoneEvidence);
        }
        let expected = if expiration.local_day_ordinal == valuation.local_day_ordinal {
            ExpirationClass::ZeroDaysToExpiry
        } else {
            ExpirationClass::NonZeroDaysToExpiry
        };
        if classification != expected {
            return Err(ExpirationContextError::ClassificationMismatch);
        }
        Ok(Self {
            classification,
            timezone_id: timezone_id.to_owned(),
            valuation,
            expiration,
        })
    }

    /// Returns the caller-supplied timezone identifier evidence.
    /// 简体中文：返回调用方提供的时区标识证据。
    pub fn timezone_id(&self) -> &str {
        &self.timezone_id
    }

    /// Returns the validated calendar classification.
    /// 简体中文：返回已核对的日历分类。
    pub const fn classification(&self) -> ExpirationClass {
        self.classification
    }

    /// Returns the exact valuation instant supplied by the caller.
    /// 简体中文：返回调用方提供的精确估值时刻。
    pub const fn valuation(&self) -> ExchangeInstant {
        self.valuation
    }

    /// Returns the exact contract expiration instant supplied by the caller.
    /// 简体中文：返回调用方提供的精确合约到期时刻。
    pub const fn expiration(&self) -> ExchangeInstant {
        self.expiration
    }
}

/// Invalid or inconsistent caller-provided expiry evidence.
/// 简体中文：调用方到期证据无效或相互不一致。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExpirationContextError {
    /// Timezone identifier evidence is empty, oversized, or contains controls.
    /// 简体中文：时区标识证据为空、过长或含控制字符。
    InvalidTimezoneEvidence,
    /// UTC offset exceeds the supported civil-time range of fourteen hours.
    /// 简体中文：UTC 偏移超出正负十四小时的民用时间范围。
    UtcOffsetOutOfRange,
    /// Applying the offset overflowed the supported timestamp range.
    /// 简体中文：应用偏移后时间戳溢出支持范围。
    LocalInstantOutOfRange,
    /// UTC plus offset does not match the supplied local-day ordinal.
    /// 简体中文：UTC 时间加偏移后与提供的本地日期序号不匹配。
    LocalDateDoesNotMatchInstant,
    /// The explicit class disagrees with the two exchange-local dates.
    /// 简体中文：显式分类与两个交易所本地日期不一致。
    ClassificationMismatch,
}

impl std::fmt::Display for ExpirationContextError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidTimezoneEvidence => "EXPIRATION_TIMEZONE_EVIDENCE_INVALID",
            Self::UtcOffsetOutOfRange => "EXPIRATION_UTC_OFFSET_OUT_OF_RANGE",
            Self::LocalInstantOutOfRange => "EXPIRATION_LOCAL_INSTANT_OUT_OF_RANGE",
            Self::LocalDateDoesNotMatchInstant => "EXPIRATION_LOCAL_DATE_MISMATCH",
            Self::ClassificationMismatch => "EXPIRATION_CLASSIFICATION_MISMATCH",
        })
    }
}

impl std::error::Error for ExpirationContextError {}

/// Explicit dividend model input. European Black-Scholes accepts a known
/// continuous-yield approximation; American production pricing remains gated.
/// Neither model approximates a discrete cash-dividend schedule.
/// 简体中文：显式股息模型输入。欧式 Black-Scholes 接受已知连续收益率近似；美式生产定价仍受精度门控。两个模型都不近似离散现金股息日程。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DividendAssumption {
    /// No dividends during the modeled interval.
    /// 简体中文：模型区间内无股息。
    NoDividends,
    /// Continuously compounded annual dividend yield.
    /// 简体中文：连续复利年化股息收益率。
    ContinuousYield(f64),
    /// A discrete schedule is present or has not been established.
    /// 简体中文：存在离散股息计划，或尚未确认股息假设。
    DiscreteScheduleUnknown,
}

/// Evidence describing the dividend model that applies throughout an exact
/// valuation-to-expiry interval.
/// 简体中文：描述精确估值至到期间适用股息模型的证据。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DividendCoverageKind {
    /// The source explicitly reports no discrete cash dividend in the interval.
    /// 简体中文：来源明确报告该区间内没有离散现金股息。
    NoCashDividendInWindow,
    /// The source explicitly supplies a continuous-yield approximation for the interval.
    /// 简体中文：来源明确提供该区间的连续收益率近似。
    ContinuousYieldApproximation(f64),
    /// A discrete cash-dividend event occurs within the interval.
    /// 简体中文：该区间内存在离散现金股息事件。
    DiscreteCashDividendInWindow,
    /// The source does not establish the dividend schedule for the interval.
    /// 简体中文：来源未能确认该区间的股息日程。
    Unknown,
}

/// Bounded provider evidence for an underlying's dividend assumption.
/// Its source reference and revision are provenance only; this type does not
/// qualify the contract or authenticate the provider response.
/// 简体中文：标的股息假设的有界供应商证据。来源引用和版本仅记录来源；本类型不负责核验合约或认证供应商响应。
#[derive(Clone, Debug, PartialEq)]
pub struct DividendWindowEvidence {
    underlying: Underlying,
    source: ProviderMetadataRef,
    revision: u64,
    observed_at_ms: i64,
    effective_from_ms: i64,
    effective_until_ms: i64,
    coverage: DividendCoverageKind,
}

impl DividendWindowEvidence {
    /// Creates provider-linked evidence with a nonzero revision and ordered
    /// effective UTC epoch-millisecond window. Only market-data metadata
    /// references are accepted.
    /// 简体中文：创建绑定供应商记录的证据，要求版本非零、UTC Unix 毫秒有效窗口有序，且来源引用属于行情元数据。
    pub fn new(
        underlying: Underlying,
        source: ProviderMetadataRef,
        revision: u64,
        observed_at_ms: i64,
        effective_from_ms: i64,
        effective_until_ms: i64,
        coverage: DividendCoverageKind,
    ) -> Result<Self, DividendEvidenceError> {
        if revision == 0 {
            return Err(DividendEvidenceError::InvalidRevision);
        }
        if effective_from_ms < 0 || effective_until_ms < effective_from_ms || observed_at_ms < 0 {
            return Err(DividendEvidenceError::InvalidWindow);
        }
        if source.kind() != ProviderMetadataKind::MarketData
            || !matches!(source.source(), MetadataSource::MarketData(_))
        {
            return Err(DividendEvidenceError::UnsupportedSource);
        }
        if let DividendCoverageKind::ContinuousYieldApproximation(value) = coverage
            && (!value.is_finite() || value.abs() > MAX_MODEL_RATE_ABS)
        {
            return Err(DividendEvidenceError::ContinuousYieldOutOfRange);
        }
        Ok(Self {
            underlying,
            source,
            revision,
            observed_at_ms,
            effective_from_ms,
            effective_until_ms,
            coverage,
        })
    }

    /// Returns the typed underlying identity attached by the adapter.
    /// 简体中文：返回适配器绑定的类型化标的身份。
    pub const fn underlying(&self) -> &Underlying {
        &self.underlying
    }

    /// Returns the provider metadata reference for this evidence.
    /// 简体中文：返回该证据对应的供应商元数据引用。
    pub const fn source(&self) -> &ProviderMetadataRef {
        &self.source
    }

    /// Returns the provider revision attached to the evidence.
    /// 简体中文：返回证据携带的供应商版本。
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns when the evidence was observed, in UTC epoch milliseconds.
    /// 简体中文：返回证据观测时刻，单位为 UTC Unix 毫秒。
    pub const fn observed_at_ms(&self) -> i64 {
        self.observed_at_ms
    }

    /// Returns the inclusive start of the effective UTC interval.
    /// 简体中文：返回 UTC 有效区间的包含起点。
    pub const fn effective_from_ms(&self) -> i64 {
        self.effective_from_ms
    }

    /// Returns the inclusive end of the effective UTC interval.
    /// 简体中文：返回 UTC 有效区间的包含终点。
    pub const fn effective_until_ms(&self) -> i64 {
        self.effective_until_ms
    }

    /// Returns the explicit coverage semantics.
    /// 简体中文：返回显式的覆盖语义。
    pub const fn coverage(&self) -> DividendCoverageKind {
        self.coverage
    }
}

/// Invalid dividend evidence rejected before model admission.
/// 简体中文：模型接收之前拒绝的无效股息证据。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DividendEvidenceError {
    /// Provider revision zero is reserved for missing evidence.
    /// 简体中文：供应商版本零保留表示证据缺失。
    InvalidRevision,
    /// The observation or effective window is negative or reversed.
    /// 简体中文：观测时间或有效窗口为负数或顺序倒置。
    InvalidWindow,
    /// The source reference is not a market-data record.
    /// 简体中文：来源引用不是行情数据记录。
    UnsupportedSource,
    /// The continuous yield is non-finite or outside the model bound.
    /// 简体中文：连续收益率非有限或超出模型范围。
    ContinuousYieldOutOfRange,
}

impl std::fmt::Display for DividendEvidenceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidRevision => "DIVIDEND_EVIDENCE_REVISION_INVALID",
            Self::InvalidWindow => "DIVIDEND_EVIDENCE_WINDOW_INVALID",
            Self::UnsupportedSource => "DIVIDEND_EVIDENCE_SOURCE_UNSUPPORTED",
            Self::ContinuousYieldOutOfRange => "DIVIDEND_EVIDENCE_YIELD_OUT_OF_RANGE",
        })
    }
}

impl std::error::Error for DividendEvidenceError {}

/// Validated model inputs. Financial prices stay exact in the public API and
/// are converted to binary floating point only inside this non-authoritative
/// analytical solver.
/// 简体中文：已验证的模型输入。公开 API 中的金融价格保持精确类型，只在非权威解析求解器内部转换为二进制浮点数。
#[derive(Clone, Debug, PartialEq)]
pub struct SolverInput {
    option_kind: OptionKind,
    underlying: Underlying,
    spot: Price,
    strike: Strike,
    option_price: Price,
    years_to_expiry: f64,
    risk_free_rate: f64,
    exercise_style: ExerciseStyle,
    expiration: ExpirationContext,
    dividend_assumption: DividendAssumption,
    dividend_evidence: Option<DividendWindowEvidence>,
    observation: ObservationWindow,
    checked_at_ms: i64,
}

impl SolverInput {
    /// Creates validated inputs. Time to expiry is derived from the explicit
    /// UTC instants in `expiration` using ACT/365F; rates and dividend yields
    /// use continuously compounded annual units. `checked_at_ms` must equal the
    /// valuation instant. Exact expiry is retained for intrinsic-only output.
    /// `observed_at_ms` and `checked_at_ms` are UTC epoch milliseconds.
    /// The resulting input is analytical evidence only and does not authorize trading.
    /// 简体中文：创建并验证求解器输入。到期时间根据 `expiration` 中的 UTC 时刻按 ACT/365F 派生；利率和股息收益率使用连续复利年化单位。`checked_at_ms` 必须等于估值时刻。精确到期时间仅允许输出内在价值。观测和检查时刻使用 UTC Unix 毫秒；模型输入不构成交易授权。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        option_kind: OptionKind,
        underlying: Underlying,
        spot: Price,
        strike: Strike,
        option_price: Price,
        risk_free_rate: f64,
        exercise_style: ExerciseStyle,
        expiration: ExpirationContext,
        dividend_assumption: DividendAssumption,
        dividend_evidence: Option<DividendWindowEvidence>,
        observed_at_ms: i64,
        checked_at_ms: i64,
        max_age_ms: u64,
    ) -> Result<Self, SolverInputError> {
        let observation = ObservationWindow::new(observed_at_ms, checked_at_ms, max_age_ms)
            .map_err(SolverInputError::Freshness)?;
        if checked_at_ms != expiration.valuation.utc_epoch_millis {
            return Err(SolverInputError::ValuationInstantMismatch);
        }
        let remaining_millis = expiration
            .expiration
            .utc_epoch_millis
            .checked_sub(checked_at_ms)
            .ok_or(SolverInputError::TimeOutOfRange)?;
        if remaining_millis < 0 {
            return Err(SolverInputError::Expired);
        }
        if expiration.classification == ExpirationClass::ZeroDaysToExpiry
            && remaining_millis > MAX_ZERO_DTE_REMAINING_MS
        {
            return Err(SolverInputError::ExpirationContextMismatch);
        }
        let years_to_expiry = remaining_millis as f64 / MILLIS_PER_YEAR_ACT_365F;
        if !years_to_expiry.is_finite() || years_to_expiry > MAX_MODEL_TIME_YEARS {
            return Err(SolverInputError::TimeOutOfRange);
        }
        if !risk_free_rate.is_finite() || risk_free_rate.abs() > MAX_MODEL_RATE_ABS {
            return Err(SolverInputError::RateOutOfRange);
        }
        if let DividendAssumption::ContinuousYield(yield_rate) = dividend_assumption
            && (!yield_rate.is_finite() || yield_rate.abs() > MAX_MODEL_RATE_ABS)
        {
            return Err(SolverInputError::DividendOutOfRange);
        }
        if remaining_millis > 0 {
            match (dividend_assumption, dividend_evidence.as_ref()) {
                (DividendAssumption::NoDividends, None)
                | (DividendAssumption::ContinuousYield(_), None) => {
                    return Err(SolverInputError::DividendEvidenceMissing);
                }
                (DividendAssumption::NoDividends, Some(evidence))
                    if evidence.coverage != DividendCoverageKind::NoCashDividendInWindow =>
                {
                    return Err(SolverInputError::DividendEvidenceMismatch);
                }
                (DividendAssumption::ContinuousYield(yield_rate), Some(evidence))
                    if evidence.coverage
                        != DividendCoverageKind::ContinuousYieldApproximation(yield_rate) =>
                {
                    return Err(SolverInputError::DividendEvidenceMismatch);
                }
                _ => {}
            }
            if let Some(evidence) = dividend_evidence.as_ref() {
                if evidence.underlying != underlying {
                    return Err(SolverInputError::DividendUnderlyingMismatch);
                }
                if evidence.observed_at_ms > checked_at_ms
                    || checked_at_ms.saturating_sub(evidence.observed_at_ms)
                        > i64::try_from(max_age_ms).unwrap_or(i64::MAX)
                {
                    return Err(SolverInputError::DividendEvidenceStale);
                }
                if evidence.effective_from_ms > checked_at_ms
                    || evidence.effective_until_ms < expiration.expiration.utc_epoch_millis
                {
                    return Err(SolverInputError::DividendEvidenceDoesNotCoverModelInterval);
                }
            }
        }

        let spot_value = exact_price_to_f64(spot);
        let strike_value = exact_strike_to_f64(strike);
        let option_price_value = exact_price_to_f64(option_price);
        if !spot_value.is_finite()
            || spot_value <= 0.0
            || spot_value > MAX_MODEL_PRICE
            || !strike_value.is_finite()
            || strike_value <= 0.0
            || strike_value > MAX_MODEL_PRICE
            || !option_price_value.is_finite()
            || option_price_value < 0.0
            || option_price_value > MAX_MODEL_PRICE
        {
            return Err(SolverInputError::PriceOutOfRange);
        }

        Ok(Self {
            option_kind,
            underlying,
            spot,
            strike,
            option_price,
            years_to_expiry,
            risk_free_rate,
            exercise_style,
            expiration,
            dividend_assumption,
            dividend_evidence,
            observation,
            checked_at_ms,
        })
    }

    /// Returns the UTC Unix-millisecond market observation timestamp.
    /// 简体中文：返回 UTC Unix 毫秒行情观测时间。
    pub const fn observed_at_ms(&self) -> i64 {
        self.observation.observed_at_ms()
    }

    /// Returns the maximum accepted option observation age in milliseconds.
    /// 返回允许的期权行情最大观测年龄（毫秒）。
    pub const fn max_age_ms(&self) -> u64 {
        self.observation.max_age_ms()
    }

    /// Revalidates quote and dividend evidence at the supplied evaluation time.
    /// A future or over-age dividend observation fails closed.
    /// 简体中文：按给定的评估时刻重新核验行情与股息证据；未来或超龄的股息观测会被拒绝。
    pub fn validate_fresh_at(&self, checked_at_ms: i64) -> Result<(), SolverInputError> {
        if checked_at_ms < self.expiration.valuation.utc_epoch_millis {
            return Err(SolverInputError::ValuationInstantMismatch);
        }
        if checked_at_ms > self.expiration.expiration.utc_epoch_millis
            || (checked_at_ms == self.expiration.expiration.utc_epoch_millis
                && checked_at_ms > self.expiration.valuation.utc_epoch_millis)
        {
            return Err(SolverInputError::Expired);
        }
        self.observation
            .validate_at(checked_at_ms)
            .map_err(SolverInputError::Freshness)?;
        if let Some(evidence) = self.dividend_evidence.as_ref() {
            let max_age_ms = i64::try_from(self.observation.max_age_ms()).unwrap_or(i64::MAX);
            let evidence_age_ms = checked_at_ms
                .checked_sub(evidence.observed_at_ms)
                .ok_or(SolverInputError::DividendEvidenceStale)?;
            if evidence_age_ms < 0 || evidence_age_ms > max_age_ms {
                return Err(SolverInputError::DividendEvidenceStale);
            }
            if evidence.effective_from_ms > checked_at_ms
                || evidence.effective_until_ms < self.expiration.expiration.utc_epoch_millis
            {
                return Err(SolverInputError::DividendEvidenceDoesNotCoverModelInterval);
            }
        }
        Ok(())
    }

    /// Returns the immutable UTC epoch-millisecond valuation instant.
    /// 简体中文：返回不可变的 UTC Unix 毫秒估值时刻。
    pub const fn valuation_at_ms(&self) -> i64 {
        self.expiration.valuation.utc_epoch_millis
    }

    /// Returns the ACT/365F year fraction derived from the exact expiry instants.
    /// 简体中文：返回从精确到期时刻派生的 ACT/365F 年分数。
    pub const fn years_to_expiry(&self) -> f64 {
        self.years_to_expiry
    }

    /// Returns the typed underlying identity supplied with this option input.
    /// 简体中文：返回随期权输入提供的类型化标的身份。
    pub const fn underlying(&self) -> &Underlying {
        &self.underlying
    }

    /// Returns caller-provided dividend evidence, if present. This evidence
    /// does not qualify a broker contract or authenticate a provider response.
    /// 简体中文：返回调用方提供的股息证据（如有）。该证据不核验券商合约，也不认证供应商响应。
    pub const fn dividend_evidence(&self) -> Option<&DividendWindowEvidence> {
        self.dividend_evidence.as_ref()
    }

    /// Returns the expiry class validated against the exchange-local dates.
    /// 简体中文：返回已按交易所本地日期核验的到期分类。
    pub const fn expiration_class(&self) -> ExpirationClass {
        self.expiration.classification
    }

    pub(crate) const fn spot(&self) -> Price {
        self.spot
    }

    pub(crate) const fn option_price(&self) -> Price {
        self.option_price
    }

    pub(crate) fn matches_contract(&self, contract: &OptionContract) -> bool {
        self.strike == contract.symbol().strike()
            && self.underlying == *contract.symbol().underlying()
            && matches!(
                (self.option_kind, contract.symbol().right()),
                (OptionKind::Call, OptionRight::Call) | (OptionKind::Put, OptionRight::Put)
            )
    }

    pub(crate) fn fingerprint(&self) -> SolverInputFingerprint {
        SolverInputFingerprint {
            option_kind: self.option_kind,
            underlying: self.underlying.clone(),
            spot: self.spot,
            strike: self.strike,
            option_price: self.option_price,
            years_to_expiry_bits: canonical_float_bits(self.years_to_expiry),
            risk_free_rate_bits: canonical_float_bits(self.risk_free_rate),
            exercise_style: self.exercise_style,
            expiration_class: self.expiration.classification,
            expiration_timezone_id: self.expiration.timezone_id.clone(),
            valuation_instant: InstantFingerprint::from(self.expiration.valuation),
            expiration_instant: InstantFingerprint::from(self.expiration.expiration),
            valuation_at_ms: self.checked_at_ms,
            dividend_assumption: DividendFingerprint::from(self.dividend_assumption),
            dividend_evidence: self
                .dividend_evidence
                .as_ref()
                .map(DividendEvidenceFingerprint::from),
            observed_at_ms: self.observation.observed_at_ms(),
            max_age_ms: self.observation.max_age_ms(),
        }
    }

    pub(crate) fn values(&self) -> (f64, f64, f64) {
        (
            exact_price_to_f64(self.spot),
            exact_strike_to_f64(self.strike),
            exact_price_to_f64(self.option_price),
        )
    }

    pub(crate) const fn is_at_expiry(&self) -> bool {
        self.years_to_expiry == 0.0
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct SolverInputFingerprint {
    option_kind: OptionKind,
    underlying: Underlying,
    spot: Price,
    strike: Strike,
    option_price: Price,
    years_to_expiry_bits: u64,
    risk_free_rate_bits: u64,
    exercise_style: ExerciseStyle,
    expiration_class: ExpirationClass,
    expiration_timezone_id: String,
    valuation_instant: InstantFingerprint,
    expiration_instant: InstantFingerprint,
    valuation_at_ms: i64,
    dividend_assumption: DividendFingerprint,
    dividend_evidence: Option<DividendEvidenceFingerprint>,
    observed_at_ms: i64,
    max_age_ms: u64,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct InstantFingerprint {
    utc_epoch_millis: i64,
    utc_offset_minutes: i16,
    local_day_ordinal: i64,
}

impl From<ExchangeInstant> for InstantFingerprint {
    fn from(value: ExchangeInstant) -> Self {
        Self {
            utc_epoch_millis: value.utc_epoch_millis(),
            utc_offset_minutes: value.utc_offset_minutes(),
            local_day_ordinal: value.local_day_ordinal(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum DividendFingerprint {
    NoDividends,
    ContinuousYield(u64),
    DiscreteScheduleUnknown,
}

impl From<DividendAssumption> for DividendFingerprint {
    fn from(value: DividendAssumption) -> Self {
        match value {
            DividendAssumption::NoDividends => Self::NoDividends,
            DividendAssumption::ContinuousYield(value) => {
                Self::ContinuousYield(canonical_float_bits(value))
            }
            DividendAssumption::DiscreteScheduleUnknown => Self::DiscreteScheduleUnknown,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct DividendEvidenceFingerprint {
    underlying: Underlying,
    source: ProviderMetadataRef,
    revision: u64,
    observed_at_ms: i64,
    effective_from_ms: i64,
    effective_until_ms: i64,
    coverage: DividendCoverageFingerprint,
}

impl From<&DividendWindowEvidence> for DividendEvidenceFingerprint {
    fn from(value: &DividendWindowEvidence) -> Self {
        Self {
            underlying: value.underlying.clone(),
            source: value.source.clone(),
            revision: value.revision,
            observed_at_ms: value.observed_at_ms,
            effective_from_ms: value.effective_from_ms,
            effective_until_ms: value.effective_until_ms,
            coverage: DividendCoverageFingerprint::from(value.coverage),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum DividendCoverageFingerprint {
    NoCashDividendInWindow,
    ContinuousYieldApproximation(u64),
    DiscreteCashDividendInWindow,
    Unknown,
}

impl From<DividendCoverageKind> for DividendCoverageFingerprint {
    fn from(value: DividendCoverageKind) -> Self {
        match value {
            DividendCoverageKind::NoCashDividendInWindow => Self::NoCashDividendInWindow,
            DividendCoverageKind::ContinuousYieldApproximation(value) => {
                Self::ContinuousYieldApproximation(canonical_float_bits(value))
            }
            DividendCoverageKind::DiscreteCashDividendInWindow => {
                Self::DiscreteCashDividendInWindow
            }
            DividendCoverageKind::Unknown => Self::Unknown,
        }
    }
}

fn canonical_float_bits(value: f64) -> u64 {
    if value == 0.0 { 0 } else { value.to_bits() }
}

/// Model limitation or convergence outcome. These are unavailable analytical
/// results, not quote or execution errors.
/// 简体中文：模型假设、边界或收敛方面的不可用结果；它们不是行情或执行错误。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelUnavailable {
    /// American exercise is unsupported by the European Black-Scholes model.
    /// 简体中文：欧式 Black-Scholes 模型不支持美式行权。
    AmericanExerciseUnsupported,
    /// The declared dividend assumption is not supported.
    /// 简体中文：声明的股息假设不受支持。
    DividendAssumptionUnsupported,
    /// A price lies outside the model's supported no-arbitrage or value bounds.
    /// 简体中文：价格超出模型支持的无套利条件或数值范围。
    PriceOutsideModelBounds,
    /// The bounded search did not meet its convergence tolerance.
    /// 简体中文：有界搜索未达到收敛容差。
    NonConvergent,
    /// Model price changes too little with volatility to identify implied volatility.
    /// 简体中文：模型价格对波动率变化不敏感，无法可靠识别隐含波动率。
    LowVega,
    /// A bounded finite-difference Greek failed its numerical stability check.
    /// 简体中文：有界有限差分 Greek 未通过数值稳定性检查。
    UnstableGreeks,
    /// An intermediate or output calculation exceeded its finite bounds.
    /// 简体中文：中间计算或输出超出有限数值范围。
    CalculationOutOfRange,
    /// The market observation is stale at calculation time.
    /// 简体中文：计算时行情观测已过期。
    StaleInput,
    /// Dividend evidence is future-dated or stale at calculation time.
    /// 简体中文：计算时股息证据来自未来或已过期。
    DividendEvidenceStale,
    /// The contract expired before the model was evaluated.
    /// 简体中文：模型计算前合约已经到期。
    Expired,
    /// Remaining time is below the American tree's documented one-minute resolution.
    /// 简体中文：剩余时间短于美式二叉树规定的一分钟分辨率。
    TimeBelowResolution,
    /// American price and implied-volatility accuracy have not passed an independent oracle gate.
    /// 简体中文：美式价格和隐含波动率尚未通过独立 oracle 精度验收。
    AmericanPricingAccuracyUnverified,
}

/// Solver result. Unsupported conditions never fall back to guessed inputs.
/// 简体中文：求解结果。不支持的条件不会退化为猜测输入或近似模型。
#[derive(Clone, Debug, PartialEq)]
pub enum SolverOutcome {
    /// The solver returned validated model metrics.
    /// 简体中文：求解器返回了经过验证的模型指标。
    Available(OptionMetrics),
    /// Exact expiry has no time value or defined model Greeks; only intrinsic
    /// value is returned using exact domain money types.
    /// 简体中文：精确到期时没有时间价值且模型 Greeks 未定义；只使用领域精确金额类型返回内在价值。
    AtExpiryIntrinsic(Price),
    /// The model could not produce a supported result.
    /// 简体中文：模型无法生成受支持的结果。
    Unavailable(ModelUnavailable),
}

/// Solve a bounded European Black-Scholes implied volatility and its model
/// Greeks. Controls are fixed at 96 bisection iterations, volatility `[0.0001,
/// 5.0]`, and a `1e-10` option-price tolerance. European exercise and a known
/// continuous dividend yield are required. The caller supplies consistent
/// expiry instants; same-day options use ACT/365F elapsed time, and exact expiry
/// returns intrinsic value without Greeks. `evaluation_at_ms` is the explicit
/// UTC epoch-millisecond time used to recheck quote and dividend evidence.
/// Returned metrics are estimates, not execution or risk authority.
/// 简体中文：求解有界的欧式 Black-Scholes 隐含波动率及模型 Greeks。参数固定为 96 次二分迭代、波动率 `[0.0001, 5.0]` 和 `1e-10` 期权价格容差；必须显式声明欧式行权和经过证据确认的连续股息窗口。当日到期使用按 UTC 时刻派生的 ACT/365F 剩余时间；精确到期只返回内在价值且不返回 Greeks。`evaluation_at_ms` 是重新核验行情和股息证据的显式 UTC Unix 毫秒时刻。指标是估算，不是成交价或执行/风控权威数据。
pub fn solve_european_black_scholes(input: &SolverInput, evaluation_at_ms: i64) -> SolverOutcome {
    solve_european_black_scholes_with_limit(input, evaluation_at_ms, MAX_ITERATIONS)
}

fn solve_european_black_scholes_with_limit(
    input: &SolverInput,
    evaluation_at_ms: i64,
    iteration_limit: usize,
) -> SolverOutcome {
    if let Err(error) = input.validate_fresh_at(evaluation_at_ms) {
        return input_error_outcome(error);
    }
    if input.is_at_expiry() {
        return expiry_intrinsic(input);
    }
    if input.exercise_style == ExerciseStyle::American {
        return SolverOutcome::Unavailable(ModelUnavailable::AmericanExerciseUnsupported);
    }
    let dividend_yield = match input.dividend_assumption {
        DividendAssumption::NoDividends => 0.0,
        DividendAssumption::ContinuousYield(yield_rate) => yield_rate,
        DividendAssumption::DiscreteScheduleUnknown => {
            return SolverOutcome::Unavailable(ModelUnavailable::DividendAssumptionUnsupported);
        }
    };

    let (spot, strike, market_price) = input.values();
    let discounted_spot = spot * (-dividend_yield * input.years_to_expiry).exp();
    let discounted_strike = strike * (-input.risk_free_rate * input.years_to_expiry).exp();
    let intrinsic = match input.option_kind {
        OptionKind::Call => (discounted_spot - discounted_strike).max(0.0),
        OptionKind::Put => (discounted_strike - discounted_spot).max(0.0),
    };
    let upper_bound = match input.option_kind {
        OptionKind::Call => discounted_spot,
        OptionKind::Put => discounted_strike,
    };
    if !discounted_spot.is_finite()
        || !discounted_strike.is_finite()
        || !intrinsic.is_finite()
        || market_price + PRICE_TOLERANCE < intrinsic
        || market_price > upper_bound + PRICE_TOLERANCE
    {
        return SolverOutcome::Unavailable(ModelUnavailable::PriceOutsideModelBounds);
    }

    let min_price = black_scholes_price(
        input.option_kind,
        spot,
        strike,
        input.risk_free_rate,
        dividend_yield,
        input.years_to_expiry,
        MIN_VOLATILITY,
    );
    let max_price = black_scholes_price(
        input.option_kind,
        spot,
        strike,
        input.risk_free_rate,
        dividend_yield,
        input.years_to_expiry,
        MAX_VOLATILITY,
    );
    if !min_price.is_finite()
        || !max_price.is_finite()
        || market_price < min_price - PRICE_TOLERANCE
        || market_price > max_price + PRICE_TOLERANCE
    {
        return SolverOutcome::Unavailable(ModelUnavailable::PriceOutsideModelBounds);
    }

    let volatility = match implied_volatility_with_limit(
        input,
        spot,
        strike,
        market_price,
        dividend_yield,
        iteration_limit,
    ) {
        Some(value) => value,
        None => return SolverOutcome::Unavailable(ModelUnavailable::NonConvergent),
    };
    let Some(greeks) = calculate_greeks(input, spot, strike, volatility, dividend_yield) else {
        return SolverOutcome::Unavailable(ModelUnavailable::CalculationOutOfRange);
    };

    let observed_at = input.observed_at_ms();
    let now = evaluation_at_ms;
    let max_age = input.observation.max_age_ms();
    let make = |kind, value, unit| {
        MetricObservation::new(
            kind,
            value,
            unit,
            MetricSource::BlackScholesEuropeanV2,
            MetricQuality::ModelEstimate,
            observed_at,
            now,
            max_age,
        )
    };
    let metrics = (|| -> Result<OptionMetrics, MetricsError> {
        OptionMetrics::new(
            Some(make(
                MetricKind::ImpliedVolatility,
                volatility,
                MetricUnit::ImpliedVolatilityFraction,
            )?),
            Some(make(
                MetricKind::Delta,
                greeks.delta,
                MetricUnit::DeltaPerUnderlyingUnit,
            )?),
            Some(make(
                MetricKind::Gamma,
                greeks.gamma,
                MetricUnit::GammaPerUnderlyingDollarSquared,
            )?),
            Some(make(
                MetricKind::Theta,
                greeks.theta,
                MetricUnit::ThetaPerYearFractionPerUnderlyingUnit,
            )?),
            Some(make(
                MetricKind::Vega,
                greeks.vega,
                MetricUnit::VegaPerVolatilityUnitPerUnderlyingUnit,
            )?),
            Some(make(
                MetricKind::Rho,
                greeks.rho,
                MetricUnit::RhoPerRateUnitPerUnderlyingUnit,
            )?),
        )
    })();
    match metrics {
        Ok(metrics) => SolverOutcome::Available(metrics),
        Err(_) => SolverOutcome::Unavailable(ModelUnavailable::CalculationOutOfRange),
    }
}

pub(crate) fn input_error_outcome(error: SolverInputError) -> SolverOutcome {
    SolverOutcome::Unavailable(match error {
        SolverInputError::Expired => ModelUnavailable::Expired,
        SolverInputError::DividendEvidenceStale => ModelUnavailable::DividendEvidenceStale,
        _ => ModelUnavailable::StaleInput,
    })
}

/// Compute the explicitly declared model for the immutable snapshot valued at
/// `input.valuation_at_ms()`; its model price and remaining-time horizon stay
/// anchored to that valuation instant. `evaluation_at_ms` does not advance the
/// valuation or reprice the snapshot. It is used only to revalidate quote and
/// dividend freshness/window, expiry, and whether the result is still valid to
/// return. Positive-time American results remain unavailable pending
/// independent accuracy acceptance; the bounded CRR candidate is test-only.
/// Exact-expiry intrinsic output requires both instants to equal contract expiry.
///
/// ## 简体中文
///
/// 求解器针对 `input.valuation_at_ms()` 所确定的不可变快照计算；模型价格与剩余期限始终锚定该估值时刻。`evaluation_at_ms` 不会推进估值时刻，也不会重新定价快照，只用于重新核验行情和股息的新鲜度/窗口、过期状态以及结果是否仍可交付。正剩余时间的美式结果在独立精度验收通过前仍不可用；有界 CRR 候选仅编译于测试。只有估值时刻和检查时刻都恰好等于合约到期时，才可返回到期内在价值。
///
/// `evaluation_at_ms` uses UTC epoch milliseconds.
/// 简体中文：`evaluation_at_ms` 使用 UTC Unix 毫秒。
pub fn solve_option_model(input: &SolverInput, evaluation_at_ms: i64) -> SolverOutcome {
    match input.exercise_style {
        ExerciseStyle::European => solve_european_black_scholes(input, evaluation_at_ms),
        ExerciseStyle::American => solve_american_crr(input, evaluation_at_ms),
    }
}

fn expiry_intrinsic(input: &SolverInput) -> SolverOutcome {
    let (spot, strike, _) = input.values();
    let value = match input.option_kind {
        OptionKind::Call if spot > strike => input
            .spot
            .decimal()
            .checked_sub(input.strike.decimal())
            .ok()
            .and_then(|amount| Price::new(amount).ok()),
        OptionKind::Put if strike > spot => input
            .strike
            .decimal()
            .checked_sub(input.spot.decimal())
            .ok()
            .and_then(|amount| Price::new(amount).ok()),
        _ => Some(Price::ZERO),
    };
    value.map_or(
        SolverOutcome::Unavailable(ModelUnavailable::CalculationOutOfRange),
        SolverOutcome::AtExpiryIntrinsic,
    )
}

fn implied_volatility_with_limit(
    input: &SolverInput,
    spot: f64,
    strike: f64,
    market_price: f64,
    dividend_yield: f64,
    iteration_limit: usize,
) -> Option<f64> {
    let mut low = MIN_VOLATILITY;
    let mut high = MAX_VOLATILITY;
    for _ in 0..iteration_limit {
        let mid = low + (high - low) / 2.0;
        let model_price = black_scholes_price(
            input.option_kind,
            spot,
            strike,
            input.risk_free_rate,
            dividend_yield,
            input.years_to_expiry,
            mid,
        );
        if !model_price.is_finite() {
            return None;
        }
        let difference = model_price - market_price;
        if difference.abs() <= PRICE_TOLERANCE {
            return Some(mid);
        }
        if difference < 0.0 {
            low = mid;
        } else {
            high = mid;
        }
    }
    let candidate = low + (high - low) / 2.0;
    let price = black_scholes_price(
        input.option_kind,
        spot,
        strike,
        input.risk_free_rate,
        dividend_yield,
        input.years_to_expiry,
        candidate,
    );
    if price.is_finite() && (price - market_price).abs() <= PRICE_TOLERANCE * 10.0 {
        Some(candidate)
    } else {
        None
    }
}

#[derive(Clone, Copy)]
struct ModelGreeks {
    delta: f64,
    gamma: f64,
    theta: f64,
    vega: f64,
    rho: f64,
}

fn calculate_greeks(
    input: &SolverInput,
    spot: f64,
    strike: f64,
    volatility: f64,
    dividend_yield: f64,
) -> Option<ModelGreeks> {
    let time_sqrt = input.years_to_expiry.sqrt();
    let sigma_sqrt_time = volatility * time_sqrt;
    let d1 = ((spot / strike).ln()
        + (input.risk_free_rate - dividend_yield + 0.5 * volatility * volatility)
            * input.years_to_expiry)
        / sigma_sqrt_time;
    let d2 = d1 - sigma_sqrt_time;
    let discounted_spot = spot * (-dividend_yield * input.years_to_expiry).exp();
    let discounted_strike = strike * (-input.risk_free_rate * input.years_to_expiry).exp();
    let density = normal_density(d1);
    let delta = match input.option_kind {
        OptionKind::Call => (-dividend_yield * input.years_to_expiry).exp() * normal_cdf(d1),
        OptionKind::Put => (-dividend_yield * input.years_to_expiry).exp() * (normal_cdf(d1) - 1.0),
    };
    let gamma =
        (-dividend_yield * input.years_to_expiry).exp() * density / (spot * sigma_sqrt_time);
    let theta_diffusion = -discounted_spot * density * volatility / (2.0 * time_sqrt);
    let (theta, rho) = match input.option_kind {
        OptionKind::Call => (
            theta_diffusion - input.risk_free_rate * discounted_strike * normal_cdf(d2)
                + dividend_yield * discounted_spot * normal_cdf(d1),
            strike
                * input.years_to_expiry
                * (-input.risk_free_rate * input.years_to_expiry).exp()
                * normal_cdf(d2),
        ),
        OptionKind::Put => (
            theta_diffusion + input.risk_free_rate * discounted_strike * normal_cdf(-d2)
                - dividend_yield * discounted_spot * normal_cdf(-d1),
            -strike
                * input.years_to_expiry
                * (-input.risk_free_rate * input.years_to_expiry).exp()
                * normal_cdf(-d2),
        ),
    };
    let vega = discounted_spot * density * time_sqrt;
    let result = ModelGreeks {
        delta,
        gamma,
        theta,
        vega,
        rho,
    };
    [
        result.delta,
        result.gamma,
        result.theta,
        result.vega,
        result.rho,
    ]
    .iter()
    .all(|value| value.is_finite())
    .then_some(result)
}

fn black_scholes_price(
    option_kind: OptionKind,
    spot: f64,
    strike: f64,
    risk_free_rate: f64,
    dividend_yield: f64,
    years_to_expiry: f64,
    volatility: f64,
) -> f64 {
    let sigma_sqrt_time = volatility * years_to_expiry.sqrt();
    let d1 = ((spot / strike).ln()
        + (risk_free_rate - dividend_yield + 0.5 * volatility * volatility) * years_to_expiry)
        / sigma_sqrt_time;
    let d2 = d1 - sigma_sqrt_time;
    let discounted_spot = spot * (-dividend_yield * years_to_expiry).exp();
    let discounted_strike = strike * (-risk_free_rate * years_to_expiry).exp();
    match option_kind {
        OptionKind::Call => discounted_spot * normal_cdf(d1) - discounted_strike * normal_cdf(d2),
        OptionKind::Put => discounted_strike * normal_cdf(-d2) - discounted_spot * normal_cdf(-d1),
    }
}

// Abramowitz-Stegun 7.1.26 approximation, bounded and deterministic. It is
// adequate for indicative Greeks and never drives a money threshold.
fn normal_cdf(value: f64) -> f64 {
    let absolute = value.abs();
    let t = 1.0 / (1.0 + 0.231_641_9 * absolute);
    let polynomial = t
        * (0.319_381_530
            + t * (-0.356_563_782
                + t * (1.781_477_937 + t * (-1.821_255_978 + t * 1.330_274_429))));
    let tail = normal_density(absolute) * polynomial;
    if value >= 0.0 { 1.0 - tail } else { tail }
}

fn normal_density(value: f64) -> f64 {
    (-0.5 * value * value).exp() / (2.0 * std::f64::consts::PI).sqrt()
}

fn exact_price_to_f64(value: Price) -> f64 {
    exact_decimal_to_f64(value.decimal())
}

fn exact_strike_to_f64(value: Strike) -> f64 {
    exact_decimal_to_f64(value.decimal())
}

fn exact_decimal_to_f64(value: domain::ExactDecimal) -> f64 {
    value.coefficient() as f64 / 10_f64.powi(value.scale() as i32)
}

/// Invalid model input rejected before worker admission.
/// 简体中文：在提交到 worker 前拒绝的无效模型输入。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SolverInputError {
    /// The source observation is invalid, future-dated, or stale.
    /// 简体中文：来源观测无效、时间超前或已过期。
    Freshness(MetricsError),
    /// Derived time is unrepresentable or exceeds the model bound.
    /// 简体中文：派生到期时间不可表示或超过模型上限。
    TimeOutOfRange,
    /// The valuation instant does not equal the checked model-input time.
    /// 简体中文：估值时刻与本次模型输入的检查时刻不一致。
    ValuationInstantMismatch,
    /// The contract expired before the model valuation instant.
    /// 简体中文：合约在模型估值时刻之前已经到期。
    Expired,
    /// Same-day class conflicts with the supported maximum remaining duration.
    /// 简体中文：当日到期分类与支持的最长剩余时间冲突。
    ExpirationContextMismatch,
    /// The risk-free rate is non-finite or outside the model bound.
    /// 简体中文：无风险利率非有限或超出模型上限。
    RateOutOfRange,
    /// The continuous dividend yield is non-finite or outside the model bound.
    /// 简体中文：连续股息收益率非有限或超出模型上限。
    DividendOutOfRange,
    /// A no-dividend or continuous-yield assumption has no provider-linked window evidence.
    /// 简体中文：无股息或连续收益率假设缺少绑定供应商的窗口证据。
    DividendEvidenceMissing,
    /// The evidence coverage does not match the declared dividend assumption.
    /// 简体中文：证据覆盖类型与声明的股息假设不匹配。
    DividendEvidenceMismatch,
    /// Evidence names a different underlying than the modeled option.
    /// 简体中文：证据标的与模型期权标的不一致。
    DividendUnderlyingMismatch,
    /// Dividend evidence is future-dated or older than the allowed freshness window.
    /// 简体中文：股息证据时间超前或超过允许的新鲜度窗口。
    DividendEvidenceStale,
    /// The evidence effective interval does not cover valuation through expiry.
    /// 简体中文：证据有效区间未覆盖从估值时刻到到期时刻的完整窗口。
    DividendEvidenceDoesNotCoverModelInterval,
    /// Spot, strike, or option price is outside the model's numeric bounds.
    /// 简体中文：标的价格、行权价或期权价格超出模型数值范围。
    PriceOutOfRange,
}

impl std::fmt::Display for SolverInputError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Freshness(_) => "SOLVER_INPUT_FRESHNESS_INVALID",
            Self::TimeOutOfRange => "SOLVER_INPUT_TIME_OUT_OF_RANGE",
            Self::ValuationInstantMismatch => "SOLVER_INPUT_VALUATION_INSTANT_MISMATCH",
            Self::Expired => "SOLVER_INPUT_EXPIRED",
            Self::ExpirationContextMismatch => "SOLVER_INPUT_EXPIRATION_CONTEXT_MISMATCH",
            Self::RateOutOfRange => "SOLVER_INPUT_RATE_OUT_OF_RANGE",
            Self::DividendOutOfRange => "SOLVER_INPUT_DIVIDEND_OUT_OF_RANGE",
            Self::DividendEvidenceMissing => "SOLVER_INPUT_DIVIDEND_EVIDENCE_MISSING",
            Self::DividendEvidenceMismatch => "SOLVER_INPUT_DIVIDEND_EVIDENCE_MISMATCH",
            Self::DividendUnderlyingMismatch => "SOLVER_INPUT_DIVIDEND_UNDERLYING_MISMATCH",
            Self::DividendEvidenceStale => "SOLVER_INPUT_DIVIDEND_EVIDENCE_STALE",
            Self::DividendEvidenceDoesNotCoverModelInterval => {
                "SOLVER_INPUT_DIVIDEND_EVIDENCE_INTERVAL_MISMATCH"
            }
            Self::PriceOutOfRange => "SOLVER_INPUT_PRICE_OUT_OF_RANGE",
        })
    }
}

impl std::error::Error for SolverInputError {}

#[cfg(test)]
mod tests {
    use domain::{
        MarketDataProviderId, MetadataSource, Price, ProviderMetadataKind, ProviderMetadataRef,
        ProviderRecordId, Strike, Underlying,
    };

    use super::{
        DividendAssumption, DividendCoverageKind, DividendWindowEvidence, ExchangeInstant,
        ExerciseStyle, ExpirationClass, ExpirationContext, ModelUnavailable, OptionKind,
        SolverInput, SolverOutcome, solve_european_black_scholes,
    };

    fn price(value: &str) -> Price {
        Price::parse_json_number(value).expect("valid price")
    }

    fn expiration_context(
        class: ExpirationClass,
        valuation_at_ms: i64,
        remaining_ms: i64,
    ) -> ExpirationContext {
        let expiration_at_ms = valuation_at_ms + remaining_ms;
        let valuation_day = valuation_at_ms.div_euclid(86_400_000);
        let expiration_day = expiration_at_ms.div_euclid(86_400_000);
        ExpirationContext::new(
            class,
            "UTC",
            ExchangeInstant::new(valuation_at_ms, 0, valuation_day).expect("valuation instant"),
            ExchangeInstant::new(expiration_at_ms, 0, expiration_day).expect("expiration instant"),
        )
        .expect("consistent expiry evidence")
    }

    fn dividend_evidence(
        underlying: &str,
        observed_at_ms: i64,
        effective_from_ms: i64,
        effective_until_ms: i64,
        coverage: DividendCoverageKind,
    ) -> DividendWindowEvidence {
        DividendWindowEvidence::new(
            Underlying::new(underlying).expect("underlying"),
            ProviderMetadataRef::new(
                MetadataSource::MarketData(MarketDataProviderId::Alpaca),
                ProviderMetadataKind::MarketData,
                ProviderRecordId::new("synthetic-dividend-window").expect("record id"),
            ),
            1,
            observed_at_ms,
            effective_from_ms,
            effective_until_ms,
            coverage,
        )
        .expect("synthetic dividend evidence")
    }

    fn solver_input(
        underlying: &str,
        expiration: ExpirationContext,
        dividends: DividendAssumption,
        evidence: Option<DividendWindowEvidence>,
        checked_at_ms: i64,
        option_price: &str,
        risk_free_rate: f64,
    ) -> Result<SolverInput, super::SolverInputError> {
        SolverInput::new(
            OptionKind::Call,
            Underlying::new(underlying).expect("underlying"),
            price("100"),
            Strike::parse_json_number("100").expect("valid strike"),
            price(option_price),
            risk_free_rate,
            ExerciseStyle::European,
            expiration,
            dividends,
            evidence,
            checked_at_ms,
            checked_at_ms,
            2_000,
        )
    }

    fn input(
        exercise: ExerciseStyle,
        expiration: ExpirationClass,
        dividends: DividendAssumption,
        option_price: &str,
    ) -> SolverInput {
        let checked_at_ms = 10_000;
        let remaining_ms = match expiration {
            ExpirationClass::ZeroDaysToExpiry => 60 * 60 * 1_000,
            ExpirationClass::NonZeroDaysToExpiry => 182 * 86_400_000 + 43_200_000,
        };
        let expiry_at_ms = checked_at_ms + remaining_ms;
        let dividend_evidence = match dividends {
            DividendAssumption::NoDividends => Some(dividend_evidence(
                "QQQ",
                checked_at_ms,
                checked_at_ms,
                expiry_at_ms,
                DividendCoverageKind::NoCashDividendInWindow,
            )),
            DividendAssumption::ContinuousYield(value) => Some(dividend_evidence(
                "QQQ",
                checked_at_ms,
                checked_at_ms,
                expiry_at_ms,
                DividendCoverageKind::ContinuousYieldApproximation(value),
            )),
            DividendAssumption::DiscreteScheduleUnknown => None,
        };
        let expiry = expiration_context(expiration, checked_at_ms, remaining_ms);
        if exercise == ExerciseStyle::European {
            solver_input(
                "QQQ",
                expiry,
                dividends,
                dividend_evidence,
                checked_at_ms,
                option_price,
                0.02,
            )
            .expect("valid solver input")
        } else {
            SolverInput::new(
                OptionKind::Call,
                Underlying::new("QQQ").expect("underlying"),
                price("100"),
                Strike::parse_json_number("100").expect("valid strike"),
                price(option_price),
                0.02,
                exercise,
                expiry,
                dividends,
                dividend_evidence,
                checked_at_ms,
                checked_at_ms,
                2_000,
            )
            .expect("valid solver input")
        }
    }

    #[test]
    fn solves_european_no_dividend_iv_and_records_explicit_model_units() {
        let input = input(
            ExerciseStyle::European,
            ExpirationClass::NonZeroDaysToExpiry,
            DividendAssumption::NoDividends,
            "6.12",
        );
        let SolverOutcome::Available(metrics) =
            solve_european_black_scholes(&input, input.valuation_at_ms())
        else {
            panic!("expected a convergent model estimate");
        };
        let iv = metrics.implied_volatility().expect("IV present");
        assert!((iv.value_at(10_000).expect("fresh IV") - 0.2).abs() < 0.01);
        assert_eq!(iv.unit(), crate::MetricUnit::ImpliedVolatilityFraction);
        assert_eq!(iv.source(), crate::MetricSource::BlackScholesEuropeanV2);
        assert_eq!(
            metrics.delta().expect("delta").unit(),
            crate::MetricUnit::DeltaPerUnderlyingUnit
        );
        assert!(
            metrics
                .gamma()
                .expect("gamma")
                .value_at(10_000)
                .unwrap()
                .is_finite()
        );
        assert!(
            metrics
                .theta()
                .expect("theta")
                .value_at(10_000)
                .unwrap()
                .is_finite()
        );
        assert_eq!(
            metrics.theta().expect("theta").unit(),
            crate::MetricUnit::ThetaPerYearFractionPerUnderlyingUnit
        );
        assert!(
            metrics
                .vega()
                .expect("vega")
                .value_at(10_000)
                .unwrap()
                .is_finite()
        );
        assert!(
            metrics
                .rho()
                .expect("rho")
                .value_at(10_000)
                .unwrap()
                .is_finite()
        );
    }

    #[test]
    fn synchronous_entrypoint_rechecks_future_dividend_evidence_at_explicit_evaluation_time() {
        let mut input = input(
            ExerciseStyle::European,
            ExpirationClass::NonZeroDaysToExpiry,
            DividendAssumption::NoDividends,
            "6.12",
        );
        let valuation_at_ms = input.valuation_at_ms();
        let evaluation_at_ms = valuation_at_ms + 1;
        input
            .dividend_evidence
            .as_mut()
            .expect("synthetic dividend evidence")
            .observed_at_ms = evaluation_at_ms + 1;

        assert!(input.observation.validate_at(evaluation_at_ms).is_ok());
        assert_eq!(
            solve_european_black_scholes(&input, evaluation_at_ms),
            SolverOutcome::Unavailable(ModelUnavailable::DividendEvidenceStale)
        );
    }

    #[test]
    fn dividend_evidence_accepts_exact_max_age_and_expiry_window_endpoints() {
        let input = input(
            ExerciseStyle::European,
            ExpirationClass::NonZeroDaysToExpiry,
            DividendAssumption::NoDividends,
            "6.12",
        );
        let expiry_at_ms = input.expiration.expiration.utc_epoch_millis;
        let evidence = input
            .dividend_evidence
            .as_ref()
            .expect("synthetic dividend evidence");
        assert_eq!(evidence.effective_until_ms, expiry_at_ms);
        assert_eq!(
            input.validate_fresh_at(input.valuation_at_ms() + 2_000),
            Ok(())
        );
    }

    #[test]
    fn positive_time_snapshot_is_unavailable_at_its_expiry_instant() {
        let input = input(
            ExerciseStyle::European,
            ExpirationClass::NonZeroDaysToExpiry,
            DividendAssumption::NoDividends,
            "6.12",
        );
        let expiry_at_ms = input.expiration.expiration.utc_epoch_millis;
        assert_eq!(
            solve_european_black_scholes(&input, expiry_at_ms),
            SolverOutcome::Unavailable(ModelUnavailable::Expired)
        );
    }

    #[test]
    fn unsupported_exercise_and_discrete_dividend_inputs_are_unavailable() {
        let cases = [
            (
                ExerciseStyle::American,
                ExpirationClass::NonZeroDaysToExpiry,
                DividendAssumption::NoDividends,
                ModelUnavailable::AmericanExerciseUnsupported,
            ),
            (
                ExerciseStyle::European,
                ExpirationClass::NonZeroDaysToExpiry,
                DividendAssumption::DiscreteScheduleUnknown,
                ModelUnavailable::DividendAssumptionUnsupported,
            ),
        ];
        for (exercise, expiration, dividends, expected) in cases {
            let input = input(exercise, expiration, dividends, "6.12");
            assert_eq!(
                solve_european_black_scholes(&input, input.valuation_at_ms()),
                SolverOutcome::Unavailable(expected)
            );
        }
    }

    #[test]
    fn european_model_accepts_continuous_yield_and_positive_time_0dte() {
        let continuous_yield = input(
            ExerciseStyle::European,
            ExpirationClass::NonZeroDaysToExpiry,
            DividendAssumption::ContinuousYield(0.01),
            "5.8",
        );
        assert!(matches!(
            solve_european_black_scholes(&continuous_yield, continuous_yield.valuation_at_ms(),),
            SolverOutcome::Available(_)
        ));

        let remaining_time_ms = 60 * 60 * 1_000;
        let checked_at_ms = 10_000;
        let same_day = SolverInput::new(
            OptionKind::Call,
            Underlying::new("QQQ").expect("underlying"),
            price("100"),
            Strike::parse_json_number("100").expect("strike"),
            price("0.0854"),
            0.02,
            ExerciseStyle::European,
            expiration_context(
                ExpirationClass::ZeroDaysToExpiry,
                checked_at_ms,
                remaining_time_ms,
            ),
            DividendAssumption::NoDividends,
            Some(dividend_evidence(
                "QQQ",
                checked_at_ms,
                checked_at_ms,
                checked_at_ms + remaining_time_ms,
                DividendCoverageKind::NoCashDividendInWindow,
            )),
            10_000,
            checked_at_ms,
            2_000,
        )
        .expect("positive time remains before same-day expiry");
        let SolverOutcome::Available(metrics) =
            solve_european_black_scholes(&same_day, same_day.valuation_at_ms())
        else {
            panic!("positive-time 0DTE should have a finite analytical estimate");
        };
        assert_eq!(
            metrics.implied_volatility().unwrap().source(),
            crate::MetricSource::BlackScholesEuropeanV2
        );
    }

    #[test]
    fn american_public_entrypoint_withholds_unverified_price_and_iv() {
        let input = input(
            ExerciseStyle::American,
            ExpirationClass::NonZeroDaysToExpiry,
            DividendAssumption::NoDividends,
            "6.12",
        );
        assert_eq!(
            super::solve_option_model(&input, input.valuation_at_ms()),
            SolverOutcome::Unavailable(ModelUnavailable::AmericanPricingAccuracyUnverified)
        );
    }

    #[test]
    fn negative_rate_american_put_does_not_use_strike_as_its_upper_bound() {
        let checked_at_ms = 10_000;
        let remaining_ms = 2 * 365 * 86_400_000;
        let input = SolverInput::new(
            OptionKind::Put,
            Underlying::new("QQQ").expect("underlying"),
            price("0.0001"),
            Strike::parse_json_number("100").expect("strike"),
            price("271.8"),
            -0.5,
            ExerciseStyle::American,
            expiration_context(
                ExpirationClass::NonZeroDaysToExpiry,
                checked_at_ms,
                remaining_ms,
            ),
            DividendAssumption::NoDividends,
            Some(dividend_evidence(
                "QQQ",
                checked_at_ms,
                checked_at_ms,
                checked_at_ms + remaining_ms,
                DividendCoverageKind::NoCashDividendInWindow,
            )),
            checked_at_ms,
            checked_at_ms,
            2_000,
        )
        .expect("legal negative-rate test input");
        assert_eq!(
            super::solve_american_crr(&input, input.valuation_at_ms()),
            SolverOutcome::Unavailable(ModelUnavailable::AmericanPricingAccuracyUnverified)
        );
    }

    #[test]
    fn rejects_out_of_range_prices_and_non_finite_inputs() {
        assert_eq!(
            SolverInput::new(
                OptionKind::Call,
                Underlying::new("QQQ").expect("underlying"),
                price("100"),
                Strike::parse_json_number("100").expect("strike"),
                price("5"),
                f64::NAN,
                ExerciseStyle::European,
                expiration_context(
                    ExpirationClass::NonZeroDaysToExpiry,
                    10_000,
                    182 * 86_400_000 + 43_200_000,
                ),
                DividendAssumption::NoDividends,
                None,
                10_000,
                10_000,
                2_000,
            ),
            Err(super::SolverInputError::RateOutOfRange)
        );
        let too_cheap = input(
            ExerciseStyle::European,
            ExpirationClass::NonZeroDaysToExpiry,
            DividendAssumption::NoDividends,
            "0.01",
        );
        assert_eq!(
            solve_european_black_scholes(&too_cheap, too_cheap.valuation_at_ms()),
            SolverOutcome::Unavailable(ModelUnavailable::PriceOutsideModelBounds)
        );
    }

    #[test]
    fn dividend_evidence_must_cover_the_underlying_and_model_window() {
        let checked_at_ms = 10_000;
        let expiry_at_ms = checked_at_ms + 2 * 86_400_000;
        let context = expiration_context(
            ExpirationClass::NonZeroDaysToExpiry,
            checked_at_ms,
            2 * 86_400_000,
        );
        let missing = solver_input(
            "QQQ",
            context.clone(),
            DividendAssumption::NoDividends,
            None,
            checked_at_ms,
            "1",
            0.02,
        );
        assert_eq!(
            missing,
            Err(super::SolverInputError::DividendEvidenceMissing)
        );
        assert_eq!(
            solver_input(
                "SPY",
                context.clone(),
                DividendAssumption::NoDividends,
                None,
                checked_at_ms,
                "1",
                0.02,
            ),
            Err(super::SolverInputError::DividendEvidenceMissing)
        );

        let wrong_underlying = solver_input(
            "QQQ",
            context.clone(),
            DividendAssumption::NoDividends,
            Some(dividend_evidence(
                "SPY",
                checked_at_ms,
                checked_at_ms,
                expiry_at_ms,
                DividendCoverageKind::NoCashDividendInWindow,
            )),
            checked_at_ms,
            "1",
            0.02,
        );
        assert_eq!(
            wrong_underlying,
            Err(super::SolverInputError::DividendUnderlyingMismatch)
        );

        let stale = solver_input(
            "QQQ",
            context.clone(),
            DividendAssumption::NoDividends,
            Some(dividend_evidence(
                "QQQ",
                checked_at_ms - 2_001,
                checked_at_ms,
                expiry_at_ms,
                DividendCoverageKind::NoCashDividendInWindow,
            )),
            checked_at_ms,
            "1",
            0.02,
        );
        assert_eq!(stale, Err(super::SolverInputError::DividendEvidenceStale));

        let uncovered_expiry = solver_input(
            "QQQ",
            context.clone(),
            DividendAssumption::NoDividends,
            Some(dividend_evidence(
                "QQQ",
                checked_at_ms,
                checked_at_ms,
                expiry_at_ms - 1,
                DividendCoverageKind::NoCashDividendInWindow,
            )),
            checked_at_ms,
            "1",
            0.02,
        );
        assert_eq!(
            uncovered_expiry,
            Err(super::SolverInputError::DividendEvidenceDoesNotCoverModelInterval)
        );

        let discrete_event_with_no_dividend_assumption = solver_input(
            "QQQ",
            context,
            DividendAssumption::NoDividends,
            Some(dividend_evidence(
                "QQQ",
                checked_at_ms,
                checked_at_ms,
                expiry_at_ms,
                DividendCoverageKind::DiscreteCashDividendInWindow,
            )),
            checked_at_ms,
            "1",
            0.02,
        );
        assert_eq!(
            discrete_event_with_no_dividend_assumption,
            Err(super::SolverInputError::DividendEvidenceMismatch)
        );
    }

    #[test]
    fn bounded_bisection_reports_nonconvergence_when_iteration_budget_is_exhausted() {
        let input = input(
            ExerciseStyle::European,
            ExpirationClass::NonZeroDaysToExpiry,
            DividendAssumption::NoDividends,
            "6.12",
        );
        assert_eq!(
            super::solve_european_black_scholes_with_limit(&input, input.valuation_at_ms(), 0,),
            SolverOutcome::Unavailable(ModelUnavailable::NonConvergent)
        );
    }

    #[test]
    fn expiry_context_checks_offsets_local_dates_and_classification() {
        let valuation =
            ExchangeInstant::new(10_000, -300, -1).expect("UTC offset maps to prior day");
        let expiry = ExchangeInstant::new(20_000, -300, -1).expect("same local day");
        assert!(
            ExpirationContext::new(
                ExpirationClass::ZeroDaysToExpiry,
                "America/New_York",
                valuation,
                expiry,
            )
            .is_ok()
        );
        assert_eq!(
            ExchangeInstant::new(10_000, -300, 0),
            Err(super::ExpirationContextError::LocalDateDoesNotMatchInstant)
        );
        assert_eq!(
            ExpirationContext::new(
                ExpirationClass::NonZeroDaysToExpiry,
                "America/New_York",
                valuation,
                expiry,
            ),
            Err(super::ExpirationContextError::ClassificationMismatch)
        );
    }

    #[test]
    fn expiry_time_boundaries_are_derived_from_utc_instants() {
        let checked_at_ms = 1_700_000_000_000;
        for (remaining_ms, below_resolution) in [
            (0_i64, false),
            (59_999, true),
            (60_000, false),
            (60_001, false),
        ] {
            let context = expiration_context(
                ExpirationClass::ZeroDaysToExpiry,
                checked_at_ms,
                remaining_ms,
            );
            if remaining_ms == 0 {
                let input = SolverInput::new(
                    OptionKind::Call,
                    Underlying::new("QQQ").expect("underlying"),
                    price("110"),
                    Strike::parse_json_number("100").expect("strike"),
                    price("10"),
                    0.0,
                    ExerciseStyle::American,
                    context,
                    DividendAssumption::DiscreteScheduleUnknown,
                    None,
                    checked_at_ms,
                    checked_at_ms,
                    2_000,
                )
                .expect("exact expiry input");
                assert_eq!(
                    super::solve_option_model(&input, input.valuation_at_ms()),
                    SolverOutcome::AtExpiryIntrinsic(price("10"))
                );
            } else {
                let input = SolverInput::new(
                    OptionKind::Call,
                    Underlying::new("QQQ").expect("underlying"),
                    price("100"),
                    Strike::parse_json_number("100").expect("strike"),
                    price("0.01"),
                    0.02,
                    ExerciseStyle::American,
                    context,
                    DividendAssumption::NoDividends,
                    Some(dividend_evidence(
                        "QQQ",
                        checked_at_ms,
                        checked_at_ms,
                        checked_at_ms + remaining_ms,
                        DividendCoverageKind::NoCashDividendInWindow,
                    )),
                    checked_at_ms,
                    checked_at_ms,
                    2_000,
                )
                .expect("positive same-day time");
                let outcome = super::solve_option_model(&input, input.valuation_at_ms());
                assert_eq!(
                    outcome,
                    SolverOutcome::Unavailable(if below_resolution {
                        ModelUnavailable::TimeBelowResolution
                    } else {
                        ModelUnavailable::AmericanPricingAccuracyUnverified
                    }),
                    "remaining_ms={remaining_ms}"
                );
                assert!(
                    (input.years_to_expiry() - remaining_ms as f64 / (365.0 * 86_400_000.0)).abs()
                        < f64::EPSILON
                );
            }
        }
    }

    #[test]
    fn zero_dte_more_than_one_day_and_expiration_class_mismatches_are_rejected() {
        let checked_at_ms = 10_000;
        let valuation = ExchangeInstant::new(checked_at_ms, 840, 0).expect("valuation date");
        let same_local_day_after_more_than_one_utc_day = ExchangeInstant::new(
            checked_at_ms + 24 * 60 * 60 * 1_000 + 30 * 60 * 1_000,
            -840,
            0,
        )
        .expect("offset evidence keeps both instants on same local day");
        let context = ExpirationContext::new(
            ExpirationClass::ZeroDaysToExpiry,
            "caller-timezone-evidence",
            valuation,
            same_local_day_after_more_than_one_utc_day,
        )
        .expect("same local date classification");
        assert_eq!(
            SolverInput::new(
                OptionKind::Call,
                Underlying::new("QQQ").expect("underlying"),
                price("100"),
                Strike::parse_json_number("100").expect("strike"),
                price("1"),
                0.02,
                ExerciseStyle::European,
                context,
                DividendAssumption::NoDividends,
                None,
                checked_at_ms,
                checked_at_ms,
                2_000,
            ),
            Err(super::SolverInputError::ExpirationContextMismatch)
        );
        assert_eq!(
            ExpirationContext::new(
                ExpirationClass::ZeroDaysToExpiry,
                "UTC",
                ExchangeInstant::new(10_000, 0, 0).expect("valuation"),
                ExchangeInstant::new(90_000_000, 0, 1).expect("next day"),
            ),
            Err(super::ExpirationContextError::ClassificationMismatch)
        );
    }

    #[test]
    fn input_distinguishes_expired_from_exact_expiry() {
        let checked_at_ms = 10_000;
        let expired = ExpirationContext::new(
            ExpirationClass::ZeroDaysToExpiry,
            "UTC",
            ExchangeInstant::new(checked_at_ms, 0, 0).expect("valuation"),
            ExchangeInstant::new(checked_at_ms - 1, 0, 0).expect("expired instant"),
        )
        .expect("same local day evidence");
        assert_eq!(
            SolverInput::new(
                OptionKind::Call,
                Underlying::new("QQQ").expect("underlying"),
                price("100"),
                Strike::parse_json_number("100").expect("strike"),
                price("1"),
                0.02,
                ExerciseStyle::European,
                expired,
                DividendAssumption::NoDividends,
                None,
                checked_at_ms,
                checked_at_ms,
                2_000,
            ),
            Err(super::SolverInputError::Expired)
        );

        let valuation_at_ms = 2 * 86_400_000 + 10_000;
        let previous_day_expiry = ExpirationContext::new(
            ExpirationClass::NonZeroDaysToExpiry,
            "UTC",
            ExchangeInstant::new(valuation_at_ms, 0, 2).expect("valuation"),
            ExchangeInstant::new(2 * 86_400_000 - 1, 0, 1).expect("past expiry"),
        )
        .expect("a prior expiry has non-zero local-day distance");
        assert_eq!(
            solver_input(
                "QQQ",
                previous_day_expiry,
                DividendAssumption::NoDividends,
                None,
                valuation_at_ms,
                "1",
                0.02,
            ),
            Err(super::SolverInputError::Expired)
        );
    }
}
