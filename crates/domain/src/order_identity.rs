//! Namespaced order identities that keep logical IDs separate from broker-native IDs.
//!
//! Local intent and logical order IDs remain stable domain identities. Native references are
//! qualified by their execution broker, environment, and account namespace before they can be
//! associated with that lineage.
//!
//! # 简体中文
//!
//! 本模块定义带命名空间的订单身份，并将内部逻辑 ID 与券商原生 ID 分开。
//!
//! 本地 IntentId 和 LogicalOrderId 是稳定的领域身份。只有在执行券商、环境和账户命名空间均匹配后，原生引用才能关联到该订单链。

use crate::{
    AccountNamespace, AccountRouteHash, ExecutionBrokerId, ExecutionRoute, IdentifierError,
    IntentId, LogicalOrderId, MetadataSource, OrderId, ProviderMetadataKind, ProviderMetadataRef,
    ProviderUnavailableReason, ProviderValue,
};
use std::fmt;

/// Schwab-native order identifier, distinct from an internal order ID.
/// Schwab 原生订单标识，与内部订单 ID 使用不同类型。
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SchwabNativeOrderId(OrderId);

/// IBKR client identifier supplied on an order chain.
/// IBKR 订单链携带的 clientId。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct IbkrClientId(u32);

/// IBKR session-local order identifier.
/// IBKR session 内的订单标识。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct IbkrOrderId(u32);

/// Stable positive IBKR permanent order identifier.
/// 稳定且为正数的 IBKR permanent order id。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct IbkrPermanentOrderId(u64);

/// Client-supplied IBKR order reference, retained separately from numeric broker IDs.
/// 客户端提供的 IBKR orderRef，与券商数字 ID 分开保存。
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct IbkrOrderRef(OrderId);

/// Stable local identifier for one IBKR client connection generation.
/// 一个 IBKR 客户端连接代次的稳定本地标识。
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct IbkrSessionId(OrderId);

/// Alpaca-native order identifier preserved as an opaque provider value.
/// 作为供应商不透明值保留的 Alpaca 原生订单标识。
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct AlpacaNativeOrderId(crate::ProviderRecordId);

/// Native order identity carried by an execution broker.
/// 执行券商携带的原生订单身份。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum BrokerNativeOrderReference {
    /// Schwab account hash and native order ID.
    /// Schwab accountHash 与原生 orderId。
    Schwab(SchwabNativeOrderReference),
    /// IBKR session, client, order, permanent, and orderRef values kept distinct.
    /// 分别保存 IBKR session、clientId、orderId、permId 和 orderRef。
    InteractiveBrokers(IbkrNativeOrderReference),
    /// Alpaca opaque native order identifier.
    /// Alpaca 不透明原生订单标识。
    Alpaca(AlpacaNativeOrderId),
    /// Native order identifier for a named custom broker.
    /// 已命名自定义券商的原生订单标识。
    Other(OtherNativeOrderReference),
}

/// Schwab-native order identifiers, distinct from local IDs and account scopes.
/// Schwab 原生订单标识，与本地 ID 和账户 scope 分开。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SchwabNativeOrderReference {
    account_hash: AccountRouteHash,
    order_id: SchwabNativeOrderId,
}

/// IBKR identifiers for one order chain across its session generation.
/// 一个 IBKR 订单链在其 session 代次内使用的标识集合。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct IbkrNativeOrderReference {
    session_id: IbkrSessionId,
    client_id: IbkrClientId,
    order_id: IbkrOrderId,
    permanent_id: ProviderValue<IbkrPermanentOrderId>,
    order_ref: ProviderValue<IbkrOrderRef>,
}

/// Native order identifier for an explicitly named custom broker.
/// 显式命名自定义券商的原生订单标识。
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct OtherNativeOrderReference {
    broker: ExecutionBrokerId,
    order_id: crate::ProviderRecordId,
}

