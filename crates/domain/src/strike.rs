//! Exact positive option strikes in the OCC eight-digit thousandths format.
//!
//! Conversion preserves thousandth precision without floating-point rounding.
//!
//! # 简体中文
//!
//! 使用 OCC 八位千分之一格式表示精确正数期权行权价。
//!
//! 转换会保留千分之一精度，不使用浮点舍入。

use crate::{DecimalError, ExactDecimal};
use std::fmt;

/// Largest strike encoded by the eight-digit OCC strike field at 0.001 precision.
/// OCC 八位行权价字段以 0.001 精度编码时可表示的最大行权价。
pub const MAX_OCC_STRIKE_MILLS: u32 = 99_999_999;
/// Human-readable maximum represented by [`MAX_OCC_STRIKE_MILLS`].
/// [`MAX_OCC_STRIKE_MILLS`] 对应的可读最大值。
pub const MAX_OCC_STRIKE_TEXT: &str = "99999.999";

/// An OCC option strike in exact thousandths of a dollar.
/// 以美元千分之一精确表示的 OCC 期权行权价。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Strike(u32);

/// Failure to represent a value as a valid OCC strike.
/// 无法将数值表示为有效 OCC 行权价。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StrikeError {
    /// The exact decimal parser rejected the input or value.
    /// 精确十进制解析器拒绝了输入或数值。
    Decimal(DecimalError),
    /// OCC strikes must be greater than zero.
    /// OCC 行权价必须大于零。
    Zero,
    /// The value is negative.
    /// 数值为负数。
    Negative,
    /// The value has more than three decimal places.
    /// 数值的小数位数超过三位。
    OverPrecision,
    /// The value exceeds the OCC strike field maximum.
    /// 数值超过 OCC 行权价字段上限。
    ExceedsOccMaximum,
}

impl Strike {
    /// Creates a positive strike from integer thousandths within the OCC bound.
    /// 根据 OCC 上限内的整数千分之一创建正数行权价。
    pub fn from_mills(value: u32) -> Result<Self, StrikeError> {
        if value == 0 {
            return Err(StrikeError::Zero);
        }
        if value > MAX_OCC_STRIKE_MILLS {
            return Err(StrikeError::ExceedsOccMaximum);
        }
        Ok(Self(value))
    }

    /// Converts a positive decimal exactly, rejecting precision beyond thousandths.
    /// 精确转换正数十进制值，并拒绝超过千分之一精度的输入。
    pub fn from_decimal(value: ExactDecimal) -> Result<Self, StrikeError> {
        if value.is_negative() {
            return Err(StrikeError::Negative);
        }
        if value.is_zero() {
            return Err(StrikeError::Zero);
        }
        if value.scale() > 3 {
            return Err(StrikeError::OverPrecision);
        }
        let mills = value.to_scaled_integer(3).map_err(|error| match error {
            DecimalError::Overflow => StrikeError::ExceedsOccMaximum,
            DecimalError::InexactDivision => StrikeError::OverPrecision,
            other => StrikeError::Decimal(other),
        })?;
        let mills = u32::try_from(mills).map_err(|_| StrikeError::ExceedsOccMaximum)?;
        Self::from_mills(mills)
    }

    /// Parses a strict JSON number as a positive OCC strike.
    /// 将严格 JSON 数字解析为正数 OCC 行权价。
    pub fn parse_json_number(value: &str) -> Result<Self, StrikeError> {
        Self::from_decimal(ExactDecimal::parse_json_number(value).map_err(StrikeError::Decimal)?)
    }

    /// Returns the strike in integer thousandths of a dollar.
    /// 返回以美元千分之一为单位的行权价整数值。
    pub const fn mills(self) -> u32 {
        self.0
    }

    /// Returns the exact decimal strike value.
    /// 返回精确十进制行权价值。
    pub fn decimal(self) -> ExactDecimal {
        ExactDecimal::from_parts(i128::from(self.0), 3)
            .expect("OCC strike scale is within ExactDecimal's fixed bound")
    }

    /// Returns the absolute strike width in integer thousandths.
    /// 返回以整数千分之一表示的绝对行权价宽度。
    pub const fn checked_width(self, other: Self) -> u32 {
        self.0.abs_diff(other.0)
    }
}

impl fmt::Display for Strike {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.decimal(), formatter)
    }
}

impl fmt::Display for StrikeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Decimal(error) => return fmt::Display::fmt(error, formatter),
            Self::Zero => "STRIKE_MUST_BE_POSITIVE",
            Self::Negative => "STRIKE_MUST_BE_POSITIVE",
            Self::OverPrecision => "STRIKE_EXCEEDS_THREE_DECIMAL_PLACES",
            Self::ExceedsOccMaximum => "STRIKE_EXCEEDS_OCC_MAXIMUM",
        })
    }
}

impl std::error::Error for StrikeError {}

#[cfg(test)]
mod tests {
    use super::{MAX_OCC_STRIKE_MILLS, Strike, StrikeError};
    use crate::ExactDecimal;

    #[test]
    fn strike_uses_occ_mills_without_binary_float_rounding() {
        let strike = Strike::parse_json_number("740.125").unwrap();
        assert_eq!(strike.mills(), 740_125);
        assert_eq!(strike.to_string(), "740.125");
        assert_eq!(
            Strike::from_decimal(ExactDecimal::from_parts(740_125, 3).unwrap()),
            Ok(strike)
        );
    }

    #[test]
    fn strike_enforces_positive_range_and_precision() {
        assert_eq!(Strike::parse_json_number("0"), Err(StrikeError::Zero));
        assert_eq!(Strike::parse_json_number("-1"), Err(StrikeError::Negative));
        assert_eq!(
            Strike::parse_json_number("740.0005"),
            Err(StrikeError::OverPrecision)
        );
        assert_eq!(
            Strike::parse_json_number("100000"),
            Err(StrikeError::ExceedsOccMaximum)
        );
        assert_eq!(
            Strike::from_mills(MAX_OCC_STRIKE_MILLS)
                .unwrap()
                .to_string(),
            "99999.999"
        );
        assert_eq!(
            Strike::from_mills(MAX_OCC_STRIKE_MILLS + 1),
            Err(StrikeError::ExceedsOccMaximum)
        );
    }

    #[test]
    fn width_subtraction_is_exact_in_mills() {
        let low = Strike::parse_json_number("740.125").unwrap();
        let high = Strike::parse_json_number("742.125").unwrap();
        assert_eq!(low.checked_width(high), 2_000);
    }
}
