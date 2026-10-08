//! Timestamped IV and Greek observations with explicit units and provenance.
//!
//! Every numeric observation is bounded and revalidated at use time. Unknown
//! provider units are preserved but cannot be position-aggregated.
//!
//! ## 简体中文
//!
//! 本模块定义带时间戳、显式单位和来源的 IV/Greek 观测。数值有界且必须在使用时重新验证；未知 provider 单位会保留，但不能用于持仓聚合。

use std::{error::Error, fmt};

use domain::ContractMultiplier;

/// Maximum accepted source age for an IV/Greek observation.
/// 简体中文：IV/Greek 观测允许的最大来源年龄，单位为毫秒。
pub const MAX_METRIC_AGE_MS: u64 = 60_000;
/// Broad finite-value ceiling for untrusted provider-native fields.
/// 简体中文：不可信 provider-native 字段的有限绝对值上限。
pub const MAX_PROVIDER_METRIC_ABS: f64 = 1_000_000.0;
/// Maximum supported absolute signed position size before contract multiplier.
/// 简体中文：乘以合约乘数之前允许的最大绝对持仓合约数。
pub const MAX_SIGNED_CONTRACTS: u32 = 100_000;
/// Maximum total underlying units aggregated across positions.
/// 简体中文：跨持仓聚合时允许的最大标的单位总数。
pub const MAX_AGGREGATE_UNITS: u64 = 1_000_000_000_000;

/// IV or Greek field carried by a provider or model.
/// 简体中文：provider 或模型携带的 IV/Greek 字段类型。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum MetricKind {
    /// Implied volatility.
    /// 简体中文：隐含波动率。
    ImpliedVolatility,
    /// Delta sensitivity.
    /// 简体中文：Delta 敏感度。
    Delta,
    /// Gamma sensitivity.
    /// 简体中文：Gamma 敏感度。
    Gamma,
    /// Theta sensitivity.
    /// 简体中文：Theta 敏感度。
    Theta,
    /// Vega sensitivity.
    /// 简体中文：Vega 敏感度。
    Vega,
    /// Rho sensitivity.
    /// 简体中文：Rho 敏感度。
    Rho,
}

/// Greek fields that can be position-weighted. IV is deliberately excluded.
/// 简体中文：可按持仓加权聚合的 Greek 字段；隐含波动率不在其中。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum GreekKind {
    /// Delta sensitivity.
    /// 简体中文：Delta 敏感度。
    Delta,
    /// Gamma sensitivity.
    /// 简体中文：Gamma 敏感度。
    Gamma,
    /// Theta sensitivity.
    /// 简体中文：Theta 敏感度。
    Theta,
    /// Vega sensitivity.
    /// 简体中文：Vega 敏感度。
    Vega,
    /// Rho sensitivity.
    /// 简体中文：Rho 敏感度。
    Rho,
}

impl From<GreekKind> for MetricKind {
    fn from(value: GreekKind) -> Self {
        match value {
            GreekKind::Delta => Self::Delta,
            GreekKind::Gamma => Self::Gamma,
            GreekKind::Theta => Self::Theta,
            GreekKind::Vega => Self::Vega,
            GreekKind::Rho => Self::Rho,
        }
    }
}

/// Provenance class. `SchwabProviderNative` means the provider field is preserved
/// without claiming a unit contract that is absent from the checked-in
/// Schwab evidence. Model values identify the exact local model version.
/// 简体中文：指标来源类别。provider-native 值不推断 Schwab 未明确承诺的单位；模型值标识精确的本地模型版本。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum MetricSource {
    /// Value preserved from a Schwab provider field without an inferred unit.
    /// 简体中文：保留 Schwab provider 字段原值，不推断单位。
    SchwabProviderNative,
    /// Value produced by the explicitly versioned European Black-Scholes model.
    /// 简体中文：由明确版本的欧式 Black-Scholes 模型生成的值。
    BlackScholesEuropeanV1,
    /// Value produced by European Black-Scholes V2 using ACT/365F expiry input.
    /// 简体中文：由使用 ACT/365F 到期输入的欧式 Black-Scholes V2 模型生成的值。
    BlackScholesEuropeanV2,
    /// Reserved provenance ID for a future American CRR model after independent accuracy acceptance.
    /// No production pricing entrypoint currently emits this source.
    /// 简体中文：为通过独立精度验收后的美式 CRR 模型预留的来源 ID。当前生产定价入口不会发出此来源。
    AmericanCrrV1,
}

