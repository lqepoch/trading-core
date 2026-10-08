//! Exact quote arithmetic for debit verticals.
//!
//! This module computes leg-derived references only; quote identity, freshness,
//! generation, and feed-quality checks remain caller responsibilities.
//!
//! ## 简体中文
//!
//! 本模块使用精确数值计算 debit vertical 的腿间报价参考值。合约身份、行情新鲜度、连接 generation 和 feed 质量仍由调用方负责验证。

use std::{error::Error, fmt};

use domain::{DecimalError, ExactDecimal, Money, Price};

/// The two semantic legs of a debit vertical: long higher-strike and short
/// lower-strike. Contract identity and orientation must be validated upstream.
/// 简体中文：debit vertical 的两条语义腿：较高行权价的多头腿和较低行权价的空头腿。合约身份与方向必须由上游验证。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerticalLeg {
    /// Long higher-strike option leg.
    /// 简体中文：较高行权价的多头期权腿。
    Long,
    /// Short lower-strike option leg.
    /// 简体中文：较低行权价的空头期权腿。
    Short,
}

/// A quote side, used to identify a missing or invalid exact value.
/// 简体中文：用于标识缺失或无效精确值的报价方向。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuoteSide {
    /// Best bid side.
    /// 简体中文：最佳买价。
    Bid,
    /// Best ask side.
    /// 简体中文：最佳卖价。
    Ask,
}

impl VerticalLeg {
    const fn code(self) -> &'static str {
        match self {
            Self::Long => "LONG",
            Self::Short => "SHORT",
        }
    }
}

impl QuoteSide {
    const fn code(self) -> &'static str {
        match self {
            Self::Bid => "BID",
            Self::Ask => "ASK",
        }
    }
}

/// One option leg's bid and ask. `None` means that side was not supplied.
/// 简体中文：单个期权腿的买价与卖价；`None` 表示该侧未提供。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LegQuote {
    bid: Option<Price>,
    ask: Option<Price>,
}

impl LegQuote {
    /// Construct from already validated non-negative domain prices.
    /// 简体中文：使用已验证的非负领域价格创建单腿报价。
    pub const fn from_prices(bid: Option<Price>, ask: Option<Price>) -> Self {
        Self { bid, ask }
    }

    /// Construct from exact decimals, rejecting negative wire values before
    /// they enter the non-negative `Price` domain type.
    /// 简体中文：从精确十进制值创建报价；在转换为非负 `Price` 领域类型前拒绝负 wire 值。
    pub fn from_exact(
        bid: Option<ExactDecimal>,
        ask: Option<ExactDecimal>,
    ) -> Result<Self, PricingError> {
        Ok(Self {
            bid: exact_price(bid, QuoteSide::Bid)?,
            ask: exact_price(ask, QuoteSide::Ask)?,
        })
    }

    fn required(self, leg: VerticalLeg, side: QuoteSide) -> Result<Price, PricingError> {
        let value = match side {
            QuoteSide::Bid => self.bid,
            QuoteSide::Ask => self.ask,
        };
        value.ok_or(PricingError::MissingQuote { leg, side })
    }
}

/// Exact leg-derived market for a long higher-strike, short lower-strike
/// debit vertical. These values are analytical references, not a native
/// complex-order-book quote or a prediction of fill probability.
/// 简体中文：由较高行权价多头腿和较低行权价空头腿精确推导的 market 参考值；不等于原生复杂订单簿报价，也不预测成交概率。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyntheticVerticalQuote {
    /// Synthetic bid derived as long-leg bid minus short-leg ask.
    /// 简体中文：合成买价，等于多头腿买价减去空头腿卖价。
    pub synthetic_bid: Money,
    /// Synthetic ask derived as long-leg ask minus short-leg bid.
    /// 简体中文：合成卖价，等于多头腿卖价减去空头腿买价。
    pub synthetic_ask: Money,
    /// Exact midpoint of the synthetic bid and ask.
    /// 简体中文：合成买价与卖价之间的精确中点。
    pub raw_mid: Money,
}

