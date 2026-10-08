//! Exact multi-leg option order intent values without broker wire semantics.
//!
//! A combo intent keeps each leg's instrument, side, open/close effect, ratio, and quantity. Its
//! signed cash direction is explicit as net debit, net credit, or zero cost; all amounts reuse
//! the existing exact `Price` and `ExactDecimal` types.
//!
//! # 简体中文
//!
//! 本模块定义精确多腿期权订单意图，不包含券商 wire 语义。
//!
//! 组合意图保留每条腿的合约、方向、开平仓意图、ratio 和数量。净现金方向显式区分净借记、净贷记和零成本；金额复用现有精确 `Price` 与 `ExactDecimal`。

use crate::{
    ContractMultiplier, DecimalError, ExactDecimal, ExecutionRoute, InstrumentKey, IntentId,
    LogicalOrderId, MAX_SAFE_INTEGER, Price, Quantity,
};
use std::fmt;

/// Positive non-zero price required by a net debit or net credit.
/// 净借记或净贷记所需的正数价格。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct NonZeroPrice(Price);

/// Explicit net price direction for a multi-leg option order.
/// 多腿期权订单显式净价方向。
///
/// Passing binary floating-point directly is rejected by the exact-price API.
/// 精确价格 API 会拒绝直接传入二进制浮点数。
///
/// ```compile_fail
/// use domain::NetOrderPrice;
/// let _ = NetOrderPrice::net_debit(2.75_f64);
/// ```
///
/// The adapter must parse a source decimal representation into `ExactDecimal` first.
/// 适配器必须先将来源十进制表示解析为 `ExactDecimal`。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum NetOrderPrice {
    /// Pay a positive exact net amount.
    /// 支付正数精确净额。
    NetDebit(NonZeroPrice),
    /// Receive a positive exact net amount.
    /// 收取正数精确净额。
    NetCredit(NonZeroPrice),
    /// Submit an explicitly zero-cost order with no debit or credit direction.
    /// 显式表示零成本订单，不赋予借记或贷记方向。
    ZeroCost,
}

/// Positive exact price increment used to validate a broker tick schedule.
/// 用于校验券商报价档位的正数精确价格增量。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct PriceTick(ExactDecimal);

/// Why a price cannot be represented as a non-zero net debit or credit.
/// 价格不能表示为非零净借记或净贷记的原因。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NonZeroPriceError {
    /// The amount is zero and must use `NetOrderPrice::ZeroCost`.
    /// 金额为零，必须使用 `NetOrderPrice::ZeroCost`。
    Zero,
}

/// Tick increment validation failure.
/// 价格档位增量校验失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PriceTickError {
    /// A zero or negative increment is invalid.
    /// 零或负数增量无效。
    NonPositive,
    /// Exact scaling exceeded the supported decimal representation.
    /// 精确缩放超出支持的十进制表示范围。
    Decimal(DecimalError),
    /// The exact order price is not a multiple of the increment.
    /// 精确订单价格不是增量的整数倍。
    OffIncrement,
}

/// Which side of the option market a leg trades.
/// 期权腿的买卖方向。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum OrderSide {
    /// Buy the option contract.
    /// 买入期权合约。
    Buy,
    /// Sell the option contract.
    /// 卖出期权合约。
    Sell,
}

/// Whether a leg opens or closes a position.
/// 订单腿是开仓还是平仓。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PositionEffect {
    /// Open a position in the selected side.
    /// 按指定方向开仓。
    Open,
    /// Close a position in the selected side.
    /// 按指定方向平仓。
    Close,
}

/// One fully identified option order leg.
/// 一条具有完整合约身份的期权订单腿。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct OptionOrderLeg {
    instrument: InstrumentKey,
    side: OrderSide,
    effect: PositionEffect,
    ratio: crate::LegRatio,
    quantity: Quantity,
}

/// Broker-neutral immutable option combo intent.
/// 券商中立且不可变的期权组合意图。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct OptionComboIntent {
    intent_id: IntentId,
    logical_order_id: LogicalOrderId,
    route: ExecutionRoute,
    quantity: Quantity,
    legs: Vec<OptionOrderLeg>,
    net_price: NetOrderPrice,
}