/// Explicit value units. Provider values are intentionally tagged as
/// provider-native/unknown; the crate never guesses percent-vs-fraction or
/// per-contract-vs-per-share conventions.
/// 简体中文：显式指标单位。provider 值使用未知单位标记；本 crate 不猜测百分数与小数、每合约与每股之间的约定。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum MetricUnit {
    /// Provider-native value whose unit contract is unknown.
    /// 简体中文：单位约定未知的 provider 原生数值。
    ProviderNativeUnknown,
    /// Value with no identified unit.
    /// 简体中文：未识别单位的数值。
    Unknown,
    /// Implied volatility expressed as a fraction.
    /// 简体中文：以小数比例表示的隐含波动率。
    ImpliedVolatilityFraction,
    /// Delta per one underlying unit.
    /// 简体中文：每一个标的单位对应的 Delta。
    DeltaPerUnderlyingUnit,
    /// Gamma per squared underlying-dollar move.
    /// 简体中文：每标的美元变动平方对应的 Gamma。
    GammaPerUnderlyingDollarSquared,
    /// Theta per calendar year and underlying unit.
    /// 简体中文：每日历年、每标的单位对应的 Theta。
    ThetaPerCalendarYearPerUnderlyingUnit,
    /// Theta per ACT/365F model-year fraction and underlying unit.
    /// 简体中文：每 ACT/365F 模型年分数、每标的单位对应的 Theta。
    ThetaPerYearFractionPerUnderlyingUnit,
    /// Vega per volatility unit and underlying unit.
    /// 简体中文：每波动率单位、每标的单位对应的 Vega。
    VegaPerVolatilityUnitPerUnderlyingUnit,
    /// Rho per rate unit and underlying unit.
    /// 简体中文：每利率单位、每标的单位对应的 Rho。
    RhoPerRateUnitPerUnderlyingUnit,
}

/// Quality label stored with every accepted value.
/// 简体中文：每个已接受数值携带的质量标记。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum MetricQuality {
    /// Reported by the provider.
    /// 简体中文：由 provider 报告。
    ProviderReported,
    /// Estimated by an analytical model.
    /// 简体中文：由解析模型估算。
    ModelEstimate,
}

/// Source timestamp plus a bounded freshness window. Times are UTC epoch
/// milliseconds, matching the quote source timestamp domain.
/// 简体中文：来源观测时间与有界新鲜度窗口，时间使用与行情来源一致的 UTC Unix 毫秒。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObservationWindow {
    observed_at_ms: i64,
    max_age_ms: u64,
}

impl ObservationWindow {
    /// Creates a window and validates it at the supplied check time.
    /// 简体中文：创建时间窗口，并按给定检查时刻验证时间戳和新鲜度。
    pub fn new(
        observed_at_ms: i64,
        checked_at_ms: i64,
        max_age_ms: u64,
    ) -> Result<Self, MetricsError> {
        let window = Self {
            observed_at_ms,
            max_age_ms,
        };
        window.validate_at(checked_at_ms)?;
        Ok(window)
    }

    /// Returns the UTC Unix-millisecond source timestamp.
    /// 简体中文：返回 UTC Unix 毫秒来源时间戳。
    pub const fn observed_at_ms(self) -> i64 {
        self.observed_at_ms
    }

    /// Returns the configured maximum age in milliseconds.
    /// 简体中文：返回配置的最大年龄，单位为毫秒。
    pub const fn max_age_ms(self) -> u64 {
        self.max_age_ms
    }

    /// Revalidates this window against the current UTC Unix-millisecond time.
    /// 简体中文：按当前 UTC Unix 毫秒时刻重新验证窗口。
    pub fn validate_at(self, checked_at_ms: i64) -> Result<(), MetricsError> {
        if self.observed_at_ms < 0 || checked_at_ms < 0 {
            return Err(MetricsError::InvalidTimestamp);
        }
        if self.max_age_ms == 0 || self.max_age_ms > MAX_METRIC_AGE_MS {
            return Err(MetricsError::InvalidFreshnessWindow);
        }
        let age = checked_at_ms
            .checked_sub(self.observed_at_ms)
            .ok_or(MetricsError::InvalidTimestamp)?;
        if age < 0 {
            return Err(MetricsError::FutureObservation);
        }
        let age = u64::try_from(age).map_err(|_| MetricsError::InvalidTimestamp)?;
        if age > self.max_age_ms {
            return Err(MetricsError::StaleObservation);
        }
        Ok(())
    }
}

/// A finite, bounded, timestamped metric. Fields are private so every public
/// construction path validates unit/source/quality and freshness together.
/// 简体中文：有限、有界且带时间戳的指标观测。字段保持私有，所有构造方式都会同时验证单位、来源、质量和新鲜度。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MetricObservation {
    kind: MetricKind,
    value: f64,
    unit: MetricUnit,
    source: MetricSource,
    observed_at_ms: i64,
    quality: MetricQuality,
    max_age_ms: u64,
}

impl MetricObservation {
    /// Builds an observation from a Schwab provider-native field.
    /// The unit remains unknown and is not eligible for aggregation.
    /// 简体中文：从 Schwab provider 原生字段构造观测。单位仍为未知，因此不可用于聚合。
    pub fn provider_native(
        kind: MetricKind,
        value: f64,
        observed_at_ms: i64,
        checked_at_ms: i64,
        max_age_ms: u64,
    ) -> Result<Self, MetricsError> {
        Self::new(
            kind,
            value,
            MetricUnit::ProviderNativeUnknown,
            MetricSource::SchwabProviderNative,
            MetricQuality::ProviderReported,
            observed_at_ms,
            checked_at_ms,
            max_age_ms,
        )
    }