/// A quote is incomplete, invalid, crossed, inconsistent, or not exactly
/// representable with the domain decimal bounds.
/// 简体中文：报价不完整、无效、交叉、不一致，或无法在领域十进制范围内精确表示。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PricingError {
    /// A required leg/side quote is absent.
    /// 简体中文：所需腿或报价方向的数据缺失。
    MissingQuote {
        /// Leg whose quote is missing.
        /// 简体中文：缺少报价的腿。
        leg: VerticalLeg,
        /// Bid or ask side that is missing.
        /// 简体中文：缺失的是买价还是卖价。
        side: QuoteSide,
    },
    /// A wire quote is negative.
    /// 简体中文：wire 报价为负数。
    NegativeQuote {
        /// Bid or ask side containing the negative value.
        /// 简体中文：包含负值的买价或卖价方向。
        side: QuoteSide,
    },
    /// One leg's bid exceeds its ask.
    /// 简体中文：某一腿的买价高于卖价。
    CrossedLegQuote {
        /// Leg whose bid is above its ask.
        /// 简体中文：买价高于卖价的腿。
        leg: VerticalLeg,
    },
    /// The derived synthetic bid exceeds its ask.
    /// 简体中文：推导出的合成买价高于卖价。
    InconsistentSyntheticQuote,
    /// Exact decimal arithmetic exceeded a supported bound.
    /// 简体中文：精确十进制运算超出支持范围。
    Arithmetic(DecimalError),
}

impl fmt::Display for PricingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingQuote { leg, side } => {
                write!(formatter, "MISSING_{}_{}_QUOTE", leg.code(), side.code())
            }
            Self::NegativeQuote { side } => write!(formatter, "NEGATIVE_{}_QUOTE", side.code()),
            Self::CrossedLegQuote { leg } => {
                write!(formatter, "CROSSED_{}_LEG_QUOTE", leg.code())
            }
            Self::InconsistentSyntheticQuote => formatter.write_str("INCONSISTENT_SYNTHETIC_QUOTE"),
            Self::Arithmetic(error) => write!(formatter, "PRICING_ARITHMETIC_{error}"),
        }
    }
}

impl Error for PricingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Arithmetic(error) => Some(error),
            _ => None,
        }
    }
}

impl From<DecimalError> for PricingError {
    fn from(error: DecimalError) -> Self {
        Self::Arithmetic(error)
    }
}

/// Derive the synthetic bid, ask, and exact midpoint from the two leg NBBOs.
///
/// `long` must represent the higher-strike long put H and `short` the lower-
/// strike short put L. The function validates quote presence, non-negativity,
/// per-leg bid/ask ordering, and derived ordering; it does not apply timestamp,
/// generation, feed-quality, or contract-identity policy.
/// Callers must not treat the midpoint as a fillability, fair-value, or risk signal.
/// 简体中文：从两条腿的 NBBO 推导合成买价、卖价和精确中点。`long` 应为较高行权价的多头 put H，`short` 应为较低行权价的空头 put L。函数验证报价存在性、非负性、单腿买卖顺序和合成报价顺序，但不验证时间戳、generation、feed 质量或合约身份。调用方不得把中点当作可成交性、公允价值或风险信号。
pub fn synthetic_debit_vertical(
    long: &LegQuote,
    short: &LegQuote,
) -> Result<SyntheticVerticalQuote, PricingError> {
    let long_bid = long.required(VerticalLeg::Long, QuoteSide::Bid)?;
    let long_ask = long.required(VerticalLeg::Long, QuoteSide::Ask)?;
    let short_bid = short.required(VerticalLeg::Short, QuoteSide::Bid)?;
    let short_ask = short.required(VerticalLeg::Short, QuoteSide::Ask)?;

    if long_bid > long_ask {
        return Err(PricingError::CrossedLegQuote {
            leg: VerticalLeg::Long,
        });
    }
    if short_bid > short_ask {
        return Err(PricingError::CrossedLegQuote {
            leg: VerticalLeg::Short,
        });
    }

    let synthetic_bid = long_bid.checked_difference(short_ask)?;
    let synthetic_ask = long_ask.checked_difference(short_bid)?;

    if synthetic_bid > synthetic_ask {
        return Err(PricingError::InconsistentSyntheticQuote);
    }

    let raw_mid = synthetic_bid.checked_midpoint(synthetic_ask)?;
    Ok(SyntheticVerticalQuote {
        synthetic_bid,
        synthetic_ask,
        raw_mid,
    })
}

