//! American pricing boundary with a fail-closed independent-accuracy gate.
//!
//! The experimental CRR lattice is compiled only in unit tests. Production
//! callers receive a typed unavailable result until independently generated
//! price and IV references establish a frozen error budget.
//!
//! ## 简体中文
//!
//! 本模块提供带独立精度门的美式定价边界。实验性 CRR 二叉树仅在单元测试中编译；独立生成价格和 IV 参考并冻结误差预算前，生产调用会收到类型化不可用结果。

use super::*;

const MIN_AMERICAN_REMAINING_MILLIS: i64 = 60_000;

/// Evaluate the American model boundary at an explicit UTC millisecond time.
///
/// Exact expiry returns intrinsic value without Greeks. Positive-time American
/// prices and implied volatility remain unavailable until a pinned independent
/// oracle has been run, an error budget has been frozen, and the supported
/// dividend schedule is qualified by the upstream contract/data adapter.
/// Inputs outside the theoretical American payoff bounds are rejected first.
/// `evaluation_at_ms` is the explicit UTC epoch-millisecond time used to
/// recheck quote and dividend evidence.
///
/// ## 简体中文
///
/// 在显式 UTC 毫秒时刻评估美式模型边界。精确到期只返回内在价值且不返回 Greeks。独立 oracle 尚未运行、误差预算尚未冻结、上游合约/行情适配器尚未核验支持的股息日程前，正剩余时间的美式价格和隐含波动率均不可用；理论 payoff 上下界之外的输入会先被拒绝。`evaluation_at_ms` 使用 UTC Unix 毫秒并用于重新核验行情和股息证据。
pub fn solve_american_crr(input: &SolverInput, evaluation_at_ms: i64) -> SolverOutcome {
    if let Err(error) = input.validate_fresh_at(evaluation_at_ms) {
        return input_error_outcome(error);
    }
    if input.is_at_expiry() {
        return expiry_intrinsic(input);
    }
    if input.exercise_style != ExerciseStyle::American {
        return SolverOutcome::Unavailable(ModelUnavailable::AmericanExerciseUnsupported);
    }
    let Some(remaining_millis) = input
        .expiration
        .expiration
        .utc_epoch_millis
        .checked_sub(evaluation_at_ms)
    else {
        return SolverOutcome::Unavailable(ModelUnavailable::CalculationOutOfRange);
    };
    if remaining_millis < MIN_AMERICAN_REMAINING_MILLIS {
        return SolverOutcome::Unavailable(ModelUnavailable::TimeBelowResolution);
    }
    if input.dividend_assumption == DividendAssumption::DiscreteScheduleUnknown {
        return SolverOutcome::Unavailable(ModelUnavailable::DividendAssumptionUnsupported);
    }
    let dividend_yield = match input.dividend_assumption {
        DividendAssumption::NoDividends => 0.0,
        DividendAssumption::ContinuousYield(value) => value,
        DividendAssumption::DiscreteScheduleUnknown => {
            return SolverOutcome::Unavailable(ModelUnavailable::DividendAssumptionUnsupported);
        }
    };
    let (spot, strike, market_price) = input.values();
    let intrinsic = payoff(input.option_kind, spot, strike);
    let Some(upper_bound) = american_upper_bound(
        input.option_kind,
        spot,
        strike,
        input.risk_free_rate,
        dividend_yield,
        input.years_to_expiry,
    ) else {
        return SolverOutcome::Unavailable(ModelUnavailable::CalculationOutOfRange);
    };
    if !intrinsic.is_finite()
        || market_price + PRICE_TOLERANCE < intrinsic
        || market_price > upper_bound + PRICE_TOLERANCE
    {
        return SolverOutcome::Unavailable(ModelUnavailable::PriceOutsideModelBounds);
    }
    SolverOutcome::Unavailable(ModelUnavailable::AmericanPricingAccuracyUnverified)
}

/// Conservative model-independent upper bound, including negative-rate put value.
/// 简体中文：保守的理论价格上界，覆盖负利率下的看跌期权价值。
fn american_upper_bound(
    kind: OptionKind,
    spot: f64,
    strike: f64,
    rate: f64,
    dividend_yield: f64,
    years: f64,
) -> Option<f64> {
    if ![spot, strike, rate, dividend_yield, years]
        .into_iter()
        .all(f64::is_finite)
        || spot <= 0.0
        || strike <= 0.0
        || years < 0.0
    {
        return None;
    }
    let bound = match kind {
        OptionKind::Call if dividend_yield >= 0.0 => spot,
        OptionKind::Call => spot * (-dividend_yield * years).exp(),
        OptionKind::Put => strike * ((-rate).max(0.0) * years).exp(),
    };
    bound.is_finite().then_some(bound)
}