    /// Builds a metric with explicit unit, source, quality, and freshness data.
    /// The value and provenance must be compatible with `kind`.
    /// 简体中文：使用显式单位、来源、质量和新鲜度数据构造指标；数值与来源信息必须与 `kind` 相容。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        kind: MetricKind,
        value: f64,
        unit: MetricUnit,
        source: MetricSource,
        quality: MetricQuality,
        observed_at_ms: i64,
        checked_at_ms: i64,
        max_age_ms: u64,
    ) -> Result<Self, MetricsError> {
        let _window = ObservationWindow::new(observed_at_ms, checked_at_ms, max_age_ms)?;
        validate_value(kind, value, unit)?;
        validate_provenance(kind, unit, source, quality)?;
        Ok(Self {
            kind,
            value,
            unit,
            source,
            observed_at_ms,
            quality,
            max_age_ms,
        })
    }

    /// Returns the metric kind.
    /// 简体中文：返回指标类型。
    pub const fn kind(self) -> MetricKind {
        self.kind
    }

    /// Revalidates freshness and returns the numeric value only when current.
    /// 简体中文：重新验证新鲜度；仅在观测仍有效时返回数值。
    pub fn value_at(self, checked_at_ms: i64) -> Result<f64, MetricsError> {
        self.validate_at(checked_at_ms)?;
        Ok(self.value)
    }

    /// Returns the declared unit.
    /// 简体中文：返回声明的单位。
    pub const fn unit(self) -> MetricUnit {
        self.unit
    }

    /// Returns the provenance source.
    /// 简体中文：返回数据来源。
    pub const fn source(self) -> MetricSource {
        self.source
    }

    /// Returns the UTC Unix-millisecond source timestamp.
    /// 简体中文：返回 UTC Unix 毫秒来源时间戳。
    pub const fn observed_at_ms(self) -> i64 {
        self.observed_at_ms
    }

    /// Returns the reported or estimated quality label.
    /// 简体中文：返回 provider 报告或模型估算的质量标记。
    pub const fn quality(self) -> MetricQuality {
        self.quality
    }

    /// Checks this observation's configured freshness window.
    /// 简体中文：按给定时刻检查此观测配置的新鲜度窗口。
    pub fn validate_at(self, checked_at_ms: i64) -> Result<(), MetricsError> {
        ObservationWindow {
            observed_at_ms: self.observed_at_ms,
            max_age_ms: self.max_age_ms,
        }
        .validate_at(checked_at_ms)
    }
}

/// Fixed-size optional metric set for a single option leg.
/// 简体中文：单个期权腿的固定字段可选指标集合。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OptionMetrics {
    implied_volatility: Option<MetricObservation>,
    delta: Option<MetricObservation>,
    gamma: Option<MetricObservation>,
    theta: Option<MetricObservation>,
    vega: Option<MetricObservation>,
    rho: Option<MetricObservation>,
}

impl OptionMetrics {
    /// Builds a metric set and rejects observations in the wrong field.
    /// 简体中文：构造指标集合；若某观测与其字段类型不匹配则返回错误。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        implied_volatility: Option<MetricObservation>,
        delta: Option<MetricObservation>,
        gamma: Option<MetricObservation>,
        theta: Option<MetricObservation>,
        vega: Option<MetricObservation>,
        rho: Option<MetricObservation>,
    ) -> Result<Self, MetricsError> {
        let values = [
            (MetricKind::ImpliedVolatility, implied_volatility),
            (MetricKind::Delta, delta),
            (MetricKind::Gamma, gamma),
            (MetricKind::Theta, theta),
            (MetricKind::Vega, vega),
            (MetricKind::Rho, rho),
        ];
        for (expected, observation) in values {
            if observation.is_some_and(|value| value.kind != expected) {
                return Err(MetricsError::MetricKindMismatch);
            }
        }
        Ok(Self {
            implied_volatility,
            delta,
            gamma,
            theta,
            vega,
            rho,
        })
    }

    /// Returns the optional implied-volatility observation.
    /// 简体中文：返回可选的隐含波动率观测。
    pub const fn implied_volatility(&self) -> Option<MetricObservation> {
        self.implied_volatility
    }

    /// Returns the optional delta observation.
    /// 简体中文：返回可选的 Delta 观测。
    pub const fn delta(&self) -> Option<MetricObservation> {
        self.delta
    }

    /// Returns the optional gamma observation.
    /// 简体中文：返回可选的 Gamma 观测。
    pub const fn gamma(&self) -> Option<MetricObservation> {
        self.gamma
    }

    /// Returns the optional theta observation.
    /// 简体中文：返回可选的 Theta 观测。
    pub const fn theta(&self) -> Option<MetricObservation> {
        self.theta
    }

    /// Returns the optional vega observation.
    /// 简体中文：返回可选的 Vega 观测。
    pub const fn vega(&self) -> Option<MetricObservation> {
        self.vega
    }

    /// Returns the optional rho observation.
    /// 简体中文：返回可选的 Rho 观测。
    pub const fn rho(&self) -> Option<MetricObservation> {
        self.rho
    }

    /// Returns the optional observation for one aggregatable Greek.
    /// Implied volatility is not a `GreekKind`.
    /// 简体中文：返回指定可聚合 Greek 的可选观测；隐含波动率不是 `GreekKind`。
    pub fn greek(&self, kind: GreekKind) -> Option<MetricObservation> {
        match kind {
            GreekKind::Delta => self.delta,
            GreekKind::Gamma => self.gamma,
            GreekKind::Theta => self.theta,
            GreekKind::Vega => self.vega,
            GreekKind::Rho => self.rho,
        }
    }

    /// Prefer a provider-reported value independently for each field, then
    /// fall back to the matching explicit model estimate. Provider values keep
    /// their `ProviderNativeUnknown` unit and are not made aggregate-safe by
    /// this selection.
    /// 简体中文：逐字段优先选择当前有效的 provider 报告值，否则选择匹配的有效模型估算。provider 单位仍为未知，不会因此变得可聚合。
    pub fn merge_provider_preferred(provider: &Self, model: &Self, checked_at_ms: i64) -> Self {
        fn select(
            provider: Option<MetricObservation>,
            model: Option<MetricObservation>,
            checked_at_ms: i64,
        ) -> Option<MetricObservation> {
            provider
                .filter(|metric| {
                    metric.source == MetricSource::SchwabProviderNative
                        && metric.validate_at(checked_at_ms).is_ok()
                })
                .or_else(|| {
                    model.filter(|metric| {
                        matches!(
                            metric.source,
                            MetricSource::BlackScholesEuropeanV1
                                | MetricSource::BlackScholesEuropeanV2
                        ) && metric.validate_at(checked_at_ms).is_ok()
                    })
                })
        }

        Self {
            implied_volatility: select(
                provider.implied_volatility,
                model.implied_volatility,
                checked_at_ms,
            ),
            delta: select(provider.delta, model.delta, checked_at_ms),
            gamma: select(provider.gamma, model.gamma, checked_at_ms),
            theta: select(provider.theta, model.theta, checked_at_ms),
            vega: select(provider.vega, model.vega, checked_at_ms),
            rho: select(provider.rho, model.rho, checked_at_ms),
        }
    }
}

