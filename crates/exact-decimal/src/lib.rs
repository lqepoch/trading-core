#![forbid(unsafe_code)]

//! Exact bounded base-10 arithmetic for financial values.
//!
//! Parsing and arithmetic preserve exact values or return a typed error; they
//! never round. Input size, exponent, scale, and coefficient are bounded.
//!
//! # 简体中文
//!
//! 金融值使用精确且有界的十进制运算。
//!
//! 解析与运算会保留精确值，无法表示时返回带类型的错误；不会进行舍入。输入长度、指数、
//! scale 和系数均有上限。

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

/// Maximum bytes accepted for one exact decimal token.
/// 一个精确十进制 token 可接受的最大字节数。
pub const MAX_DECIMAL_INPUT_BYTES: usize = 128;
/// Maximum absolute base-10 scientific exponent accepted by the parser.
/// 解析器接受的十进制科学计数法指数绝对值上限。
pub const MAX_DECIMAL_EXPONENT: i32 = 38;
/// Maximum normalized decimal scale supported by an `i128` coefficient.
/// `i128` 系数可支持的最大规范化十进制 scale。
pub const MAX_DECIMAL_SCALE: u32 = 28;

/// An exact finite base-10 value stored as a signed `i128` coefficient and scale.
///
/// The representation is normalized: zero has scale zero, and non-zero values
/// have no trailing coefficient zero while scale is non-zero. Arithmetic never
/// rounds; operations that cannot be represented return an error.
/// 表示会规范化：零的 scale 为零；scale 非零的非零值，其系数不含末尾零。算术运算不会舍入；无法精确表示的操作会返回错误。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ExactDecimal {
    coefficient: i128,
    scale: u8,
}

/// Failure to parse or exactly represent a decimal value.
/// 解析十进制值或精确表示结果失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecimalError {
    /// The input token exceeds [`MAX_DECIMAL_INPUT_BYTES`].
    /// 输入 token 超过 [`MAX_DECIMAL_INPUT_BYTES`]。
    InputTooLong,
    /// The input does not match the strict JSON number grammar.
    /// 输入不符合严格 JSON 数字语法。
    InvalidJsonNumber,
    /// The scientific exponent exceeds [`MAX_DECIMAL_EXPONENT`].
    /// 科学计数法指数超过 [`MAX_DECIMAL_EXPONENT`]。
    ExponentOutOfRange,
    /// The normalized scale exceeds [`MAX_DECIMAL_SCALE`].
    /// 规范化后的 scale 超过 [`MAX_DECIMAL_SCALE`]。
    ScaleOutOfRange,
    /// The coefficient or intermediate arithmetic overflows `i128`.
    /// 系数或中间运算结果超出 `i128` 范围。
    Overflow,
    /// The requested division has a non-terminating or inexact decimal result.
    /// 请求的除法结果不是有限十进制值，或无法精确表示。
    InexactDivision,
}

impl ExactDecimal {
    /// Exact decimal zero in normalized representation.
    /// 规范化表示中的精确十进制零。
    pub const ZERO: Self = Self {
        coefficient: 0,
        scale: 0,
    };

    /// Constructs an exact integer value with scale zero.
    /// 以 scale 零构造精确整数值。
    pub const fn from_integer(value: i128) -> Self {
        Self {
            coefficient: value,
            scale: 0,
        }
    }

    /// Build a normalized decimal from coefficient × 10^-scale.
    /// 根据 coefficient × 10^-scale 构造规范化十进制值。
    pub fn from_parts(coefficient: i128, scale: u32) -> Result<Self, DecimalError> {
        let (coefficient, scale) = normalize(coefficient, scale)?;
        Ok(Self {
            coefficient,
            scale: scale as u8,
        })
    }