/// Combo construction failure that preserves exact quantity and identity requirements.
/// 组合构造失败；保留精确数量和身份要求。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComboOrderError {
    /// A multi-leg combo requires at least two legs.
    /// 多腿组合至少需要两条腿。
    TooFewLegs,
    /// A leg quantity does not equal combo quantity multiplied by its ratio.
    /// 腿数量不等于组合数量乘以腿比例。
    LegQuantityRatioMismatch,
    /// Checked ratio multiplication exceeded the supported quantity range.
    /// 有检查的 ratio 乘法超出支持的数量范围。
    QuantityOverflow,
}

impl NonZeroPrice {
    /// Creates a non-zero net amount from an exact non-negative price.
    /// 根据精确非负价格创建非零净额。
    pub fn new(value: Price) -> Result<Self, NonZeroPriceError> {
        if value.decimal().is_zero() {
            return Err(NonZeroPriceError::Zero);
        }
        Ok(Self(value))
    }

    /// Returns the exact positive price.
    /// 返回精确正数价格。
    pub const fn price(self) -> Price {
        self.0
    }
}

impl NetOrderPrice {
    /// Creates a positive exact net debit.
    /// 创建正数精确净借记。
    pub fn net_debit(value: Price) -> Result<Self, NonZeroPriceError> {
        NonZeroPrice::new(value).map(Self::NetDebit)
    }

    /// Creates a positive exact net credit.
    /// 创建正数精确净贷记。
    pub fn net_credit(value: Price) -> Result<Self, NonZeroPriceError> {
        NonZeroPrice::new(value).map(Self::NetCredit)
    }

    /// Returns the exact non-negative amount without erasing its direction variant.
    /// 返回精确非负金额，同时保留其方向变体。
    pub const fn amount(self) -> Price {
        match self {
            Self::NetDebit(value) | Self::NetCredit(value) => value.price(),
            Self::ZeroCost => Price::ZERO,
        }
    }

    /// Returns whether the order explicitly represents zero net cost.
    /// 返回订单是否显式表示零净成本。
    pub const fn is_zero_cost(self) -> bool {
        matches!(self, Self::ZeroCost)
    }

    /// Validates that the exact amount lies on the selected positive tick increment.
    /// 校验精确金额是否位于所选正数价格档位增量上。
    pub fn validate_tick(self, tick: PriceTick) -> Result<(), PriceTickError> {
        let amount = self.amount().decimal();
        let increment = tick.0;
        let scale = amount.scale().max(increment.scale());
        let amount_units = amount
            .to_scaled_integer(scale)
            .map_err(PriceTickError::Decimal)?;
        let increment_units = increment
            .to_scaled_integer(scale)
            .map_err(PriceTickError::Decimal)?;
        if increment_units <= 0 {
            return Err(PriceTickError::NonPositive);
        }
        if amount_units % increment_units == 0 {
            Ok(())
        } else {
            Err(PriceTickError::OffIncrement)
        }
    }
}

impl PriceTick {
    /// Creates a positive exact tick increment.
    /// 创建正数精确价格档位增量。
    pub fn new(value: ExactDecimal) -> Result<Self, PriceTickError> {
        if value <= ExactDecimal::ZERO {
            return Err(PriceTickError::NonPositive);
        }
        Ok(Self(value))
    }

    /// Returns the exact price increment.
    /// 返回精确价格增量。
    pub const fn decimal(self) -> ExactDecimal {
        self.0
    }
}

impl OptionOrderLeg {
    /// Creates an order leg with explicit contract, side, effect, ratio, and quantity.
    /// 根据显式合约、方向、开平仓意图、ratio 和数量创建订单腿。
    pub const fn new(
        instrument: InstrumentKey,
        side: OrderSide,
        effect: PositionEffect,
        ratio: crate::LegRatio,
        quantity: Quantity,
    ) -> Self {
        Self {
            instrument,
            side,
            effect,
            ratio,
            quantity,
        }
    }

    /// Returns the fully qualified instrument identity.
    /// 返回完整核验的合约身份。
    pub const fn instrument(&self) -> &InstrumentKey {
        &self.instrument
    }

    /// Returns the explicit buy or sell direction.
    /// 返回显式买入或卖出方向。
    pub const fn side(&self) -> OrderSide {
        self.side
    }

    /// Returns the explicit open or close effect.
    /// 返回显式开仓或平仓意图。
    pub const fn effect(&self) -> PositionEffect {
        self.effect
    }