/// A freshness-checked long-leg IV minus short-leg IV difference.
///
/// Both legs must be fractional IV observations with matching source and
/// quality. The value is signed and is not an implied volatility for a spread.
///
/// ## 简体中文
///
/// 经新鲜度校验的多头腿 IV 减空头腿 IV 差值。两腿必须使用小数比例 IV，且来源与质量相同。结果可为负数，也不代表价差组合的隐含波动率。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LongMinusShortIvDifference {
    long_leg: MetricObservation,
    short_leg: MetricObservation,
}

impl LongMinusShortIvDifference {
    /// Creates a difference from two fresh, comparable implied-volatility observations.
    ///
    /// `checked_at_ms` is UTC epoch milliseconds. Provider-native unknown units
    /// and values from different model versions are rejected.
    ///
    /// ## 简体中文
    ///
    /// 根据两条新鲜且可比较的隐含波动率观测创建差值。`checked_at_ms` 使用 UTC Unix 毫秒；未知 provider 单位及不同模型版本的值会被拒绝。
    pub fn new(
        long_leg: MetricObservation,
        short_leg: MetricObservation,
        checked_at_ms: i64,
    ) -> Result<Self, MetricsError> {
        for leg in [long_leg, short_leg] {
            if leg.kind != MetricKind::ImpliedVolatility {
                return Err(MetricsError::MetricKindMismatch);
            }
            if leg.unit != MetricUnit::ImpliedVolatilityFraction {
                return Err(MetricsError::UnitMismatch);
            }
            leg.validate_at(checked_at_ms)?;
        }
        if long_leg.source != short_leg.source {
            return Err(MetricsError::SourceMismatch);
        }
        if long_leg.quality != short_leg.quality {
            return Err(MetricsError::QualityMismatch);
        }
        Ok(Self {
            long_leg,
            short_leg,
        })
    }

    /// Returns long-leg IV minus short-leg IV while revalidating both timestamps.
    /// The result unit is a volatility fraction difference, not a percentage point.
    ///
    /// ## 简体中文
    ///
    /// 返回多头腿 IV 减空头腿 IV，并重新检查两条观测的时间有效性。结果单位为波动率小数差值，不是百分比点。
    pub fn value_at(self, checked_at_ms: i64) -> Result<f64, MetricsError> {
        let long = self.long_leg.value_at(checked_at_ms)?;
        let short = self.short_leg.value_at(checked_at_ms)?;
        let difference = long - short;
        if difference.is_finite() {
            Ok(difference)
        } else {
            Err(MetricsError::NonFiniteValue)
        }
    }
}

/// Signed contract count and validated contract multiplier for one option leg.
/// 简体中文：单个期权腿的有符号合约数量与已验证合约乘数。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SignedContractPosition {
    quantity: i32,
    multiplier: ContractMultiplier,
}

impl SignedContractPosition {
    /// Creates a bounded signed position.
    /// Positive quantities represent long exposure and negative quantities short exposure.
    /// 简体中文：创建有界的有符号持仓。正数量表示多头，负数量表示空头。
    pub fn new(quantity: i32, multiplier: ContractMultiplier) -> Result<Self, MetricsError> {
        if quantity.unsigned_abs() > MAX_SIGNED_CONTRACTS {
            return Err(MetricsError::PositionOutOfRange);
        }
        if multiplier.get() > u64::from(MAX_SIGNED_CONTRACTS) {
            return Err(MetricsError::MultiplierOutOfRange);
        }
        Ok(Self {
            quantity,
            multiplier,
        })
    }

