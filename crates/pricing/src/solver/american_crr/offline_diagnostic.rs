//! Bounded, non-authoritative American CRR diagnostics.
//!
//! This module exposes the same research candidate used by the regression
//! study. It never produces a production `SolverOutcome` or `OptionMetrics`.
//!
//! ## 简体中文
//!
//! 本模块公开回归研究使用的同一美式 CRR 候选算法，但不生成生产 `SolverOutcome` 或 `OptionMetrics`。

use std::time::{Duration, Instant};

use super::super::*;
use super::{MIN_AMERICAN_REMAINING_MILLIS, american_upper_bound, payoff};

const TREE_STUDY_MAX_STEPS: usize = 1025;
const TREE_PARITY_GAP_MAX: f64 = 0.05;
const TREE_PRICE_MAX: f64 = MAX_MODEL_PRICE;
const RICHARDSON_COARSE_STEPS: usize = 512;
const RICHARDSON_FINE_STEPS: usize = 1024;
const RICHARDSON_NODE_VISIT_LIMIT: u64 = 1_316_872;
const RICHARDSON_MAX_WALL_TIME: Duration = Duration::from_secs(1);

/// Explicit model-accuracy state for an offline American candidate.
/// 简体中文：离线美式候选的明确模型精度状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AmericanCrrOfflineDiagnosticAccuracy {
    /// No production accuracy budget has been accepted.
    /// 简体中文：尚未验收生产精度预算。
    AccuracyUnverified,
}

/// Explicit authority boundary for an offline American candidate.
/// 简体中文：离线美式候选的明确权威边界。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AmericanCrrOfflineDiagnosticAuthority {
    /// Research diagnostics only; not tradable and not risk authority.
    /// 简体中文：仅供研究诊断，不可交易，也不是风险权威数据。
    DiagnosticOnlyNotTradable,
}

/// Numerical procedure used for a bounded offline American candidate.
/// 简体中文：有界离线美式候选使用的数值方法。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AmericanCrrOfflineDiagnosticMethod {
    /// Richardson extrapolation of the 512/513 and 1024/1025 CRR pairs.
    /// 简体中文：对 512/513 与 1024/1025 步 CRR 配对执行 Richardson 外推。
    RichardsonCrr512To1024,
}

/// A separately typed, non-authoritative American price and native lattice Greeks.
///
/// Values are per one underlying unit. Theta is per ACT/365F model year. The
/// private fields prevent construction from arbitrary caller values; this type
/// intentionally has no conversion to `SolverOutcome` or `OptionMetrics`.
///
/// ## 简体中文
///
/// 独立类型表示非权威的美式价格和原生格点 Greeks，数值按一个标的单位计量，Theta
/// 按 ACT/365F 模型年计量。私有字段阻止调用方任意构造；该类型有意不提供到
/// `SolverOutcome` 或 `OptionMetrics` 的转换。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AmericanCrrOfflineDiagnostic {
    accuracy: AmericanCrrOfflineDiagnosticAccuracy,
    authority: AmericanCrrOfflineDiagnosticAuthority,
    method: AmericanCrrOfflineDiagnosticMethod,
    price_per_underlying_unit: f64,
    delta_per_underlying_unit: f64,
    gamma_per_underlying_dollar_squared: f64,
    theta_per_act365f_year_per_underlying_unit: f64,
    coarse_pair_gap: f64,
    fine_pair_gap: f64,
    node_visits: u64,
}

impl AmericanCrrOfflineDiagnostic {
    /// Returns the explicit unverified accuracy state.
    /// 简体中文：返回明确的未验证精度状态。
    pub const fn accuracy(&self) -> AmericanCrrOfflineDiagnosticAccuracy {
        self.accuracy
    }

    /// Returns the explicit non-tradable diagnostic authority state.
    /// 简体中文：返回明确的不可交易诊断权威状态。
    pub const fn authority(&self) -> AmericanCrrOfflineDiagnosticAuthority {
        self.authority
    }

    /// Returns the numerical method identifier.
    /// 简体中文：返回数值方法标识。
    pub const fn method(&self) -> AmericanCrrOfflineDiagnosticMethod {
        self.method
    }