    /// Returns the integer combo ratio for this leg.
    /// 返回此腿的整数组合比例。
    pub const fn ratio(&self) -> crate::LegRatio {
        self.ratio
    }

    /// Returns this leg's exact whole-contract quantity.
    /// 返回此腿精确整数合约数量。
    pub const fn quantity(&self) -> Quantity {
        self.quantity
    }

    /// Returns the multiplier held in the fully qualified instrument key.
    /// 返回完整核验合约键中的乘数。
    pub const fn multiplier(&self) -> ContractMultiplier {
        self.instrument.as_option().contract().multiplier()
    }
}

impl OptionComboIntent {
    /// Creates an immutable combo intent after validating every leg's ratio quantity.
    /// 校验每条腿的 ratio 数量后创建不可变组合意图。
    pub fn new(
        intent_id: IntentId,
        logical_order_id: LogicalOrderId,
        route: ExecutionRoute,
        quantity: Quantity,
        legs: Vec<OptionOrderLeg>,
        net_price: NetOrderPrice,
    ) -> Result<Self, ComboOrderError> {
        if legs.len() < 2 {
            return Err(ComboOrderError::TooFewLegs);
        }
        for leg in &legs {
            let expected = quantity
                .get()
                .checked_mul(leg.ratio.get())
                .ok_or(ComboOrderError::QuantityOverflow)?;
            if expected > MAX_SAFE_INTEGER {
                return Err(ComboOrderError::QuantityOverflow);
            }
            if leg.quantity.get() != expected {
                return Err(ComboOrderError::LegQuantityRatioMismatch);
            }
        }
        Ok(Self {
            intent_id,
            logical_order_id,
            route,
            quantity,
            legs,
            net_price,
        })
    }

    /// Returns the internal strategy intent ID.
    /// 返回内部策略 IntentId。
    pub const fn intent_id(&self) -> &IntentId {
        &self.intent_id
    }

    /// Returns the internal logical order ID.
    /// 返回内部 LogicalOrderId。
    pub const fn logical_order_id(&self) -> &LogicalOrderId {
        &self.logical_order_id
    }

    /// Returns the strategy-bound fixed execution route.
    /// 返回绑定策略的固定执行路由。
    pub const fn route(&self) -> &ExecutionRoute {
        &self.route
    }

    /// Returns the number of base combo contracts.
    /// 返回组合基础合约数量。
    pub const fn quantity(&self) -> Quantity {
        self.quantity
    }

    /// Borrows all validated order legs.
    /// 借用所有已校验订单腿。
    pub fn legs(&self) -> &[OptionOrderLeg] {
        &self.legs
    }

    /// Returns the exact net price and explicit direction.
    /// 返回带显式方向的精确净价。
    pub const fn net_price(&self) -> NetOrderPrice {
        self.net_price
    }
}

impl fmt::Display for NonZeroPriceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NET_DEBIT_OR_CREDIT_MUST_BE_POSITIVE")
    }
}

impl std::error::Error for NonZeroPriceError {}

impl fmt::Display for PriceTickError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonPositive => formatter.write_str("PRICE_TICK_MUST_BE_POSITIVE"),
            Self::Decimal(error) => error.fmt(formatter),
            Self::OffIncrement => formatter.write_str("ORDER_PRICE_OFF_TICK"),
        }
    }
}

impl std::error::Error for PriceTickError {}

impl fmt::Display for ComboOrderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::TooFewLegs => "OPTION_COMBO_REQUIRES_AT_LEAST_TWO_LEGS",
            Self::LegQuantityRatioMismatch => "OPTION_COMBO_LEG_QUANTITY_RATIO_MISMATCH",
            Self::QuantityOverflow => "OPTION_COMBO_QUANTITY_OVERFLOW",
        })
    }
}

impl std::error::Error for ComboOrderError {}

#[cfg(test)]
mod tests {
    use super::{
        ComboOrderError, NetOrderPrice, NonZeroPriceError, OptionComboIntent, OptionOrderLeg,
        OrderSide, PositionEffect, PriceTick, PriceTickError,
    };
    use crate::{
        AccountNamespace, AccountScope, BrokerEnvironment, ContractCurrency, ContractMultiplier,
        Deliverable, DeliverableComponent, ExactDecimal, ExecutionBrokerId, ExecutionRoute,
        ExpirationDate, InstrumentKey, IntentId, LegRatio, LogicalOrderId, OptionContract,
        OptionExerciseStyle, OptionInstrumentKey, OptionRight, OptionSettlementType, OptionSymbol,
        Price, Quantity, StrategyInstanceId, Strike, Underlying,
    };