    /// Returns the signed contract count.
    /// 简体中文：返回有符号合约数量。
    pub const fn quantity(self) -> i32 {
        self.quantity
    }

    /// Returns the validated contract multiplier.
    /// 简体中文：返回已验证的合约乘数。
    pub const fn multiplier(self) -> ContractMultiplier {
        self.multiplier
    }

    fn signed_underlying_units(self) -> Result<i128, MetricsError> {
        i128::from(self.quantity)
            .checked_mul(i128::from(self.multiplier.get()))
            .ok_or(MetricsError::ArithmeticOverflow)
    }
}

/// One position's validated option metrics and signed position size.
/// 简体中文：单个持仓的已验证期权指标和有符号持仓规模。
#[derive(Clone, Debug, PartialEq)]
pub struct GreekPosition {
    metrics: OptionMetrics,
    position: SignedContractPosition,
}

impl GreekPosition {
    /// Pairs one leg's metrics with its signed contract position.
    /// 简体中文：将单个腿的指标与其有符号合约持仓配对。
    pub const fn new(metrics: OptionMetrics, position: SignedContractPosition) -> Self {
        Self { metrics, position }
    }
}

/// Position-weighted Greek. It is an analytical value, not an execution or
/// risk authorization signal.
/// 简体中文：按持仓加权的 Greek 分析值，不是执行或风险授权信号。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AggregatedGreek {
    kind: GreekKind,
    value: f64,
    unit: MetricUnit,
    source: MetricSource,
    quality: MetricQuality,
    oldest_observation_ms: i64,
    max_age_ms: u64,
}

impl AggregatedGreek {
    /// Returns the aggregated Greek kind.
    /// 简体中文：返回聚合后的 Greek 类型。
    pub const fn kind(self) -> GreekKind {
        self.kind
    }

    /// Returns the common unit of all aggregated observations.
    /// 简体中文：返回所有参与聚合的观测所共有的单位。
    pub const fn unit(self) -> MetricUnit {
        self.unit
    }

    /// Returns the common source of all aggregated observations.
    /// 简体中文：返回所有参与聚合的观测所共有的数据来源。
    pub const fn source(self) -> MetricSource {
        self.source
    }

    /// Returns the common quality label of all aggregated observations.
    /// 简体中文：返回所有参与聚合的观测所共有的质量标记。
    pub const fn quality(self) -> MetricQuality {
        self.quality
    }

    /// Returns the oldest timestamp among the aggregated observations.
    /// 简体中文：返回参与聚合的最早观测时间戳。
    pub const fn oldest_observation_ms(self) -> i64 {
        self.oldest_observation_ms
    }

    /// Revalidates the aggregate's freshness before exposing its value.
    /// 简体中文：在提供聚合值之前重新验证其新鲜度。
    pub fn value_at(self, checked_at_ms: i64) -> Result<f64, MetricsError> {
        ObservationWindow::new(self.oldest_observation_ms, checked_at_ms, self.max_age_ms)?;
        Ok(self.value)
    }
}

/// Sum signed quantity × multiplier × each leg's per-underlying metric.
/// Every observation must be fresh, have an explicit known unit, and have
/// exactly matching source and unit. Unknown provider-native units fail closed.
/// 简体中文：按有符号合约数量 × 乘数 × 每标的单位指标求和。每个观测必须新鲜且具有已知单位，并且来源、单位、质量完全一致；未知 provider 单位会拒绝聚合。
pub fn aggregate_greek(
    kind: GreekKind,
    positions: &[GreekPosition],
    checked_at_ms: i64,
) -> Result<AggregatedGreek, MetricsError> {
    if positions.is_empty() {
        return Err(MetricsError::NoPositions);
    }
    let metric_kind = MetricKind::from(kind);
    let mut total_units = 0i128;
    let mut total = 0.0f64;
    let mut selected_unit = None;
    let mut selected_source = None;
    let mut selected_quality = None;
    let mut oldest_observation_ms = i64::MAX;
    let mut strictest_max_age_ms = MAX_METRIC_AGE_MS;

    for position in positions {
        let observation = position
            .metrics
            .greek(kind)
            .ok_or(MetricsError::MissingMetric)?;
        if observation.kind != metric_kind {
            return Err(MetricsError::MetricKindMismatch);
        }
        if matches!(
            observation.unit,
            MetricUnit::ProviderNativeUnknown | MetricUnit::Unknown
        ) {
            return Err(MetricsError::UnknownUnitCannotAggregate);
        }
        observation.validate_at(checked_at_ms)?;
        if let Some(unit) = selected_unit {
            if unit != observation.unit {
                return Err(MetricsError::UnitMismatch);
            }
        } else {
            selected_unit = Some(observation.unit);
        }
        if let Some(source) = selected_source {
            if source != observation.source {
                return Err(MetricsError::SourceMismatch);
            }
        } else {
            selected_source = Some(observation.source);
        }
        if let Some(quality) = selected_quality {
            if quality != observation.quality {
                return Err(MetricsError::QualityMismatch);
            }
        } else {
            selected_quality = Some(observation.quality);
        }

        let signed_units = position.position.signed_underlying_units()?;
        total_units = total_units
            .checked_add(signed_units.abs())
            .ok_or(MetricsError::ArithmeticOverflow)?;
        if total_units > i128::from(MAX_AGGREGATE_UNITS) {
            return Err(MetricsError::AggregateOutOfRange);
        }
        total += observation.value * signed_units as f64;
        if !total.is_finite() || total.abs() > MAX_PROVIDER_METRIC_ABS * MAX_AGGREGATE_UNITS as f64
        {
            return Err(MetricsError::ValueOutOfRange);
        }
        oldest_observation_ms = oldest_observation_ms.min(observation.observed_at_ms);
        strictest_max_age_ms = strictest_max_age_ms.min(observation.max_age_ms);
    }

    Ok(AggregatedGreek {
        kind,
        value: total,
        unit: selected_unit.ok_or(MetricsError::NoPositions)?,
        source: selected_source.ok_or(MetricsError::NoPositions)?,
        quality: selected_quality.ok_or(MetricsError::NoPositions)?,
        oldest_observation_ms,
        max_age_ms: strictest_max_age_ms,
    })
}