/// A native broker order identity qualified by broker, environment, and account.
/// 按券商、环境和账户限定的券商原生订单身份。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ProviderOrderIdentity {
    account_namespace: AccountNamespace,
    native: BrokerNativeOrderReference,
}

/// Reference to an unknown native order value while retaining its account and provider record.
/// 保留账户和供应商来源记录的未知原生订单引用。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct UnresolvedProviderOrderReference {
    account_namespace: AccountNamespace,
    metadata: ProviderMetadataRef,
}

/// State of the broker-native identifier attached to an internal order lineage.
/// 内部订单链关联的券商原生标识状态。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ProviderOrderEvidence {
    /// The native order identifier has not been assigned yet.
    /// 尚未分配原生订单标识。
    NotAssigned,
    /// A broker-native order identity has been qualified.
    /// 已核验出券商原生订单身份。
    Known(ProviderOrderIdentity),
    /// The provider did not make a usable native identifier available.
    /// 供应商未提供可用的原生标识。
    Unavailable(ProviderUnavailableReason),
    /// A source reference is retained, but its native identity remains unresolved.
    /// 已保留来源引用，但原生身份仍未解析。
    Unknown(UnresolvedProviderOrderReference),
}

/// Internal intent and logical order identity fixed to one execution route.
/// 固定到一个执行路由的内部 intent 与逻辑订单身份。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct RoutedOrderIdentity {
    intent_id: IntentId,
    logical_order_id: LogicalOrderId,
    route: ExecutionRoute,
    provider_order: ProviderOrderEvidence,
}

/// Identity construction or namespace validation failure.
/// 订单身份构造或命名空间校验失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrderIdentityError {
    /// Native order identity belongs to another execution broker.
    /// 原生订单身份属于另一个执行券商。
    BrokerMismatch,
    /// An unknown native field reference came from the wrong provider boundary.
    /// 未知原生字段引用来自错误的供应商边界。
    MetadataSourceMismatch,
    /// A provider metadata reference is not an order record.
    /// 供应商元数据引用不是订单记录。
    MetadataKindMismatch,
    /// The custom order identity requires a custom `Other` broker ID.
    /// 自定义订单身份需要 `Other` 券商 ID。
    ExpectedCustomBroker,
    /// The routed order and provider identity have different account namespaces.
    /// 内部路由与供应商订单身份的账户命名空间不同。
    NamespaceMismatch,
    /// An IBKR numeric order ID cannot be negative.
    /// IBKR 数字订单标识不能为负数。
    NegativeIbkrOrderId,
    /// An IBKR numeric order ID exceeds the supported session-local range.
    /// IBKR 数字订单标识超出支持的 session 内范围。
    IbkrOrderIdOutOfRange,
    /// An IBKR permanent order ID must be positive.
    /// IBKR permanent order id 必须为正数。
    NonPositiveIbkrPermanentId,
}

impl SchwabNativeOrderId {
    /// Creates a Schwab-native order ID after common bounded identifier validation.
    /// 按通用有界标识规则校验后创建 Schwab 原生订单 ID。
    pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        OrderId::new(value).map(Self)
    }

    /// Returns the provider ID text for adapter lookup.
    /// 返回供应商 ID 文本，供适配器查找。
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for SchwabNativeOrderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("SchwabNativeOrderId")
            .field(&"[REDACTED]")
            .finish()
    }
}