    /// Parse one strict JSON-number token, including exact scientific notation.
    ///
    /// The parser accepts JSON grammar only; it does not accept surrounding
    /// whitespace, `NaN`, infinity, a leading plus, or a leading-zero integer.
    /// Input length and exponent are bounded before any scale expansion occurs.
    /// 只接受 JSON number 语法，不接受周围空白、`NaN`、无穷值、前导加号或带前导零的整数。扩展 scale 前会先限制输入长度和指数。
    pub fn parse_json_number(input: &str) -> Result<Self, DecimalError> {
        if input.len() > MAX_DECIMAL_INPUT_BYTES {
            return Err(DecimalError::InputTooLong);
        }
        let bytes = input.as_bytes();
        if bytes.is_empty() {
            return Err(DecimalError::InvalidJsonNumber);
        }

        let mut cursor = 0;
        let negative = bytes[cursor] == b'-';
        if negative {
            cursor += 1;
            if cursor == bytes.len() {
                return Err(DecimalError::InvalidJsonNumber);
            }
        }

        let integer_start = cursor;
        match bytes.get(cursor) {
            Some(b'0') => {
                cursor += 1;
                if bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
                    return Err(DecimalError::InvalidJsonNumber);
                }
            }
            Some(b'1'..=b'9') => {
                cursor += 1;
                while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
                    cursor += 1;
                }
            }
            _ => return Err(DecimalError::InvalidJsonNumber),
        }
        let integer_end = cursor;

        let mut fraction_start = cursor;
        let mut fraction_len = 0usize;
        if bytes.get(cursor) == Some(&b'.') {
            cursor += 1;
            fraction_start = cursor;
            while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
                cursor += 1;
            }
            fraction_len = cursor - fraction_start;
            if fraction_len == 0 {
                return Err(DecimalError::InvalidJsonNumber);
            }
        }

        let exponent = if matches!(bytes.get(cursor), Some(b'e' | b'E')) {
            cursor += 1;
            let exponent_negative = match bytes.get(cursor) {
                Some(b'-') => {
                    cursor += 1;
                    true
                }
                Some(b'+') => {
                    cursor += 1;
                    false
                }
                _ => false,
            };
            let exponent_start = cursor;
            let mut magnitude = 0i32;
            while let Some(byte @ b'0'..=b'9') = bytes.get(cursor).copied() {
                magnitude = magnitude
                    .checked_mul(10)
                    .and_then(|value| value.checked_add(i32::from(byte - b'0')))
                    .ok_or(DecimalError::ExponentOutOfRange)?;
                if magnitude > MAX_DECIMAL_EXPONENT {
                    return Err(DecimalError::ExponentOutOfRange);
                }
                cursor += 1;
            }
            if cursor == exponent_start {
                return Err(DecimalError::InvalidJsonNumber);
            }
            if exponent_negative {
                -magnitude
            } else {
                magnitude
            }
        } else {
            0
        };

        if cursor != bytes.len() {
            return Err(DecimalError::InvalidJsonNumber);
        }

        let mut digits = String::with_capacity(bytes.len());
        digits.push_str(&input[integer_start..integer_end]);
        if fraction_len > 0 {
            digits.push_str(&input[fraction_start..fraction_start + fraction_len]);
        }
        let first_nonzero = digits.bytes().position(|byte| byte != b'0');
        let Some(first_nonzero) = first_nonzero else {
            return Ok(Self::ZERO);
        };
        digits.drain(..first_nonzero);

        let mut scale =
            i32::try_from(fraction_len).map_err(|_| DecimalError::ScaleOutOfRange)? - exponent;
        if scale < 0 {
            let zeros = usize::try_from(-scale).map_err(|_| DecimalError::Overflow)?;
            if digits.len().saturating_add(zeros) > MAX_DECIMAL_INPUT_BYTES {
                return Err(DecimalError::Overflow);
            }
            digits.extend(std::iter::repeat_n('0', zeros));
            scale = 0;
        }
        while scale > 0 && digits.ends_with('0') {
            digits.pop();
            scale -= 1;
        }
        if scale > MAX_DECIMAL_SCALE as i32 {
            return Err(DecimalError::ScaleOutOfRange);
        }

        let mut signed_digits = String::with_capacity(digits.len() + usize::from(negative));
        if negative {
            signed_digits.push('-');
        }
        signed_digits.push_str(&digits);
        let coefficient = signed_digits
            .parse::<i128>()
            .map_err(|_| DecimalError::Overflow)?;
        Self::from_parts(coefficient, scale as u32)
    }

    /// Returns the signed integer coefficient in normalized representation.
    /// 返回规范化表示中的有符号整数系数。
    pub const fn coefficient(self) -> i128 {
        self.coefficient
    }

    /// Returns the number of decimal places in the normalized representation.
    /// 返回规范化表示中的小数位数。
    pub const fn scale(self) -> u32 {
        self.scale as u32
    }

    /// Returns whether this value is exactly zero.
    /// 返回此值是否精确等于零。
    pub const fn is_zero(self) -> bool {
        self.coefficient == 0
    }

    /// Returns whether this value is negative.
    /// 返回此值是否为负数。
    pub const fn is_negative(self) -> bool {
        self.coefficient < 0
    }

    /// Returns the exact coefficient at `target_scale`. Increasing the scale appends zeroes;
    /// decreasing it succeeds only when all discarded decimal places are zero. No rounding is
    /// performed; an unsupported scale, coefficient overflow, or inexact reduction returns an error.
    /// 返回目标 `target_scale` 下的精确整数系数。增大 scale 会补零；减小时仅当被移除的小数位全为零时成功。
    /// 不会舍入；目标 scale 超限、系数溢出或无法精确缩小时都会返回错误。
    pub fn to_scaled_integer(self, target_scale: u32) -> Result<i128, DecimalError> {
        if target_scale > MAX_DECIMAL_SCALE {
            return Err(DecimalError::ScaleOutOfRange);
        }
        if target_scale >= self.scale() {
            let factor = power_of_ten(target_scale - self.scale())?;
            self.coefficient
                .checked_mul(factor)
                .ok_or(DecimalError::Overflow)
        } else {
            let divisor = power_of_ten(self.scale() - target_scale)?;
            if self.coefficient % divisor != 0 {
                return Err(DecimalError::InexactDivision);
            }
            Ok(self.coefficient / divisor)
        }
    }

    /// Adds two values exactly, returning an error on overflow.
    /// 精确地相加两个值；溢出时返回错误。
    pub fn checked_add(self, other: Self) -> Result<Self, DecimalError> {
        let scale = self.scale().max(other.scale());
        let left = self.to_scaled_integer(scale)?;
        let right = other.to_scaled_integer(scale)?;
        Self::from_parts(
            left.checked_add(right).ok_or(DecimalError::Overflow)?,
            scale,
        )
    }

    /// Subtracts another value exactly, returning an error on overflow.
    /// 精确地减去另一个值；溢出时返回错误。
    pub fn checked_sub(self, other: Self) -> Result<Self, DecimalError> {
        let scale = self.scale().max(other.scale());
        let left = self.to_scaled_integer(scale)?;
        let right = other.to_scaled_integer(scale)?;
        Self::from_parts(
            left.checked_sub(right).ok_or(DecimalError::Overflow)?,
            scale,
        )
    }

    /// Multiplies by an integer exactly, returning an error on overflow.
    /// 精确地乘以整数；溢出时返回错误。
    pub fn checked_mul_integer(self, multiplier: i128) -> Result<Self, DecimalError> {
        Self::from_parts(
            self.coefficient
                .checked_mul(multiplier)
                .ok_or(DecimalError::Overflow)?,
            self.scale(),
        )
    }

    /// Multiplies exact decimals by canceling powers of ten from the product coefficient against
    /// the resulting scale. It never rounds; an unrepresentable scale or coefficient returns an error.
    /// 精确相乘两个十进制值，并从乘积系数中约去与结果 scale 对应的 10 因子。不会舍入；结果 scale 或系数超出表示范围时返回错误。
    pub fn checked_mul(self, other: Self) -> Result<Self, DecimalError> {
        if self.is_zero() || other.is_zero() {
            return Ok(Self::ZERO);
        }

        let mut left = self.coefficient;
        let mut right = other.coefficient;
        let mut scale = self.scale() + other.scale();
        while scale > 0 {
            if left % 10 == 0 {
                left /= 10;
            } else if right % 10 == 0 {
                right /= 10;
            } else if left % 2 == 0 && right % 5 == 0 {
                left /= 2;
                right /= 5;
            } else if left % 5 == 0 && right % 2 == 0 {
                left /= 5;
                right /= 2;
            } else {
                break;
            }
            scale -= 1;
        }
        if scale > MAX_DECIMAL_SCALE {
            return Err(DecimalError::ScaleOutOfRange);
        }
        Self::from_parts(
            left.checked_mul(right).ok_or(DecimalError::Overflow)?,
            scale,
        )
    }

    /// Divide by two only when the exact result fits the supported decimal scale.
    /// 仅当除以二的精确结果符合受支持的十进制 scale 时才返回结果。
    pub fn checked_divide_by_two_exact(self) -> Result<Self, DecimalError> {
        if self.coefficient % 2 == 0 {
            return Self::from_parts(self.coefficient / 2, self.scale());
        }
        if self.scale() == MAX_DECIMAL_SCALE {
            return Err(DecimalError::ScaleOutOfRange);
        }
        Self::from_parts(
            self.coefficient
                .checked_mul(5)
                .ok_or(DecimalError::Overflow)?,
            self.scale() + 1,
        )
    }

    /// Returns the exact midpoint without rounding.
    /// 返回不舍入的精确中点。
    pub fn checked_midpoint(self, other: Self) -> Result<Self, DecimalError> {
        self.checked_add(other)?.checked_divide_by_two_exact()
    }
}