fn validate_value(kind: MetricKind, value: f64, unit: MetricUnit) -> Result<(), MetricsError> {
    if !value.is_finite() {
        return Err(MetricsError::NonFiniteValue);
    }
    let (minimum, maximum) = if unit == MetricUnit::ProviderNativeUnknown
        || unit == MetricUnit::Unknown
    {
        if kind == MetricKind::ImpliedVolatility {
            (0.0, MAX_PROVIDER_METRIC_ABS)
        } else {
            (-MAX_PROVIDER_METRIC_ABS, MAX_PROVIDER_METRIC_ABS)
        }
    } else {
        match kind {
            MetricKind::ImpliedVolatility => (0.0, 5.0),
            MetricKind::Delta => (-1.0, 1.0),
            MetricKind::Gamma => (0.0, 1_000_000.0),
            MetricKind::Theta | MetricKind::Vega | MetricKind::Rho => (-1_000_000.0, 1_000_000.0),
        }
    };
    if value < minimum || value > maximum {
        return Err(MetricsError::ValueOutOfRange);
    }
    Ok(())
}

fn validate_provenance(
    kind: MetricKind,
    unit: MetricUnit,
    source: MetricSource,
    quality: MetricQuality,
) -> Result<(), MetricsError> {
    match source {
        MetricSource::SchwabProviderNative => {
            if !matches!(
                unit,
                MetricUnit::ProviderNativeUnknown | MetricUnit::Unknown
            ) || quality != MetricQuality::ProviderReported
            {
                return Err(MetricsError::InvalidProvenance);
            }
        }
        MetricSource::AmericanCrrV1 => return Err(MetricsError::InvalidProvenance),
        MetricSource::BlackScholesEuropeanV1 | MetricSource::BlackScholesEuropeanV2 => {
            let expected_unit = match kind {
                MetricKind::ImpliedVolatility => MetricUnit::ImpliedVolatilityFraction,
                MetricKind::Delta => MetricUnit::DeltaPerUnderlyingUnit,
                MetricKind::Gamma => MetricUnit::GammaPerUnderlyingDollarSquared,
                MetricKind::Theta if source == MetricSource::BlackScholesEuropeanV1 => {
                    MetricUnit::ThetaPerCalendarYearPerUnderlyingUnit
                }
                MetricKind::Theta => MetricUnit::ThetaPerYearFractionPerUnderlyingUnit,
                MetricKind::Vega => MetricUnit::VegaPerVolatilityUnitPerUnderlyingUnit,
                MetricKind::Rho => MetricUnit::RhoPerRateUnitPerUnderlyingUnit,
            };
            if unit != expected_unit || quality != MetricQuality::ModelEstimate {
                return Err(MetricsError::InvalidProvenance);
            }
        }
    }
    Ok(())
}

