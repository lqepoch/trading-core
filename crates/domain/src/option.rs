//! Validated option, expiration, currency, and deliverable identities.
//!
//! These types establish the contract terms represented by an option symbol;
//! they do not prove broker inventory, quote freshness, or order authority.
//!
//! # 简体中文
//!
//! 提供经过校验的期权、到期日、货币和交割物身份类型。
//!
//! 这些类型用于确立期权标识所表示的合约条款；它们不证明券商库存、报价新鲜度或订单权限。

use crate::{DecimalError, ExactDecimal, Strike, StrikeError, quantity::ContractMultiplier};
use std::fmt;

const OCC_SYMBOL_BYTES: usize = 21;
const MAX_DELIVERABLE_COMPONENTS: usize = 16;

/// Canonical ASCII option root, between one and six characters.
/// 一至六个字符的规范 ASCII 期权根代码。
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Underlying(String);

/// Option right encoded by OCC as `C` or `P`.
/// OCC 使用 `C` 或 `P` 编码的期权方向。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum OptionRight {
    /// Call option right.
    /// 看涨期权方向。
    Call,
    /// Put option right.
    /// 看跌期权方向。
    Put,
}

/// Calendar date supported by OCC's two-digit year encoding (2000–2099).
/// OCC 两位年份编码支持的日历日期（2000–2099）。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ExpirationDate {
    year: u16,
    month: u8,
    day: u8,
}

/// Three-letter uppercase currency code.
/// 三个大写字母组成的货币代码。
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ContractCurrency(String);

/// Supported economic asset identities in an option deliverable basket.
/// 期权交割物篮子中支持的经济资产身份。
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum DeliverableAsset {
    /// Equity shares identified by their underlying symbol.
    /// 由标的代码标识的股票。
    Equity(Underlying),
    /// Cash identified by its currency code.
    /// 由货币代码标识的现金。
    Cash(ContractCurrency),
}

/// One asset and exact positive amount in a contract's deliverable basket.
/// 合约交割物篮子中的一种资产及其精确正数量。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct DeliverableComponent {
    asset: DeliverableAsset,
    quantity: ExactDecimal,
}

/// Sorted, unambiguous deliverable basket used to compare option contracts.
/// 用于比较期权合约的已排序且无歧义的交割物篮子。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Deliverable(Vec<DeliverableComponent>);

/// OCC symbol identity without pricing or broker transport behavior.
/// OCC 期权标识身份，不包含定价或券商传输行为。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct OptionSymbol {
    underlying: Underlying,
    expiration: ExpirationDate,
    right: OptionRight,
    strike: Strike,
}

/// A symbol identity paired with terms that must match across a vertical.
/// 与 vertical 中必须匹配的合约条款配对的期权标识。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct OptionContract {
    symbol: OptionSymbol,
    multiplier: ContractMultiplier,
    currency: ContractCurrency,
    deliverable: Deliverable,
}

/// OCC symbol validation failure.
/// OCC 期权标识校验失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OptionSymbolError {
    /// The symbol does not have the required 21-byte ASCII representation.
    /// 标识不符合 21 字节 ASCII 表示要求。
    InvalidLength,
    /// The option root is empty, malformed, or not canonical.
    /// 期权根代码为空、格式错误或不规范。
    InvalidRoot,
    /// The expiration date is invalid or outside the supported year range.
    /// 到期日无效或超出受支持年份范围。
    InvalidExpiration,
    /// The option right is not encoded as `C` or `P`.
    /// 期权方向未编码为 `C` 或 `P`。
    InvalidRight,
    /// The strike text or value is invalid.
    /// 行权价文本或数值无效。
    InvalidStrike,
    /// The strike validator rejected the decoded value.
    /// 行权价校验器拒绝了解码后的值。
    Strike(StrikeError),
}