impl FromStr for ExactDecimal {
    type Err = DecimalError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_json_number(value)
    }
}

impl Ord for ExactDecimal {
    fn cmp(&self, other: &Self) -> Ordering {
        let sign_order = self.coefficient.signum().cmp(&other.coefficient.signum());
        if sign_order != Ordering::Equal {
            return sign_order;
        }
        if self.coefficient == 0 {
            return Ordering::Equal;
        }
        if self.coefficient < 0 {
            compare_magnitude(*other, *self)
        } else {
            compare_magnitude(*self, *other)
        }
    }
}

impl PartialOrd for ExactDecimal {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for ExactDecimal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.coefficient == 0 {
            return formatter.write_str("0");
        }

        let negative = self.coefficient < 0;
        let digits = self.coefficient.unsigned_abs().to_string();
        if negative {
            formatter.write_str("-")?;
        }
        let scale = self.scale();
        if scale == 0 {
            return formatter.write_str(&digits);
        }

        let scale = scale as usize;
        if digits.len() > scale {
            let point = digits.len() - scale;
            formatter.write_str(&digits[..point])?;
            formatter.write_str(".")?;
            formatter.write_str(&digits[point..])
        } else {
            formatter.write_str("0.")?;
            for _ in 0..(scale - digits.len()) {
                formatter.write_str("0")?;
            }
            formatter.write_str(&digits)
        }
    }
}