/// Stable, content-free validation errors for metric data.
/// 简体中文：稳定且不包含输入内容的指标校验错误。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MetricsError {
    /// A timestamp is negative or cannot be represented as a valid age.
    /// 简体中文：时间戳为负数，或无法表示为有效观测年龄。
    InvalidTimestamp,
    /// The observation timestamp is later than the check time.
    /// 简体中文：观测时间晚于检查时间。
    FutureObservation,
    /// The observation exceeds its configured age window.
    /// 简体中文：观测已超出配置的新鲜度窗口。
    StaleObservation,
    /// The configured age window is zero or exceeds the crate bound.
    /// 简体中文：新鲜度窗口为零或超过 crate 上限。
    InvalidFreshnessWindow,
    /// The metric value is NaN or infinite.
    /// 简体中文：指标数值为 NaN 或无穷大。
    NonFiniteValue,
    /// The metric value is outside its kind-specific bound.
    /// 简体中文：指标数值超出该类型的取值范围。
    ValueOutOfRange,
    /// Source, quality, and unit do not form an accepted provenance tuple.
    /// 简体中文：来源、质量和单位组合不符合允许的来源约定。
    InvalidProvenance,
    /// A metric was supplied in a different field than its declared kind.
    /// 简体中文：观测声明的类型与所在字段不匹配。
    MetricKindMismatch,
    /// A required position metric is absent.
    /// 简体中文：持仓缺少必需的指标。
    MissingMetric,
    /// Positions use different units.
    /// 简体中文：不同持仓使用了不同单位。
    UnitMismatch,
    /// A provider-native or unknown unit cannot be aggregated.
    /// 简体中文：provider-native 或未知单位不能参与聚合。
    UnknownUnitCannotAggregate,
    /// Positions use different metric sources.
    /// 简体中文：不同持仓使用了不同指标来源。
    SourceMismatch,
    /// Positions use different quality labels.
    /// 简体中文：不同持仓使用了不同质量标记。
    QualityMismatch,
    /// The signed contract count exceeds the supported bound.
    /// 简体中文：有符号合约数量超过支持上限。
    PositionOutOfRange,
    /// The contract multiplier exceeds the supported bound.
    /// 简体中文：合约乘数超过支持上限。
    MultiplierOutOfRange,
    /// Integer or floating-point aggregation overflowed.
    /// 简体中文：整数或浮点聚合发生溢出。
    ArithmeticOverflow,
    /// The aggregated value or total units exceed their bounds.
    /// 简体中文：聚合数值或标的单位总量超过上限。
    AggregateOutOfRange,
    /// The aggregation input contains no positions.
    /// 简体中文：聚合输入中没有持仓。
    NoPositions,
}

impl fmt::Display for MetricsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidTimestamp => "METRIC_INVALID_TIMESTAMP",
            Self::FutureObservation => "METRIC_FUTURE_OBSERVATION",
            Self::StaleObservation => "METRIC_STALE_OBSERVATION",
            Self::InvalidFreshnessWindow => "METRIC_INVALID_FRESHNESS_WINDOW",
            Self::NonFiniteValue => "METRIC_NON_FINITE_VALUE",
            Self::ValueOutOfRange => "METRIC_VALUE_OUT_OF_RANGE",
            Self::InvalidProvenance => "METRIC_INVALID_PROVENANCE",
            Self::MetricKindMismatch => "METRIC_KIND_MISMATCH",
            Self::MissingMetric => "METRIC_MISSING_GREEK",
            Self::UnitMismatch => "METRIC_UNIT_MISMATCH",
            Self::UnknownUnitCannotAggregate => "METRIC_UNKNOWN_UNIT_CANNOT_AGGREGATE",
            Self::SourceMismatch => "METRIC_SOURCE_MISMATCH",
            Self::QualityMismatch => "METRIC_QUALITY_MISMATCH",
            Self::PositionOutOfRange => "METRIC_POSITION_OUT_OF_RANGE",
            Self::MultiplierOutOfRange => "METRIC_MULTIPLIER_OUT_OF_RANGE",
            Self::ArithmeticOverflow => "METRIC_ARITHMETIC_OVERFLOW",
            Self::AggregateOutOfRange => "METRIC_AGGREGATE_OUT_OF_RANGE",
            Self::NoPositions => "METRIC_NO_POSITIONS",
        })
    }
}

impl Error for MetricsError {}

#[cfg(test)]
mod tests {
    use domain::ContractMultiplier;

    use super::{
        GreekKind, GreekPosition, MetricKind, MetricObservation, MetricUnit, MetricsError,
        OptionMetrics, SignedContractPosition, aggregate_greek,
    };

    fn model(kind: MetricKind, value: f64, observed_at: i64, now: i64) -> MetricObservation {
        let unit = match kind {
            MetricKind::ImpliedVolatility => MetricUnit::ImpliedVolatilityFraction,
            MetricKind::Delta => MetricUnit::DeltaPerUnderlyingUnit,
            MetricKind::Gamma => MetricUnit::GammaPerUnderlyingDollarSquared,
            MetricKind::Theta => MetricUnit::ThetaPerCalendarYearPerUnderlyingUnit,
            MetricKind::Vega => MetricUnit::VegaPerVolatilityUnitPerUnderlyingUnit,
            MetricKind::Rho => MetricUnit::RhoPerRateUnitPerUnderlyingUnit,
        };
        MetricObservation::new(
            kind,
            value,
            unit,
            super::MetricSource::BlackScholesEuropeanV1,
            super::MetricQuality::ModelEstimate,
            observed_at,
            now,
            2_000,
        )
        .expect("valid model metric")
    }

    fn metrics(delta: MetricObservation) -> OptionMetrics {
        OptionMetrics::new(None, Some(delta), None, None, None, None).expect("valid metric set")
    }

    #[test]
    fn american_crr_provenance_stays_rejected_until_accuracy_acceptance() {
        assert_eq!(
            MetricObservation::new(
                MetricKind::ImpliedVolatility,
                0.2,
                MetricUnit::ImpliedVolatilityFraction,
                super::MetricSource::AmericanCrrV1,
                super::MetricQuality::ModelEstimate,
                10,
                10,
                2_000,
            ),
            Err(MetricsError::InvalidProvenance)
        );
    }