impl IbkrClientId {
    /// Creates a client ID while preserving the valid zero value used by IBKR sessions.
    /// 创建 clientId，并保留 IBKR session 可使用的零值。
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns the exact client ID.
    /// 返回精确 clientId。
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl IbkrOrderId {
    /// Creates a non-negative IBKR order ID.
    /// 创建非负 IBKR orderId。
    pub fn new(value: i64) -> Result<Self, OrderIdentityError> {
        if value < 0 {
            return Err(OrderIdentityError::NegativeIbkrOrderId);
        }
        u32::try_from(value)
            .map(Self)
            .map_err(|_| OrderIdentityError::IbkrOrderIdOutOfRange)
    }

    /// Returns the exact session-local order ID.
    /// 返回精确的 session 内 orderId。
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl IbkrPermanentOrderId {
    /// Creates a positive IBKR permanent order ID.
    /// 创建正数 IBKR permId。
    pub fn new(value: i64) -> Result<Self, OrderIdentityError> {
        if value <= 0 {
            return Err(OrderIdentityError::NonPositiveIbkrPermanentId);
        }
        Ok(Self(value as u64))
    }

    /// Returns the exact positive permanent order ID.
    /// 返回精确的正数 permanent order id。
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl IbkrOrderRef {
    /// Creates an IBKR order reference using bounded identifier validation.
    /// 按有界标识规则校验后创建 IBKR orderRef。
    pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        OrderId::new(value).map(Self)
    }

    /// Returns the order reference text for explicit adapter use.
    /// 返回 orderRef 文本，供适配器显式使用。
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for IbkrOrderRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("IbkrOrderRef")
            .field(&"[REDACTED]")
            .finish()
    }
}

impl fmt::Debug for IbkrSessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("IbkrSessionId")
            .field(&"[REDACTED]")
            .finish()
    }
}

impl IbkrSessionId {
    /// Creates a stable local session-generation identifier.
    /// 创建稳定的本地 session 代次标识。
    pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        OrderId::new(value).map(Self)
    }
}

impl AlpacaNativeOrderId {
    /// Creates an opaque Alpaca-native order identifier.
    /// 创建 Alpaca 原生订单不透明标识。
    pub const fn new(value: crate::ProviderRecordId) -> Self {
        Self(value)
    }

    /// Returns the native identifier text for adapter lookup.
    /// 返回原生标识文本，供适配器查找。
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for AlpacaNativeOrderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("AlpacaNativeOrderId")
            .field(&"[REDACTED]")
            .finish()
    }
}

impl SchwabNativeOrderReference {
    /// Combines a Schwab routing hash with the provider-native order ID.
    /// 将 Schwab 路由哈希与供应商原生订单 ID 组合。
    pub const fn new(account_hash: AccountRouteHash, order_id: SchwabNativeOrderId) -> Self {
        Self {
            account_hash,
            order_id,
        }
    }

    /// Returns the Schwab account route hash.
    /// 返回 Schwab accountHash。
    pub const fn account_hash(&self) -> &AccountRouteHash {
        &self.account_hash
    }

    /// Returns the Schwab-native order ID.
    /// 返回 Schwab 原生 orderId。
    pub const fn order_id(&self) -> &SchwabNativeOrderId {
        &self.order_id
    }
}

impl IbkrNativeOrderReference {
    /// Creates a typed IBKR identity and checks unresolved references are IBKR order records.
    /// 创建带类型的 IBKR 身份，并校验未知字段引用均来自 IBKR 订单记录。
    pub fn new(
        session_id: IbkrSessionId,
        client_id: IbkrClientId,
        order_id: IbkrOrderId,
        permanent_id: ProviderValue<IbkrPermanentOrderId>,
        order_ref: ProviderValue<IbkrOrderRef>,
    ) -> Result<Self, OrderIdentityError> {
        let expected = MetadataSource::Execution(ExecutionBrokerId::InteractiveBrokers);
        for metadata in [
            provider_metadata(&permanent_id),
            provider_metadata(&order_ref),
        ]
        .into_iter()
        .flatten()
        {
            validate_order_metadata(metadata, &expected)?;
        }
        Ok(Self {
            session_id,
            client_id,
            order_id,
            permanent_id,
            order_ref,
        })
    }

    /// Returns the client session generation.
    /// 返回客户端 session 代次。
    pub const fn session_id(&self) -> &IbkrSessionId {
        &self.session_id
    }

    /// Returns the client identifier.
    /// 返回 clientId。
    pub const fn client_id(&self) -> IbkrClientId {
        self.client_id
    }

    /// Returns the session-local order identifier.
    /// 返回 session 内 orderId。
    pub const fn order_id(&self) -> IbkrOrderId {
        self.order_id
    }