    /// Returns the candidate price per one underlying unit.
    /// 简体中文：返回每一个标的单位的候选价格。
    pub const fn price_per_underlying_unit(&self) -> f64 {
        self.price_per_underlying_unit
    }

    /// Returns native lattice Delta per one underlying unit.
    /// 简体中文：返回每一个标的单位的原生格点 Delta。
    pub const fn delta_per_underlying_unit(&self) -> f64 {
        self.delta_per_underlying_unit
    }

    /// Returns native lattice Gamma per squared underlying-dollar move.
    /// 简体中文：返回每标的美元变动平方的原生格点 Gamma。
    pub const fn gamma_per_underlying_dollar_squared(&self) -> f64 {
        self.gamma_per_underlying_dollar_squared
    }

    /// Returns native lattice Theta per ACT/365F model year and underlying unit.
    /// 简体中文：返回每 ACT/365F 模型年、每标的单位的原生格点 Theta。
    pub const fn theta_per_act365f_year_per_underlying_unit(&self) -> f64 {
        self.theta_per_act365f_year_per_underlying_unit
    }

    /// Returns the coarse 512/513 adjacent-tree price gap.
    /// 简体中文：返回 512/513 粗网格相邻树价格差。
    pub const fn coarse_pair_gap(&self) -> f64 {
        self.coarse_pair_gap
    }

    /// Returns the fine 1024/1025 adjacent-tree price gap.
    /// 简体中文：返回 1024/1025 细网格相邻树价格差。
    pub const fn fine_pair_gap(&self) -> f64 {
        self.fine_pair_gap
    }

    /// Returns all CRR node visits for price and Greeks under the shared budget.
    /// 简体中文：返回价格和 Greeks 共用预算下全部 CRR 节点访问数。
    pub const fn node_visits(&self) -> u64 {
        self.node_visits
    }

    /// Returns the fixed maximum CRR node visits for one diagnostic evaluation.
    /// 简体中文：返回单次诊断评估固定的 CRR 最大节点访问数。
    pub const fn node_visit_limit(&self) -> u64 {
        RICHARDSON_NODE_VISIT_LIMIT
    }
}

/// Typed rejection from the offline American diagnostic path.
/// 简体中文：离线美式诊断路径的类型化拒绝原因。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AmericanCrrOfflineDiagnosticError {
    /// Input freshness, expiry, or evidence validation failed.
    /// 简体中文：输入新鲜度、到期时间或证据校验失败。
    Input(SolverInputError),
    /// The input does not declare American exercise.
    /// 简体中文：输入未声明美式行权。
    UnsupportedExerciseStyle,
    /// Exact expiry has no positive-time lattice value.
    /// 简体中文：精确到期时没有正剩余时间格点价值。
    ExactExpiryUnsupported,
    /// Remaining time is below the one-minute numerical-resolution boundary.
    /// 简体中文：剩余时间低于一分钟数值分辨率边界。
    BelowMinimumResolution,
    /// Discrete or unknown cash-dividend schedules are not modeled.
    /// 简体中文：不支持离散或未知现金股息日程。
    DiscreteDividendScheduleUnsupported,
    /// Volatility is outside the existing solver bounds `[0.0001, 5.0]`.
    /// 简体中文：波动率超出既有求解器范围 `[0.0001, 5.0]`。
    VolatilityOutOfRange,
    /// The supplied option price is outside American intrinsic/value bounds.
    /// 简体中文：输入期权价格超出美式内在价值或理论价值边界。
    MarketPriceOutsideAmericanBounds,
    /// Inputs lie outside the existing analytical numerical envelope.
    /// 简体中文：输入超出既有解析模型数值范围。
    ModelInputOutOfRange,
    /// The caller's deadline or the one-second hard cap was reached.
    /// 简体中文：达到调用方截止时刻或固定一秒硬上限。
    DeadlineExceeded,
    /// The shared fixed tree-node budget was exhausted.
    /// 简体中文：价格与 Greeks 共用的固定树节点预算耗尽。
    NodeBudgetExceeded,
    /// A CRR tree did not have a finite risk-neutral transition probability.
    /// 简体中文：CRR 树无法得到范围内的有限风险中性转移概率。
    RiskNeutralProbabilityOutOfRange,
    /// An adjacent-step pair exceeded its fixed `$0.05` parity screen.
    /// 简体中文：相邻步数配对超过固定 `$0.05` 差异筛选。
    AdjacentStepGapExceeded {
        /// The even step count whose `(N, N + 1)` pair failed.
        /// 简体中文：未通过筛选的 `(N, N + 1)` 配对中的偶数步数。
        even_steps: usize,
        /// Absolute price gap between the adjacent trees.
        /// 简体中文：相邻树之间的绝对价格差。
        gap: f64,
    },
    /// Richardson extrapolation left the theoretical American value bounds.
    /// 简体中文：Richardson 外推结果超出美式理论价值边界。
    ExtrapolatedPriceOutsideAmericanBounds,
    /// A bounded intermediate calculation was non-finite or out of range.
    /// 简体中文：有界中间计算出现非有限值或超出范围。
    NonFiniteCalculation,
    /// The lattice did not produce all three native Greeks.
    /// 简体中文：格点未生成全部三个原生 Greeks。
    NativeGreeksUnavailable,
}