/// Currency or deliverable validation failure.
/// 货币或交割物校验失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliverableError {
    /// The currency is not exactly three uppercase ASCII letters.
    /// 货币代码不是恰好三个大写 ASCII 字母。
    InvalidCurrency,
    /// The deliverable basket contains no components.
    /// 交割物篮子不包含任何组件。
    EmptyBasket,
    /// The deliverable basket exceeds its component bound.
    /// 交割物篮子超过组件数量上限。
    TooManyComponents,
    /// An asset component has a zero or negative amount.
    /// 资产组件的数量为零或负数。
    NonPositiveQuantity,
    /// The basket contains the same asset identity more than once.
    /// 篮子中重复包含相同资产身份。
    DuplicateAsset,
    /// Exact decimal validation failed.
    /// 精确十进制校验失败。
    Decimal(DecimalError),
}

impl Underlying {
    /// Trims and uppercases an ASCII root containing one to six allowed characters.
    /// 去除空白并转为大写，要求 ASCII 根代码包含一至六个允许字符。
    pub fn new(value: &str) -> Result<Self, OptionSymbolError> {
        let normalized = value.trim().to_ascii_uppercase();
        if normalized.is_empty()
            || normalized.len() > 6
            || !normalized.is_ascii()
            || !normalized
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
        {
            return Err(OptionSymbolError::InvalidRoot);
        }
        Ok(Self(normalized))
    }

    /// Returns the canonical root text.
    /// 返回规范根代码文本。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Underlying {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl OptionRight {
    /// Returns the OCC `C` or `P` code.
    /// 返回 OCC `C` 或 `P` 编码。
    pub const fn occ_code(self) -> char {
        match self {
            Self::Call => 'C',
            Self::Put => 'P',
        }
    }

    /// Parses an uppercase OCC right code.
    /// 解析大写 OCC 期权方向编码。
    pub fn from_occ_code(value: u8) -> Result<Self, OptionSymbolError> {
        match value {
            b'C' => Ok(Self::Call),
            b'P' => Ok(Self::Put),
            _ => Err(OptionSymbolError::InvalidRight),
        }
    }
}

impl ExpirationDate {
    /// Creates a valid calendar date from 2000 through 2099.
    /// 创建 2000 至 2099 年范围内的有效日历日期。
    pub fn new(year: u16, month: u8, day: u8) -> Result<Self, OptionSymbolError> {
        if !(2000..=2099).contains(&year)
            || !(1..=12).contains(&month)
            || day == 0
            || day > days_in_month(year, month)
        {
            return Err(OptionSymbolError::InvalidExpiration);
        }
        Ok(Self { year, month, day })
    }

    /// Parses the exact `YYYY-MM-DD` date form.
    /// 解析精确的 `YYYY-MM-DD` 日期格式。
    pub fn parse_iso(value: &str) -> Result<Self, OptionSymbolError> {
        let bytes = value.as_bytes();
        if bytes.len() != 10
            || bytes[4] != b'-'
            || bytes[7] != b'-'
            || !bytes[..4].iter().all(u8::is_ascii_digit)
            || !bytes[5..7].iter().all(u8::is_ascii_digit)
            || !bytes[8..10].iter().all(u8::is_ascii_digit)
        {
            return Err(OptionSymbolError::InvalidExpiration);
        }
        let year = parse_two_or_four_digits(&bytes[..4])? as u16;
        let month = parse_two_or_four_digits(&bytes[5..7])? as u8;
        let day = parse_two_or_four_digits(&bytes[8..10])? as u8;
        Self::new(year, month, day)
    }

    /// Parses the six-digit OCC `YYMMDD` expiration form as a year in 2000–2099.
    /// 将六位 OCC `YYMMDD` 到期日解析为 2000–2099 年份。
    pub fn parse_occ_compact(value: &[u8]) -> Result<Self, OptionSymbolError> {
        if value.len() != 6 || !value.iter().all(u8::is_ascii_digit) {
            return Err(OptionSymbolError::InvalidExpiration);
        }
        let year = 2000 + parse_two_or_four_digits(&value[..2])? as u16;
        let month = parse_two_or_four_digits(&value[2..4])? as u8;
        let day = parse_two_or_four_digits(&value[4..6])? as u8;
        Self::new(year, month, day)
    }

