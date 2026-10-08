//! Validated two-leg vertical structures with canonical strike ordering.
//!
//! Validation checks economic contract identity and requires opposite sides,
//! distinct strikes, and a one-to-one ratio. It does not add pricing, inventory,
//! risk, or order-submission authority.
//!
//! # 简体中文
//!
//! 提供按规范行权价顺序排列的已校验双腿 vertical 结构。
//!
//! 校验会检查合约经济身份，并要求两腿方向相反、行权价不同且比例为一比一。本模块不提供
//! 定价、库存、风险或订单提交权限。

use crate::{
    ContractMultiplier, Deliverable, ExpirationDate, OptionContract, OptionRight, OptionSymbol,
    Strike, Underlying,
};
use std::fmt;

/// The direction carried by an option leg in a position.
/// 期权腿在持仓中采用的方向。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PositionSide {
    /// The strategy holds the option contract.
    /// 策略持有该期权合约。
    Long,
    /// The strategy is short the option contract.
    /// 策略做空该期权合约。
    Short,
}

/// Positive whole-number ratio assigned to one leg of a strategy.
/// 分配给策略某一腿的正整数比例。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct LegRatio(u64);

/// One option contract, position side, and ratio in a candidate vertical.
/// 候选 vertical 中的一份期权合约、持仓方向和比例。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct VerticalLeg {
    contract: OptionContract,
    side: PositionSide,
    ratio: LegRatio,
}

/// Which strike is long after a vertical is normalized into low and high legs.
/// vertical 规范化为低、高行权价腿后，哪一腿为多头。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum VerticalOrientation {
    /// The lower-strike leg is long.
    /// 低行权价腿为多头。
    LongLower,
    /// The higher-strike leg is long.
    /// 高行权价腿为多头。
    LongHigher,
}

/// A two-leg 1:1 vertical with matching economic contract terms.
///
/// The lower and higher strikes are stored in canonical order. The type allows
/// either option right and any positive strike width; strategy-specific rules
/// belong to the automation policy that selects these positions.
/// 较低与较高行权价按规范顺序存储。此类型允许看涨或看跌方向及任意正宽度；策略专属规则
/// 由选择这些持仓的自动化策略负责。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ValidatedVertical {
    lower: VerticalLeg,
    higher: VerticalLeg,
    orientation: VerticalOrientation,
}

/// Failure to form a valid two-leg vertical.
/// 无法构造有效双腿 vertical。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerticalError {
    /// A leg ratio was zero.
    /// 某一腿的比例为零。
    ZeroRatio,
    /// Both legs have the same strike.
    /// 两腿行权价相同。
    SameStrike,
    /// Both legs have the same position side.
    /// 两腿持仓方向相同。
    SamePositionSide,
    /// The vertical requires a one-to-one ratio for both legs.
    /// vertical 要求两腿比例均为一比一。
    NonOneToOneRatio,
    /// The legs have different underlying symbols.
    /// 两腿标的代码不同。
    UnderlyingMismatch,
    /// The legs have different expiration dates.
    /// 两腿到期日不同。
    ExpirationMismatch,
    /// The legs have different call or put rights.
    /// 两腿看涨或看跌方向不同。
    OptionRightMismatch,
    /// The legs have different contract multipliers.
    /// 两腿合约乘数不同。
    MultiplierMismatch,
    /// The legs have different currencies.
    /// 两腿货币不同。
    CurrencyMismatch,
    /// The legs have different deliverable baskets.
    /// 两腿交割物篮子不同。
    DeliverableMismatch,
}

impl LegRatio {
    /// Creates a positive whole-number leg ratio.
    /// 创建正整数腿比例。
    pub fn new(value: u64) -> Result<Self, VerticalError> {
        if value == 0 {
            return Err(VerticalError::ZeroRatio);
        }
        Ok(Self(value))
    }