    /// Returns the permanent ID evidence without substituting a missing value.
    /// 返回 permId 证据；缺失值不会被替换。
    pub const fn permanent_id(&self) -> &ProviderValue<IbkrPermanentOrderId> {
        &self.permanent_id
    }

    /// Returns the orderRef evidence without conflating it with numeric order IDs.
    /// 返回 orderRef 证据，不将其与数字订单 ID 混为一谈。
    pub const fn order_ref(&self) -> &ProviderValue<IbkrOrderRef> {
        &self.order_ref
    }
}

impl OtherNativeOrderReference {
    /// Creates an opaque native ID only for an explicitly named custom broker.
    /// 仅为显式命名的自定义券商创建不透明原生 ID。
    pub fn new(
        broker: ExecutionBrokerId,
        order_id: crate::ProviderRecordId,
    ) -> Result<Self, OrderIdentityError> {
        if !matches!(broker, ExecutionBrokerId::Other(_)) {
            return Err(OrderIdentityError::ExpectedCustomBroker);
        }
        Ok(Self { broker, order_id })
    }

    /// Returns the custom broker identity.
    /// 返回自定义券商身份。
    pub const fn broker(&self) -> &ExecutionBrokerId {
        &self.broker
    }

    /// Returns the opaque broker order identifier for adapter lookup.
    /// 返回不透明券商订单标识，供适配器查找。
    pub const fn order_id(&self) -> &crate::ProviderRecordId {
        &self.order_id
    }
}

impl fmt::Debug for OtherNativeOrderReference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OtherNativeOrderReference")
            .field("broker", &self.broker)
            .field("order_id", &"[REDACTED]")
            .finish()
    }
}

impl BrokerNativeOrderReference {
    /// Returns the execution broker associated with this native reference.
    /// 返回此原生引用所属的执行券商。
    pub fn broker(&self) -> ExecutionBrokerId {
        match self {
            Self::Schwab(_) => ExecutionBrokerId::Schwab,
            Self::InteractiveBrokers(_) => ExecutionBrokerId::InteractiveBrokers,
            Self::Alpaca(_) => ExecutionBrokerId::Alpaca,
            Self::Other(value) => value.broker.clone(),
        }
    }
}

impl ProviderOrderIdentity {
    /// Creates a native identity only when its broker matches the account namespace.
    /// 仅当原生身份的券商与账户命名空间匹配时创建身份。
    pub fn new(
        account_namespace: AccountNamespace,
        native: BrokerNativeOrderReference,
    ) -> Result<Self, OrderIdentityError> {
        if account_namespace.broker() != &native.broker() {
            return Err(OrderIdentityError::BrokerMismatch);
        }
        Ok(Self {
            account_namespace,
            native,
        })
    }

    /// Returns the exact broker, environment, and account namespace.
    /// 返回精确的券商、环境和账户命名空间。
    pub const fn account_namespace(&self) -> &AccountNamespace {
        &self.account_namespace
    }

    /// Returns the provider-specific native order fields.
    /// 返回供应商专属原生订单字段。
    pub const fn native(&self) -> &BrokerNativeOrderReference {
        &self.native
    }
}

impl UnresolvedProviderOrderReference {
    /// Retains an order record reference when a native order value is not understood.
    /// 当原生订单值无法解释时保留订单记录引用。
    pub fn new(
        account_namespace: AccountNamespace,
        metadata: ProviderMetadataRef,
    ) -> Result<Self, OrderIdentityError> {
        let expected = MetadataSource::Execution(account_namespace.broker().clone());
        validate_order_metadata(&metadata, &expected)?;
        Ok(Self {
            account_namespace,
            metadata,
        })
    }

    /// Returns the namespace to which this unresolved order record belongs.
    /// 返回此未解析订单记录所属的命名空间。
    pub const fn account_namespace(&self) -> &AccountNamespace {
        &self.account_namespace
    }

