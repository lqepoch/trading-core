//! Canonical candidate-shape validation for IBKR PAPER account identifiers.
//!
//! This validates a local accepted syntax only. IBKR's [Paper Trading FAQ](https://www.interactivebrokers.co.jp/en/support/customer-service.php?p=email)
//! documents the `DU` paper prefix, but the suffix bounds and character constraints here do not prove
//! an account is managed by a Gateway, logged in to PAPER, selected by an
//! explicit route, or authorized.
//!
//! # 简体中文
//!
//! IBKR PAPER 账户标识符的规范候选格式校验。
//!
//! 此函数仅检查格式；不能证明账户由 Gateway 管理、当前登录为 PAPER、已选定显式路由或已获授权。

/// Minimum byte length of a canonical IBKR PAPER account identifier.
/// IBKR PAPER 账户标识符的最小字节长度。
pub const IBKR_PAPER_ACCOUNT_ID_MIN_BYTES: usize = 4;

/// Maximum byte length of a canonical IBKR PAPER account identifier.
/// IBKR PAPER 账户标识符的最大字节长度。
pub const IBKR_PAPER_ACCOUNT_ID_MAX_BYTES: usize = 64;

/// Checks the locally accepted syntactic candidate shape for an IBKR PAPER account identifier.
///
/// A candidate starts with uppercase `DU`, has a nonempty uppercase ASCII
/// letter/digit suffix containing at least one digit, and is 4–64 bytes long.
/// IBKR's [Paper Trading FAQ](https://www.interactivebrokers.co.jp/en/support/customer-service.php?p=email)
/// identifies the `DU` paper prefix; the suffix and length constraints are
/// local input validation, not an IBKR guarantee.
/// Callers must separately verify the live Gateway's exact managed-account
/// match, selected PAPER environment, broker namespace, fixed route, and user
/// authorization. This function never infers those facts.
///
/// 检查 IBKR PAPER 账户标识符的规范语法：大写 `DU` 前缀、非空的大写 ASCII 字母/数字后缀、至少一位数字，
/// 且总长度为 4–64 字节。IBKR 文档说明 Paper 账户使用 `DU` 前缀；后缀与长度限制是本地输入校验，并非 IBKR 保证。
/// 调用方仍须单独核验 Gateway 实际 managed account 精确匹配、PAPER 环境、券商命名空间、固定路由和用户授权；此函数不会推断这些事实。
pub fn is_ibkr_paper_account_id(account: &str) -> bool {
    let bytes = account.as_bytes();
    if !(IBKR_PAPER_ACCOUNT_ID_MIN_BYTES..=IBKR_PAPER_ACCOUNT_ID_MAX_BYTES).contains(&bytes.len())
        || !bytes.starts_with(b"DU")
    {
        return false;
    }

    let suffix = &bytes[2..];
    suffix
        .iter()
        .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        && suffix.iter().any(u8::is_ascii_digit)
}

#[cfg(test)]
mod tests {
    use super::{
        IBKR_PAPER_ACCOUNT_ID_MAX_BYTES, IBKR_PAPER_ACCOUNT_ID_MIN_BYTES, is_ibkr_paper_account_id,
    };

    #[test]
    fn accepts_canonical_du_numeric_and_alphanumeric_candidates() {
        assert!(is_ibkr_paper_account_id("DU123456"));
        assert!(is_ibkr_paper_account_id("DUR123456"));
        assert!(is_ibkr_paper_account_id("DUA1"));
        assert_eq!(IBKR_PAPER_ACCOUNT_ID_MIN_BYTES, 4);
        let at_maximum = format!("DU{}1", "A".repeat(IBKR_PAPER_ACCOUNT_ID_MAX_BYTES - 3));
        assert_eq!(at_maximum.len(), IBKR_PAPER_ACCOUNT_ID_MAX_BYTES);
        assert!(is_ibkr_paper_account_id(&at_maximum));
    }

    #[test]
    fn rejects_non_du_live_empty_lowercase_unicode_and_out_of_range_candidates() {
        for invalid in [
            "",
            "DU",
            "DU1",
            "DUABC",
            "U123456",
            "DUa123",
            "DU12 3",
            "DU1-2",
            "DU1_2",
            "DU１２３",
            "DU123\n",
        ] {
            assert!(!is_ibkr_paper_account_id(invalid), "accepted {invalid:?}");
        }
        let over_maximum = format!("DU{}1", "A".repeat(IBKR_PAPER_ACCOUNT_ID_MAX_BYTES - 2));
        assert_eq!(over_maximum.len(), IBKR_PAPER_ACCOUNT_ID_MAX_BYTES + 1);
        assert!(!is_ibkr_paper_account_id(&over_maximum));
    }
}