    /// Returns the four-digit year.
    /// 返回四位数年份。
    pub const fn year(self) -> u16 {
        self.year
    }

    /// Returns the one-based month.
    /// 返回从一开始计数的月份。
    pub const fn month(self) -> u8 {
        self.month
    }

    /// Returns the one-based day of month.
    /// 返回从一开始计数的月份日期。
    pub const fn day(self) -> u8 {
        self.day
    }

    /// Formats the OCC six-digit `YYMMDD` date form.
    /// 格式化为 OCC 六位 `YYMMDD` 日期形式。
    pub fn occ_compact(self) -> String {
        format!("{:02}{:02}{:02}", self.year % 100, self.month, self.day)
    }
}

impl fmt::Display for ExpirationDate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:04}-{:02}-{:02}",
            self.year, self.month, self.day
        )
    }
}

impl ContractCurrency {
    /// Trims and normalizes a three-letter ASCII currency code to uppercase.
    /// 去除空白并将三个 ASCII 字母组成的货币代码规范化为大写。
    pub fn new(value: &str) -> Result<Self, DeliverableError> {
        let normalized = value.trim().to_ascii_uppercase();
        if normalized.len() != 3 || !normalized.bytes().all(|byte| byte.is_ascii_uppercase()) {
            return Err(DeliverableError::InvalidCurrency);
        }
        Ok(Self(normalized))
    }

    /// Returns the canonical three-letter currency code.
    /// 返回规范的三字母货币代码。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ContractCurrency {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl DeliverableComponent {
    /// Creates a positive equity-share deliverable component.
    /// 创建数量为正的股票交割物组件。
    pub fn equity(symbol: Underlying, quantity: ExactDecimal) -> Result<Self, DeliverableError> {
        Self::with_asset(DeliverableAsset::Equity(symbol), quantity)
    }

    /// Creates a positive cash deliverable component.
    /// 创建数量为正的现金交割物组件。
    pub fn cash(
        currency: ContractCurrency,
        quantity: ExactDecimal,
    ) -> Result<Self, DeliverableError> {
        Self::with_asset(DeliverableAsset::Cash(currency), quantity)
    }

    fn with_asset(
        asset: DeliverableAsset,
        quantity: ExactDecimal,
    ) -> Result<Self, DeliverableError> {
        if quantity <= ExactDecimal::ZERO {
            return Err(DeliverableError::NonPositiveQuantity);
        }
        Ok(Self { asset, quantity })
    }

    /// Returns the asset identity for this component.
    /// 返回此组件的资产身份。
    pub fn asset(&self) -> &DeliverableAsset {
        &self.asset
    }

    /// Returns the exact positive component amount.
    /// 返回组件的精确正数量。
    pub const fn quantity(&self) -> ExactDecimal {
        self.quantity
    }
}

impl Deliverable {
    /// Sorts a non-empty basket and rejects repeated asset identities or excess components.
    /// 对非空篮子排序，并拒绝重复资产身份或超出组件上限的输入。
    pub fn new(mut components: Vec<DeliverableComponent>) -> Result<Self, DeliverableError> {
        if components.is_empty() {
            return Err(DeliverableError::EmptyBasket);
        }
        if components.len() > MAX_DELIVERABLE_COMPONENTS {
            return Err(DeliverableError::TooManyComponents);
        }
        components.sort_by(|left, right| left.asset.cmp(&right.asset));
        if components
            .windows(2)
            .any(|pair| pair[0].asset == pair[1].asset)
        {
            return Err(DeliverableError::DuplicateAsset);
        }
        Ok(Self(components))
    }