    fn instrument(strike: &str, multiplier: u64, shares: &str) -> InstrumentKey {
        let underlying = Underlying::new("SPY").unwrap();
        let deliverable = Deliverable::new(vec![
            DeliverableComponent::equity(
                underlying.clone(),
                ExactDecimal::parse_json_number(shares).unwrap(),
            )
            .unwrap(),
        ])
        .unwrap();
        let contract = OptionContract::new(
            OptionSymbol::new(
                underlying,
                ExpirationDate::new(2026, 9, 30).unwrap(),
                OptionRight::Put,
                Strike::parse_json_number(strike).unwrap(),
            ),
            ContractMultiplier::new(multiplier).unwrap(),
            ContractCurrency::new("USD").unwrap(),
            deliverable,
        );
        InstrumentKey::option(OptionInstrumentKey::new(
            contract,
            crate::TradingClass::new("SPY").unwrap(),
            OptionExerciseStyle::American,
            OptionSettlementType::Physical,
        ))
    }

    fn route() -> ExecutionRoute {
        ExecutionRoute::new(
            StrategyInstanceId::new("synthetic-combo-strategy").unwrap(),
            AccountNamespace::new(
                ExecutionBrokerId::Schwab,
                BrokerEnvironment::Paper,
                AccountScope::new("synthetic-combo-account").unwrap(),
            ),
        )
    }

    fn leg(
        strike: &str,
        side: OrderSide,
        effect: PositionEffect,
        ratio: u64,
        quantity: u64,
    ) -> OptionOrderLeg {
        OptionOrderLeg::new(
            instrument(strike, 100, "100"),
            side,
            effect,
            LegRatio::new(ratio).unwrap(),
            Quantity::new(quantity).unwrap(),
        )
    }