impl fmt::Display for DecimalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InputTooLong => "DECIMAL_INPUT_TOO_LONG",
            Self::InvalidJsonNumber => "DECIMAL_JSON_NUMBER_INVALID",
            Self::ExponentOutOfRange => "DECIMAL_EXPONENT_OUT_OF_RANGE",
            Self::ScaleOutOfRange => "DECIMAL_SCALE_OUT_OF_RANGE",
            Self::Overflow => "DECIMAL_OVERFLOW",
            Self::InexactDivision => "DECIMAL_DIVISION_INEXACT",
        })
    }
}

impl std::error::Error for DecimalError {}

fn normalize(coefficient: i128, mut scale: u32) -> Result<(i128, u32), DecimalError> {
    if coefficient == 0 {
        return Ok((0, 0));
    }
    let mut coefficient = coefficient;
    while scale > 0 && coefficient % 10 == 0 {
        coefficient /= 10;
        scale -= 1;
    }
    if scale > MAX_DECIMAL_SCALE {
        return Err(DecimalError::ScaleOutOfRange);
    }
    Ok((coefficient, scale))
}

fn power_of_ten(exponent: u32) -> Result<i128, DecimalError> {
    let mut value = 1i128;
    for _ in 0..exponent {
        value = value.checked_mul(10).ok_or(DecimalError::Overflow)?;
    }
    Ok(value)
}

