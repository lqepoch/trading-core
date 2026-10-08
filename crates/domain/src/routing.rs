//! Stable account namespaces and strategy-bound execution routes.
//!
//! An execution route binds one strategy instance to one broker, environment, and account scope.
//! It is descriptive domain data and grants no transport or order-submission capability.
//!
//! # 简体中文
//!
//! 本模块定义稳定账户命名空间以及绑定策略实例的执行路由。
//!
//! 执行路由将一个策略实例绑定到一个券商、环境和账户 scope。它只是领域数据，不授予传输或订单提交权限。

use crate::{AccountScope, BrokerEnvironment, ExecutionBrokerId, StrategyInstanceId};

/// Fully qualified execution account identity used to partition funds and order state.
/// 用于隔离资金与订单状态的完整执行账户身份。
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct AccountNamespace {
    broker: ExecutionBrokerId,
    environment: BrokerEnvironment,
    account: AccountScope,
}

/// Immutable route captured by one strategy instance.
/// 由一个策略实例固定持有的不可变路由。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ExecutionRoute {
    strategy_instance_id: StrategyInstanceId,
    account_namespace: AccountNamespace,
}

impl AccountNamespace {
    /// Creates a namespace from the execution broker, account environment, and local account scope.
    /// 根据执行券商、账户环境和本地账户 scope 创建命名空间。
    pub const fn new(
        broker: ExecutionBrokerId,
        environment: BrokerEnvironment,
        account: AccountScope,
    ) -> Self {
        Self {
            broker,
            environment,
            account,
        }
    }

    /// Returns the execution broker that owns this account namespace.
    /// 返回拥有此账户命名空间的执行券商。
    pub const fn broker(&self) -> &ExecutionBrokerId {
        &self.broker
    }

    /// Returns the logical paper or live environment.
    /// 返回逻辑 paper 或 live 环境。
    pub const fn environment(&self) -> BrokerEnvironment {
        self.environment
    }

    /// Returns the internal account partition identity.
    /// 返回内部账户分区身份。
    pub const fn account_scope(&self) -> &AccountScope {
        &self.account
    }
}

impl ExecutionRoute {
    /// Captures a fixed account namespace for one strategy instance.
    /// 为一个策略实例固定账户命名空间。
    pub const fn new(
        strategy_instance_id: StrategyInstanceId,
        account_namespace: AccountNamespace,
    ) -> Self {
        Self {
            strategy_instance_id,
            account_namespace,
        }
    }

    /// Returns the strategy instance whose order lineage uses this route.
    /// 返回使用此路由的策略实例。
    pub const fn strategy_instance_id(&self) -> &StrategyInstanceId {
        &self.strategy_instance_id
    }

    /// Returns the immutable broker, environment, and account namespace.
    /// 返回不可变的券商、环境和账户命名空间。
    pub const fn account_namespace(&self) -> &AccountNamespace {
        &self.account_namespace
    }
}

#[cfg(test)]
mod tests {
    use super::{AccountNamespace, ExecutionRoute};
    use crate::{AccountScope, BrokerEnvironment, ExecutionBrokerId, StrategyInstanceId};

    fn account_namespace(
        broker: ExecutionBrokerId,
        environment: BrokerEnvironment,
    ) -> AccountNamespace {
        AccountNamespace::new(
            broker,
            environment,
            AccountScope::new("same-operator-account-label").unwrap(),
        )
    }

    #[test]
    fn broker_and_environment_are_part_of_account_identity() {
        let paper = account_namespace(ExecutionBrokerId::Schwab, BrokerEnvironment::Paper);
        let live = account_namespace(ExecutionBrokerId::Schwab, BrokerEnvironment::Live);
        let other_broker = account_namespace(
            ExecutionBrokerId::InteractiveBrokers,
            BrokerEnvironment::Paper,
        );
        assert_ne!(paper, live);
        assert_ne!(paper, other_broker);
        assert_eq!(paper.account_scope(), live.account_scope());
    }

    #[test]
    fn execution_route_captures_strategy_identity_and_account_namespace() {
        let namespace = account_namespace(ExecutionBrokerId::Schwab, BrokerEnvironment::Paper);
        let strategy = StrategyInstanceId::new("synthetic-strategy-001").unwrap();
        let route = ExecutionRoute::new(strategy.clone(), namespace.clone());
        assert_eq!(route.strategy_instance_id(), &strategy);
        assert_eq!(route.account_namespace(), &namespace);
        assert!(!format!("{namespace:?}").contains("same-operator-account-label"));
    }
}