    /// Returns the opaque provider record reference.
    /// 返回供应商不透明记录引用。
    pub const fn metadata(&self) -> &ProviderMetadataRef {
        &self.metadata
    }
}

impl RoutedOrderIdentity {
    /// Associates internal IDs and provider evidence with one fixed execution route.
    /// 将内部 ID 与供应商证据关联到一个固定执行路由。
    pub fn new(
        intent_id: IntentId,
        logical_order_id: LogicalOrderId,
        route: ExecutionRoute,
        provider_order: ProviderOrderEvidence,
    ) -> Result<Self, OrderIdentityError> {
        match &provider_order {
            ProviderOrderEvidence::Known(identity)
                if identity.account_namespace() != route.account_namespace() =>
            {
                return Err(OrderIdentityError::NamespaceMismatch);
            }
            ProviderOrderEvidence::Unknown(identity)
                if identity.account_namespace() != route.account_namespace() =>
            {
                return Err(OrderIdentityError::NamespaceMismatch);
            }
            _ => {}
        }
        Ok(Self {
            intent_id,
            logical_order_id,
            route,
            provider_order,
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

    /// Returns the route fixed to the strategy instance.
    /// 返回固定到策略实例的执行路由。
    pub const fn route(&self) -> &ExecutionRoute {
        &self.route
    }

    /// Returns native-order evidence without treating unavailable or unknown as success.
    /// 返回原生订单证据；不可用或未知状态不会被当作成功。
    pub const fn provider_order(&self) -> &ProviderOrderEvidence {
        &self.provider_order
    }
}

fn provider_metadata<T>(value: &ProviderValue<T>) -> Option<&ProviderMetadataRef> {
    match value {
        ProviderValue::Unknown(metadata) => Some(metadata),
        ProviderValue::Known(_) | ProviderValue::Unavailable(_) => None,
    }
}

fn validate_order_metadata(
    metadata: &ProviderMetadataRef,
    expected_source: &MetadataSource,
) -> Result<(), OrderIdentityError> {
    if metadata.source() != expected_source {
        return Err(OrderIdentityError::MetadataSourceMismatch);
    }
    if metadata.kind() != ProviderMetadataKind::Order {
        return Err(OrderIdentityError::MetadataKindMismatch);
    }
    Ok(())
}

impl fmt::Display for OrderIdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::BrokerMismatch => "ORDER_IDENTITY_BROKER_MISMATCH",
            Self::MetadataSourceMismatch => "ORDER_IDENTITY_METADATA_SOURCE_MISMATCH",
            Self::MetadataKindMismatch => "ORDER_IDENTITY_METADATA_KIND_MISMATCH",
            Self::ExpectedCustomBroker => "ORDER_IDENTITY_EXPECTED_CUSTOM_BROKER",
            Self::NamespaceMismatch => "ORDER_IDENTITY_NAMESPACE_MISMATCH",
            Self::NegativeIbkrOrderId => "IBKR_ORDER_ID_MUST_BE_NON_NEGATIVE",
            Self::IbkrOrderIdOutOfRange => "IBKR_ORDER_ID_OUT_OF_RANGE",
            Self::NonPositiveIbkrPermanentId => "IBKR_PERMANENT_ORDER_ID_MUST_BE_POSITIVE",
        })
    }
}

impl std::error::Error for OrderIdentityError {}

#[cfg(test)]
mod tests {
    use super::{
        BrokerNativeOrderReference, IbkrClientId, IbkrNativeOrderReference, IbkrOrderId,
        IbkrOrderRef, IbkrPermanentOrderId, IbkrSessionId, OrderIdentityError,
        ProviderOrderEvidence, ProviderOrderIdentity, RoutedOrderIdentity, SchwabNativeOrderId,
        SchwabNativeOrderReference, UnresolvedProviderOrderReference,
    };
    use crate::{
        AccountNamespace, AccountRouteHash, AccountScope, BrokerEnvironment, ExecutionBrokerId,
        ExecutionRoute, IntentId, LogicalOrderId, MarketDataProviderId, MetadataSource,
        ProviderMetadataKind, ProviderMetadataRef, ProviderRecordId, ProviderUnavailableReason,
        ProviderValue, StrategyInstanceId,
    };