impl std::fmt::Display for AmericanCrrOfflineDiagnosticError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Input(error) => return write!(formatter, "{error}"),
            Self::UnsupportedExerciseStyle => "AMERICAN_DIAGNOSTIC_EXERCISE_UNSUPPORTED",
            Self::ExactExpiryUnsupported => "AMERICAN_DIAGNOSTIC_EXACT_EXPIRY_UNSUPPORTED",
            Self::BelowMinimumResolution => "AMERICAN_DIAGNOSTIC_BELOW_MINIMUM_RESOLUTION",
            Self::DiscreteDividendScheduleUnsupported => {
                "AMERICAN_DIAGNOSTIC_DISCRETE_DIVIDEND_UNSUPPORTED"
            }
            Self::VolatilityOutOfRange => "AMERICAN_DIAGNOSTIC_VOLATILITY_OUT_OF_RANGE",
            Self::MarketPriceOutsideAmericanBounds => {
                "AMERICAN_DIAGNOSTIC_MARKET_PRICE_OUTSIDE_BOUNDS"
            }
            Self::ModelInputOutOfRange => "AMERICAN_DIAGNOSTIC_MODEL_INPUT_OUT_OF_RANGE",
            Self::DeadlineExceeded => "AMERICAN_DIAGNOSTIC_DEADLINE_EXCEEDED",
            Self::NodeBudgetExceeded => "AMERICAN_DIAGNOSTIC_NODE_BUDGET_EXCEEDED",
            Self::RiskNeutralProbabilityOutOfRange => {
                "AMERICAN_DIAGNOSTIC_RISK_NEUTRAL_PROBABILITY_OUT_OF_RANGE"
            }
            Self::AdjacentStepGapExceeded { .. } => "AMERICAN_DIAGNOSTIC_STEP_GAP_EXCEEDED",
            Self::ExtrapolatedPriceOutsideAmericanBounds => {
                "AMERICAN_DIAGNOSTIC_EXTRAPOLATED_PRICE_OUTSIDE_BOUNDS"
            }
            Self::NonFiniteCalculation => "AMERICAN_DIAGNOSTIC_NON_FINITE_CALCULATION",
            Self::NativeGreeksUnavailable => "AMERICAN_DIAGNOSTIC_NATIVE_GREEKS_UNAVAILABLE",
        })
    }
}

impl std::error::Error for AmericanCrrOfflineDiagnosticError {}

