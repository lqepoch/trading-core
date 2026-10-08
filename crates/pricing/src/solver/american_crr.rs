//! American pricing boundary with a fail-closed independent-accuracy gate.
//!
//! Positive-time production solving remains fail-closed. A separate bounded
//! offline diagnostic exposes a research candidate with explicitly unverified
//! accuracy and no trading authority.
//!
//! ## 简体中文
//!
//! 本模块提供带独立精度门的美式定价边界。正剩余时间的生产求解仍失败关闭；独立离线诊断接口只公开精度未验证、无交易权威的研究候选。

use super::*;

#[path = "american_crr/offline_diagnostic.rs"]
mod offline_diagnostic;

pub use offline_diagnostic::{
    AmericanCrrOfflineDiagnostic, AmericanCrrOfflineDiagnosticAccuracy,
    AmericanCrrOfflineDiagnosticAuthority, AmericanCrrOfflineDiagnosticError,
    AmericanCrrOfflineDiagnosticMethod, evaluate_american_crr_offline_diagnostic,
};

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
    use super::offline_diagnostic::{
        CandidateBudget, CandidateInputs, crr_tree_node_visits, crr_tree_price,
        paired_candidate_price, richardson_candidate_domain_supported,
        richardson_candidate_price_until, richardson_candidate_result_until,
    };
    use super::*;
    use std::time::{Duration, Instant};

    const TREE_STEPS_EVEN: usize = 256;
    const TREE_STEPS_ODD: usize = 257;
    const TREE_PARITY_GAP_MAX: f64 = 0.05;
    const TREE_PRICE_MAX: f64 = MAX_MODEL_PRICE;
    const RICHARDSON_NODE_VISIT_LIMIT: u64 = 1_316_872;

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

    #[test]
    fn richardson_candidate_domain_keeps_time_and_model_bounds_explicit() {
        let exact_minimum = CandidateInputs {
            kind: OptionKind::Put,
            spot: 100.0,
            strike: 100.0,
            rate: 0.01,
            dividend_yield: 0.0,
            years: 60_000.0 / MILLIS_PER_YEAR_ACT_365F,
            volatility: 0.25,
        };
        assert!(richardson_candidate_domain_supported(exact_minimum));
        assert!(!richardson_candidate_domain_supported(CandidateInputs {
            years: 59_999.0 / MILLIS_PER_YEAR_ACT_365F,
            ..exact_minimum
        }));
        assert!(!richardson_candidate_domain_supported(CandidateInputs {
            years: MAX_MODEL_TIME_YEARS + 1.0e-12,
            ..exact_minimum
        }));
        assert!(!richardson_candidate_domain_supported(CandidateInputs {
            volatility: MAX_VOLATILITY + 1.0e-12,
            ..exact_minimum
        }));
        assert!(!richardson_candidate_domain_supported(CandidateInputs {
            spot: TREE_PRICE_MAX + 1.0,
            ..exact_minimum
        }));
    }

    #[test]
    fn richardson_candidate_requires_deadline_and_exact_node_budget() {
        let input = CandidateInputs {
            kind: OptionKind::Call,
            spot: 100.0,
            strike: 100.0,
            rate: 0.02,
            dividend_yield: 0.0,
            years: 30.0 / 365.0,
            volatility: 0.3,
        };
        let mut expired_budget = CandidateBudget::new(
            Instant::now() - Duration::from_millis(1),
            RICHARDSON_NODE_VISIT_LIMIT,
        );
        assert!(richardson_candidate_price_until(input, &mut expired_budget).is_none());
        assert_eq!(expired_budget.node_visits, 0);

        let mut short_budget = CandidateBudget::new(
            Instant::now() + Duration::from_secs(5),
            RICHARDSON_NODE_VISIT_LIMIT - 1,
        );
        assert!(richardson_candidate_price_until(input, &mut short_budget).is_none());
        assert_eq!(short_budget.node_visits, RICHARDSON_NODE_VISIT_LIMIT - 1);

        let mut exact_budget = CandidateBudget::new(
            Instant::now() + Duration::from_secs(5),
            RICHARDSON_NODE_VISIT_LIMIT,
        );
        let price = richardson_candidate_price_until(input, &mut exact_budget)
            .expect("bounded Richardson candidate price");
        assert!(price.is_finite());
        assert_eq!(exact_budget.node_visits, RICHARDSON_NODE_VISIT_LIMIT);
    }

    #[derive(Clone, Debug)]
    struct OracleRecord {
        id: String,
        kind: OptionKind,
        spot: f64,
        strike: f64,
        rate: f64,
        dividend_yield: f64,
        volatility: f64,
        maturity_millis: i64,
        act365f_years: f64,
        cash_dividend: f64,
        cash_dividend_offset_millis: i64,
        time_grid: usize,
        space_grid: usize,
        price: f64,
        delta: f64,
        gamma: f64,
        theta: f64,
    }

    impl OracleRecord {
        fn candidate_inputs(&self) -> CandidateInputs {
            CandidateInputs {
                kind: self.kind,
                spot: self.spot,
                strike: self.strike,
                rate: self.rate,
                dividend_yield: self.dividend_yield,
                years: self.maturity_millis as f64 / (365.0 * 86_400_000.0),
                volatility: self.volatility,
            }
        }
    }

    #[derive(Clone, Debug)]
    struct FiniteDifferenceProbe {
        id: String,
        kind: OptionKind,
        maturity_millis: i64,
        act365f_years: f64,
        spot: f64,
        strike: f64,
        rate: f64,
        dividend_yield: f64,
        volatility: f64,
        time_grid: usize,
        space_grid: usize,
        axis: String,
        bump_fraction: f64,
        bump_value: f64,
        bump_unit: String,
        base_price: f64,
        positive_axis_price: f64,
        negative_axis_price: f64,
    }

    impl FiniteDifferenceProbe {
        fn candidate_inputs(&self) -> CandidateInputs {
            CandidateInputs {
                kind: self.kind,
                spot: self.spot,
                strike: self.strike,
                rate: self.rate,
                dividend_yield: self.dividend_yield,
                years: self.maturity_millis as f64 / (365.0 * 86_400_000.0),
                volatility: self.volatility,
            }
        }
    }

    fn oracle_fixture() -> Vec<OracleRecord> {
        let fixture = include_str!("../../tests/fixtures/american_quantlib_v143_grid.csv");
        let mut lines = fixture.lines();
        assert_eq!(
            lines.next(),
            Some(
                "kind,id,quantlib,option,t_ms,t_act365f,spot,strike,rate,continuous_yield,sigma,cash_dividend,cash_dividend_offset_ms,t_grid,x_grid,price,delta,gamma,theta"
            )
        );
        lines
            .map(|line| {
                let fields: Vec<_> = line.split(',').collect();
                assert_eq!(fields.len(), 19, "bad QuantLib fixture row: {line}");
                assert_eq!(fields[0], "price", "unexpected QuantLib record: {line}");
                assert_eq!(fields[2], "1.43", "unexpected QuantLib version: {line}");
                OracleRecord {
                    id: fields[1].to_owned(),
                    kind: match fields[3] {
                        "call" => OptionKind::Call,
                        "put" => OptionKind::Put,
                        other => panic!("unknown option type {other} in {line}"),
                    },
                    maturity_millis: fields[4].parse().expect("maturity millis"),
                    act365f_years: fields[5].parse().expect("ACT/365F time"),
                    spot: fields[6].parse().expect("spot"),
                    strike: fields[7].parse().expect("strike"),
                    rate: fields[8].parse().expect("rate"),
                    dividend_yield: fields[9].parse().expect("continuous yield"),
                    volatility: fields[10].parse().expect("volatility"),
                    cash_dividend: fields[11].parse().expect("cash dividend"),
                    cash_dividend_offset_millis: fields[12].parse().expect("cash dividend offset"),
                    time_grid: fields[13].parse().expect("time grid"),
                    space_grid: fields[14].parse().expect("space grid"),
                    price: fields[15].parse().expect("price"),
                    delta: fields[16].parse().expect("delta"),
                    gamma: fields[17].parse().expect("gamma"),
                    theta: fields[18].parse().expect("theta"),
                }
            })
            .collect()
    }

    fn finite_difference_probe_fixture() -> Vec<FiniteDifferenceProbe> {
        let fixture = include_str!("../../tests/fixtures/american_quantlib_v143_fd.csv");
        let mut lines = fixture.lines();
        assert_eq!(
            lines.next(),
            Some(
                "kind,id,quantlib,option,t_ms,t_act365f,spot,strike,rate,continuous_yield,sigma,t_grid,x_grid,axis,bump_fraction,bump_value,bump_unit,base_price,positive_axis_price,negative_axis_price"
            )
        );
        lines
            .map(|line| {
                let fields: Vec<_> = line.split(',').collect();
                assert_eq!(fields.len(), 20, "bad QuantLib FD fixture row: {line}");
                assert_eq!(
                    fields[0], "price-probe",
                    "unexpected QuantLib FD record: {line}"
                );
                assert_eq!(fields[2], "1.43", "unexpected QuantLib version: {line}");
                FiniteDifferenceProbe {
                    id: fields[1].to_owned(),
                    kind: match fields[3] {
                        "call" => OptionKind::Call,
                        "put" => OptionKind::Put,
                        other => panic!("unknown option type {other} in {line}"),
                    },
                    maturity_millis: fields[4].parse().expect("maturity millis"),
                    act365f_years: fields[5].parse().expect("ACT/365F time"),
                    spot: fields[6].parse().expect("spot"),
                    strike: fields[7].parse().expect("strike"),
                    rate: fields[8].parse().expect("rate"),
                    dividend_yield: fields[9].parse().expect("continuous yield"),
                    volatility: fields[10].parse().expect("volatility"),
                    time_grid: fields[11].parse().expect("time grid"),
                    space_grid: fields[12].parse().expect("space grid"),
                    axis: fields[13].to_owned(),
                    bump_fraction: fields[14].parse().expect("bump fraction"),
                    bump_value: fields[15].parse().expect("bump value"),
                    bump_unit: fields[16].to_owned(),
                    base_price: fields[17].parse().expect("base price"),
                    positive_axis_price: fields[18].parse().expect("positive axis price"),
                    negative_axis_price: fields[19].parse().expect("negative axis price"),
                }
            })
            .collect()
    }

    fn paired_price_without_parity_gate(input: CandidateInputs, even_steps: usize) -> Option<f64> {
        let even = crr_tree_price(input, even_steps)?;
        let odd = crr_tree_price(input, even_steps + 1)?;
        Some((even + odd) / 2.0)
    }

    fn finite_difference_greeks(
        input: CandidateInputs,
        spot_bump: f64,
        time_bump: f64,
    ) -> Option<(f64, f64, f64)> {
        let base = paired_candidate_price(input)?;
        let up = paired_candidate_price(CandidateInputs {
            spot: input.spot + spot_bump,
            ..input
        })?;
        let down = paired_candidate_price(CandidateInputs {
            spot: input.spot - spot_bump,
            ..input
        })?;
        let later = paired_candidate_price(CandidateInputs {
            years: input.years + time_bump,
            ..input
        })?;
        let earlier = paired_candidate_price(CandidateInputs {
            years: input.years - time_bump,
            ..input
        })?;
        Some((
            (up - down) / (2.0 * spot_bump),
            (up - 2.0 * base + down) / spot_bump.powi(2),
            -(later - earlier) / (2.0 * time_bump),
        ))
    }

    fn implied_volatility_from_crr_price(
        input: CandidateInputs,
        target_price: f64,
        even_steps: usize,
    ) -> Option<f64> {
        let mut low = 0.0001;
        let low_price = loop {
            let low_input = CandidateInputs {
                volatility: low,
                ..input
            };
            if let Some(price) = paired_price_without_parity_gate(low_input, even_steps) {
                break price;
            }
            low *= 2.0;
            if low >= 5.0 {
                return None;
            }
        };
        if low_price > target_price {
            return None;
        }
        let mut high = 5.0;
        let high_price = loop {
            let high_input = CandidateInputs {
                volatility: high,
                ..input
            };
            if let Some(price) = paired_price_without_parity_gate(high_input, even_steps) {
                break price;
            }
            high *= 0.5;
            if high <= low {
                return None;
            }
        };
        if high_price < target_price {
            return None;
        }
        for _ in 0..48 {
            let mid = low + (high - low) / 2.0;
            let mid_input = CandidateInputs {
                volatility: mid,
                ..input
            };
            let price = paired_price_without_parity_gate(mid_input, even_steps)?;
            if price < target_price {
                low = mid;
            } else {
                high = mid;
            }
        }
        Some(low + (high - low) / 2.0)
    }

    #[test]
    fn quantlib_v143_price_greek_iv_matrix_reports_candidate_error_and_convergence() {
        let fixture = oracle_fixture();
        assert_eq!(fixture.len(), 36 * 4);
        assert!(fixture.iter().all(|row| {
            [(100, 400), (400, 800), (800, 1600), (1600, 3200)]
                .contains(&(row.time_grid, row.space_grid))
        }));

        let finest: Vec<_> = fixture
            .iter()
            .filter(|row| row.time_grid == 1600 && row.space_grid == 3200)
            .collect();
        assert_eq!(finest.len(), 36);
        let mut case_ids: Vec<_> = finest.iter().map(|row| row.id.as_str()).collect();
        case_ids.sort_unstable();
        case_ids.dedup();
        assert_eq!(case_ids.len(), 36);
        assert!(finest.iter().any(|row| row.spot / row.strike <= 0.80));
        assert!(finest.iter().any(|row| row.spot / row.strike >= 1.20));
        assert!(finest.iter().any(|row| row.rate <= -0.15));
        assert!(finest.iter().any(|row| row.rate >= 0.15));
        assert!(finest.iter().any(|row| row.dividend_yield <= -0.02));
        assert!(finest.iter().any(|row| row.dividend_yield >= 0.08));
        assert!(finest.iter().any(|row| row.volatility <= 0.05));
        assert!(finest.iter().any(|row| row.volatility >= 2.0));
        for row in &finest {
            let derived_years = row.maturity_millis as f64 / (365.0 * 86_400_000.0);
            assert!((row.act365f_years - derived_years).abs() <= 1.0e-15);
        }
        let mut priced = 0_usize;
        let mut candidate_rejected = 0_usize;
        let mut candidate_rejected_ids = Vec::new();
        let mut price_error_max = 0.0_f64;
        let mut price_error_max_id = String::new();
        let mut price_relative_error_max = 0.0_f64;
        let mut price_relative_error_max_id = String::new();
        let mut parity_gap_max = 0.0_f64;
        let mut ql_price_grid_delta_max = 0.0_f64;
        let mut ql_delta_grid_delta_max = 0.0_f64;
        let mut ql_gamma_grid_delta_max = 0.0_f64;
        let mut ql_theta_grid_delta_max = 0.0_f64;
        let mut delta_error_max = std::array::from_fn::<_, 3, _>(|_| (0.0_f64, String::new()));
        let mut gamma_error_max = std::array::from_fn::<_, 3, _>(|_| (0.0_f64, String::new()));
        let mut theta_error_max = std::array::from_fn::<_, 3, _>(|_| (0.0_f64, String::new()));
        let mut delta_relative_error_max =
            std::array::from_fn::<_, 3, _>(|_| (0.0_f64, String::new()));
        let mut gamma_relative_error_max =
            std::array::from_fn::<_, 3, _>(|_| (0.0_f64, String::new()));
        let mut theta_relative_error_max =
            std::array::from_fn::<_, 3, _>(|_| (0.0_f64, String::new()));
        let mut crr_grid_price_error_max = [(0.0_f64, 0_usize); 5];
        let mut crr_grid_pair_elapsed = [Duration::ZERO; 5];
        let mut candidate_pair_supported = [0_usize; 3];
        let mut candidate_pair_rejected = [0_usize; 3];
        let mut candidate_pair_max_abs_error =
            std::array::from_fn::<_, 3, _>(|_| (0.0_f64, String::new()));
        let mut candidate_pair_max_rel_error =
            std::array::from_fn::<_, 3, _>(|_| (0.0_f64, String::new()));
        let mut richardson_supported = 0_usize;
        let mut richardson_rejected = 0_usize;
        let mut richardson_max_abs_error = (0.0_f64, String::new());
        let mut richardson_max_rel_error = (0.0_f64, String::new());
        let mut richardson_node_visits_max = 0_u64;
        let mut richardson_elapsed = Duration::ZERO;
        let mut richardson_rejected_ids = Vec::new();
        let mut delta_comparisons = [0_usize; 3];
        let mut gamma_comparisons = [0_usize; 3];
        let mut theta_comparisons = [0_usize; 3];

        for row in &finest {
            let previous = fixture
                .iter()
                .find(|candidate| {
                    candidate.id == row.id
                        && candidate.time_grid == 800
                        && candidate.space_grid == 1600
                })
                .expect("preceding QuantLib grid");
            ql_price_grid_delta_max =
                ql_price_grid_delta_max.max((row.price - previous.price).abs());
            ql_delta_grid_delta_max =
                ql_delta_grid_delta_max.max((row.delta - previous.delta).abs());
            ql_gamma_grid_delta_max =
                ql_gamma_grid_delta_max.max((row.gamma - previous.gamma).abs());
            ql_theta_grid_delta_max =
                ql_theta_grid_delta_max.max((row.theta - previous.theta).abs());

            if row.cash_dividend > 0.0 {
                continue;
            }
            let input = row.candidate_inputs();
            let mut gated_pair_prices = [None; 3];
            for (index, steps) in [64, 128, 256, 512, 1024].into_iter().enumerate() {
                let pair_started_at = Instant::now();
                let even = crr_tree_price(input, steps);
                let odd = crr_tree_price(input, steps + 1);
                crr_grid_pair_elapsed[index] += pair_started_at.elapsed();
                if let (Some(even), Some(odd)) = (even, odd) {
                    let pair_gap = (even - odd).abs();
                    let price = (even + odd) / 2.0;
                    let error = (price - row.price).abs();
                    if error > crr_grid_price_error_max[index].0 {
                        crr_grid_price_error_max[index] = (error, steps);
                    }
                    if index >= 2 {
                        let study_index = index - 2;
                        if pair_gap <= TREE_PARITY_GAP_MAX {
                            candidate_pair_supported[study_index] += 1;
                            gated_pair_prices[study_index] = Some(price);
                            let relative_error = error / row.price.abs().max(1.0e-12);
                            if error > candidate_pair_max_abs_error[study_index].0 {
                                candidate_pair_max_abs_error[study_index] = (error, row.id.clone());
                            }
                            if relative_error > candidate_pair_max_rel_error[study_index].0 {
                                candidate_pair_max_rel_error[study_index] =
                                    (relative_error, row.id.clone());
                            }
                        } else {
                            candidate_pair_rejected[study_index] += 1;
                        }
                    }
                } else if index >= 2 {
                    candidate_pair_rejected[index - 2] += 1;
                }
            }
            let mut budget = CandidateBudget::new(
                Instant::now() + Duration::from_secs(5),
                RICHARDSON_NODE_VISIT_LIMIT,
            );
            let candidate_started_at = Instant::now();
            let extrapolated = richardson_candidate_price_until(input, &mut budget);
            richardson_elapsed += candidate_started_at.elapsed();
            richardson_node_visits_max = richardson_node_visits_max.max(budget.node_visits);
            if let Some(extrapolated) = extrapolated {
                richardson_supported += 1;
                let error = (extrapolated - row.price).abs();
                let relative_error = error / row.price.abs().max(1.0e-12);
                if error > richardson_max_abs_error.0 {
                    richardson_max_abs_error = (error, row.id.clone());
                }
                if relative_error > richardson_max_rel_error.0 {
                    richardson_max_rel_error = (relative_error, row.id.clone());
                }
            } else {
                richardson_rejected += 1;
                richardson_rejected_ids.push(row.id.clone());
            }
            let Some(candidate_price) = paired_candidate_price(input) else {
                candidate_rejected += 1;
                candidate_rejected_ids.push(row.id.clone());
                let even = crr_tree_price(input, TREE_STEPS_EVEN);
                let odd = crr_tree_price(input, TREE_STEPS_ODD);
                let reason = match (even, odd) {
                    (None, _) => "256-step-tree-invalid",
                    (_, None) => "257-step-tree-invalid",
                    (Some(even), Some(odd)) => {
                        if (even - odd).abs() > TREE_PARITY_GAP_MAX {
                            "parity-gap-over-0.05"
                        } else {
                            "paired-candidate-unavailable"
                        }
                    }
                };
                let gap = match (even, odd) {
                    (Some(even), Some(odd)) => (even - odd).abs(),
                    _ => f64::NAN,
                };
                println!(
                    "CRR_REJECTED id={} reason={reason} pair_gap={gap:.12}",
                    row.id
                );
                continue;
            };
            priced += 1;
            let error = (candidate_price - row.price).abs();
            if error > price_error_max {
                price_error_max = error;
                price_error_max_id.clone_from(&row.id);
            }
            let relative_error = error / row.price.abs().max(1.0e-12);
            if relative_error > price_relative_error_max {
                price_relative_error_max = relative_error;
                price_relative_error_max_id.clone_from(&row.id);
            }
            let even = crr_tree_price(input, TREE_STEPS_EVEN).expect("even CRR value");
            let odd = crr_tree_price(input, TREE_STEPS_ODD).expect("odd CRR value");
            parity_gap_max = parity_gap_max.max((even - odd).abs());

            for (index, spot_fraction) in [0.0001, 0.001, 0.01].into_iter().enumerate() {
                let spot_bump = input.spot * spot_fraction;
                let h_up = paired_candidate_price(CandidateInputs {
                    spot: input.spot + spot_bump,
                    ..input
                });
                let h_down = paired_candidate_price(CandidateInputs {
                    spot: input.spot - spot_bump,
                    ..input
                });
                if let (Some(up), Some(down)) = (h_up, h_down) {
                    let delta_error = ((up - down) / (2.0 * spot_bump) - row.delta).abs();
                    if delta_error > delta_error_max[index].0 {
                        delta_error_max[index] = (delta_error, row.id.clone());
                    }
                    let delta_relative_error = delta_error / row.delta.abs().max(1.0e-8);
                    if delta_relative_error > delta_relative_error_max[index].0 {
                        delta_relative_error_max[index] = (delta_relative_error, row.id.clone());
                    }
                    delta_comparisons[index] += 1;
                    let gamma_error =
                        ((up - 2.0 * candidate_price + down) / spot_bump.powi(2) - row.gamma).abs();
                    if gamma_error > gamma_error_max[index].0 {
                        gamma_error_max[index] = (gamma_error, row.id.clone());
                    }
                    let gamma_relative_error = gamma_error / row.gamma.abs().max(1.0e-8);
                    if gamma_relative_error > gamma_relative_error_max[index].0 {
                        gamma_relative_error_max[index] = (gamma_relative_error, row.id.clone());
                    }
                    gamma_comparisons[index] += 1;
                }
            }

            for (index, time_fraction) in [0.001, 0.01, 0.05].into_iter().enumerate() {
                let time_bump = input.years * time_fraction;
                if let Some((_, _, theta)) =
                    finite_difference_greeks(input, input.spot * 0.001, time_bump)
                {
                    let error = (theta - row.theta).abs();
                    if error > theta_error_max[index].0 {
                        theta_error_max[index] = (error, row.id.clone());
                    }
                    let relative_error = error / row.theta.abs().max(1.0e-8);
                    if relative_error > theta_relative_error_max[index].0 {
                        theta_relative_error_max[index] = (relative_error, row.id.clone());
                    }
                    theta_comparisons[index] += 1;
                }
            }
        }

        println!(
            "CRR_ORACLE_MATRIX cases={} price_supported={} candidate_rejected={} max_price_abs_error={:.12} max_abs_case={} max_price_relative_error={:.8}% max_rel_case={} max_256_257_gap={:.12}",
            finest.len() - 2,
            priced,
            candidate_rejected,
            price_error_max,
            price_error_max_id,
            price_relative_error_max * 100.0,
            price_relative_error_max_id,
            parity_gap_max
        );
        println!(
            "QL_GRID_800_1600_TO_1600_3200 max_price_delta={:.12} max_delta_delta={:.12} max_gamma_delta={:.12} max_theta_delta={:.12}",
            ql_price_grid_delta_max,
            ql_delta_grid_delta_max,
            ql_gamma_grid_delta_max,
            ql_theta_grid_delta_max
        );
        println!(
            "CRR_PRICE_MAX_ABS_ERROR_BY_STEP_PAIR_64_128_256_512_1024={crr_grid_price_error_max:?}"
        );
        println!(
            "CRR_PRICE_PAIR_PARITY_SUPPORT_BY_STEPS_256_512_1024={candidate_pair_supported:?} rejected={candidate_pair_rejected:?}"
        );
        println!(
            "CRR_PRICE_PAIR_GATED_MAX_ABS_ERROR_BY_STEPS_256_512_1024={candidate_pair_max_abs_error:?}"
        );
        println!(
            "CRR_PRICE_PAIR_GATED_MAX_REL_ERROR_BY_STEPS_256_512_1024={candidate_pair_max_rel_error:?}"
        );
        println!(
            "CRR_RICHARDSON_512_TO_1024_SUPPORT={richardson_supported} rejected={richardson_rejected} max_abs_error={richardson_max_abs_error:?} max_rel_error={richardson_max_rel_error:?} max_node_visits={richardson_node_visits_max} total_elapsed={richardson_elapsed:?}"
        );
        println!("CRR_RICHARDSON_REJECTED_IDS={richardson_rejected_ids:?}");
        println!(
            "CRR_PAIR_NODE_VISITS_BY_STEPS_256_512_1024={:?}",
            [256, 512, 1024]
                .map(|steps| crr_tree_node_visits(steps) + crr_tree_node_visits(steps + 1))
        );
        println!("CRR_34_CASE_PAIR_ELAPSED_BY_STEPS_64_128_256_512_1024={crr_grid_pair_elapsed:?}");
        println!("CRR_GREEK_MAX_ABS_DELTA_ERROR_BY_SPOT_BUMP_0.01_0.1_1={delta_error_max:?}");
        println!("CRR_GREEK_MAX_ABS_GAMMA_ERROR_BY_SPOT_BUMP_0.01_0.1_1={gamma_error_max:?}");
        println!("CRR_GREEK_MAX_ABS_THETA_ERROR_BY_TIME_BUMP_0.1_1_5_PERCENT={theta_error_max:?}");
        println!(
            "CRR_GREEK_MAX_REL_DELTA_ERROR_BY_SPOT_BUMP_0.01_0.1_1={delta_relative_error_max:?}"
        );
        println!(
            "CRR_GREEK_MAX_REL_GAMMA_ERROR_BY_SPOT_BUMP_0.01_0.1_1={gamma_relative_error_max:?}"
        );
        println!(
            "CRR_GREEK_MAX_REL_THETA_ERROR_BY_TIME_BUMP_0.1_1_5_PERCENT={theta_relative_error_max:?}"
        );
        println!(
            "CRR_GREEK_COMPARISONS delta={delta_comparisons:?} gamma={gamma_comparisons:?} theta={theta_comparisons:?}"
        );

        let iv_ids = [
            "american_call_90d_atm_no_div",
            "american_put_180d_itm_no_div",
            "american_call_180d_itm_q04",
            "american_put_90d_otm_q02",
            "american_put_30d_negative_r",
            "american_call_30d_no_div_baseline",
            "american_put_30d_no_div_baseline",
            "0dte_atm_call_1h_no_div",
            "0dte_atm_put_60s_no_div",
            "matrix_call_105_30d",
        ];
        let mut iv_error_max = std::array::from_fn::<_, 4, _>(|_| (0.0_f64, String::new()));
        let mut iv_relative_error_max =
            std::array::from_fn::<_, 4, _>(|_| (0.0_f64, String::new()));
        let mut iv_comparisons = [0_usize; 4];
        let mut iv_failures = [0_usize; 4];
        for id in iv_ids {
            let row = finest.iter().find(|row| row.id == id).expect("IV case");
            let input = row.candidate_inputs();
            let mut values = Vec::new();
            for steps in [64, 128, 256, 512] {
                let volatility = implied_volatility_from_crr_price(input, row.price, steps);
                let error = volatility.map(|value| (value - row.volatility).abs());
                let index = values.len();
                if let Some(error) = error {
                    iv_comparisons[index] += 1;
                    if error > iv_error_max[index].0 {
                        iv_error_max[index] = (error, row.id.clone());
                    }
                    let relative_error = error / row.volatility;
                    if relative_error > iv_relative_error_max[index].0 {
                        iv_relative_error_max[index] = (relative_error, row.id.clone());
                    }
                } else {
                    iv_failures[index] += 1;
                }
                values.push(error);
            }
            println!(
                "CRR_IV_ERROR id={} sigma={:.8} abs_error_by_steps_64_128_256_512={values:?}",
                row.id, row.volatility
            );
        }
        println!("CRR_IV_MAX_ABS_ERROR_BY_STEP_PAIR_64_128_256_512={iv_error_max:?}");
        println!("CRR_IV_MAX_REL_ERROR_BY_STEP_PAIR_64_128_256_512={iv_relative_error_max:?}");
        println!("CRR_IV_COMPARISONS_BY_STEP_PAIR_64_128_256_512={iv_comparisons:?}");
        println!("CRR_IV_FAILURES_BY_STEP_PAIR_64_128_256_512={iv_failures:?}");

        // These bounds preserve this recorded synthetic sample; they are not production accuracy gates.
        // 这些上限用于固定当前合成样本，不是生产精度门槛。
        assert_eq!(priced + candidate_rejected, finest.len() - 2);
        assert_eq!(priced, 31);
        assert_eq!(candidate_rejected, 3);
        assert_eq!(candidate_pair_supported, [31, 34, 34]);
        assert_eq!(candidate_pair_rejected, [3, 0, 0]);
        assert_eq!(richardson_supported, 32);
        assert_eq!(richardson_rejected, 2);
        assert_eq!(richardson_node_visits_max, RICHARDSON_NODE_VISIT_LIMIT);
        richardson_rejected_ids.sort();
        assert_eq!(
            richardson_rejected_ids,
            ["0dte_atm_put_59999ms_no_div", "0dte_itm_put_59999ms_no_div"]
        );
        assert_eq!(
            [256, 512, 1024]
                .map(|steps| crr_tree_node_visits(steps) + crr_tree_node_visits(steps + 1)),
            [66_564, 264_196, 1_052_676]
        );
        candidate_rejected_ids.sort();
        assert_eq!(
            candidate_rejected_ids,
            [
                "matrix_call_120_90d",
                "matrix_put_100_365d",
                "matrix_put_120_180d"
            ]
        );
        assert!(price_error_max < 0.012, "sample regression envelope only");
        assert!(
            price_relative_error_max < 0.20,
            "sample regression envelope only"
        );
        assert!(crr_grid_price_error_max[4].0 < crr_grid_price_error_max[2].0);
        assert!(crr_grid_price_error_max[4].0 < 0.005);
        assert!(ql_price_grid_delta_max < 0.002);
        assert!(ql_delta_grid_delta_max < 1.0e-5);
        assert!(ql_gamma_grid_delta_max < 3.0e-5);
        assert!(ql_theta_grid_delta_max < 2.0);
        assert_eq!(delta_comparisons, [31; 3]);
        assert_eq!(gamma_comparisons, [31; 3]);
        assert_eq!(theta_comparisons, [31; 3]);
        assert_eq!(iv_comparisons, [10; 4]);
        assert_eq!(iv_failures, [0; 4]);
        assert!(
            iv_error_max[2].0 < 0.0011,
            "sample IV regression envelope only"
        );

        let price_for = |id: &str| {
            finest
                .iter()
                .find(|row| row.id == id)
                .expect("dividend sensitivity fixture")
                .price
        };
        let call_cash_dividend_diff = price_for("american_call_30d_cash_div_1")
            - price_for("american_call_30d_no_div_baseline");
        let put_cash_dividend_diff = price_for("american_put_30d_cash_div_1")
            - price_for("american_put_30d_no_div_baseline");
        assert!((-0.52..-0.50).contains(&call_cash_dividend_diff));
        assert!((0.61..0.64).contains(&put_cash_dividend_diff));
        assert_eq!(
            finest
                .iter()
                .find(|row| row.id == "american_call_30d_cash_div_1")
                .expect("cash dividend timing fixture")
                .cash_dividend_offset_millis,
            10 * 86_400_000
        );
        for boundary_millis in [59_999, 60_000, 60_001] {
            assert!(
                finest
                    .iter()
                    .any(|row| row.maturity_millis == boundary_millis)
            );
        }
    }

    #[test]
    fn quantlib_v143_matched_finite_difference_probes_report_candidate_greek_differences() {
        let oracle = oracle_fixture();
        let finest: Vec<_> = oracle
            .iter()
            .filter(|row| row.time_grid == 1600 && row.space_grid == 3200)
            .collect();
        let probes = finite_difference_probe_fixture();
        assert_eq!(probes.len(), 34 * 6);

        let spot_fractions = [0.0001, 0.001, 0.01];
        let time_fractions = [0.001, 0.01, 0.05];
        let mut observed_keys = Vec::with_capacity(probes.len());
        let mut delta_error_max = std::array::from_fn::<_, 3, _>(|_| (0.0_f64, String::new()));
        let mut gamma_error_max = std::array::from_fn::<_, 3, _>(|_| (0.0_f64, String::new()));
        let mut theta_error_max = std::array::from_fn::<_, 3, _>(|_| (0.0_f64, String::new()));
        let mut delta_comparisons = [0_usize; 3];
        let mut gamma_comparisons = [0_usize; 3];
        let mut theta_comparisons = [0_usize; 3];
        let mut candidate_unavailable = [0_usize; 6];

        for probe in &probes {
            assert_eq!((probe.time_grid, probe.space_grid), (1600, 3200));
            let key = (probe.id.as_str(), probe.axis.as_str(), probe.bump_fraction);
            assert!(
                !observed_keys.contains(&key),
                "duplicate QL FD probe: {key:?}"
            );
            observed_keys.push(key);
            let reference = finest
                .iter()
                .find(|row| row.id == probe.id)
                .expect("matching original QuantLib finest-grid row");
            assert_eq!(probe.kind, reference.kind);
            assert_eq!(probe.maturity_millis, reference.maturity_millis);
            assert_eq!(probe.spot, reference.spot);
            assert_eq!(probe.strike, reference.strike);
            assert_eq!(probe.rate, reference.rate);
            assert_eq!(probe.dividend_yield, reference.dividend_yield);
            assert_eq!(probe.volatility, reference.volatility);
            assert!((probe.act365f_years - reference.act365f_years).abs() <= 1.0e-15);
            assert!((probe.base_price - reference.price).abs() <= 1.0e-10);
            assert!(
                [
                    probe.base_price,
                    probe.positive_axis_price,
                    probe.negative_axis_price
                ]
                .into_iter()
                .all(f64::is_finite)
            );

            let (bump_index, bump) = match probe.axis.as_str() {
                "spot" => {
                    let index = spot_fractions
                        .iter()
                        .position(|fraction| *fraction == probe.bump_fraction)
                        .expect("known spot bump fraction");
                    assert_eq!(probe.bump_unit, "underlying-unit");
                    assert!((probe.bump_value - probe.spot * probe.bump_fraction).abs() <= 1.0e-14);
                    (index, probe.bump_value)
                }
                "time" => {
                    let index = time_fractions
                        .iter()
                        .position(|fraction| *fraction == probe.bump_fraction)
                        .expect("known time bump fraction");
                    assert_eq!(probe.bump_unit, "millisecond");
                    assert_eq!(probe.bump_value.fract(), 0.0);
                    assert!(probe.bump_value > 0.0);
                    assert!(probe.bump_value < probe.maturity_millis as f64);
                    (index + 3, probe.bump_value / (365.0 * 86_400_000.0))
                }
                other => panic!("unknown finite-difference axis {other}"),
            };
            let input = probe.candidate_inputs();
            let base = paired_candidate_price(input);
            let (positive, negative) = if probe.axis == "spot" {
                (
                    paired_candidate_price(CandidateInputs {
                        spot: input.spot + bump,
                        ..input
                    }),
                    paired_candidate_price(CandidateInputs {
                        spot: input.spot - bump,
                        ..input
                    }),
                )
            } else {
                (
                    paired_candidate_price(CandidateInputs {
                        years: input.years + bump,
                        ..input
                    }),
                    paired_candidate_price(CandidateInputs {
                        years: input.years - bump,
                        ..input
                    }),
                )
            };
            let (Some(base), Some(positive), Some(negative)) = (base, positive, negative) else {
                candidate_unavailable[bump_index] += 1;
                continue;
            };

            if probe.axis == "spot" {
                let ql_delta =
                    (probe.positive_axis_price - probe.negative_axis_price) / (2.0 * bump);
                let crr_delta = (positive - negative) / (2.0 * bump);
                delta_error_max[bump_index].0 = delta_error_max[bump_index]
                    .0
                    .max((crr_delta - ql_delta).abs());
                if delta_error_max[bump_index].1.is_empty()
                    || (crr_delta - ql_delta).abs() == delta_error_max[bump_index].0
                {
                    delta_error_max[bump_index].1.clone_from(&probe.id);
                }
                let ql_gamma = (probe.positive_axis_price - 2.0 * probe.base_price
                    + probe.negative_axis_price)
                    / bump.powi(2);
                let crr_gamma = (positive - 2.0 * base + negative) / bump.powi(2);
                gamma_error_max[bump_index].0 = gamma_error_max[bump_index]
                    .0
                    .max((crr_gamma - ql_gamma).abs());
                if gamma_error_max[bump_index].1.is_empty()
                    || (crr_gamma - ql_gamma).abs() == gamma_error_max[bump_index].0
                {
                    gamma_error_max[bump_index].1.clone_from(&probe.id);
                }
                delta_comparisons[bump_index] += 1;
                gamma_comparisons[bump_index] += 1;
            } else {
                let ql_theta =
                    -(probe.positive_axis_price - probe.negative_axis_price) / (2.0 * bump);
                let crr_theta = -(positive - negative) / (2.0 * bump);
                theta_error_max[bump_index - 3].0 = theta_error_max[bump_index - 3]
                    .0
                    .max((crr_theta - ql_theta).abs());
                if theta_error_max[bump_index - 3].1.is_empty()
                    || (crr_theta - ql_theta).abs() == theta_error_max[bump_index - 3].0
                {
                    theta_error_max[bump_index - 3].1.clone_from(&probe.id);
                }
                theta_comparisons[bump_index - 3] += 1;
            }
        }

        assert_eq!(observed_keys.len(), 34 * 6);
        for row in &finest {
            if row.cash_dividend == 0.0 {
                for (axis, fractions) in
                    [("spot", &spot_fractions[..]), ("time", &time_fractions[..])]
                {
                    for fraction in fractions {
                        assert!(observed_keys.contains(&(row.id.as_str(), axis, *fraction)));
                    }
                }
            }
        }
        assert_eq!(delta_comparisons, [31; 3]);
        assert_eq!(gamma_comparisons, [31; 3]);
        assert_eq!(theta_comparisons, [31; 3]);
        assert_eq!(candidate_unavailable, [3; 6]);
        println!("MATCHED_QL_CRR_FD_SPOT_DELTA_MAX_ABS_BY_BUMP_0.01_0.1_1={delta_error_max:?}");
        println!("MATCHED_QL_CRR_FD_SPOT_GAMMA_MAX_ABS_BY_BUMP_0.01_0.1_1={gamma_error_max:?}");
        println!(
            "MATCHED_QL_CRR_FD_TIME_THETA_MAX_ABS_BY_BUMP_0.1_1_5_PERCENT={theta_error_max:?}"
        );
        println!(
            "MATCHED_QL_CRR_FD_COMPARISONS delta={delta_comparisons:?} gamma={gamma_comparisons:?} theta={theta_comparisons:?} unavailable={candidate_unavailable:?}"
        );
    }

    #[test]
    fn richardson_native_lattice_delta_gamma_report_quantlib_differences() {
        let fixture = oracle_fixture();
        let finest: Vec<_> = fixture
            .iter()
            .filter(|row| {
                row.time_grid == 1600 && row.space_grid == 3200 && row.cash_dividend == 0.0
            })
            .collect();
        let mut supported = 0_usize;
        let mut unavailable = 0_usize;
        let mut delta_error_max = (0.0_f64, String::new());
        let mut gamma_error_max = (0.0_f64, String::new());
        let mut theta_magnitude_max = 0.0_f64;

        for row in &finest {
            let mut budget = CandidateBudget::new(
                Instant::now() + Duration::from_secs(5),
                RICHARDSON_NODE_VISIT_LIMIT,
            );
            let Ok(candidate) =
                richardson_candidate_result_until(row.candidate_inputs(), &mut budget)
            else {
                unavailable += 1;
                continue;
            };
            let Some(greeks) = candidate.native_greeks else {
                unavailable += 1;
                continue;
            };
            supported += 1;
            let delta_error = (greeks.delta - row.delta).abs();
            if delta_error > delta_error_max.0 {
                delta_error_max = (delta_error, row.id.clone());
            }
            let gamma_error = (greeks.gamma - row.gamma).abs();
            if gamma_error > gamma_error_max.0 {
                gamma_error_max = (gamma_error, row.id.clone());
            }
            theta_magnitude_max = theta_magnitude_max.max(greeks.theta.abs());
        }
        assert_eq!(supported, 32);
        assert_eq!(unavailable, 2);
        println!("CRR_RICHARDSON_NATIVE_GREEKS_SUPPORT={supported} unavailable={unavailable}");
        println!("CRR_RICHARDSON_NATIVE_DELTA_MAX_ABS_VS_QL_DELTA={delta_error_max:?}");
        println!("CRR_RICHARDSON_NATIVE_GAMMA_MAX_ABS_VS_QL_GAMMA={gamma_error_max:?}");
        println!(
            "CRR_RICHARDSON_NATIVE_THETA_MAX_ABS_PER_ACT365F_YEAR={theta_magnitude_max:.12} convention=two-step-local-calendar-decay"
        );
    }

    #[test]
    fn richardson_theta_matches_quantlib_central_fd_and_reports_daily_units() {
        let fixture = oracle_fixture();
        let probes = finite_difference_probe_fixture();
        let finest: Vec<_> = fixture
            .iter()
            .filter(|row| {
                row.time_grid == 1600 && row.space_grid == 3200 && row.cash_dividend == 0.0
            })
            .collect();
        let minimum_years = 60_000.0 / MILLIS_PER_YEAR_ACT_365F;
        let mut comparisons = 0_usize;
        let mut boundary_unavailable = 0_usize;
        let mut theta_error_max_per_year = (0.0_f64, String::new());
        let mut theta_error_max_per_day = (0.0_f64, String::new());

        for row in &finest {
            let Some(probe) = probes.iter().find(|probe| {
                probe.id == row.id && probe.axis == "time" && probe.bump_fraction == 0.01
            }) else {
                panic!("missing matched one-percent time probe for {}", row.id);
            };
            let bump_years = probe.bump_value / MILLIS_PER_YEAR_ACT_365F;
            let ql_theta =
                -(probe.positive_axis_price - probe.negative_axis_price) / (2.0 * bump_years);
            if row.act365f_years - bump_years < minimum_years {
                boundary_unavailable += 1;
                continue;
            }

            let input = row.candidate_inputs();
            let mut later_budget = CandidateBudget::new(
                Instant::now() + Duration::from_secs(5),
                RICHARDSON_NODE_VISIT_LIMIT,
            );
            let later = richardson_candidate_price_until(
                CandidateInputs {
                    years: input.years + bump_years,
                    ..input
                },
                &mut later_budget,
            )
            .expect("Richardson later-expiry candidate price");
            let mut earlier_budget = CandidateBudget::new(
                Instant::now() + Duration::from_secs(5),
                RICHARDSON_NODE_VISIT_LIMIT,
            );
            let earlier = richardson_candidate_price_until(
                CandidateInputs {
                    years: input.years - bump_years,
                    ..input
                },
                &mut earlier_budget,
            )
            .expect("Richardson earlier-expiry candidate price");
            let candidate_theta = -(later - earlier) / (2.0 * bump_years);
            let error_per_year = (candidate_theta - ql_theta).abs();
            let error_per_day = error_per_year / 365.0;
            if error_per_year > theta_error_max_per_year.0 {
                theta_error_max_per_year = (error_per_year, row.id.clone());
            }
            if error_per_day > theta_error_max_per_day.0 {
                theta_error_max_per_day = (error_per_day, row.id.clone());
            }
            comparisons += 1;
        }

        assert_eq!(comparisons, 26);
        assert_eq!(boundary_unavailable, 8);
        println!(
            "CRR_RICHARDSON_MATCHED_CENTRAL_THETA_COMPARISONS={comparisons} bump=1%_ACT365F time_unit_per_year={theta_error_max_per_year:?} time_unit_per_calendar_day={theta_error_max_per_day:?}"
        );
    }

    #[test]
    fn experimental_american_iv_roundtrip_recovers_its_own_lattice_prices() {
        let fixture = oracle_fixture();
        let finest: Vec<_> = fixture
            .iter()
            .filter(|row| {
                row.time_grid == 1600 && row.space_grid == 3200 && row.cash_dividend == 0.0
            })
            .collect();
        let case_ids = [
            "american_call_90d_atm_no_div",
            "american_put_180d_itm_no_div",
            "american_call_180d_itm_q04",
            "american_put_90d_otm_q02",
            "american_put_30d_negative_r",
            "american_call_30d_no_div_baseline",
            "american_put_30d_no_div_baseline",
            "0dte_atm_call_1h_no_div",
            "0dte_atm_put_60s_no_div",
            "matrix_call_105_30d",
        ];
        let step_pairs = [256, 512];
        let mut maximum_sigma_error = 0.0_f64;
        let mut roundtrips = 0_usize;

        for id in case_ids {
            let row = finest
                .iter()
                .find(|row| row.id == id)
                .expect("IV roundtrip case");
            let input = row.candidate_inputs();
            for steps in step_pairs {
                let target_price = paired_price_without_parity_gate(input, steps)
                    .expect("finite synthetic candidate price");
                let recovered_volatility =
                    implied_volatility_from_crr_price(input, target_price, steps)
                        .expect("same-model IV root");
                let error = (recovered_volatility - input.volatility).abs();
                println!(
                    "CRR_IV_SELF_ROUNDTRIP id={id} steps={steps} target={target_price:.12} recovered={recovered_volatility:.12} abs_sigma_error={error:.12}"
                );
                maximum_sigma_error = maximum_sigma_error.max(error);
                roundtrips += 1;
            }
        }
        assert_eq!(roundtrips, 20);
        assert!(maximum_sigma_error <= 2.0e-11);
        println!(
            "CRR_IV_SELF_ROUNDTRIP cases={roundtrips} max_abs_sigma_error={maximum_sigma_error:.16} step_pairs={step_pairs:?}"
        );
    }
}