    /// Returns the whole-number ratio.
    /// 返回整数比例。
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl VerticalLeg {
    /// Combines a contract with its position side and validated ratio.
    /// 将合约与其持仓方向和已校验比例组合。
    pub const fn new(contract: OptionContract, side: PositionSide, ratio: LegRatio) -> Self {
        Self {
            contract,
            side,
            ratio,
        }
    }

    /// Borrows the option contract.
    /// 借用期权合约。
    pub fn contract(&self) -> &OptionContract {
        &self.contract
    }

    /// Returns the position side.
    /// 返回持仓方向。
    pub const fn side(&self) -> PositionSide {
        self.side
    }

    /// Returns the leg ratio.
    /// 返回腿比例。
    pub const fn ratio(&self) -> LegRatio {
        self.ratio
    }

    /// Borrows the option symbol.
    /// 借用期权标识。
    pub const fn symbol(&self) -> &OptionSymbol {
        self.contract.symbol()
    }

    /// Returns the strike of this leg.
    /// 返回此腿的行权价。
    pub const fn strike(&self) -> Strike {
        self.contract.symbol().strike()
    }
}

impl ValidatedVertical {
    /// Validate and normalize two legs into ascending strike order.
    /// 校验两条腿，并按行权价从低到高规范化。
    pub fn new(first: VerticalLeg, second: VerticalLeg) -> Result<Self, VerticalError> {
        if first.ratio.get() != 1 || second.ratio.get() != 1 {
            return Err(VerticalError::NonOneToOneRatio);
        }
        if first.side == second.side {
            return Err(VerticalError::SamePositionSide);
        }

        validate_shared_contract_terms(first.contract(), second.contract())?;

        let first_strike = first.strike();
        let second_strike = second.strike();
        if first_strike == second_strike {
            return Err(VerticalError::SameStrike);
        }

        let (lower, higher) = if first_strike < second_strike {
            (first, second)
        } else {
            (second, first)
        };
        let orientation = match lower.side {
            PositionSide::Long => VerticalOrientation::LongLower,
            PositionSide::Short => VerticalOrientation::LongHigher,
        };

        Ok(Self {
            lower,
            higher,
            orientation,
        })
    }

    /// Borrows the lower-strike leg.
    /// 借用低行权价腿。
    pub fn lower_leg(&self) -> &VerticalLeg {
        &self.lower
    }

    /// Borrows the higher-strike leg.
    /// 借用高行权价腿。
    pub fn higher_leg(&self) -> &VerticalLeg {
        &self.higher
    }

    /// Borrows the shared underlying symbol.
    /// 借用两腿共有的标的代码。
    pub fn underlying(&self) -> &Underlying {
        self.lower.symbol().underlying()
    }

    /// Returns the shared expiration date.
    /// 返回两腿共有的到期日。
    pub const fn expiration(&self) -> ExpirationDate {
        self.lower.symbol().expiration()
    }

    /// Returns the shared call or put right.
    /// 返回两腿共有的看涨或看跌方向。
    pub const fn right(&self) -> OptionRight {
        self.lower.symbol().right()
    }

    /// Returns the lower strike.
    /// 返回较低行权价。
    pub const fn lower_strike(&self) -> Strike {
        self.lower.strike()
    }

    /// Returns the higher strike.
    /// 返回较高行权价。
    pub const fn higher_strike(&self) -> Strike {
        self.higher.strike()
    }

    /// Returns the exact width in integer thousandths.
    /// 返回以整数千分之一表示的精确宽度。
    pub const fn width_mills(&self) -> u32 {
        self.lower_strike().checked_width(self.higher_strike())
    }

    /// Returns the exact decimal strike width.
    /// 返回精确十进制行权价宽度。
    pub fn width(&self) -> crate::ExactDecimal {
        crate::ExactDecimal::from_parts(i128::from(self.width_mills()), 3)
            .expect("vertical strike width fits the exact decimal scale")
    }

    /// Returns which normalized leg is long.
    /// 返回规范化后的哪条腿为多头。
    pub const fn orientation(&self) -> VerticalOrientation {
        self.orientation
    }

    /// Returns the shared contract multiplier.
    /// 返回两腿共有的合约乘数。
    pub const fn multiplier(&self) -> ContractMultiplier {
        self.lower.contract.multiplier()
    }

