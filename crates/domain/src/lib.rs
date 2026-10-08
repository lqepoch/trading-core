#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

//! Strongly typed, broker-independent trading values and invariants.
//!
//! This crate contains executable domain capabilities, not runtime or broker
//! integration. Provider IDs, fixed routes, exact combo prices, and qualified
//! contract keys are domain values; wire serialization and broker transport
//! belong in adapters.
//!
//! # 简体中文
//!
//! 本 crate 提供强类型、与券商无关的交易值和不变量。
//!
//! 本 crate 实现可执行的领域能力，不包含 runtime 或券商集成。行情来源 ID、固定执行路由、精确组合净价和完整合约键属于领域值；wire 序列化与券商传输由适配器负责。

mod automation;
mod combo_order;
mod ibkr_paper_account;
mod identifiers;
mod instrument;
mod option;
mod order_identity;
mod provider;
mod quantity;
mod routing;
mod strike;
mod vertical;

pub use automation::{AutomationCapabilities, AutomationCapability, OperationClass};
pub use combo_order::{
    ComboOrderError, NetOrderPrice, NonZeroPrice, NonZeroPriceError, OptionComboIntent,
    OptionOrderLeg, OrderSide, PositionEffect, PriceTick, PriceTickError,
};
pub use exact_decimal::{
    DecimalError, ExactDecimal, MAX_DECIMAL_EXPONENT, MAX_DECIMAL_INPUT_BYTES, MAX_DECIMAL_SCALE,
};
pub use ibkr_paper_account::{
    IBKR_PAPER_ACCOUNT_ID_MAX_BYTES, IBKR_PAPER_ACCOUNT_ID_MIN_BYTES, is_ibkr_paper_account_id,
};
pub use identifiers::{
    AccountRouteHash, AccountScope, ActorId, IdentifierError, IntentId, LogicalOrderId, OrderId,
    Revision, SpreadKey, StrategyInstanceId,
};
pub use instrument::{
    ContractTerm, InstrumentField, InstrumentKey, InstrumentQualificationError,
    OptionExerciseStyle, OptionInstrumentCandidate, OptionInstrumentKey, OptionSettlementType,
    TradingClass,
};
pub use option::{
    ContractCurrency, Deliverable, DeliverableAsset, DeliverableComponent, DeliverableError,
    ExpirationDate, OptionContract, OptionRight, OptionSymbol, OptionSymbolError, Underlying,
};
pub use order_identity::{
    AlpacaNativeOrderId, BrokerNativeOrderReference, IbkrClientId, IbkrNativeOrderReference,
    IbkrOrderId, IbkrOrderRef, IbkrPermanentOrderId, IbkrSessionId, OrderIdentityError,
    OtherNativeOrderReference, ProviderOrderEvidence, ProviderOrderIdentity, RoutedOrderIdentity,
    SchwabNativeOrderId, SchwabNativeOrderReference, UnresolvedProviderOrderReference,
};
pub use provider::{
    BrokerEnvironment, ExecutionBrokerId, MarketDataProviderId, MetadataSource, ProviderCode,
    ProviderMetadataKind, ProviderMetadataRef, ProviderRecordId, ProviderUnavailableReason,
    ProviderValue,
};
pub use quantity::{
    ContractMultiplier, InventoryQuantity, MAX_SAFE_INTEGER, Quantity, QuantityError,
};
pub use routing::{AccountNamespace, ExecutionRoute};
pub use strike::{MAX_OCC_STRIKE_MILLS, MAX_OCC_STRIKE_TEXT, Strike, StrikeError};
pub use vertical::{
    LegRatio, PositionSide, ValidatedVertical, VerticalError, VerticalLeg, VerticalOrientation,
};

/// A non-negative quote or order price. Cash flows and derived spreads use [`Money`].
/// 非负的报价或订单价格；现金流和派生价差使用 [`Money`]。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Price(ExactDecimal);

impl Price {
    /// Exact zero price, useful for explicitly priced zero-net orders.
    /// 精确的零价格，可用于显式定价为零净额的订单。
    pub const ZERO: Self = Self(ExactDecimal::ZERO);
}

/// A signed monetary amount, including negative synthetic spread values.
/// 有符号货币金额，包括为合成价差表示的负数。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Money(ExactDecimal);

/// An invalid value supplied where a non-negative price is required.
/// 需要非负价格时传入了无效值。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PriceError;

/// Parsing error for a non-negative price.
/// 解析非负价格时发生的错误。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PriceParseError {
    /// The decimal token is invalid or cannot be represented exactly.
    /// 十进制 token 无效或无法精确表示。
    Decimal(DecimalError),
    /// The parsed amount is negative.
    /// 解析得到的金额为负数。
    Negative,
}

impl Price {
    /// Creates a price when the exact decimal is non-negative.
    /// 仅当精确十进制值非负时创建价格。
    pub fn new(value: ExactDecimal) -> Result<Self, PriceError> {
        if value.is_negative() {
            Err(PriceError)
        } else {
            Ok(Self(value))
        }
    }