fn compare_magnitude(left: ExactDecimal, right: ExactDecimal) -> Ordering {
    let left_digits = left.coefficient.unsigned_abs().to_string();
    let right_digits = right.coefficient.unsigned_abs().to_string();
    let left_order = left_digits.len() as i64 - i64::from(left.scale);
    let right_order = right_digits.len() as i64 - i64::from(right.scale);
    match left_order.cmp(&right_order) {
        Ordering::Equal => {
            let max_len = left_digits.len().max(right_digits.len());
            for index in 0..max_len {
                let left_digit = left_digits.as_bytes().get(index).copied().unwrap_or(b'0');
                let right_digit = right_digits.as_bytes().get(index).copied().unwrap_or(b'0');
                match left_digit.cmp(&right_digit) {
                    Ordering::Equal => {}
                    ordering => return ordering,
                }
            }
            Ordering::Equal
        }
        ordering => ordering,
    }
}

#[cfg(test)]
mod tests {
    use super::{DecimalError, ExactDecimal, MAX_DECIMAL_INPUT_BYTES, MAX_DECIMAL_SCALE};
    use std::cmp::Ordering;

    fn decimal(value: &str) -> ExactDecimal {
        ExactDecimal::parse_json_number(value).unwrap()
    }

    #[test]
    fn parser_normalizes_json_numbers_and_scientific_notation_exactly() {
        for (input, expected) in [
            ("0", "0"),
            ("-0.000", "0"),
            ("1.2300e2", "123"),
            ("1.2300e1", "12.3"),
            ("1200e-2", "12"),
            ("-1.25e-2", "-0.0125"),
            ("1e-28", "0.0000000000000000000000000001"),
            (
                "170141183460469231731687303715884105727",
                "170141183460469231731687303715884105727",
            ),
            (
                "-170141183460469231731687303715884105728",
                "-170141183460469231731687303715884105728",
            ),
        ] {
            assert_eq!(decimal(input).to_string(), expected, "input {input}");
        }
    }

    #[test]
    fn parser_rejects_invalid_json_forms_and_unrepresentable_precision() {
        for input in [
            "", "+1", ".5", "1.", "01", " 1", "1 ", "NaN", "Infinity", "1e",
        ] {
            assert_eq!(
                ExactDecimal::parse_json_number(input),
                Err(DecimalError::InvalidJsonNumber),
                "{input:?}"
            );
        }
        assert_eq!(
            ExactDecimal::parse_json_number("1e-29"),
            Err(DecimalError::ScaleOutOfRange)
        );
        assert_eq!(
            ExactDecimal::parse_json_number("1e+39"),
            Err(DecimalError::ExponentOutOfRange)
        );
        assert_eq!(
            ExactDecimal::parse_json_number("2e38"),
            Err(DecimalError::Overflow)
        );
        assert_eq!(
            ExactDecimal::parse_json_number(&"1".repeat(MAX_DECIMAL_INPUT_BYTES + 1)),
            Err(DecimalError::InputTooLong)
        );
    }