    fn account_namespace(broker: ExecutionBrokerId) -> AccountNamespace {
        AccountNamespace::new(
            broker,
            BrokerEnvironment::Paper,
            AccountScope::new("synthetic-native-account").unwrap(),
        )
    }

    fn route(broker: ExecutionBrokerId) -> ExecutionRoute {
        ExecutionRoute::new(
            StrategyInstanceId::new("synthetic-strategy-order-route").unwrap(),
            account_namespace(broker),
        )
    }

    fn execution_metadata(
        broker: ExecutionBrokerId,
        kind: ProviderMetadataKind,
    ) -> ProviderMetadataRef {
        ProviderMetadataRef::new(
            MetadataSource::Execution(broker),
            kind,
            ProviderRecordId::new(format!("synthetic-order-evidence-{kind:?}")).unwrap(),
        )
    }

    fn ibkr_native(session: &str, order_id: u32) -> BrokerNativeOrderReference {
        BrokerNativeOrderReference::InteractiveBrokers(
            IbkrNativeOrderReference::new(
                IbkrSessionId::new(session).unwrap(),
                IbkrClientId::new(7),
                IbkrOrderId::new(i64::from(order_id)).unwrap(),
                ProviderValue::Unavailable(ProviderUnavailableReason::NotReported),
                ProviderValue::Unavailable(ProviderUnavailableReason::NotReported),
            )
            .unwrap(),
        )
    }

    #[test]
    fn identical_numeric_order_ids_are_distinct_across_brokers() {
        let schwab = ProviderOrderIdentity::new(
            account_namespace(ExecutionBrokerId::Schwab),
            BrokerNativeOrderReference::Schwab(SchwabNativeOrderReference::new(
                AccountRouteHash::new("synthetic-schwab-route").unwrap(),
                SchwabNativeOrderId::new("8712").unwrap(),
            )),
        )
        .unwrap();
        let ibkr = ProviderOrderIdentity::new(
            account_namespace(ExecutionBrokerId::InteractiveBrokers),
            ibkr_native("ibkr-session-a", 8712),
        )
        .unwrap();
        assert_ne!(schwab, ibkr);
    }

    #[test]
    fn reused_ibkr_client_and_order_ids_are_scoped_to_session_generation() {
        let first = ProviderOrderIdentity::new(
            account_namespace(ExecutionBrokerId::InteractiveBrokers),
            ibkr_native("ibkr-session-a", 42),
        )
        .unwrap();
        let second = ProviderOrderIdentity::new(
            account_namespace(ExecutionBrokerId::InteractiveBrokers),
            ibkr_native("ibkr-session-b", 42),
        )
        .unwrap();
        let reused_client = ProviderOrderIdentity::new(
            account_namespace(ExecutionBrokerId::InteractiveBrokers),
            ibkr_native("ibkr-session-a", 43),
        )
        .unwrap();
        assert_ne!(first, second);
        assert_ne!(first, reused_client);
    }

    #[test]
    fn ibkr_order_id_validation_distinguishes_negative_from_out_of_range() {
        assert_eq!(
            IbkrOrderId::new(-1),
            Err(OrderIdentityError::NegativeIbkrOrderId)
        );
        assert_eq!(
            IbkrOrderId::new(i64::from(u32::MAX) + 1),
            Err(OrderIdentityError::IbkrOrderIdOutOfRange)
        );
        assert_eq!(
            IbkrOrderId::new(i64::from(u32::MAX)).unwrap().get(),
            u32::MAX
        );
    }

