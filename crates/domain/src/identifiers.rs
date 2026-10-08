//! Bounded identities and revision values used across domain boundaries.
//!
//! Account scope and route-hash debug output is redacted. `Revision` is a
//! value counter only; it does not represent a durable writer epoch or fence.
//!
//! # 简体中文
//!
//! 提供跨领域边界使用的有界身份标识和修订号。
//!
//! 账户 scope 和 route hash 的 Debug 输出会脱敏。`Revision` 只是数值计数器，不代表持久化
//! writer epoch 或隔离令牌。

use crate::ValidatedVertical;
use std::fmt;

const MAX_IDENTIFIER_BYTES: usize = 256;

/// Identifier construction failure.
/// 构造标识符失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentifierError {
    /// The identifier is empty or contains only whitespace.
    /// 标识符为空或只包含空白字符。
    Empty,
    /// The identifier exceeds the bounded byte length.
    /// 标识符超过有界字节长度。
    TooLong,
    /// The identifier contains a control character.
    /// 标识符包含控制字符。
    InvalidCharacters,
}

macro_rules! opaque_identifier {
    ($type_name:ident) => {
        /// Opaque identifier bounded by the shared identifier validation rules.
        /// 按共享标识符规则校验的有界不透明标识。
        #[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
        pub struct $type_name(String);

        impl $type_name {
            /// Creates an identifier after rejecting blank, oversized, or control-character input.
            /// 拒绝空白、超长或含控制字符的输入后创建标识符。
            pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
                let value = value.into();
                validate_identifier(&value)?;
                Ok(Self(value))
            }

            /// Returns the identifier text. Callers remain responsible for redacting it in logs.
            /// 返回标识符文本；调用方仍须负责在日志中对其脱敏。
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Debug for $type_name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_tuple(stringify!($type_name))
                    .field(&self.0)
                    .finish()
            }
        }

        impl fmt::Display for $type_name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

opaque_identifier!(OrderId);
opaque_identifier!(IntentId);
opaque_identifier!(LogicalOrderId);
opaque_identifier!(ActorId);
opaque_identifier!(StrategyInstanceId);

/// Account identity used for state partitioning; the value must never appear in Debug output.
/// 用于状态分区的账户身份；该值不得出现在 Debug 输出中。
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct AccountScope(String);

/// Schwab routing hash, kept distinct from an account-scope fingerprint.
/// Schwab 路由哈希，与账户 scope 指纹保持不同类型。
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct AccountRouteHash(String);

/// Canonical key for a validated vertical strategy.
/// 已校验 vertical 策略的规范键。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SpreadKey(ValidatedVertical);

/// Monotonic in-process revision counter.
/// 进程内单调递增的修订号计数器。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Revision(u64);

impl AccountScope {
    /// Creates a bounded account partition identity.
    /// 创建有界的账户分区身份标识。
    pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        validate_identifier(&value)?;
        Ok(Self(value))
    }

    /// Returns the account-scope text; avoid exposing it in diagnostics.
    /// 返回账户 scope 文本；避免在诊断信息中暴露该值。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AccountScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("AccountScope")
            .field(&"[REDACTED]")
            .finish()
    }
}

impl AccountRouteHash {
    /// Creates a bounded Schwab routing hash value.
    /// 创建有界的 Schwab 路由哈希值。
    pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        validate_identifier(&value)?;
        Ok(Self(value))
    }

    /// Returns the routing hash text; avoid exposing it in diagnostics.
    /// 返回路由哈希文本；避免在诊断信息中暴露该值。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AccountRouteHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("AccountRouteHash")
            .field(&"[REDACTED]")
            .finish()
    }
}

impl SpreadKey {
    /// Creates a key from the complete validated vertical value.
    /// 根据完整的已校验 vertical 值创建键。
    pub fn from_vertical(vertical: &ValidatedVertical) -> Self {
        Self(vertical.clone())
    }

