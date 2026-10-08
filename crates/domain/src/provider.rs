//! Provider identities and explicit evidence states for broker-neutral values.
//!
//! Provider identifiers describe data provenance or execution ownership separately. This module
//! contains no SDK DTOs and does not authorize requests or broker mutations.
//!
//! # 简体中文
//!
//! 本模块为券商中立值提供行情来源身份、执行券商身份和显式证据状态。
//!
//! 行情来源与执行归属使用不同类型。本模块不包含 SDK DTO，也不授予请求或券商写入权限。

use crate::{IdentifierError, OrderId};
use std::fmt;

/// Identifies the source that supplied market data.
/// 标识提供行情数据的来源。
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum MarketDataProviderId {
    /// Alpaca market-data source.
    /// Alpaca 行情来源。
    Alpaca,
    /// Schwab market-data source.
    /// Schwab 行情来源。
    Schwab,
    /// Interactive Brokers market-data source.
    /// Interactive Brokers 行情来源。
    InteractiveBrokers,
    /// An explicitly named future or operator-defined source.
    /// 显式命名的未来来源或运维方定义来源。
    Other(ProviderCode),
}

/// Identifies the broker that owns an execution account and order route.
/// 标识执行账户和订单路由所属的券商。
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ExecutionBrokerId {
    /// Alpaca execution system.
    /// Alpaca 执行系统。
    Alpaca,
    /// Schwab execution system.
    /// Schwab 执行系统。
    Schwab,
    /// Interactive Brokers execution system.
    /// Interactive Brokers 执行系统。
    InteractiveBrokers,
    /// An explicitly named future or operator-defined broker.
    /// 显式命名的未来券商或运维方定义券商。
    Other(ProviderCode),
}

/// Logical account environment selected for a fixed execution route.
/// 固定执行路由选择的逻辑账户环境。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum BrokerEnvironment {
    /// Broker paper or simulated account environment.
    /// 券商 paper 或模拟账户环境。
    Paper,
    /// Broker live account environment.
    /// 券商 live 账户环境。
    Live,
}

/// The domain boundary that owns a provider metadata record.
/// 拥有供应商元数据记录的领域边界。
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum MetadataSource {
    /// A market-data provider supplied the record.
    /// 记录由行情来源提供。
    MarketData(MarketDataProviderId),
    /// An execution broker supplied the record.
    /// 记录由执行券商提供。
    Execution(ExecutionBrokerId),
}

/// Kind of provider record referenced by broker-neutral evidence.
/// 券商中立证据引用的供应商记录类型。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ProviderMetadataKind {
    /// Qualified or candidate contract record.
    /// 已核验或候选合约记录。
    Contract,
    /// Quote or market-data record.
    /// 报价或行情记录。
    MarketData,
    /// Broker order record.
    /// 券商订单记录。
    Order,
    /// Broker account record.
    /// 券商账户记录。
    Account,
}

/// Why an upstream value is unavailable without treating it as zero or success.
/// 上游值不可用的原因；不得将其视为零或成功。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ProviderUnavailableReason {
    /// The provider response omitted this field.
    /// 供应商响应中没有此字段。
    NotReported,
    /// The configured feed or account is not entitled to this field.
    /// 当前 feed 或账户没有此字段的权限。
    NotEntitled,
    /// The provider protocol does not support this field.
    /// 供应商协议不支持此字段。
    Unsupported,
    /// The field could not be validated without discarding source evidence.
    /// 无法在保留来源证据的前提下校验此字段。
    Invalid,
}

/// Explicitly identifies a value as known, unavailable, or unresolved at its source.
/// 显式标识一个值为已知、不可用或来源尚未解析。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ProviderValue<T> {
    /// A value validated from provider evidence.
    /// 已从供应商证据校验出的值。
    Known(T),
    /// A value that the source did not provide or cannot provide.
    /// 来源未提供或无法提供的值。
    Unavailable(ProviderUnavailableReason),
    /// A value whose field or encoding is unknown; the source record remains addressable.
    /// 字段或编码未知的值；仍保留对来源记录的引用。
    Unknown(ProviderMetadataRef),
}

/// Bounded non-empty provider code for an explicitly identified extension source.
/// 显式扩展来源使用的有界非空供应商代码。
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ProviderCode(OrderId);