    #[test]
    fn combo_keeps_each_leg_direction_effect_ratio_quantity_multiplier_and_contract() {
        let intent = OptionComboIntent::new(
            IntentId::new("intent-combo-01").unwrap(),
            LogicalOrderId::new("logical-combo-01").unwrap(),
            route(),
            Quantity::new(1).unwrap(),
            vec![
                leg("600", OrderSide::Buy, PositionEffect::Open, 1, 1),
                leg("605", OrderSide::Sell, PositionEffect::Open, 2, 2),
            ],
            NetOrderPrice::net_debit(Price::parse_json_number("2.75").unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(intent.quantity().get(), 1);
        assert_eq!(intent.legs()[0].side(), OrderSide::Buy);
        assert_eq!(intent.legs()[0].effect(), PositionEffect::Open);
        assert_eq!(intent.legs()[1].ratio().get(), 2);
        assert_eq!(intent.legs()[1].quantity().get(), 2);
        assert_eq!(intent.legs()[1].multiplier().get(), 100);
        assert_eq!(
            intent.legs()[1]
                .instrument()
                .as_option()
                .strike()
                .to_string(),
            "605"
        );
        assert_eq!(intent.net_price().amount().decimal().to_string(), "2.75");
        assert_eq!(intent.intent_id().as_str(), "intent-combo-01");
        assert_eq!(intent.logical_order_id().as_str(), "logical-combo-01");
    }

    #[test]
    fn zero_cost_is_explicit_and_debit_credit_do_not_infer_direction_from_sign() {
        let zero = NetOrderPrice::ZeroCost;
        let debit = NetOrderPrice::net_debit(Price::parse_json_number("2.75").unwrap()).unwrap();
        let credit = NetOrderPrice::net_credit(Price::parse_json_number("2.75").unwrap()).unwrap();
        assert!(zero.is_zero_cost());
        assert_eq!(zero.amount().decimal(), ExactDecimal::ZERO);
        assert_ne!(debit, credit);
        assert_ne!(zero, debit);
        assert_eq!(debit.amount().decimal().to_string(), "2.75");
        assert_eq!(credit.amount().decimal().to_string(), "2.75");
        assert_eq!(
            NetOrderPrice::net_debit(Price::ZERO),
            Err(NonZeroPriceError::Zero)
        );
        assert_eq!(
            NetOrderPrice::net_credit(Price::ZERO),
            Err(NonZeroPriceError::Zero)
        );
    }

    #[test]
    fn decimal_boundaries_and_price_ticks_use_exact_scaled_integers() {
        let debit = NetOrderPrice::net_debit(Price::parse_json_number("2.73").unwrap()).unwrap();
        let credit = NetOrderPrice::net_credit(Price::parse_json_number("3.25").unwrap()).unwrap();
        assert!(debit.amount().decimal() < credit.amount().decimal());
        assert_eq!(
            debit.validate_tick(
                PriceTick::new(ExactDecimal::parse_json_number("0.01").unwrap()).unwrap(),
            ),
            Ok(())
        );
        assert_eq!(
            credit.validate_tick(
                PriceTick::new(ExactDecimal::parse_json_number("0.05").unwrap()).unwrap(),
            ),
            Ok(())
        );
        assert_eq!(
            debit.validate_tick(
                PriceTick::new(ExactDecimal::parse_json_number("0.05").unwrap()).unwrap(),
            ),
            Err(PriceTickError::OffIncrement)
        );
        assert_eq!(
            PriceTick::new(ExactDecimal::ZERO),
            Err(PriceTickError::NonPositive)
        );
    }

    #[test]
    fn combo_construction_rejects_missing_legs_and_ratio_quantity_mismatch() {
        let base = || {
            (
                IntentId::new("intent-combo-errors").unwrap(),
                LogicalOrderId::new("logical-combo-errors").unwrap(),
                route(),
                Quantity::new(1).unwrap(),
                NetOrderPrice::ZeroCost,
            )
        };
        let (intent_id, logical_order_id, route, quantity, price) = base();
        assert_eq!(
            OptionComboIntent::new(
                intent_id,
                logical_order_id,
                route,
                quantity,
                vec![leg("600", OrderSide::Buy, PositionEffect::Open, 1, 1)],
                price,
            ),
            Err(ComboOrderError::TooFewLegs)
        );

        let (intent_id, logical_order_id, route, quantity, price) = base();
        assert_eq!(
            OptionComboIntent::new(
                intent_id,
                logical_order_id,
                route,
                quantity,
                vec![
                    leg("600", OrderSide::Buy, PositionEffect::Open, 1, 1),
                    leg("605", OrderSide::Sell, PositionEffect::Close, 2, 1),
                ],
                price,
            ),
            Err(ComboOrderError::LegQuantityRatioMismatch)
        );
    }

    #[test]
    fn combo_ratio_quantity_overflow_is_rejected() {
        let ratio = LegRatio::new(u64::MAX).unwrap();
        let too_large = OptionOrderLeg::new(
            instrument("605", 100, "100"),
            OrderSide::Sell,
            PositionEffect::Open,
            ratio,
            Quantity::new(1).unwrap(),
        );
        assert_eq!(
            OptionComboIntent::new(
                IntentId::new("intent-combo-overflow").unwrap(),
                LogicalOrderId::new("logical-combo-overflow").unwrap(),
                route(),
                Quantity::new(2).unwrap(),
                vec![
                    leg("600", OrderSide::Buy, PositionEffect::Open, 1, 2),
                    too_large,
                ],
                NetOrderPrice::ZeroCost,
            ),
            Err(ComboOrderError::QuantityOverflow)
        );

        let above_safe_integer = OptionOrderLeg::new(
            instrument("605", 100, "100"),
            OrderSide::Sell,
            PositionEffect::Open,
            LegRatio::new(2).unwrap(),
            Quantity::new(2).unwrap(),
        );
        assert_eq!(
            OptionComboIntent::new(
                IntentId::new("intent-combo-safe-boundary").unwrap(),
                LogicalOrderId::new("logical-combo-safe-boundary").unwrap(),
                route(),
                Quantity::new(crate::MAX_SAFE_INTEGER).unwrap(),
                vec![
                    OptionOrderLeg::new(
                        instrument("600", 100, "100"),
                        OrderSide::Buy,
                        PositionEffect::Open,
                        LegRatio::new(1).unwrap(),
                        Quantity::new(crate::MAX_SAFE_INTEGER).unwrap(),
                    ),
                    above_safe_integer,
                ],
                NetOrderPrice::ZeroCost,
            ),
            Err(ComboOrderError::QuantityOverflow)
        );
    }
}