    /// Borrows the shared contract currency.
    /// 借用两腿共有的合约货币。
    pub fn currency(&self) -> &crate::ContractCurrency {
        self.lower.contract.currency()
    }

    /// Borrows the shared deliverable basket.
    /// 借用两腿共有的交割物篮子。
    pub fn deliverable(&self) -> &Deliverable {
        self.lower.contract.deliverable()
    }
}

fn validate_shared_contract_terms(
    first: &OptionContract,
    second: &OptionContract,
) -> Result<(), VerticalError> {
    if first.symbol().underlying() != second.symbol().underlying() {
        return Err(VerticalError::UnderlyingMismatch);
    }
    if first.symbol().expiration() != second.symbol().expiration() {
        return Err(VerticalError::ExpirationMismatch);
    }
    if first.symbol().right() != second.symbol().right() {
        return Err(VerticalError::OptionRightMismatch);
    }
    if first.multiplier() != second.multiplier() {
        return Err(VerticalError::MultiplierMismatch);
    }
    if first.currency() != second.currency() {
        return Err(VerticalError::CurrencyMismatch);
    }
    if first.deliverable() != second.deliverable() {
        return Err(VerticalError::DeliverableMismatch);
    }
    Ok(())
}

impl fmt::Display for VerticalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ZeroRatio => "VERTICAL_RATIO_MUST_BE_POSITIVE",
            Self::SameStrike => "VERTICAL_STRIKES_MUST_DIFFER",
            Self::SamePositionSide => "VERTICAL_REQUIRES_LONG_AND_SHORT_LEGS",
            Self::NonOneToOneRatio => "VERTICAL_REQUIRES_ONE_TO_ONE_RATIO",
            Self::UnderlyingMismatch => "VERTICAL_UNDERLYING_MISMATCH",
            Self::ExpirationMismatch => "VERTICAL_EXPIRATION_MISMATCH",
            Self::OptionRightMismatch => "VERTICAL_OPTION_RIGHT_MISMATCH",
            Self::MultiplierMismatch => "VERTICAL_MULTIPLIER_MISMATCH",
            Self::CurrencyMismatch => "VERTICAL_CURRENCY_MISMATCH",
            Self::DeliverableMismatch => "VERTICAL_DELIVERABLE_MISMATCH",
        })
    }
}

impl std::error::Error for VerticalError {}

#[cfg(test)]
mod tests {
    use super::{
        LegRatio, PositionSide, ValidatedVertical, VerticalError, VerticalLeg, VerticalOrientation,
    };
    use crate::{
        ContractCurrency, ContractMultiplier, Deliverable, DeliverableComponent, ExactDecimal,
        ExpirationDate, OptionContract, OptionRight, OptionSymbol, SpreadKey, Strike, Underlying,
    };

    fn contract(
        strike: &str,
        right: OptionRight,
        multiplier: u64,
        currency: &str,
        delivered_shares: &str,
    ) -> OptionContract {
        let underlying = Underlying::new("QQQ").unwrap();
        let deliverable = equity_deliverable("QQQ", delivered_shares);
        contract_with_terms(
            strike,
            underlying,
            ExpirationDate::new(2026, 8, 14).unwrap(),
            right,
            multiplier,
            currency,
            deliverable,
        )
    }

    fn contract_with_terms(
        strike: &str,
        underlying: Underlying,
        expiration: ExpirationDate,
        right: OptionRight,
        multiplier: u64,
        currency: &str,
        deliverable: Deliverable,
    ) -> OptionContract {
        let symbol = OptionSymbol::new(
            underlying,
            expiration,
            right,
            Strike::parse_json_number(strike).unwrap(),
        );
        OptionContract::new(
            symbol,
            ContractMultiplier::new(multiplier).unwrap(),
            ContractCurrency::new(currency).unwrap(),
            deliverable,
        )
    }

    fn equity_deliverable(root: &str, shares: &str) -> Deliverable {
        Deliverable::new(vec![
            DeliverableComponent::equity(
                Underlying::new(root).unwrap(),
                ExactDecimal::parse_json_number(shares).unwrap(),
            )
            .unwrap(),
        ])
        .unwrap()
    }