    #[test]
    fn from_parts_normalizes_and_never_rounds_over_scale_limit() {
        assert_eq!(ExactDecimal::from_parts(10, 29).unwrap(), decimal("1e-28"));
        assert_eq!(
            ExactDecimal::from_parts(1, 29),
            Err(DecimalError::ScaleOutOfRange)
        );
        assert_eq!(
            ExactDecimal::from_parts(0, u32::MAX).unwrap(),
            ExactDecimal::ZERO
        );
        assert_eq!(
            ExactDecimal::from_parts(1, MAX_DECIMAL_SCALE)
                .unwrap()
                .scale(),
            MAX_DECIMAL_SCALE
        );
    }

    #[test]
    fn add_subtract_multiply_and_midpoint_are_checked_and_exact() {
        assert_eq!(
            decimal("0.1")
                .checked_add(decimal("0.02"))
                .unwrap()
                .to_string(),
            "0.12"
        );
        assert_eq!(
            decimal("0.1")
                .checked_sub(decimal("0.12"))
                .unwrap()
                .to_string(),
            "-0.02"
        );
        assert_eq!(
            decimal("1.25")
                .checked_mul(decimal("0.2"))
                .unwrap()
                .to_string(),
            "0.25"
        );
        assert_eq!(
            decimal("0.01")
                .checked_midpoint(decimal("0.02"))
                .unwrap()
                .to_string(),
            "0.015"
        );
        assert_eq!(
            decimal("0.0000000000000000000000000001").checked_divide_by_two_exact(),
            Err(DecimalError::ScaleOutOfRange)
        );
    }

    #[test]
    fn arithmetic_reports_overflow_instead_of_wrapping() {
        let max = decimal("170141183460469231731687303715884105727");
        let min = decimal("-170141183460469231731687303715884105728");
        assert_eq!(
            max.checked_add(ExactDecimal::from_integer(1)),
            Err(DecimalError::Overflow)
        );
        assert_eq!(max.checked_mul_integer(2), Err(DecimalError::Overflow));
        assert_eq!(min.checked_mul_integer(-1), Err(DecimalError::Overflow));
        assert_eq!(max.checked_midpoint(max), Err(DecimalError::Overflow));
        assert_eq!(min.to_string(), "-170141183460469231731687303715884105728");
        assert!(min < ExactDecimal::ZERO);
        assert!(min < max);
    }

    #[test]
    fn exact_ordering_handles_sign_and_different_scales_without_rescaling_overflow() {
        assert!(decimal("-10") < decimal("-2"));
        assert!(decimal("0.099") < decimal("0.1"));
        assert!(decimal("1e20") > decimal("99999999999999999999"));
        assert!(decimal("1e38") > decimal("1e-28"));
        assert!(decimal("0") < decimal("1"));
        assert!(decimal("1") > decimal("0"));
        assert!(decimal("-1") < decimal("0"));
        assert!(decimal("0") > decimal("-1"));
        assert!(decimal("2.7499999999999999") < decimal("2.75"));
        assert!(decimal("2.7500000000000001") > decimal("2.75"));
        assert!(decimal("3.2499999999999999") < decimal("3.25"));
        assert!(decimal("3.2500000000000001") > decimal("3.25"));
    }

    #[test]
    fn exact_decimal_ordering_is_consistent_with_equality_and_transitive() {
        let values = [
            decimal("-10"),
            decimal("-1.01"),
            decimal("-1"),
            decimal("-0.01"),
            decimal("0"),
            decimal("0.01"),
            decimal("1"),
            decimal("1.000"),
            decimal("1.01"),
            decimal("10"),
        ];
        for left in values {
            for middle in values {
                assert_eq!(left.cmp(&middle) == Ordering::Equal, left == middle);
                for right in values {
                    if left <= middle && middle <= right {
                        assert!(left <= right, "{left} <= {middle} <= {right}");
                    }
                }
            }
        }
    }