/// Evaluate an explicitly offline American CRR research diagnostic.
///
/// The input must carry fresh quote and matching dividend-window evidence at
/// `evaluation_at_ms`. The bounded model accepts the existing solver envelope:
/// spot and strike `(0, $1,000,000]`, ACT/365F remaining time from 60 seconds
/// through 10 years, continuously compounded rate/yield `[-1, 1]`, and
/// volatility `[0.0001, 5.0]`. It assumes constant volatility and either a
/// covered no-cash-dividend interval or a continuous-yield approximation;
/// discrete and unknown cash-dividend schedules are rejected. The caller's
/// absolute monotonic deadline is capped to one second after entry. A single
/// 1,316,872-node budget covers the four CRR trees; price and native Delta,
/// Gamma, and Theta are extracted from those same trees, so Greeks add no
/// hidden grid work. This is not an accuracy guarantee or trading authority.
///
/// ## 简体中文
///
/// 显式计算离线美式 CRR 研究诊断。输入必须在 `evaluation_at_ms` 时持有新鲜行情及匹配的
/// 股息窗口证据。模型范围复用既有求解器约束：标的与行权价 `(0, $1,000,000]`，ACT/365F
/// 剩余时间 60 秒至 10 年，连续复利利率/收益率 `[-1, 1]`，波动率 `[0.0001, 5.0]`。
/// 模型假设波动率恒定，且股息为有证据覆盖的无现金股息区间或连续收益率近似；离散和
/// 未知现金股息日程会被拒绝。调用方单调时钟绝对截止时刻最多放宽到调用开始后 1 秒。
/// 四棵 CRR 树共用 1,316,872 个节点预算；价格及原生 Delta、Gamma、Theta 都从相同树中
/// 提取，不会额外运行 Greeks 网格。本结果不保证精度，也不构成交易授权。
pub fn evaluate_american_crr_offline_diagnostic(
    input: &SolverInput,
    volatility_fraction: f64,
    evaluation_at_ms: i64,
    absolute_deadline: Instant,
) -> Result<AmericanCrrOfflineDiagnostic, AmericanCrrOfflineDiagnosticError> {
    let started_at = Instant::now();
    if absolute_deadline <= started_at {
        return Err(AmericanCrrOfflineDiagnosticError::DeadlineExceeded);
    }
    let Some(hard_deadline) = started_at.checked_add(RICHARDSON_MAX_WALL_TIME) else {
        return Err(AmericanCrrOfflineDiagnosticError::DeadlineExceeded);
    };
    let mut budget = CandidateBudget::new(
        absolute_deadline.min(hard_deadline),
        RICHARDSON_NODE_VISIT_LIMIT,
    );

    input
        .validate_fresh_at(evaluation_at_ms)
        .map_err(AmericanCrrOfflineDiagnosticError::Input)?;
    if !budget.has_time() {
        return Err(AmericanCrrOfflineDiagnosticError::DeadlineExceeded);
    }
    if input.exercise_style != ExerciseStyle::American {
        return Err(AmericanCrrOfflineDiagnosticError::UnsupportedExerciseStyle);
    }
    let remaining_millis = input
        .expiration
        .expiration
        .utc_epoch_millis
        .checked_sub(evaluation_at_ms)
        .ok_or(AmericanCrrOfflineDiagnosticError::ModelInputOutOfRange)?;
    if remaining_millis == 0 {
        return Err(AmericanCrrOfflineDiagnosticError::ExactExpiryUnsupported);
    }
    if remaining_millis < MIN_AMERICAN_REMAINING_MILLIS {
        return Err(AmericanCrrOfflineDiagnosticError::BelowMinimumResolution);
    }
    if input.dividend_assumption == DividendAssumption::DiscreteScheduleUnknown {
        return Err(AmericanCrrOfflineDiagnosticError::DiscreteDividendScheduleUnsupported);
    }
    if !volatility_fraction.is_finite()
        || !(MIN_VOLATILITY..=MAX_VOLATILITY).contains(&volatility_fraction)
    {
        return Err(AmericanCrrOfflineDiagnosticError::VolatilityOutOfRange);
    }

    let years = remaining_millis as f64 / MILLIS_PER_YEAR_ACT_365F;
    let dividend_yield = match input.dividend_assumption {
        DividendAssumption::NoDividends => 0.0,
        DividendAssumption::ContinuousYield(value) => value,
        DividendAssumption::DiscreteScheduleUnknown => {
            return Err(AmericanCrrOfflineDiagnosticError::DiscreteDividendScheduleUnsupported);
        }
    };
    let (spot, strike, market_price) = input.values();
    let candidate_input = CandidateInputs {
        kind: input.option_kind,
        spot,
        strike,
        rate: input.risk_free_rate,
        dividend_yield,
        years,
        volatility: volatility_fraction,
    };
    if !richardson_candidate_domain_supported(candidate_input) {
        return Err(AmericanCrrOfflineDiagnosticError::ModelInputOutOfRange);
    }
    let intrinsic = payoff(input.option_kind, spot, strike);
    let upper_bound = american_upper_bound(
        input.option_kind,
        spot,
        strike,
        input.risk_free_rate,
        dividend_yield,
        years,
    )
    .ok_or(AmericanCrrOfflineDiagnosticError::NonFiniteCalculation)?;
    if !intrinsic.is_finite()
        || market_price + PRICE_TOLERANCE < intrinsic
        || market_price > upper_bound + PRICE_TOLERANCE
    {
        return Err(AmericanCrrOfflineDiagnosticError::MarketPriceOutsideAmericanBounds);
    }

    let candidate = richardson_candidate_result_until(candidate_input, &mut budget)
        .map_err(AmericanCrrOfflineDiagnosticError::from)?;
    if !budget.has_time() {
        return Err(AmericanCrrOfflineDiagnosticError::DeadlineExceeded);
    }
    let native_greeks = candidate
        .native_greeks
        .ok_or(AmericanCrrOfflineDiagnosticError::NativeGreeksUnavailable)?;
    Ok(AmericanCrrOfflineDiagnostic {
        accuracy: AmericanCrrOfflineDiagnosticAccuracy::AccuracyUnverified,
        authority: AmericanCrrOfflineDiagnosticAuthority::DiagnosticOnlyNotTradable,
        method: AmericanCrrOfflineDiagnosticMethod::RichardsonCrr512To1024,
        price_per_underlying_unit: candidate.price,
        delta_per_underlying_unit: native_greeks.delta,
        gamma_per_underlying_dollar_squared: native_greeks.gamma,
        theta_per_act365f_year_per_underlying_unit: native_greeks.theta,
        coarse_pair_gap: candidate.coarse_pair_gap,
        fine_pair_gap: candidate.fine_pair_gap,
        node_visits: budget.node_visits,
    })
}