/// Opaque provider record identifier whose diagnostic representation is redacted.
/// 供应商记录的不透明标识；诊断输出会对其脱敏。
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ProviderRecordId(OrderId);

/// Reference to a provider record without importing provider-owned data transfer objects.
/// 对供应商记录的引用，不引入供应商定义的数据传输对象。
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct ProviderMetadataRef {
    source: MetadataSource,
    kind: ProviderMetadataKind,
    record_id: ProviderRecordId,
}

impl ProviderCode {
    /// Creates a bounded custom provider code.
    /// 创建有界的自定义供应商代码。
    pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        OrderId::new(value).map(Self)
    }

    /// Returns the canonical provider code text.
    /// 返回规范供应商代码文本。
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for ProviderCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ProviderCode")
            .field(&self.0)
            .finish()
    }
}

impl fmt::Display for ProviderCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.as_str())
    }
}

impl ProviderRecordId {
    /// Creates an opaque, bounded provider record identifier.
    /// 创建有界的供应商记录不透明标识。
    pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        OrderId::new(value).map(Self)
    }

    /// Returns the provider identifier text for explicit adapter use.
    /// 返回供应商标识文本，供适配器显式使用。
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for ProviderRecordId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ProviderRecordId")
            .field(&"[REDACTED]")
            .finish()
    }
}

impl ProviderMetadataRef {
    /// Creates a typed reference to one provider-owned record.
    /// 创建一个带类型的供应商记录引用。
    pub const fn new(
        source: MetadataSource,
        kind: ProviderMetadataKind,
        record_id: ProviderRecordId,
    ) -> Self {
        Self {
            source,
            kind,
            record_id,
        }
    }

    /// Returns the provider boundary that supplied the record.
    /// 返回提供该记录的供应商边界。
    pub const fn source(&self) -> &MetadataSource {
        &self.source
    }

    /// Returns the kind of provider record.
    /// 返回供应商记录类型。
    pub const fn kind(&self) -> ProviderMetadataKind {
        self.kind
    }

    /// Returns the provider record identifier for adapter lookup.
    /// 返回供应商记录标识，供适配器查找。
    pub const fn record_id(&self) -> &ProviderRecordId {
        &self.record_id
    }
}

impl fmt::Debug for ProviderMetadataRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderMetadataRef")
            .field("source", &self.source)
            .field("kind", &self.kind)
            .field("record_id", &"[REDACTED]")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ExecutionBrokerId, MarketDataProviderId, MetadataSource, ProviderCode,
        ProviderMetadataKind, ProviderMetadataRef, ProviderRecordId, ProviderUnavailableReason,
        ProviderValue,
    };

    #[test]
    fn market_data_and_execution_ids_are_separate_domain_types() {
        let market = MarketDataProviderId::Alpaca;
        let execution = ExecutionBrokerId::Schwab;
        assert_eq!(market, MarketDataProviderId::Alpaca);
        assert_eq!(execution, ExecutionBrokerId::Schwab);
        assert_ne!(format!("{market:?}"), format!("{execution:?}"));
    }

    #[test]
    fn unknown_metadata_is_addressable_but_record_ids_are_redacted() {
        let metadata = ProviderMetadataRef::new(
            MetadataSource::MarketData(MarketDataProviderId::Alpaca),
            ProviderMetadataKind::Contract,
            ProviderRecordId::new("synthetic-native-contract-9841").unwrap(),
        );
        let value: ProviderValue<u64> = ProviderValue::Unknown(metadata.clone());
        assert_eq!(metadata.kind(), ProviderMetadataKind::Contract);
        assert!(format!("{metadata:?}").contains("REDACTED"));
        assert!(!format!("{metadata:?}").contains("9841"));
        assert!(matches!(
            value,
            ProviderValue::Unknown(ProviderMetadataRef { .. })
        ));
    }

    #[test]
    fn custom_provider_codes_and_unavailability_are_explicit() {
        let code = ProviderCode::new("synthetic-feed").unwrap();
        assert_eq!(code.as_str(), "synthetic-feed");
        let unavailable: ProviderValue<u32> =
            ProviderValue::Unavailable(ProviderUnavailableReason::NotReported);
        assert_eq!(
            unavailable,
            ProviderValue::Unavailable(ProviderUnavailableReason::NotReported)
        );
    }
}