fn payoff(kind: OptionKind, spot: f64, strike: f64) -> f64 {
    match kind {
        OptionKind::Call => (spot - strike).max(0.0),
        OptionKind::Put => (strike - spot).max(0.0),
    }
}

#[cfg(test)]
mod experimental_candidate {
    use super::*;

    const TREE_STEPS_EVEN: usize = 256;
    const TREE_STEPS_ODD: usize = 257;
    const TREE_PARITY_GAP_MAX: f64 = 0.05;
    const TREE_PRICE_MAX: f64 = 1_000_000.0;

    /// Inputs for the bounded test-only American tree candidate.
    /// 简体中文：有界且仅供测试使用的美式二叉树候选模型输入。
    #[derive(Clone, Copy)]
    struct CandidateInputs {
        kind: OptionKind,
        spot: f64,
        strike: f64,
        rate: f64,
        dividend_yield: f64,
        years: f64,
        volatility: f64,
    }

    fn crr_tree_price(input: CandidateInputs, steps: usize) -> Option<f64> {
        let CandidateInputs {
            kind,
            spot,
            strike,
            rate,
            dividend_yield,
            years,
            volatility,
        } = input;
        if !matches!(steps, TREE_STEPS_EVEN | TREE_STEPS_ODD)
            || ![spot, strike, rate, dividend_yield, years, volatility]
                .into_iter()
                .all(f64::is_finite)
            || spot <= 0.0
            || strike <= 0.0
            || years <= 0.0
            || volatility <= 0.0
        {
            return None;
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
            || !(0.0..=1.0).contains(&probability_up)
            || discount <= 0.0
        {
            return None;
        }

        let mut values = vec![0.0; steps + 1];
        let node_ratio = up / down;
        let mut terminal_spot = spot * down.powi(steps as i32);
        for value in &mut values {
            if !terminal_spot.is_finite() {
                return None;
            }
            *value = payoff(kind, terminal_spot, strike);
            terminal_spot *= node_ratio;
        }
        for time_index in (0..steps).rev() {
            let mut node_spot = spot * down.powi(time_index as i32);
            for index in 0..=time_index {
                let continuation = discount
                    * (probability_up * values[index + 1] + (1.0 - probability_up) * values[index]);
                let value = continuation.max(payoff(kind, node_spot, strike));
                if !value.is_finite() || value < 0.0 {
                    return None;
                }
                values[index] = value;
                node_spot *= node_ratio;
            }
        }
        values
            .first()
            .copied()
            .filter(|value| value.is_finite() && *value <= TREE_PRICE_MAX)
    }

    fn paired_candidate_price(input: CandidateInputs) -> Option<f64> {
        let even = crr_tree_price(input, TREE_STEPS_EVEN)?;
        let odd = crr_tree_price(input, TREE_STEPS_ODD)?;
        if (even - odd).abs() > TREE_PARITY_GAP_MAX {
            return None;
        }
        Some((even + odd) / 2.0)
    }

    #[test]
    fn negative_rate_put_upper_bound_can_exceed_strike() {
        let bound = super::american_upper_bound(OptionKind::Put, 0.0001, 100.0, -0.5, 0.0, 2.0)
            .expect("finite theoretical American put bound");
        assert!(bound > 100.0);
        assert!((bound - 100.0 * 1.0_f64.exp()).abs() < 1.0e-12);
    }

    #[test]
    fn experimental_tree_work_and_values_stay_inside_fixed_bounds() {
        let value = paired_candidate_price(CandidateInputs {
            kind: OptionKind::Put,
            spot: 40.0,
            strike: 40.0,
            rate: 0.0488,
            dividend_yield: 0.0,
            years: 1.0 / 3.0,
            volatility: 0.3,
        })
        .expect("bounded adjacent-step American tree candidate");
        assert!(value.is_finite());
        assert!((0.0..=TREE_PRICE_MAX).contains(&value));
        assert!(
            paired_candidate_price(CandidateInputs {
                kind: OptionKind::Call,
                spot: 100.0,
                strike: 100.0,
                rate: 1.0,
                dividend_yield: -1.0,
                years: 0.01,
                volatility: 0.0001,
            })
            .is_none()
        );
    }
}