    #[test]
    fn ibkr_client_order_perm_and_order_ref_fields_remain_distinct() {
        let metadata = ProviderMetadataRef::new(
            MetadataSource::Execution(ExecutionBrokerId::InteractiveBrokers),
            ProviderMetadataKind::Order,
            ProviderRecordId::new("synthetic-ibkr-order-record").unwrap(),
        );
        let base = IbkrNativeOrderReference::new(
            IbkrSessionId::new("ibkr-session-fields").unwrap(),
            IbkrClientId::new(5),
            IbkrOrderId::new(18).unwrap(),
            ProviderValue::Known(IbkrPermanentOrderId::new(991).unwrap()),
            ProviderValue::Known(IbkrOrderRef::new("strategy-order-tag").unwrap()),
        )
        .unwrap();
        let no_perm = IbkrNativeOrderReference::new(
            IbkrSessionId::new("ibkr-session-fields").unwrap(),
            IbkrClientId::new(5),
            IbkrOrderId::new(18).unwrap(),
            ProviderValue::Unavailable(ProviderUnavailableReason::NotReported),
            ProviderValue::Known(IbkrOrderRef::new("strategy-order-tag").unwrap()),
        )
        .unwrap();
        let unknown_ref = IbkrNativeOrderReference::new(
            IbkrSessionId::new("ibkr-session-fields").unwrap(),
            IbkrClientId::new(5),
            IbkrOrderId::new(18).unwrap(),
            ProviderValue::Known(IbkrPermanentOrderId::new(991).unwrap()),
            ProviderValue::Unknown(metadata),
        )
        .unwrap();
        assert_ne!(base, no_perm);
        assert_ne!(base, unknown_ref);
        assert_eq!(base.client_id().get(), 5);
        assert_eq!(base.order_id().get(), 18);
        assert_eq!(
            base.permanent_id(),
            &ProviderValue::Known(IbkrPermanentOrderId::new(991).unwrap())
        );
        assert!(matches!(base.order_ref(), ProviderValue::Known(_)));
    }

    #[test]
    fn ibkr_order_reference_accepts_only_ibkr_order_metadata() {
        for kind in [
            ProviderMetadataKind::Account,
            ProviderMetadataKind::Contract,
            ProviderMetadataKind::MarketData,
        ] {
            assert_eq!(
                IbkrNativeOrderReference::new(
                    IbkrSessionId::new("ibkr-session-kind-check").unwrap(),
                    IbkrClientId::new(5),
                    IbkrOrderId::new(18).unwrap(),
                    ProviderValue::Unavailable(ProviderUnavailableReason::NotReported),
                    ProviderValue::Unknown(execution_metadata(
                        ExecutionBrokerId::InteractiveBrokers,
                        kind,
                    )),
                ),
                Err(OrderIdentityError::MetadataKindMismatch),
                "IBKR {kind:?} metadata is not evidence for an order"
            );
        }

        let accepted = IbkrNativeOrderReference::new(
            IbkrSessionId::new("ibkr-session-kind-check").unwrap(),
            IbkrClientId::new(5),
            IbkrOrderId::new(18).unwrap(),
            ProviderValue::Unavailable(ProviderUnavailableReason::NotReported),
            ProviderValue::Unknown(execution_metadata(
                ExecutionBrokerId::InteractiveBrokers,
                ProviderMetadataKind::Order,
            )),
        );
        assert!(accepted.is_ok());
    }

    #[test]
    fn broker_and_route_namespace_mismatches_fail_closed() {
        let schwab_native = BrokerNativeOrderReference::Schwab(SchwabNativeOrderReference::new(
            AccountRouteHash::new("synthetic-route").unwrap(),
            SchwabNativeOrderId::new("17").unwrap(),
        ));
        assert_eq!(
            ProviderOrderIdentity::new(
                account_namespace(ExecutionBrokerId::InteractiveBrokers),
                schwab_native,
            ),
            Err(OrderIdentityError::BrokerMismatch)
        );

        let other_namespace = account_namespace(ExecutionBrokerId::Schwab);
        let identity = ProviderOrderIdentity::new(
            other_namespace,
            BrokerNativeOrderReference::Schwab(SchwabNativeOrderReference::new(
                AccountRouteHash::new("synthetic-route").unwrap(),
                SchwabNativeOrderId::new("17").unwrap(),
            )),
        )
        .unwrap();
        assert_eq!(
            RoutedOrderIdentity::new(
                IntentId::new("intent-17").unwrap(),
                LogicalOrderId::new("logical-17").unwrap(),
                route(ExecutionBrokerId::InteractiveBrokers),
                ProviderOrderEvidence::Known(identity),
            ),
            Err(OrderIdentityError::NamespaceMismatch)
        );
    }