    fn position(quantity: i32, delta: MetricObservation) -> GreekPosition {
        GreekPosition::new(
            metrics(delta),
            SignedContractPosition::new(
                quantity,
                ContractMultiplier::new(100).expect("multiplier"),
            )
            .expect("position"),
        )
    }

    #[test]
    fn rejects_non_finite_negative_iv_future_and_stale_values() {
        assert_eq!(
            MetricObservation::provider_native(MetricKind::Delta, f64::NAN, 10, 10, 100),
            Err(MetricsError::NonFiniteValue)
        );
        assert_eq!(
            MetricObservation::provider_native(MetricKind::Gamma, f64::INFINITY, 10, 10, 100),
            Err(MetricsError::NonFiniteValue)
        );
        assert_eq!(
            MetricObservation::provider_native(MetricKind::ImpliedVolatility, -0.1, 10, 10, 100),
            Err(MetricsError::ValueOutOfRange)
        );
        assert_eq!(
            MetricObservation::provider_native(MetricKind::Delta, 0.5, 11, 10, 100),
            Err(MetricsError::FutureObservation)
        );
        assert_eq!(
            MetricObservation::provider_native(MetricKind::Delta, 0.5, 1, 10, 5),
            Err(MetricsError::StaleObservation)
        );
    }

    #[test]
    fn signed_quantity_times_multiplier_aggregates_only_matching_known_units() {
        let long = model(MetricKind::Delta, 0.6, 100, 100);
        let short = model(MetricKind::Delta, 0.2, 100, 100);
        let aggregated = aggregate_greek(
            GreekKind::Delta,
            &[position(2, long), position(-1, short)],
            100,
        )
        .expect("same model unit can be combined");
        assert_eq!(aggregated.value_at(100), Ok(100.0));
        assert_eq!(aggregated.unit(), MetricUnit::DeltaPerUnderlyingUnit);

        let provider = MetricObservation::provider_native(MetricKind::Delta, 0.3, 100, 100, 2_000)
            .expect("provider unit is explicit");
        assert_eq!(
            aggregate_greek(
                GreekKind::Delta,
                &[position(1, long), position(-1, provider)],
                100
            ),
            Err(MetricsError::UnknownUnitCannotAggregate)
        );
    }

    #[test]
    fn rejects_positions_and_unknown_mixed_provenance_fail_closed() {
        assert_eq!(
            aggregate_greek(GreekKind::Delta, &[], 100),
            Err(MetricsError::NoPositions)
        );
        let native = MetricObservation::provider_native(MetricKind::Delta, 0.3, 100, 100, 2_000)
            .expect("native metric");
        let unknown = MetricObservation::new(
            MetricKind::Delta,
            0.4,
            MetricUnit::Unknown,
            super::MetricSource::SchwabProviderNative,
            super::MetricQuality::ProviderReported,
            100,
            100,
            2_000,
        )
        .expect("unknown unit remains explicit");
        assert_eq!(
            aggregate_greek(GreekKind::Delta, &[position(1, native)], 100),
            Err(MetricsError::UnknownUnitCannotAggregate)
        );
        assert_eq!(
            aggregate_greek(GreekKind::Delta, &[position(1, unknown)], 100),
            Err(MetricsError::UnknownUnitCannotAggregate)
        );
    }

    #[test]
    fn provider_values_take_priority_without_guessing_their_units() {
        let provider_delta =
            MetricObservation::provider_native(MetricKind::Delta, 0.4, 100, 100, 2_000)
                .expect("provider delta");
        let provider = OptionMetrics::new(None, Some(provider_delta), None, None, None, None)
            .expect("provider metrics");
        let model_delta = model(MetricKind::Delta, 0.6, 100, 100);
        let model_gamma = model(MetricKind::Gamma, 0.2, 100, 100);
        let model =
            OptionMetrics::new(None, Some(model_delta), Some(model_gamma), None, None, None)
                .expect("model metrics");

        let selected = OptionMetrics::merge_provider_preferred(&provider, &model, 100);
        assert_eq!(
            selected.delta().expect("selected delta").value_at(100),
            Ok(0.4)
        );
        assert_eq!(
            selected.delta().expect("selected delta").unit(),
            MetricUnit::ProviderNativeUnknown
        );
        assert_eq!(
            selected.gamma().expect("model fallback").value_at(100),
            Ok(0.2)
        );
    }

    #[test]
    fn stale_provider_value_falls_back_to_a_fresh_model_estimate() {
        let stale_provider =
            MetricObservation::provider_native(MetricKind::Delta, 0.4, 100, 100, 2_000)
                .expect("initially fresh provider value");
        let fresh_model = model(MetricKind::Delta, 0.6, 2_000, 2_000);
        let provider = OptionMetrics::new(None, Some(stale_provider), None, None, None, None)
            .expect("provider set");
        let model =
            OptionMetrics::new(None, Some(fresh_model), None, None, None, None).expect("model set");

        let selected = OptionMetrics::merge_provider_preferred(&provider, &model, 2_500);
        assert_eq!(
            selected
                .delta()
                .expect("fresh model fallback")
                .value_at(2_500),
            Ok(0.6)
        );
    }
}