    /// Borrows the complete validated vertical represented by this key.
    /// 借用此键表示的完整已校验 vertical。
    pub fn vertical(&self) -> &ValidatedVertical {
        &self.0
    }

    /// Legacy five-field label retained for compatibility display only.
    ///
    /// This value intentionally omits orientation and contract terms. It must
    /// not be used as an actor, ownership, cache, or equality key.
    /// 这是仅为兼容性展示保留的五字段旧标签，有意省略方向和合约条款；不得用作 actor、归属、缓存或相等性键。
    pub fn legacy_label(&self) -> String {
        let vertical = &self.0;
        format!(
            "{}:{}:{}:{}:{}",
            vertical.underlying(),
            vertical.expiration(),
            vertical.right().occ_code(),
            vertical.lower_strike(),
            vertical.higher_strike(),
        )
    }
}

impl Revision {
    /// Creates a revision counter from its raw value.
    /// 根据原始数值创建修订号计数器。
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the raw revision value.
    /// 返回原始修订号数值。
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Advances by one, returning `None` instead of wrapping on overflow.
    /// 加一推进；溢出时返回 `None`，不会回绕。
    pub fn checked_next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

impl fmt::Display for IdentifierError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "IDENTIFIER_EMPTY",
            Self::TooLong => "IDENTIFIER_TOO_LONG",
            Self::InvalidCharacters => "IDENTIFIER_INVALID_CHARACTERS",
        })
    }
}

impl std::error::Error for IdentifierError {}

fn validate_identifier(value: &str) -> Result<(), IdentifierError> {
    if value.is_empty() || value.chars().all(char::is_whitespace) {
        return Err(IdentifierError::Empty);
    }
    if value.len() > MAX_IDENTIFIER_BYTES {
        return Err(IdentifierError::TooLong);
    }
    if value.chars().any(char::is_control) {
        return Err(IdentifierError::InvalidCharacters);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        AccountRouteHash, AccountScope, ActorId, IdentifierError, IntentId, LogicalOrderId,
        OrderId, Revision, StrategyInstanceId,
    };

    #[test]
    fn opaque_ids_preserve_their_bytes_and_reject_empty_or_unbounded_values() {
        assert_eq!(OrderId::new("Ord-01").unwrap().as_str(), "Ord-01");
        assert_eq!(IntentId::new("Intent-01").unwrap().as_str(), "Intent-01");
        assert_eq!(
            LogicalOrderId::new("Logical-01").unwrap().as_str(),
            "Logical-01"
        );
        assert_eq!(ActorId::new("Actor-01").unwrap().as_str(), "Actor-01");
        assert_eq!(
            StrategyInstanceId::new("Strategy-01").unwrap().as_str(),
            "Strategy-01"
        );
        assert_eq!(OrderId::new(""), Err(IdentifierError::Empty));
        assert_eq!(OrderId::new(" \t "), Err(IdentifierError::Empty));
        assert_eq!(
            OrderId::new("id\nforged"),
            Err(IdentifierError::InvalidCharacters)
        );
        assert_eq!(ActorId::new("x".repeat(257)), Err(IdentifierError::TooLong));
    }

    #[test]
    fn account_identifiers_are_redacted_from_debug_output() {
        let scope = AccountScope::new("synthetic-account-scope-2718").unwrap();
        let route = AccountRouteHash::new("synthetic-route-hash-3141").unwrap();
        let scope_debug = format!("{scope:?}");
        let route_debug = format!("{route:?}");
        assert!(scope_debug.contains("REDACTED"));
        assert!(route_debug.contains("REDACTED"));
        assert!(!scope_debug.contains("2718"));
        assert!(!route_debug.contains("3141"));
    }

    #[test]
    fn revisions_advance_with_checked_overflow() {
        assert_eq!(Revision::new(41).checked_next(), Some(Revision::new(42)));
        assert_eq!(Revision::new(u64::MAX).checked_next(), None);
    }
}