    #[test]
    fn bounded_decimal_grid_preserves_exact_arithmetic_order_and_roundtrips() {
        fn reference_power_of_ten(exponent: u32) -> i128 {
            (0..exponent).fold(1i128, |value, _| value * 10)
        }

        fn reference_normalize(mut coefficient: i128, mut scale: u32) -> ExactDecimal {
            while scale > 0 && coefficient % 10 == 0 {
                coefficient /= 10;
                scale -= 1;
            }
            ExactDecimal {
                coefficient,
                scale: u8::try_from(scale).unwrap(),
            }
        }

        fn reference_half(coefficient: i128, scale: u32) -> ExactDecimal {
            if coefficient % 2 == 0 {
                reference_normalize(coefficient / 2, scale)
            } else {
                reference_normalize(coefficient * 5, scale + 1)
            }
        }

        let values = (-13i128..=13)
            .flat_map(|coefficient| {
                (0..=6).map(move |scale| ExactDecimal::from_parts(coefficient, scale).unwrap())
            })
            .collect::<Vec<_>>();

        for value in &values {
            let rendered = value.to_string();
            let reparsed = ExactDecimal::parse_json_number(&rendered).unwrap();
            assert_eq!(reparsed, *value, "display roundtrip for {rendered}");
            assert_eq!(reparsed.coefficient, value.coefficient);
            assert_eq!(reparsed.scale, value.scale);
        }

        for left in &values {
            for right in &values {
                let common_scale = left.scale().max(right.scale());
                let left_scaled =
                    left.coefficient * reference_power_of_ten(common_scale - left.scale());
                let right_scaled =
                    right.coefficient * reference_power_of_ten(common_scale - right.scale());

                assert_eq!(
                    left.cmp(right),
                    left_scaled.cmp(&right_scaled),
                    "ordering {left} versus {right}"
                );
                assert_eq!(
                    left.checked_add(*right),
                    Ok(reference_normalize(
                        left_scaled + right_scaled,
                        common_scale
                    )),
                    "addition {left} + {right}"
                );
                assert_eq!(
                    left.checked_sub(*right),
                    Ok(reference_normalize(
                        left_scaled - right_scaled,
                        common_scale
                    )),
                    "subtraction {left} - {right}"
                );

                let product_coefficient = left.coefficient * right.coefficient;
                let product_scale = left.scale() + right.scale();
                assert_eq!(
                    left.checked_mul(*right),
                    Ok(reference_normalize(product_coefficient, product_scale)),
                    "multiplication {left} * {right}"
                );
                assert_eq!(
                    left.checked_midpoint(*right),
                    Ok(reference_half(left_scaled + right_scaled, common_scale)),
                    "midpoint of {left} and {right}"
                );
            }
        }

        for boundary in [
            ExactDecimal::from_integer(i128::MIN),
            ExactDecimal::from_integer(i128::MAX),
            decimal("-1e-28"),
            decimal("1e-28"),
        ] {
            let rendered = boundary.to_string();
            let reparsed = ExactDecimal::parse_json_number(&rendered).unwrap();
            assert_eq!(reparsed.coefficient, boundary.coefficient);
            assert_eq!(reparsed.scale, boundary.scale);
        }
    }

    #[test]
    fn multiplication_cancels_decimal_scale_before_checked_coefficient_product() {
        let ten_to_the_thirty_eight = ExactDecimal::from_integer(10_i128.pow(38));
        assert_eq!(
            ten_to_the_thirty_eight.checked_mul(decimal("0.01")),
            Ok(ExactDecimal::from_integer(10_i128.pow(36)))
        );
        assert_eq!(
            decimal("0.02").checked_mul(decimal("0.05")),
            Ok(decimal("0.001"))
        );
        assert_eq!(
            decimal("-0.025").checked_mul(decimal("0.04")),
            Ok(decimal("-0.001"))
        );
        assert_eq!(
            decimal("1e-28").checked_mul(decimal("0.1")),
            Err(DecimalError::ScaleOutOfRange)
        );
    }
}