    /// Parses a strict JSON number and rejects negative values.
    /// 解析严格 JSON 数字，并拒绝负数。
    pub fn parse_json_number(value: &str) -> Result<Self, PriceParseError> {
        Self::new(ExactDecimal::parse_json_number(value)?).map_err(|_| PriceParseError::Negative)
    }

    /// Returns the exact decimal representation.
    /// 返回精确的十进制表示。
    pub const fn decimal(self) -> ExactDecimal {
        self.0
    }

    /// Adds two non-negative prices without rounding.
    /// 对两个非负价格执行不舍入的加法。
    pub fn checked_add(self, other: Self) -> Result<Self, DecimalError> {
        Ok(Self(self.0.checked_add(other.0)?))
    }

    /// Subtracts another price and preserves a negative result as [`Money`].
    /// 减去另一个价格，并将负结果保留为 [`Money`]。
    pub fn checked_difference(self, other: Self) -> Result<Money, DecimalError> {
        Ok(Money(self.0.checked_sub(other.0)?))
    }

    /// Returns the exact midpoint of two prices without rounding.
    /// 返回两个价格之间不舍入的精确中点。
    pub fn checked_midpoint(self, other: Self) -> Result<Self, DecimalError> {
        Ok(Self(self.0.checked_midpoint(other.0)?))
    }
}

impl Money {
    /// Wraps an exact signed monetary amount.
    /// 包装一个精确的有符号货币金额。
    pub const fn new(value: ExactDecimal) -> Self {
        Self(value)
    }

    /// Parses a strict JSON number as a signed monetary amount.
    /// 将严格 JSON 数字解析为有符号货币金额。
    pub fn parse_json_number(value: &str) -> Result<Self, DecimalError> {
        Ok(Self(ExactDecimal::parse_json_number(value)?))
    }

    /// Returns the exact decimal representation.
    /// 返回精确的十进制表示。
    pub const fn decimal(self) -> ExactDecimal {
        self.0
    }

    /// Adds two amounts without rounding.
    /// 对两个金额执行不舍入的加法。
    pub fn checked_add(self, other: Self) -> Result<Self, DecimalError> {
        Ok(Self(self.0.checked_add(other.0)?))
    }

    /// Subtracts another amount without rounding.
    /// 不舍入地减去另一个金额。
    pub fn checked_sub(self, other: Self) -> Result<Self, DecimalError> {
        Ok(Self(self.0.checked_sub(other.0)?))
    }

    /// Multiplies by an integer without rounding.
    /// 不舍入地乘以整数。
    pub fn checked_mul_integer(self, multiplier: i128) -> Result<Self, DecimalError> {
        Ok(Self(self.0.checked_mul_integer(multiplier)?))
    }

    /// Returns the exact midpoint of two amounts without rounding.
    /// 返回两个金额之间不舍入的精确中点。
    pub fn checked_midpoint(self, other: Self) -> Result<Self, DecimalError> {
        Ok(Self(self.0.checked_midpoint(other.0)?))
    }
}

/// Parsing error for [`Price`] that keeps the decimal parser's exact error.
/// [`Price`] 的解析错误，并保留十进制解析器给出的精确错误。
impl From<DecimalError> for PriceParseError {
    fn from(error: DecimalError) -> Self {
        Self::Decimal(error)
    }
}

#[cfg(test)]
mod tests {
    use super::{ExactDecimal, Money, Price};

    #[test]
    fn price_is_nonnegative_while_money_preserves_signed_spread_values() {
        assert!(Price::parse_json_number("-0.01").is_err());
        assert_eq!(
            Money::parse_json_number("-0.01")
                .unwrap()
                .decimal()
                .to_string(),
            "-0.01"
        );
        assert_eq!(
            Price::parse_json_number("0.93")
                .unwrap()
                .decimal()
                .to_string(),
            "0.93"
        );
    }

    #[test]
    fn exact_price_midpoint_can_represent_half_cents_without_rounding() {
        let low = Price::parse_json_number("0.01").unwrap();
        let high = Price::parse_json_number("0.02").unwrap();
        assert_eq!(
            low.checked_midpoint(high).unwrap().decimal().to_string(),
            "0.015"
        );

        let signed = Money::parse_json_number("-0.01").unwrap();
        let positive = Money::parse_json_number("0.02").unwrap();
        assert_eq!(
            signed
                .checked_midpoint(positive)
                .unwrap()
                .decimal()
                .to_string(),
            "0.005"
        );
    }

    #[test]
    fn price_difference_returns_signed_money() {
        let bid = Price::parse_json_number("0.02").unwrap();
        let ask = Price::parse_json_number("0.05").unwrap();
        assert_eq!(
            bid.checked_difference(ask).unwrap().decimal(),
            ExactDecimal::parse_json_number("-0.03").unwrap()
        );
    }
}