impl From<CandidateFailure> for AmericanCrrOfflineDiagnosticError {
    fn from(failure: CandidateFailure) -> Self {
        match failure {
            CandidateFailure::ModelInputOutOfRange => Self::ModelInputOutOfRange,
            CandidateFailure::DeadlineExceeded => Self::DeadlineExceeded,
            CandidateFailure::NodeBudgetExceeded => Self::NodeBudgetExceeded,
            CandidateFailure::RiskNeutralProbabilityOutOfRange => {
                Self::RiskNeutralProbabilityOutOfRange
            }
            CandidateFailure::AdjacentStepGapExceeded { even_steps, gap } => {
                Self::AdjacentStepGapExceeded { even_steps, gap }
            }
            CandidateFailure::ExtrapolatedPriceOutsideAmericanBounds => {
                Self::ExtrapolatedPriceOutsideAmericanBounds
            }
            CandidateFailure::NonFiniteCalculation => Self::NonFiniteCalculation,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct CandidateInputs {
    pub(super) kind: OptionKind,
    pub(super) spot: f64,
    pub(super) strike: f64,
    pub(super) rate: f64,
    pub(super) dividend_yield: f64,
    pub(super) years: f64,
    pub(super) volatility: f64,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct CandidateGreeks {
    pub(super) delta: f64,
    pub(super) gamma: f64,
    pub(super) theta: f64,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct CandidateTreeResult {
    pub(super) price: f64,
    pub(super) native_greeks: Option<CandidateGreeks>,
    pair_gap: Option<f64>,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct CandidateRichardsonResult {
    pub(super) price: f64,
    pub(super) native_greeks: Option<CandidateGreeks>,
    pub(super) coarse_pair_gap: f64,
    pub(super) fine_pair_gap: f64,
}

pub(super) struct CandidateBudget {
    deadline: Instant,
    node_visit_limit: u64,
    pub(super) node_visits: u64,
}

impl CandidateBudget {
    pub(super) fn new(deadline: Instant, node_visit_limit: u64) -> Self {
        let started_at = Instant::now();
        let hard_deadline = started_at
            .checked_add(RICHARDSON_MAX_WALL_TIME)
            .unwrap_or(started_at);
        Self {
            deadline: deadline.min(hard_deadline),
            node_visit_limit,
            node_visits: 0,
        }
    }

    pub(super) fn has_time(&self) -> bool {
        Instant::now() < self.deadline
    }

    fn visit_node(&mut self) -> Result<(), CandidateFailure> {
        if self.node_visits >= self.node_visit_limit {
            return Err(CandidateFailure::NodeBudgetExceeded);
        }
        self.node_visits += 1;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum CandidateFailure {
    ModelInputOutOfRange,
    DeadlineExceeded,
    NodeBudgetExceeded,
    RiskNeutralProbabilityOutOfRange,
    AdjacentStepGapExceeded { even_steps: usize, gap: f64 },
    ExtrapolatedPriceOutsideAmericanBounds,
    NonFiniteCalculation,
}

#[cfg(test)]
pub(super) fn crr_tree_result(input: CandidateInputs, steps: usize) -> Option<CandidateTreeResult> {
    crr_tree_result_with_budget(input, steps, None).ok()
}

fn crr_tree_result_with_budget(
    input: CandidateInputs,
    steps: usize,
    mut budget: Option<&mut CandidateBudget>,
) -> Result<CandidateTreeResult, CandidateFailure> {
    let CandidateInputs {
        kind,
        spot,
        strike,
        rate,
        dividend_yield,
        years,
        volatility,
    } = input;
    if steps == 0
        || steps > TREE_STUDY_MAX_STEPS
        || ![spot, strike, rate, dividend_yield, years, volatility]
            .into_iter()
            .all(f64::is_finite)
        || spot <= 0.0
        || strike <= 0.0
        || years <= 0.0
        || volatility <= 0.0
    {
        return Err(CandidateFailure::ModelInputOutOfRange);
    }
    let dt = years / steps as f64;
    let sigma_step = volatility * dt.sqrt();
    let up = sigma_step.exp();
    let down = (-sigma_step).exp();
    let drift = ((rate - dividend_yield) * dt).exp();
    let probability_up = (drift - down) / (up - down);
    let discount = (-rate * dt).exp();
    if ![dt, up, down, drift, probability_up, discount]
        .into_iter()
        .all(f64::is_finite)
        || dt <= 0.0
        || up <= down
        || discount <= 0.0
    {
        return Err(CandidateFailure::NonFiniteCalculation);
    }
    if !(0.0..=1.0).contains(&probability_up) {
        return Err(CandidateFailure::RiskNeutralProbabilityOutOfRange);
    }

    let mut values = vec![0.0; steps + 1];
    let node_ratio = up / down;
    let mut terminal_spot = spot * down.powi(steps as i32);
    for (index, value) in values.iter_mut().enumerate() {
        if index % 64 == 0 {
            budget_check_time(&mut budget)?;
        }
        budget_visit_node(&mut budget)?;
        if !terminal_spot.is_finite() {
            return Err(CandidateFailure::NonFiniteCalculation);
        }
        *value = payoff(kind, terminal_spot, strike);
        terminal_spot *= node_ratio;
    }
    let mut values_at_one_step = None;
    let mut values_at_two_steps = None;
    for time_index in (0..steps).rev() {
        budget_check_time(&mut budget)?;
        let mut node_spot = spot * down.powi(time_index as i32);
        for index in 0..=time_index {
            if index % 64 == 0 {
                budget_check_time(&mut budget)?;
            }
            budget_visit_node(&mut budget)?;
            let continuation = discount
                * (probability_up * values[index + 1] + (1.0 - probability_up) * values[index]);
            let value = continuation.max(payoff(kind, node_spot, strike));
            if !value.is_finite() || value < 0.0 {
                return Err(CandidateFailure::NonFiniteCalculation);
            }
            values[index] = value;
            node_spot *= node_ratio;
        }
        if time_index == 1 {
            values_at_one_step = Some([values[0], values[1]]);
        } else if time_index == 2 {
            values_at_two_steps = Some([values[0], values[1], values[2]]);
        }
    }
    let price = values
        .first()
        .copied()
        .filter(|value| value.is_finite() && *value <= TREE_PRICE_MAX)
        .ok_or(CandidateFailure::NonFiniteCalculation)?;
    let native_greeks = match (values_at_one_step, values_at_two_steps) {
        (Some([down_value, up_value]), Some([down_down_value, center_value, up_up_value])) => {
            let spot_down = spot * down;
            let spot_up = spot * up;
            let spot_down_down = spot * down * down;
            let spot_up_up = spot * up * up;
            let delta_denominator = spot_up - spot_down;
            let up_delta_denominator = spot_up_up - spot * down * up;
            let down_delta_denominator = spot * down * up - spot_down_down;
            let gamma_denominator = 0.5 * (spot_up_up - spot_down_down);
            let denominators = [
                spot_down,
                spot_up,
                spot_down_down,
                spot_up_up,
                delta_denominator,
                up_delta_denominator,
                down_delta_denominator,
                gamma_denominator,
            ];
            if !denominators.into_iter().all(f64::is_finite)
                || delta_denominator <= 0.0
                || up_delta_denominator <= 0.0
                || down_delta_denominator <= 0.0
                || gamma_denominator <= 0.0
            {
                None
            } else {
                let delta = (up_value - down_value) / delta_denominator;
                let up_delta = (up_up_value - center_value) / up_delta_denominator;
                let down_delta = (center_value - down_down_value) / down_delta_denominator;
                let gamma = (up_delta - down_delta) / gamma_denominator;
                let theta = (center_value - price) / (2.0 * dt);
                [delta, gamma, theta]
                    .into_iter()
                    .all(f64::is_finite)
                    .then_some(CandidateGreeks {
                        delta,
                        gamma,
                        theta,
                    })
            }
        }
        _ => None,
    };
    Ok(CandidateTreeResult {
        price,
        native_greeks,
        pair_gap: None,
    })
}

fn budget_check_time(budget: &mut Option<&mut CandidateBudget>) -> Result<(), CandidateFailure> {
    if budget.as_deref().is_some_and(|budget| !budget.has_time()) {
        Err(CandidateFailure::DeadlineExceeded)
    } else {
        Ok(())
    }
}

fn budget_visit_node(budget: &mut Option<&mut CandidateBudget>) -> Result<(), CandidateFailure> {
    match budget.as_deref_mut() {
        Some(budget) => budget.visit_node(),
        None => Ok(()),
    }
}

#[cfg(test)]
pub(super) fn crr_tree_price(input: CandidateInputs, steps: usize) -> Option<f64> {
    crr_tree_result(input, steps).map(|result| result.price)
}

#[cfg(test)]
pub(super) fn crr_tree_node_visits(steps: usize) -> u64 {
    let steps = steps as u64;
    steps + 1 + steps * (steps + 1) / 2
}

#[cfg(test)]
pub(super) fn paired_candidate_price(input: CandidateInputs) -> Option<f64> {
    paired_candidate_price_with_steps(input, 256)
}

#[cfg(test)]
pub(super) fn paired_candidate_price_with_steps(
    input: CandidateInputs,
    even_steps: usize,
) -> Option<f64> {
    paired_candidate_result_with_steps(input, even_steps).map(|result| result.price)
}

#[cfg(test)]
pub(super) fn paired_candidate_result_with_steps(
    input: CandidateInputs,
    even_steps: usize,
) -> Option<CandidateTreeResult> {
    let even = crr_tree_result(input, even_steps)?;
    let odd = crr_tree_result(input, even_steps + 1)?;
    paired_candidate_result_from_trees(even, odd, even_steps).ok()
}

fn paired_candidate_result_with_budget(
    input: CandidateInputs,
    even_steps: usize,
    budget: &mut CandidateBudget,
) -> Result<CandidateTreeResult, CandidateFailure> {
    let even = crr_tree_result_with_budget(input, even_steps, Some(&mut *budget))?;
    let odd = crr_tree_result_with_budget(input, even_steps + 1, Some(&mut *budget))?;
    paired_candidate_result_from_trees(even, odd, even_steps)
}

fn paired_candidate_result_from_trees(
    even: CandidateTreeResult,
    odd: CandidateTreeResult,
    even_steps: usize,
) -> Result<CandidateTreeResult, CandidateFailure> {
    let pair_gap = (even.price - odd.price).abs();
    if pair_gap > TREE_PARITY_GAP_MAX {
        return Err(CandidateFailure::AdjacentStepGapExceeded {
            even_steps,
            gap: pair_gap,
        });
    }
    let native_greeks = match (even.native_greeks, odd.native_greeks) {
        (Some(even), Some(odd)) => {
            let greeks = CandidateGreeks {
                delta: (even.delta + odd.delta) / 2.0,
                gamma: (even.gamma + odd.gamma) / 2.0,
                theta: (even.theta + odd.theta) / 2.0,
            };
            [greeks.delta, greeks.gamma, greeks.theta]
                .into_iter()
                .all(f64::is_finite)
                .then_some(greeks)
        }
        _ => None,
    };
    Ok(CandidateTreeResult {
        price: (even.price + odd.price) / 2.0,
        native_greeks,
        pair_gap: Some(pair_gap),
    })
}

pub(super) fn richardson_candidate_domain_supported(input: CandidateInputs) -> bool {
    let minimum_years = MIN_AMERICAN_REMAINING_MILLIS as f64 / MILLIS_PER_YEAR_ACT_365F;
    [
        input.spot,
        input.strike,
        input.rate,
        input.dividend_yield,
        input.years,
        input.volatility,
    ]
    .into_iter()
    .all(f64::is_finite)
        && input.spot > 0.0
        && input.spot <= TREE_PRICE_MAX
        && input.strike > 0.0
        && input.strike <= TREE_PRICE_MAX
        && (minimum_years..=MAX_MODEL_TIME_YEARS).contains(&input.years)
        && input.rate.abs() <= MAX_MODEL_RATE_ABS
        && input.dividend_yield.abs() <= MAX_MODEL_RATE_ABS
        && (MIN_VOLATILITY..=MAX_VOLATILITY).contains(&input.volatility)
        && american_upper_bound(
            input.kind,
            input.spot,
            input.strike,
            input.rate,
            input.dividend_yield,
            input.years,
        )
        .is_some()
}

#[cfg(test)]
pub(super) fn richardson_candidate_price_until(
    input: CandidateInputs,
    budget: &mut CandidateBudget,
) -> Option<f64> {
    richardson_candidate_result_until(input, budget)
        .ok()
        .map(|result| result.price)
}

pub(super) fn richardson_candidate_result_until(
    input: CandidateInputs,
    budget: &mut CandidateBudget,
) -> Result<CandidateRichardsonResult, CandidateFailure> {
    if !richardson_candidate_domain_supported(input) {
        return Err(CandidateFailure::ModelInputOutOfRange);
    }
    if !budget.has_time() {
        return Err(CandidateFailure::DeadlineExceeded);
    }
    let coarse = paired_candidate_result_with_budget(input, RICHARDSON_COARSE_STEPS, budget)?;
    let fine = paired_candidate_result_with_budget(input, RICHARDSON_FINE_STEPS, budget)?;
    let extrapolated = 2.0 * fine.price - coarse.price;
    let intrinsic = payoff(input.kind, input.spot, input.strike);
    let upper_bound = american_upper_bound(
        input.kind,
        input.spot,
        input.strike,
        input.rate,
        input.dividend_yield,
        input.years,
    )
    .ok_or(CandidateFailure::NonFiniteCalculation)?;
    if !extrapolated.is_finite() || !budget.has_time() {
        return Err(if budget.has_time() {
            CandidateFailure::NonFiniteCalculation
        } else {
            CandidateFailure::DeadlineExceeded
        });
    }
    if extrapolated < intrinsic - PRICE_TOLERANCE || extrapolated > upper_bound + PRICE_TOLERANCE {
        return Err(CandidateFailure::ExtrapolatedPriceOutsideAmericanBounds);
    }
    let native_greeks = match (coarse.native_greeks, fine.native_greeks) {
        (Some(coarse), Some(fine)) => {
            let greeks = CandidateGreeks {
                delta: 2.0 * fine.delta - coarse.delta,
                gamma: 2.0 * fine.gamma - coarse.gamma,
                theta: 2.0 * fine.theta - coarse.theta,
            };
            [greeks.delta, greeks.gamma, greeks.theta]
                .into_iter()
                .all(f64::is_finite)
                .then_some(greeks)
        }
        _ => None,
    };
    Ok(CandidateRichardsonResult {
        price: extrapolated,
        native_greeks,
        coarse_pair_gap: coarse
            .pair_gap
            .ok_or(CandidateFailure::NonFiniteCalculation)?,
        fine_pair_gap: fine
            .pair_gap
            .ok_or(CandidateFailure::NonFiniteCalculation)?,
    })
}