    fn cash_deliverable(code: &str, amount: &str) -> Deliverable {
        Deliverable::new(vec![
            DeliverableComponent::cash(
                ContractCurrency::new(code).unwrap(),
                ExactDecimal::parse_json_number(amount).unwrap(),
            )
            .unwrap(),
        ])
        .unwrap()
    }

    fn leg(strike: &str, right: OptionRight, side: PositionSide) -> VerticalLeg {
        VerticalLeg::new(
            contract(strike, right, 100, "USD", "100"),
            side,
            LegRatio::new(1).unwrap(),
        )
    }

    #[test]
    fn vertical_accepts_call_or_put_and_any_positive_width_with_either_orientation() {
        let call = ValidatedVertical::new(
            leg("742", OptionRight::Call, PositionSide::Long),
            leg("740", OptionRight::Call, PositionSide::Short),
        )
        .unwrap();
        assert_eq!(call.lower_strike().to_string(), "740");
        assert_eq!(call.higher_strike().to_string(), "742");
        assert_eq!(call.width_mills(), 2_000);
        assert_eq!(call.width().to_string(), "2");
        assert_eq!(call.orientation(), VerticalOrientation::LongHigher);

        let put = ValidatedVertical::new(
            leg("742", OptionRight::Put, PositionSide::Short),
            leg("740", OptionRight::Put, PositionSide::Long),
        )
        .unwrap();
        assert_eq!(put.width_mills(), 2_000);
        assert_eq!(put.orientation(), VerticalOrientation::LongLower);
    }

    #[test]
    fn vertical_rejects_non_one_to_one_and_mismatched_contract_identity() {
        let first = leg("740", OptionRight::Put, PositionSide::Long);
        let ratio_two = VerticalLeg::new(
            contract("742", OptionRight::Put, 100, "USD", "100"),
            PositionSide::Short,
            LegRatio::new(2).unwrap(),
        );
        assert_eq!(
            ValidatedVertical::new(first.clone(), ratio_two),
            Err(VerticalError::NonOneToOneRatio)
        );
        assert_eq!(LegRatio::new(0), Err(VerticalError::ZeroRatio));
        assert_eq!(
            ValidatedVertical::new(
                first.clone(),
                leg("742", OptionRight::Call, PositionSide::Short)
            ),
            Err(VerticalError::OptionRightMismatch)
        );
        assert_eq!(
            ValidatedVertical::new(
                first.clone(),
                leg("740", OptionRight::Put, PositionSide::Short)
            ),
            Err(VerticalError::SameStrike)
        );
        assert_eq!(
            ValidatedVertical::new(
                first.clone(),
                leg("742", OptionRight::Put, PositionSide::Long)
            ),
            Err(VerticalError::SamePositionSide)
        );

        let different_underlying = {
            let symbol = OptionSymbol::new(
                Underlying::new("SPY").unwrap(),
                ExpirationDate::new(2026, 8, 14).unwrap(),
                OptionRight::Put,
                Strike::parse_json_number("742").unwrap(),
            );
            let deliverable = Deliverable::new(vec![
                DeliverableComponent::equity(
                    Underlying::new("SPY").unwrap(),
                    ExactDecimal::from_integer(100),
                )
                .unwrap(),
            ])
            .unwrap();
            VerticalLeg::new(
                OptionContract::new(
                    symbol,
                    ContractMultiplier::new(100).unwrap(),
                    ContractCurrency::new("USD").unwrap(),
                    deliverable,
                ),
                PositionSide::Short,
                LegRatio::new(1).unwrap(),
            )
        };
        assert_eq!(
            ValidatedVertical::new(first.clone(), different_underlying),
            Err(VerticalError::UnderlyingMismatch)
        );

        let different_expiration = VerticalLeg::new(
            contract_with_terms(
                "742",
                Underlying::new("QQQ").unwrap(),
                ExpirationDate::new(2026, 8, 21).unwrap(),
                OptionRight::Put,
                100,
                "USD",
                equity_deliverable("QQQ", "100"),
            ),
            PositionSide::Short,
            LegRatio::new(1).unwrap(),
        );
        assert_eq!(
            ValidatedVertical::new(first.clone(), different_expiration),
            Err(VerticalError::ExpirationMismatch)
        );

        let different_terms = VerticalLeg::new(
            contract("742", OptionRight::Put, 10, "USD", "100"),
            PositionSide::Short,
            LegRatio::new(1).unwrap(),
        );
        assert_eq!(
            ValidatedVertical::new(first, different_terms),
            Err(VerticalError::MultiplierMismatch)
        );

        let first = leg("740", OptionRight::Put, PositionSide::Long);
        let different_currency = VerticalLeg::new(
            contract("742", OptionRight::Put, 100, "CAD", "100"),
            PositionSide::Short,
            LegRatio::new(1).unwrap(),
        );
        assert_eq!(
            ValidatedVertical::new(first.clone(), different_currency),
            Err(VerticalError::CurrencyMismatch)
        );
        let different_deliverable = VerticalLeg::new(
            contract("742", OptionRight::Put, 100, "USD", "50"),
            PositionSide::Short,
            LegRatio::new(1).unwrap(),
        );
        assert_eq!(
            ValidatedVertical::new(first, different_deliverable),
            Err(VerticalError::DeliverableMismatch)
        );
    }