    /// Borrows the canonical sorted component list.
    /// 借用规范且已排序的组件列表。
    pub fn components(&self) -> &[DeliverableComponent] {
        &self.0
    }
}

impl OptionSymbol {
    /// Constructs a symbol identity from individually validated terms.
    /// 根据分别通过校验的条款构造期权标识身份。
    pub fn new(
        underlying: Underlying,
        expiration: ExpirationDate,
        right: OptionRight,
        strike: Strike,
    ) -> Self {
        Self {
            underlying,
            expiration,
            right,
            strike,
        }
    }

    /// Parses the fixed-width 21-byte ASCII OCC option symbol form.
    /// 解析固定宽度的 21 字节 ASCII OCC 期权标识格式。
    pub fn parse(value: &str) -> Result<Self, OptionSymbolError> {
        let bytes = value.as_bytes();
        if bytes.len() != OCC_SYMBOL_BYTES || !bytes.is_ascii() {
            return Err(OptionSymbolError::InvalidLength);
        }

        let root = &bytes[..6];
        let root_len = root
            .iter()
            .position(|byte| *byte == b' ')
            .unwrap_or(root.len());
        if root_len == 0
            || root[root_len..].iter().any(|byte| *byte != b' ')
            || !root[..root_len].iter().all(|byte| {
                byte.is_ascii_uppercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
            })
        {
            return Err(OptionSymbolError::InvalidRoot);
        }
        let root =
            std::str::from_utf8(&root[..root_len]).map_err(|_| OptionSymbolError::InvalidRoot)?;
        let underlying = Underlying(root.to_owned());
        let expiration = ExpirationDate::parse_occ_compact(&bytes[6..12])?;
        let right = OptionRight::from_occ_code(bytes[12])?;
        if !bytes[13..].iter().all(u8::is_ascii_digit) {
            return Err(OptionSymbolError::InvalidStrike);
        }
        let encoded_strike = std::str::from_utf8(&bytes[13..])
            .map_err(|_| OptionSymbolError::InvalidStrike)?
            .parse::<u32>()
            .map_err(|_| OptionSymbolError::InvalidStrike)?;
        let strike = Strike::from_mills(encoded_strike).map_err(OptionSymbolError::Strike)?;
        Ok(Self::new(underlying, expiration, right, strike))
    }

    /// Formats this identity in the canonical fixed-width OCC representation.
    /// 将此身份格式化为规范的固定宽度 OCC 表示。
    pub fn format(&self) -> String {
        format!(
            "{:<6}{}{}{:08}",
            self.underlying.as_str(),
            self.expiration.occ_compact(),
            self.right.occ_code(),
            self.strike.mills(),
        )
    }

    /// Borrows the underlying symbol.
    /// 借用标的代码。
    pub fn underlying(&self) -> &Underlying {
        &self.underlying
    }

    /// Returns the expiration date.
    /// 返回到期日。
    pub const fn expiration(&self) -> ExpirationDate {
        self.expiration
    }

    /// Returns the call or put right.
    /// 返回看涨或看跌方向。
    pub const fn right(&self) -> OptionRight {
        self.right
    }

    /// Returns the exact strike.
    /// 返回精确行权价。
    pub const fn strike(&self) -> Strike {
        self.strike
    }
}

impl OptionContract {
    /// Constructs contract terms for an already validated option symbol.
    /// 为已校验的期权标识构造合约条款。
    pub fn new(
        symbol: OptionSymbol,
        multiplier: ContractMultiplier,
        currency: ContractCurrency,
        deliverable: Deliverable,
    ) -> Self {
        Self {
            symbol,
            multiplier,
            currency,
            deliverable,
        }
    }

    /// Borrows the option symbol identity.
    /// 借用期权标识身份。
    pub const fn symbol(&self) -> &OptionSymbol {
        &self.symbol
    }