fn exact_price(
    value: Option<ExactDecimal>,
    side: QuoteSide,
) -> Result<Option<Price>, PricingError> {
    value
        .map(|value| Price::new(value).map_err(|_| PricingError::NegativeQuote { side }))
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::{LegQuote, PricingError, QuoteSide, VerticalLeg, synthetic_debit_vertical};
    use domain::{ExactDecimal, Money, Price};

    fn decimal(value: &str) -> ExactDecimal {
        ExactDecimal::parse_json_number(value).expect("valid exact decimal")
    }

    fn price(value: &str) -> Price {
        Price::parse_json_number(value).expect("valid non-negative price")
    }

    fn quote(bid: &str, ask: &str) -> LegQuote {
        LegQuote::from_prices(Some(price(bid)), Some(price(ask)))
    }

    #[test]
    fn derives_leg_synthetic_bid_ask_and_raw_mid_exactly() {
        let long = quote("5.03", "5.09");
        let short = quote("1.11", "1.14");

        let result = synthetic_debit_vertical(&long, &short).unwrap();

        assert_eq!(
            result.synthetic_bid,
            Money::parse_json_number("3.89").unwrap()
        );
        assert_eq!(
            result.synthetic_ask,
            Money::parse_json_number("3.98").unwrap()
        );
        assert_eq!(result.raw_mid, Money::parse_json_number("3.935").unwrap());
    }

    #[test]
    fn rejects_missing_negative_and_crossed_leg_quotes() {
        let complete = quote("1.00", "1.01");
        let missing = LegQuote::from_prices(None, Some(price("1.01")));
        assert_eq!(
            synthetic_debit_vertical(&missing, &complete),
            Err(PricingError::MissingQuote {
                leg: VerticalLeg::Long,
                side: QuoteSide::Bid,
            })
        );

        assert_eq!(
            LegQuote::from_exact(Some(decimal("-0.01")), Some(decimal("0.02"))),
            Err(PricingError::NegativeQuote {
                side: QuoteSide::Bid,
            })
        );

        let crossed = quote("1.02", "1.01");
        assert_eq!(
            synthetic_debit_vertical(&crossed, &complete),
            Err(PricingError::CrossedLegQuote {
                leg: VerticalLeg::Long,
            })
        );

        let cheaper_long = quote("0.10", "0.20");
        let more_expensive_short = quote("0.21", "0.30");
        let signed_reference =
            synthetic_debit_vertical(&cheaper_long, &more_expensive_short).unwrap();
        assert_eq!(
            signed_reference.synthetic_bid,
            Money::parse_json_number("-0.20").unwrap()
        );
        assert_eq!(
            signed_reference.synthetic_ask,
            Money::parse_json_number("-0.01").unwrap()
        );
        assert_eq!(
            signed_reference.raw_mid,
            Money::parse_json_number("-0.105").unwrap()
        );
    }

    #[test]
    fn exact_decimal_boundaries_are_stable_for_synthetic_vertical_midpoints() {
        let cases = [
            ("2.749999", false, false),
            ("2.75", false, false),
            ("2.750001", true, false),
            ("3.249999", true, false),
            ("3.25", true, false),
            ("3.250001", true, true),
        ];
        let lower_boundary = decimal("2.75");
        let upper_boundary = decimal("3.25");

        for (expected_mid, above_lower, above_upper) in cases {
            let long_price = decimal(expected_mid)
                .checked_add(decimal("0.60"))
                .expect("exact test input");
            let long = LegQuote::from_exact(Some(long_price), Some(long_price)).unwrap();
            let short = quote("0.60", "0.60");
            let raw_mid = synthetic_debit_vertical(&long, &short)
                .unwrap()
                .raw_mid
                .decimal();

            assert_eq!(raw_mid, decimal(expected_mid));
            assert_eq!(raw_mid > lower_boundary, above_lower);
            assert_eq!(raw_mid > upper_boundary, above_upper);
        }
    }
}