    #[test]
    fn spread_identity_keeps_orientation_multiplier_and_deliverable_terms() {
        let long_lower = ValidatedVertical::new(
            leg("740", OptionRight::Put, PositionSide::Long),
            leg("742", OptionRight::Put, PositionSide::Short),
        )
        .unwrap();
        let long_higher = ValidatedVertical::new(
            leg("740", OptionRight::Put, PositionSide::Short),
            leg("742", OptionRight::Put, PositionSide::Long),
        )
        .unwrap();
        let reversed_input = ValidatedVertical::new(
            leg("742", OptionRight::Put, PositionSide::Short),
            leg("740", OptionRight::Put, PositionSide::Long),
        )
        .unwrap();
        let standard = SpreadKey::from_vertical(&long_lower);
        let reversed = SpreadKey::from_vertical(&long_higher);
        assert_eq!(standard.legacy_label(), reversed.legacy_label());
        assert_ne!(standard, reversed);
        assert_eq!(standard, SpreadKey::from_vertical(&reversed_input));

        let nonstandard = ValidatedVertical::new(
            VerticalLeg::new(
                contract("740", OptionRight::Put, 10, "USD", "100"),
                PositionSide::Long,
                LegRatio::new(1).unwrap(),
            ),
            VerticalLeg::new(
                contract("742", OptionRight::Put, 10, "USD", "100"),
                PositionSide::Short,
                LegRatio::new(1).unwrap(),
            ),
        )
        .unwrap();
        assert_ne!(standard, SpreadKey::from_vertical(&nonstandard));

        for (currency, deliverable) in [
            ("CAD", equity_deliverable("QQQ", "100")),
            ("USD", equity_deliverable("QQQ", "50")),
            ("USD", cash_deliverable("USD", "100")),
        ] {
            let changed_terms = ValidatedVertical::new(
                VerticalLeg::new(
                    contract_with_terms(
                        "740",
                        Underlying::new("QQQ").unwrap(),
                        ExpirationDate::new(2026, 8, 14).unwrap(),
                        OptionRight::Put,
                        100,
                        currency,
                        deliverable.clone(),
                    ),
                    PositionSide::Long,
                    LegRatio::new(1).unwrap(),
                ),
                VerticalLeg::new(
                    contract_with_terms(
                        "742",
                        Underlying::new("QQQ").unwrap(),
                        ExpirationDate::new(2026, 8, 14).unwrap(),
                        OptionRight::Put,
                        100,
                        currency,
                        deliverable,
                    ),
                    PositionSide::Short,
                    LegRatio::new(1).unwrap(),
                ),
            )
            .unwrap();
            assert_eq!(
                standard.legacy_label(),
                SpreadKey::from_vertical(&changed_terms).legacy_label()
            );
            assert_ne!(standard, SpreadKey::from_vertical(&changed_terms));
        }
    }
}
