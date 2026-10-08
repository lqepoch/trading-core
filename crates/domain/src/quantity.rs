//! Bounded whole-unit quantities for orders, inventory, and contract terms.
//!
//! Order quantities and contract multipliers must be positive. Inventory may
//! be zero, but none of these value types determines authoritative or sellable
//! broker inventory.
//!
//! # 简体中文
//!
//! 为订单、库存和合约条款提供有界的整数数量类型。
//!
//! 订单数量和合约乘数必须为正数；库存数量可以为零。这些值类型本身不会确定券商权威库存
//! 或可卖数量。

/// Largest integer exactly representable as a JavaScript `Number`.
/// 可由 JavaScript `Number` 精确表示的最大整数。
pub const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// Positive whole-unit quantity safe to serialize through the legacy Node wire boundary.
/// 可安全通过旧版 Node wire 边界序列化的正整数单位数量。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Quantity(u64);

/// Whole-unit inventory quantity, where zero is a valid flat position.
/// 整数单位库存数量；零表示有效的空仓状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct InventoryQuantity(u64);

/// Positive integer contract multiplier bounded by the exact wire integer range.
/// 受精确 wire 整数范围约束的正整数合约乘数。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ContractMultiplier(u64);

/// Quantity or multiplier invariant failure.
/// 数量或合约乘数不变量校验失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuantityError {
    /// A positive quantity or multiplier was required, but zero was supplied.
    /// 需要正数数量或乘数，但传入了零。
    Zero,
    /// The value exceeds [`MAX_SAFE_INTEGER`].
    /// 数值超过 [`MAX_SAFE_INTEGER`]。
    ExceedsSafeInteger,
    /// Checked arithmetic exceeded the supported integer range.
    /// 有检查的算术运算超出了受支持的整数范围。
    Overflow,
    /// Checked inventory subtraction would produce a negative value.
    /// 有检查的库存减法会产生负数。
    Underflow,
}

impl Quantity {
    /// Creates a positive quantity within the exact wire integer range.
    /// 创建处于精确 wire 整数范围内的正数数量。
    pub fn new(value: u64) -> Result<Self, QuantityError> {
        if value == 0 {
            return Err(QuantityError::Zero);
        }
        if value > MAX_SAFE_INTEGER {
            return Err(QuantityError::ExceedsSafeInteger);
        }
        Ok(Self(value))
    }

    /// Returns the whole-unit quantity.
    /// 返回整数单位数量。
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Adds another positive quantity while checking overflow and the safe-integer bound.
    /// 相加另一个正数数量，并检查溢出和安全整数上限。
    pub fn checked_add(self, other: Self) -> Result<Self, QuantityError> {
        Self::new(self.0.checked_add(other.0).ok_or(QuantityError::Overflow)?)
    }

    /// Converts the quantity to a signed integer without loss.
    /// 无损地将数量转换为有符号整数。
    pub fn as_i128(self) -> i128 {
        i128::from(self.0)
    }
}

impl InventoryQuantity {
    /// Creates an inventory quantity; zero is allowed to represent a flat position.
    /// 创建库存数量；允许使用零表示空仓。
    pub fn new(value: u64) -> Result<Self, QuantityError> {
        if value > MAX_SAFE_INTEGER {
            return Err(QuantityError::ExceedsSafeInteger);
        }
        Ok(Self(value))
    }

    /// Returns the zero inventory quantity.
    /// 返回零库存数量。
    pub const fn zero() -> Self {
        Self(0)
    }

    /// Returns the whole-unit inventory quantity.
    /// 返回整数单位库存数量。
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Adds inventory while checking overflow and the safe-integer bound.
    /// 增加库存，并检查溢出和安全整数上限。
    pub fn checked_add(self, other: Self) -> Result<Self, QuantityError> {
        Self::new(self.0.checked_add(other.0).ok_or(QuantityError::Overflow)?)
    }

    /// Subtracts inventory and rejects a result below zero.
    /// 扣减库存，并拒绝小于零的结果。
    pub fn checked_sub(self, other: Self) -> Result<Self, QuantityError> {
        Self::new(
            self.0
                .checked_sub(other.0)
                .ok_or(QuantityError::Underflow)?,
        )
    }

    /// Converts non-zero inventory to a valid order quantity.
    /// 将非零库存转换为有效订单数量。
    pub fn as_order_quantity(self) -> Result<Quantity, QuantityError> {
        Quantity::new(self.0)
    }
}

impl ContractMultiplier {
    /// Creates a positive contract multiplier within the exact wire integer range.
    /// 创建处于精确 wire 整数范围内的正数合约乘数。
    pub fn new(value: u64) -> Result<Self, QuantityError> {
        if value == 0 {
            return Err(QuantityError::Zero);
        }
        if value > MAX_SAFE_INTEGER {
            return Err(QuantityError::ExceedsSafeInteger);
        }
        Ok(Self(value))
    }

    /// Returns the whole-number contract multiplier.
    /// 返回整数合约乘数。
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for QuantityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Zero => "QUANTITY_MUST_BE_POSITIVE",
            Self::ExceedsSafeInteger => "QUANTITY_EXCEEDS_JS_SAFE_INTEGER",
            Self::Overflow => "QUANTITY_OVERFLOW",
            Self::Underflow => "QUANTITY_UNDERFLOW",
        })
    }
}

impl std::error::Error for QuantityError {}

#[cfg(test)]
mod tests {
    use super::{ContractMultiplier, InventoryQuantity, MAX_SAFE_INTEGER, Quantity, QuantityError};

    #[test]
    fn orders_require_positive_whole_quantities_but_inventory_can_be_zero() {
        assert_eq!(Quantity::new(0), Err(QuantityError::Zero));
        assert_eq!(Quantity::new(1).unwrap().get(), 1);
        assert_eq!(
            InventoryQuantity::new(0).unwrap(),
            InventoryQuantity::zero()
        );
        assert_eq!(
            InventoryQuantity::new(1)
                .unwrap()
                .as_order_quantity()
                .unwrap()
                .get(),
            1
        );
        assert_eq!(
            InventoryQuantity::zero().as_order_quantity(),
            Err(QuantityError::Zero)
        );
    }

    #[test]
    fn quantities_and_multipliers_stop_at_the_javascript_safe_integer_boundary() {
        assert_eq!(
            Quantity::new(MAX_SAFE_INTEGER).unwrap().get(),
            MAX_SAFE_INTEGER
        );
        assert_eq!(
            Quantity::new(MAX_SAFE_INTEGER + 1),
            Err(QuantityError::ExceedsSafeInteger)
        );
        assert_eq!(ContractMultiplier::new(100).unwrap().get(), 100);
        assert_eq!(ContractMultiplier::new(0), Err(QuantityError::Zero));
    }

    #[test]
    fn quantity_arithmetic_checks_overflow_and_underflow() {
        let maximum = Quantity::new(MAX_SAFE_INTEGER).unwrap();
        assert_eq!(
            maximum.checked_add(Quantity::new(1).unwrap()),
            Err(QuantityError::ExceedsSafeInteger)
        );
        let one = InventoryQuantity::new(1).unwrap();
        assert_eq!(
            InventoryQuantity::zero().checked_sub(one),
            Err(QuantityError::Underflow)
        );
        assert_eq!(one.checked_sub(one).unwrap(), InventoryQuantity::zero());
    }
}