    /// Returns the contract multiplier.
    /// 返回合约乘数。
    pub const fn multiplier(&self) -> ContractMultiplier {
        self.multiplier
    }

    /// Borrows the contract currency.
    /// 借用合约货币。
    pub fn currency(&self) -> &ContractCurrency {
        &self.currency
    }

    /// Borrows the deliverable basket.
    /// 借用交割物篮子。
    pub fn deliverable(&self) -> &Deliverable {
        &self.deliverable
    }
}

impl fmt::Display for OptionSymbolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLength => formatter.write_str("OCC_SYMBOL_INVALID_LENGTH"),
            Self::InvalidRoot => formatter.write_str("OCC_ROOT_INVALID"),
            Self::InvalidExpiration => formatter.write_str("OCC_EXPIRATION_INVALID"),
            Self::InvalidRight => formatter.write_str("OCC_RIGHT_INVALID"),
            Self::InvalidStrike => formatter.write_str("OCC_STRIKE_INVALID"),
            Self::Strike(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for OptionSymbolError {}

impl fmt::Display for DeliverableError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidCurrency => "CONTRACT_CURRENCY_INVALID",
            Self::EmptyBasket => "DELIVERABLE_EMPTY",
            Self::TooManyComponents => "DELIVERABLE_TOO_MANY_COMPONENTS",
            Self::NonPositiveQuantity => "DELIVERABLE_QUANTITY_MUST_BE_POSITIVE",
            Self::DuplicateAsset => "DELIVERABLE_ASSET_DUPLICATE",
            Self::Decimal(error) => return fmt::Display::fmt(error, formatter),
        })
    }
}

impl std::error::Error for DeliverableError {}

fn parse_two_or_four_digits(value: &[u8]) -> Result<u32, OptionSymbolError> {
    std::str::from_utf8(value)
        .map_err(|_| OptionSymbolError::InvalidExpiration)?
        .parse::<u32>()
        .map_err(|_| OptionSymbolError::InvalidExpiration)
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: u16) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

#[cfg(test)]
mod tests {
    use super::{
        ContractCurrency, Deliverable, DeliverableComponent, DeliverableError, ExpirationDate,
        OptionRight, OptionSymbol, OptionSymbolError, Underlying,
    };
    use crate::{ContractMultiplier, ExactDecimal, Strike};

    #[test]
    fn occ_roundtrip_preserves_mill_strikes_and_canonical_root() {
        let symbol = OptionSymbol::parse("QQQ   260814P00740125").unwrap();
        assert_eq!(symbol.underlying().as_str(), "QQQ");
        assert_eq!(symbol.expiration().to_string(), "2026-08-14");
        assert_eq!(symbol.right(), OptionRight::Put);
        assert_eq!(symbol.strike().to_string(), "740.125");
        assert_eq!(symbol.format(), "QQQ   260814P00740125");

        assert_eq!(
            OptionSymbol::parse("qqq   260814P00740125"),
            Err(OptionSymbolError::InvalidRoot)
        );
        let canonical = OptionSymbol::new(
            Underlying::new(" qqq ").unwrap(),
            ExpirationDate::parse_iso("2026-08-14").unwrap(),
            OptionRight::Call,
            Strike::parse_json_number("740.125").unwrap(),
        );
        assert_eq!(canonical.format(), "QQQ   260814C00740125");
    }

    #[test]
    fn occ_parser_enforces_root_shape_ascii_and_fixed_width() {
        for symbol in [
            "QQQ/X260814P00740125",
            "QQ Q  260814P00740125",
            "QQQé 260814P00740125",
            " qqq  260814P00740125",
            "QQQ\t  260814P00740125",
            "TOOLONG260814P00740125",
            "QQQ   260814P0074012",
        ] {
            assert!(OptionSymbol::parse(symbol).is_err(), "{symbol:?}");
        }
        assert_eq!(Underlying::new("QQQ/"), Err(OptionSymbolError::InvalidRoot));
        assert_eq!(Underlying::new("é"), Err(OptionSymbolError::InvalidRoot));
        assert_eq!(Underlying::new(" qqq ").unwrap().as_str(), "QQQ");
    }