    #[test]
    fn unknown_provider_order_keeps_logical_ids_and_source_record_separate() {
        let namespace = account_namespace(ExecutionBrokerId::Schwab);
        let metadata = ProviderMetadataRef::new(
            MetadataSource::Execution(ExecutionBrokerId::Schwab),
            ProviderMetadataKind::Order,
            ProviderRecordId::new("synthetic-unknown-order-5531").unwrap(),
        );
        let unknown = UnresolvedProviderOrderReference::new(namespace.clone(), metadata).unwrap();
        let route = ExecutionRoute::new(
            StrategyInstanceId::new("strategy-unknown-order").unwrap(),
            namespace,
        );
        let identity = RoutedOrderIdentity::new(
            IntentId::new("intent-5531").unwrap(),
            LogicalOrderId::new("logical-5531").unwrap(),
            route,
            ProviderOrderEvidence::Unknown(unknown),
        )
        .unwrap();
        assert_eq!(identity.intent_id().as_str(), "intent-5531");
        assert_eq!(identity.logical_order_id().as_str(), "logical-5531");
        assert!(format!("{identity:?}").contains("REDACTED"));
        assert!(matches!(
            identity.provider_order(),
            ProviderOrderEvidence::Unknown(reference)
                if reference.metadata().record_id().as_str() == "synthetic-unknown-order-5531"
                    && reference.account_namespace() == identity.route().account_namespace()
        ));
    }

    #[test]
    fn numeric_id_validation_does_not_coerce_unknown_or_negative_values() {
        assert_eq!(
            IbkrOrderId::new(-1),
            Err(OrderIdentityError::NegativeIbkrOrderId)
        );
        assert_eq!(
            IbkrPermanentOrderId::new(0),
            Err(OrderIdentityError::NonPositiveIbkrPermanentId)
        );
        assert_eq!(
            IbkrOrderId::new(0).unwrap().get(),
            0,
            "session-local order ID zero is distinct from unavailable metadata"
        );
    }

    #[test]
    fn unresolved_metadata_from_a_market_feed_is_not_an_execution_order_identity() {
        let namespace = account_namespace(ExecutionBrokerId::Schwab);
        let wrong_source = ProviderMetadataRef::new(
            MetadataSource::MarketData(MarketDataProviderId::Schwab),
            ProviderMetadataKind::Order,
            ProviderRecordId::new("synthetic-market-record").unwrap(),
        );
        assert_eq!(
            UnresolvedProviderOrderReference::new(namespace, wrong_source),
            Err(OrderIdentityError::MetadataSourceMismatch)
        );
    }

    #[test]
    fn unresolved_provider_order_reference_requires_order_metadata_kind() {
        let namespace = account_namespace(ExecutionBrokerId::Schwab);
        for kind in [
            ProviderMetadataKind::Account,
            ProviderMetadataKind::Contract,
            ProviderMetadataKind::MarketData,
        ] {
            assert_eq!(
                UnresolvedProviderOrderReference::new(
                    namespace.clone(),
                    execution_metadata(ExecutionBrokerId::Schwab, kind),
                ),
                Err(OrderIdentityError::MetadataKindMismatch),
                "Schwab {kind:?} metadata is not evidence for an order"
            );
        }

        let accepted = UnresolvedProviderOrderReference::new(
            namespace,
            execution_metadata(ExecutionBrokerId::Schwab, ProviderMetadataKind::Order),
        )
        .unwrap();
        assert_eq!(accepted.metadata().kind(), ProviderMetadataKind::Order);
    }
}
