//! Default-deny policy types for automated order operation classes.
//!
//! These values classify intent and capability policy only. They do not grant
//! account authority or authorize transport.
//!
//! # 简体中文
//!
//! 定义自动订单操作分类及默认拒绝的能力策略类型。
//!
//! 这些值仅用于分类意图和能力策略，不会授予账户权威或传输权限。

/// One explicitly configured category of automated order operation.
/// 一类显式配置的自动订单操作能力。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum AutomationCapability {
    /// Replace an already-owned opening order.
    /// 替换已经确认归属本策略的开仓订单。
    OpeningRefresh,
    /// Submit an opening order derived from a durable confirmed fill.
    /// 提交由持久化确认成交记录派生的开仓订单。
    Replenishment,
    /// Submit, replace, or cancel a risk-reducing exit order.
    /// 提交、替换或撤销降低风险的平仓订单。
    ExitManagement,
}

/// Immutable classification of the automated order intent's business purpose.
/// 自动订单意图业务目的的不可变分类。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum OperationClass {
    /// Replace an already-owned opening order; this class never submits an opening order.
    /// 替换已确认归属本策略的开仓订单；此分类绝不提交开仓订单。
    OpeningRefreshReplace,
    /// Submit an opening replenishment derived from a durable confirmed fill.
    /// 提交由持久化确认成交记录派生的补仓开仓订单。
    ReplenishmentSubmit,
    /// Submit a risk-reducing exit order.
    /// 提交降低风险的平仓订单。
    ExitSubmit,
    /// Replace a risk-reducing exit order.
    /// 替换降低风险的平仓订单。
    ExitReplace,
    /// Cancel a risk-reducing exit order.
    /// 撤销降低风险的平仓订单。
    ExitCancel,
}

impl OperationClass {
    /// Returns the single capability required by this operation class.
    /// 返回此操作分类唯一要求的能力。
    pub const fn required_capability(self) -> AutomationCapability {
        match self {
            Self::OpeningRefreshReplace => AutomationCapability::OpeningRefresh,
            Self::ReplenishmentSubmit => AutomationCapability::Replenishment,
            Self::ExitSubmit | Self::ExitReplace | Self::ExitCancel => {
                AutomationCapability::ExitManagement
            }
        }
    }
}

/// A normalized immutable set of automated operation capabilities.
/// 归一化且不可变的自动操作能力集合。
///
/// A capability permits only its matching operation classes; it does not prove
/// ownership, risk, inventory, writer authority, or final transport admission.
/// 能力仅允许对应的操作分类；它不证明订单归属、风险、库存、写入者权威或最终传输准入。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct AutomationCapabilities {
    opening_refresh: bool,
    replenishment: bool,
    exit_management: bool,
}

impl AutomationCapabilities {
    /// Constructs a capability set from its three explicit opt-in values.
    /// 根据三个显式启用值构造能力集合。
    pub const fn new(opening_refresh: bool, replenishment: bool, exit_management: bool) -> Self {
        Self {
            opening_refresh,
            replenishment,
            exit_management,
        }
    }

    /// Returns whether the selected capability is explicitly enabled.
    /// 返回所选能力是否已显式启用。
    pub const fn is_enabled(self, capability: AutomationCapability) -> bool {
        match capability {
            AutomationCapability::OpeningRefresh => self.opening_refresh,
            AutomationCapability::Replenishment => self.replenishment,
            AutomationCapability::ExitManagement => self.exit_management,
        }
    }

    /// Returns whether this set permits the operation class at the capability layer.
    /// 返回此能力集合是否在能力层允许该操作分类。
    ///
    /// This policy result is not a broker-write authorization or a substitute for
    /// current authority and final revalidation.
    /// 此策略结果不是券商写入授权，也不能替代当前权威状态和最终复验。
    pub const fn allows(self, operation: OperationClass) -> bool {
        self.is_enabled(operation.required_capability())
    }

    /// Returns the code-owned all-disabled capability set.
    /// 返回代码默认的全部禁用能力集合。
    pub const fn all_disabled() -> Self {
        Self::new(false, false, false)
    }
}

impl Default for AutomationCapabilities {
    /// Defaults every automation capability to disabled.
    /// 默认禁用全部自动化能力。
    fn default() -> Self {
        Self::all_disabled()
    }
}

#[cfg(test)]
mod tests {
    use super::{AutomationCapabilities, AutomationCapability, OperationClass};

    const OPERATIONS: [OperationClass; 5] = [
        OperationClass::OpeningRefreshReplace,
        OperationClass::ReplenishmentSubmit,
        OperationClass::ExitSubmit,
        OperationClass::ExitReplace,
        OperationClass::ExitCancel,
    ];

    #[test]
    fn every_capability_combination_allows_only_its_operation_classes() {
        for bits in 0_u8..8 {
            let capabilities = AutomationCapabilities::new(
                bits & 0b001 != 0,
                bits & 0b010 != 0,
                bits & 0b100 != 0,
            );
            for operation in OPERATIONS {
                let expected = match operation {
                    OperationClass::OpeningRefreshReplace => bits & 0b001 != 0,
                    OperationClass::ReplenishmentSubmit => bits & 0b010 != 0,
                    OperationClass::ExitSubmit
                    | OperationClass::ExitReplace
                    | OperationClass::ExitCancel => bits & 0b100 != 0,
                };
                assert_eq!(
                    capabilities.allows(operation),
                    expected,
                    "bits={bits:03b} operation={operation:?}"
                );
            }
        }
    }

    #[test]
    fn capabilities_default_to_no_mutation_classes() {
        let capabilities = AutomationCapabilities::default();
        assert!(
            OPERATIONS
                .into_iter()
                .all(|operation| !capabilities.allows(operation))
        );
        assert!(!capabilities.is_enabled(AutomationCapability::OpeningRefresh));
        assert!(!capabilities.is_enabled(AutomationCapability::Replenishment));
        assert!(!capabilities.is_enabled(AutomationCapability::ExitManagement));
    }
}