    #[test]
    fn occ_dates_validate_calendar_and_the_two_digit_year_window() {
        assert_eq!(
            ExpirationDate::parse_occ_compact(b"000229")
                .unwrap()
                .to_string(),
            "2000-02-29"
        );
        assert_eq!(
            ExpirationDate::parse_occ_compact(b"990228")
                .unwrap()
                .to_string(),
            "2099-02-28"
        );
        assert_eq!(
            ExpirationDate::parse_occ_compact(b"260231"),
            Err(OptionSymbolError::InvalidExpiration)
        );
        assert_eq!(
            ExpirationDate::parse_iso("1999-12-31"),
            Err(OptionSymbolError::InvalidExpiration)
        );
        assert_eq!(
            ExpirationDate::parse_iso("2100-01-01"),
            Err(OptionSymbolError::InvalidExpiration)
        );
        assert_eq!(
            ExpirationDate::parse_iso("2026-02-29"),
            Err(OptionSymbolError::InvalidExpiration)
        );
    }

    #[test]
    fn occ_strike_field_enforces_positive_mill_range() {
        assert_eq!(
            OptionSymbol::parse("QQQ   260814P00000000"),
            Err(OptionSymbolError::Strike(crate::StrikeError::Zero))
        );
        assert_eq!(
            OptionSymbol::parse("QQQ   260814P99999999")
                .unwrap()
                .strike()
                .mills(),
            99_999_999
        );
        assert!(OptionSymbol::parse("QQQ   260814P99999999").is_ok());
    }

    #[test]
    fn contract_currency_and_deliverable_are_typed_and_canonical() {
        let usd = ContractCurrency::new("usd").unwrap();
        assert_eq!(usd.as_str(), "USD");
        assert_eq!(
            ContractCurrency::new("US"),
            Err(DeliverableError::InvalidCurrency)
        );

        let stock = DeliverableComponent::equity(
            Underlying::new("qqq").unwrap(),
            ExactDecimal::from_integer(100),
        )
        .unwrap();
        let cash = DeliverableComponent::cash(
            usd.clone(),
            ExactDecimal::parse_json_number("0.25").unwrap(),
        )
        .unwrap();
        let equity_named_usd = DeliverableComponent::equity(
            Underlying::new("USD").unwrap(),
            ExactDecimal::from_integer(100),
        )
        .unwrap();
        let cash_named_usd =
            DeliverableComponent::cash(usd.clone(), ExactDecimal::from_integer(100)).unwrap();
        assert_ne!(equity_named_usd, cash_named_usd);
        let deliverable = Deliverable::new(vec![cash.clone(), stock.clone()]).unwrap();
        assert!(matches!(
            deliverable.components()[0].asset(),
            super::DeliverableAsset::Equity(_)
        ));
        assert!(matches!(
            deliverable.components()[1].asset(),
            super::DeliverableAsset::Cash(_)
        ));
        assert_eq!(
            Deliverable::new(Vec::new()),
            Err(DeliverableError::EmptyBasket)
        );
        assert_eq!(
            Deliverable::new(vec![stock.clone(), stock]),
            Err(DeliverableError::DuplicateAsset)
        );
        assert_eq!(
            DeliverableComponent::equity(Underlying::new("QQQ").unwrap(), ExactDecimal::ZERO),
            Err(DeliverableError::NonPositiveQuantity)
        );

        let symbol = OptionSymbol::parse("QQQ   260814P00740000").unwrap();
        let contract = super::OptionContract::new(
            symbol,
            ContractMultiplier::new(100).unwrap(),
            usd,
            deliverable,
        );
        assert_eq!(contract.multiplier().get(), 100);
        assert_eq!(contract.currency().as_str(), "USD");
    }
}
