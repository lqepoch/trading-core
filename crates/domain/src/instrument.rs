//! Fully qualified option contract keys and evidence-preserving candidates.
//!
//! `OptionSymbol` parsing only creates candidate identity. An `InstrumentKey` exists only after
//! the trading class, currency, multiplier, deliverable, exercise style, and settlement type are
//! all known and validated.
//!
//! # 简体中文
//!
//! 本模块定义完整核验的期权合约键和保留来源证据的候选对象。
//!
//! 解析 `OptionSymbol` 只能生成候选身份。只有 trading class、currency、multiplier、deliverable、exercise style 和 settlement type 均已知并通过校验后，才能创建 `InstrumentKey`。

use crate::{
    ContractCurrency, ContractMultiplier, Deliverable, IdentifierError, OptionContract,
    OptionRight, OptionSymbol, ProviderMetadataRef, ProviderUnavailableReason, ProviderValue,
    Strike, Underlying,
};
use std::fmt;

/// Canonical trading class used as part of an option contract identity.
/// 作为期权合约身份组成部分的规范 trading class。
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct TradingClass(String);

/// Exercise terms required to distinguish option contract identity.
/// 用于区分期权合约身份的行权条款。
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum OptionExerciseStyle {
    /// American exercise.
    /// 美式行权。
    American,
    /// European exercise.
    /// 欧式行权。
    European,
    /// Bermudan exercise.
    /// 百慕大式行权。
    Bermudan,
    /// Explicitly named provider-specific exercise style.
    /// 显式命名的供应商专属行权方式。
    Other(crate::ProviderCode),
}

/// Settlement terms required to distinguish option contract identity.
/// 用于区分期权合约身份的结算条款。
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum OptionSettlementType {
    /// Physical settlement.
    /// 实物交割。
    Physical,
    /// Cash settlement.
    /// 现金结算。
    Cash,
    /// Explicitly named provider-specific settlement type.
    /// 显式命名的供应商专属结算类型。
    Other(crate::ProviderCode),
}

/// One contract term whose provider evidence may be known, unavailable, or unresolved.
/// 一个合约条款，其供应商证据可能为已知、不可用或尚未解析。
pub type ContractTerm<T> = ProviderValue<T>;

/// Field required to produce a fully qualified option key.
/// 创建完整核验期权键所必需的字段。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum InstrumentField {
    /// Provider trading class.
    /// 供应商 trading class。
    TradingClass,
    /// Contract currency.
    /// 合约货币。
    Currency,
    /// Contract multiplier.
    /// 合约乘数。
    Multiplier,
    /// Deliverable basket.
    /// 交割物篮子。
    Deliverable,
    /// Exercise style.
    /// 行权方式。
    ExerciseStyle,
    /// Settlement type.
    /// 结算类型。
    SettlementType,
}

/// Failure to qualify a provider candidate without filling in missing contract terms.
/// 无法在不填补缺失合约条款的情况下核验供应商候选对象。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstrumentQualificationError {
    /// A required contract field was unavailable for the reported reason.
    /// 必需合约字段因给定原因不可用。
    Unavailable {
        /// Contract field that could not be qualified.
        /// 无法核验的合约字段。
        field: InstrumentField,
        /// Provider-reported reason for the missing value.
        /// 供应商报告的缺失原因。
        reason: ProviderUnavailableReason,
    },
    /// A required field has an unknown provider encoding with retained record evidence.
    /// 必需字段采用未知供应商编码，并保留来源记录证据。
    Unknown {
        /// Contract field that could not be interpreted.
        /// 无法解释的合约字段。
        field: InstrumentField,
        /// Reference that lets an adapter inspect the source record.
        /// 供适配器检查来源记录的引用。
        evidence: ProviderMetadataRef,
    },
}

/// Complete broker-neutral key for a qualified option contract.
/// 已核验期权合约的完整券商中立键。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum InstrumentKey {
    /// Qualified option contract identity.
    /// 已核验的期权合约身份。
    Option(OptionInstrumentKey),
}

/// Fully qualified option identity containing every term used for cross-broker matching.
/// 包含跨券商匹配所需全部条款的完整期权身份。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct OptionInstrumentKey {
    contract: OptionContract,
    trading_class: TradingClass,
    exercise_style: OptionExerciseStyle,
    settlement_type: OptionSettlementType,
}

/// Candidate option identity retaining provider evidence for incomplete contract terms.
/// 保留不完整合约条款供应商证据的期权身份候选对象。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct OptionInstrumentCandidate {
    symbol: OptionSymbol,
    trading_class: ContractTerm<TradingClass>,
    currency: ContractTerm<ContractCurrency>,
    multiplier: ContractTerm<ContractMultiplier>,
    deliverable: ContractTerm<Deliverable>,
    exercise_style: ContractTerm<OptionExerciseStyle>,
    settlement_type: ContractTerm<OptionSettlementType>,
}

impl TradingClass {
    /// Creates a non-empty canonical trading-class value.
    /// 创建非空的规范 trading class 值。
    pub fn new(value: &str) -> Result<Self, IdentifierError> {
        let canonical = value.trim().to_ascii_uppercase();
        crate::OrderId::new(canonical.clone())?;
        Ok(Self(canonical))
    }

    /// Returns the canonical trading-class text.
    /// 返回规范 trading class 文本。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl InstrumentKey {
    /// Creates a key only from a completely qualified option identity.
    /// 仅根据完整核验的期权身份创建键。
    pub const fn option(value: OptionInstrumentKey) -> Self {
        Self::Option(value)
    }

    /// Returns the qualified option identity.
    /// 返回已核验的期权身份。
    pub const fn as_option(&self) -> &OptionInstrumentKey {
        match self {
            Self::Option(value) => value,
        }
    }
}

impl OptionInstrumentKey {
    /// Creates an option key from all validated economic and provider class terms.
    /// 根据所有已校验的经济条款和供应商分类创建期权键。
    pub const fn new(
        contract: OptionContract,
        trading_class: TradingClass,
        exercise_style: OptionExerciseStyle,
        settlement_type: OptionSettlementType,
    ) -> Self {
        Self {
            contract,
            trading_class,
            exercise_style,
            settlement_type,
        }
    }

    /// Borrows the complete existing option contract value.
    /// 借用已有完整期权合约值。
    pub const fn contract(&self) -> &OptionContract {
        &self.contract
    }

    /// Returns the provider trading class used by the qualified key.
    /// 返回此已核验键使用的供应商 trading class。
    pub const fn trading_class(&self) -> &TradingClass {
        &self.trading_class
    }

    /// Returns the qualified exercise style.
    /// 返回已核验的行权方式。
    pub const fn exercise_style(&self) -> &OptionExerciseStyle {
        &self.exercise_style
    }

    /// Returns the qualified settlement type.
    /// 返回已核验的结算类型。
    pub const fn settlement_type(&self) -> &OptionSettlementType {
        &self.settlement_type
    }

    /// Returns the underlying contract root.
    /// 返回合约标的根代码。
    pub fn underlying(&self) -> &Underlying {
        self.contract.symbol().underlying()
    }

    /// Returns the option expiration date.
    /// 返回期权到期日。
    pub const fn expiration(&self) -> crate::ExpirationDate {
        self.contract.symbol().expiration()
    }

    /// Returns the call or put right.
    /// 返回看涨或看跌方向。
    pub const fn right(&self) -> OptionRight {
        self.contract.symbol().right()
    }

    /// Returns the exact option strike.
    /// 返回精确期权行权价。
    pub const fn strike(&self) -> Strike {
        self.contract.symbol().strike()
    }
}

impl OptionInstrumentCandidate {
    /// Retains known, missing, and unknown values from a provider qualification attempt.
    /// 保留供应商合约核验中的已知、缺失和未知字段。
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        symbol: OptionSymbol,
        trading_class: ContractTerm<TradingClass>,
        currency: ContractTerm<ContractCurrency>,
        multiplier: ContractTerm<ContractMultiplier>,
        deliverable: ContractTerm<Deliverable>,
        exercise_style: ContractTerm<OptionExerciseStyle>,
        settlement_type: ContractTerm<OptionSettlementType>,
    ) -> Self {
        Self {
            symbol,
            trading_class,
            currency,
            multiplier,
            deliverable,
            exercise_style,
            settlement_type,
        }
    }

    /// Creates an unqualified candidate from a parsed option symbol.
    /// 根据已解析的期权标识创建未核验候选对象。
    ///
    /// Parsing does not infer trading class, currency, multiplier, deliverable, exercise, or
    /// settlement terms. Those fields remain explicitly unavailable until provider evidence is
    /// supplied.
    /// 解析不会推断 trading class、currency、multiplier、deliverable、exercise 或 settlement 条款；在收到供应商证据前，这些字段均明确保持不可用。
    pub const fn from_symbol_candidate(symbol: OptionSymbol) -> Self {
        Self::new(
            symbol,
            ProviderValue::Unavailable(ProviderUnavailableReason::NotReported),
            ProviderValue::Unavailable(ProviderUnavailableReason::NotReported),
            ProviderValue::Unavailable(ProviderUnavailableReason::NotReported),
            ProviderValue::Unavailable(ProviderUnavailableReason::NotReported),
            ProviderValue::Unavailable(ProviderUnavailableReason::NotReported),
            ProviderValue::Unavailable(ProviderUnavailableReason::NotReported),
        )
    }

    /// Returns the parsed symbol candidate without treating it as a qualified broker contract.
    /// 返回已解析标识候选对象，但不将其视为已核验券商合约。
    pub const fn symbol(&self) -> &OptionSymbol {
        &self.symbol
    }

    /// Returns the trading-class evidence retained by this candidate.
    /// 返回此候选对象保留的 trading class 证据。
    pub const fn trading_class_evidence(&self) -> &ContractTerm<TradingClass> {
        &self.trading_class
    }

    /// Returns the currency evidence retained by this candidate.
    /// 返回此候选对象保留的货币证据。
    pub const fn currency_evidence(&self) -> &ContractTerm<ContractCurrency> {
        &self.currency
    }

    /// Returns the multiplier evidence retained by this candidate.
    /// 返回此候选对象保留的乘数证据。
    pub const fn multiplier_evidence(&self) -> &ContractTerm<ContractMultiplier> {
        &self.multiplier
    }

    /// Returns the deliverable evidence retained by this candidate.
    /// 返回此候选对象保留的交割物证据。
    pub const fn deliverable_evidence(&self) -> &ContractTerm<Deliverable> {
        &self.deliverable
    }

    /// Returns the exercise-style evidence retained by this candidate.
    /// 返回此候选对象保留的行权方式证据。
    pub const fn exercise_style_evidence(&self) -> &ContractTerm<OptionExerciseStyle> {
        &self.exercise_style
    }

    /// Returns the settlement-type evidence retained by this candidate.
    /// 返回此候选对象保留的结算类型证据。
    pub const fn settlement_type_evidence(&self) -> &ContractTerm<OptionSettlementType> {
        &self.settlement_type
    }

    /// Qualifies this candidate only when every required contract term is known.
    /// 仅当所有必需合约条款均已知时，才核验此候选对象。
    pub fn qualify(&self) -> Result<InstrumentKey, InstrumentQualificationError> {
        let trading_class = known_term(InstrumentField::TradingClass, &self.trading_class)?;
        let currency = known_term(InstrumentField::Currency, &self.currency)?;
        let multiplier = known_term(InstrumentField::Multiplier, &self.multiplier)?;
        let deliverable = known_term(InstrumentField::Deliverable, &self.deliverable)?;
        let exercise_style = known_term(InstrumentField::ExerciseStyle, &self.exercise_style)?;
        let settlement_type = known_term(InstrumentField::SettlementType, &self.settlement_type)?;
        let contract = OptionContract::new(self.symbol.clone(), multiplier, currency, deliverable);
        Ok(InstrumentKey::option(OptionInstrumentKey::new(
            contract,
            trading_class,
            exercise_style,
            settlement_type,
        )))
    }
}

fn known_term<T: Clone>(
    field: InstrumentField,
    value: &ContractTerm<T>,
) -> Result<T, InstrumentQualificationError> {
    match value {
        ProviderValue::Known(value) => Ok(value.clone()),
        ProviderValue::Unavailable(reason) => Err(InstrumentQualificationError::Unavailable {
            field,
            reason: *reason,
        }),
        ProviderValue::Unknown(evidence) => Err(InstrumentQualificationError::Unknown {
            field,
            evidence: evidence.clone(),
        }),
    }
}

impl fmt::Display for InstrumentQualificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable { field, reason } => write!(
                formatter,
                "OPTION_CONTRACT_FIELD_UNAVAILABLE:{field:?}:{reason:?}"
            ),
            Self::Unknown { field, .. } => {
                write!(formatter, "OPTION_CONTRACT_FIELD_UNKNOWN:{field:?}")
            }
        }
    }
}

impl std::error::Error for InstrumentQualificationError {}

#[cfg(test)]
mod tests {
    use super::{
        InstrumentField, InstrumentKey, InstrumentQualificationError, OptionExerciseStyle,
        OptionInstrumentCandidate, OptionInstrumentKey, OptionSettlementType, TradingClass,
    };
    use crate::{
        ContractCurrency, ContractMultiplier, Deliverable, DeliverableComponent, ExactDecimal,
        ExpirationDate, MetadataSource, OptionContract, OptionRight, OptionSymbol,
        ProviderMetadataKind, ProviderMetadataRef, ProviderRecordId, ProviderUnavailableReason,
        ProviderValue, Strike, Underlying,
    };

    fn symbol(strike: &str) -> OptionSymbol {
        OptionSymbol::new(
            Underlying::new("SPY").unwrap(),
            ExpirationDate::new(2026, 9, 30).unwrap(),
            OptionRight::Put,
            Strike::parse_json_number(strike).unwrap(),
        )
    }

    fn deliverable(shares: &str) -> Deliverable {
        Deliverable::new(vec![
            DeliverableComponent::equity(
                Underlying::new("SPY").unwrap(),
                ExactDecimal::parse_json_number(shares).unwrap(),
            )
            .unwrap(),
        ])
        .unwrap()
    }

    fn qualified_candidate(
        symbol: OptionSymbol,
        trading_class: &str,
        currency: &str,
        shares: &str,
        multiplier: u64,
        exercise_style: OptionExerciseStyle,
        settlement_type: OptionSettlementType,
    ) -> OptionInstrumentCandidate {
        OptionInstrumentCandidate::new(
            symbol,
            ProviderValue::Known(TradingClass::new(trading_class).unwrap()),
            ProviderValue::Known(ContractCurrency::new(currency).unwrap()),
            ProviderValue::Known(ContractMultiplier::new(multiplier).unwrap()),
            ProviderValue::Known(deliverable(shares)),
            ProviderValue::Known(exercise_style),
            ProviderValue::Known(settlement_type),
        )
    }

    #[test]
    fn parsed_symbol_is_only_a_candidate_until_all_contract_terms_are_known() {
        let candidate = OptionInstrumentCandidate::from_symbol_candidate(symbol("600"));
        assert!(matches!(
            candidate.multiplier_evidence(),
            ProviderValue::Unavailable(ProviderUnavailableReason::NotReported)
        ));
        assert!(matches!(
            candidate.trading_class_evidence(),
            ProviderValue::Unavailable(ProviderUnavailableReason::NotReported)
        ));
        assert!(matches!(
            candidate.currency_evidence(),
            ProviderValue::Unavailable(ProviderUnavailableReason::NotReported)
        ));
        assert!(matches!(
            candidate.deliverable_evidence(),
            ProviderValue::Unavailable(ProviderUnavailableReason::NotReported)
        ));
        assert!(matches!(
            candidate.exercise_style_evidence(),
            ProviderValue::Unavailable(ProviderUnavailableReason::NotReported)
        ));
        assert!(matches!(
            candidate.settlement_type_evidence(),
            ProviderValue::Unavailable(ProviderUnavailableReason::NotReported)
        ));
        assert_eq!(
            candidate.qualify(),
            Err(InstrumentQualificationError::Unavailable {
                field: InstrumentField::TradingClass,
                reason: ProviderUnavailableReason::NotReported,
            })
        );
    }

    #[test]
    fn missing_multiplier_or_deliverable_never_becomes_a_standard_contract() {
        let missing_multiplier = OptionInstrumentCandidate::new(
            symbol("600"),
            ProviderValue::Known(TradingClass::new("SPY").unwrap()),
            ProviderValue::Known(ContractCurrency::new("USD").unwrap()),
            ProviderValue::Unavailable(ProviderUnavailableReason::NotReported),
            ProviderValue::Known(deliverable("100")),
            ProviderValue::Known(OptionExerciseStyle::American),
            ProviderValue::Known(OptionSettlementType::Physical),
        );
        assert_eq!(
            missing_multiplier.qualify(),
            Err(InstrumentQualificationError::Unavailable {
                field: InstrumentField::Multiplier,
                reason: ProviderUnavailableReason::NotReported,
            })
        );

        let missing_deliverable = OptionInstrumentCandidate::new(
            symbol("600"),
            ProviderValue::Known(TradingClass::new("SPY").unwrap()),
            ProviderValue::Known(ContractCurrency::new("USD").unwrap()),
            ProviderValue::Known(ContractMultiplier::new(100).unwrap()),
            ProviderValue::Unavailable(ProviderUnavailableReason::Unsupported),
            ProviderValue::Known(OptionExerciseStyle::American),
            ProviderValue::Known(OptionSettlementType::Physical),
        );
        assert_eq!(
            missing_deliverable.qualify(),
            Err(InstrumentQualificationError::Unavailable {
                field: InstrumentField::Deliverable,
                reason: ProviderUnavailableReason::Unsupported,
            })
        );
    }

    #[test]
    fn unknown_provider_fields_preserve_a_redacted_record_reference() {
        let evidence = ProviderMetadataRef::new(
            MetadataSource::MarketData(crate::MarketDataProviderId::Alpaca),
            ProviderMetadataKind::Contract,
            ProviderRecordId::new("synthetic-adjusted-contract-8842").unwrap(),
        );
        let candidate = OptionInstrumentCandidate::new(
            symbol("600"),
            ProviderValue::Known(TradingClass::new("SPY").unwrap()),
            ProviderValue::Known(ContractCurrency::new("USD").unwrap()),
            ProviderValue::Known(ContractMultiplier::new(100).unwrap()),
            ProviderValue::Known(deliverable("100")),
            ProviderValue::Known(OptionExerciseStyle::American),
            ProviderValue::Unknown(evidence.clone()),
        );
        assert_eq!(
            candidate.qualify(),
            Err(InstrumentQualificationError::Unknown {
                field: InstrumentField::SettlementType,
                evidence,
            })
        );
    }

    #[test]
    fn instrument_equality_covers_every_cross_broker_option_term() {
        let standard = qualified_candidate(
            symbol("600"),
            "SPY",
            "USD",
            "100",
            100,
            OptionExerciseStyle::American,
            OptionSettlementType::Physical,
        )
        .qualify()
        .unwrap();

        let differs_by = |candidate: OptionInstrumentCandidate| {
            assert_ne!(standard, candidate.qualify().unwrap());
        };
        differs_by(qualified_candidate(
            symbol("601"),
            "SPY",
            "USD",
            "100",
            100,
            OptionExerciseStyle::American,
            OptionSettlementType::Physical,
        ));
        differs_by(qualified_candidate(
            OptionSymbol::new(
                Underlying::new("SPY").unwrap(),
                ExpirationDate::new(2026, 10, 1).unwrap(),
                OptionRight::Put,
                Strike::parse_json_number("600").unwrap(),
            ),
            "SPY",
            "USD",
            "100",
            100,
            OptionExerciseStyle::American,
            OptionSettlementType::Physical,
        ));
        differs_by(qualified_candidate(
            OptionSymbol::new(
                Underlying::new("SPY").unwrap(),
                ExpirationDate::new(2026, 9, 30).unwrap(),
                OptionRight::Call,
                Strike::parse_json_number("600").unwrap(),
            ),
            "SPY",
            "USD",
            "100",
            100,
            OptionExerciseStyle::American,
            OptionSettlementType::Physical,
        ));
        differs_by(qualified_candidate(
            symbol("600"),
            "SPY",
            "EUR",
            "100",
            100,
            OptionExerciseStyle::American,
            OptionSettlementType::Physical,
        ));
        differs_by(qualified_candidate(
            symbol("600"),
            "SPYW",
            "USD",
            "100",
            100,
            OptionExerciseStyle::American,
            OptionSettlementType::Physical,
        ));
        differs_by(qualified_candidate(
            symbol("600"),
            "SPY",
            "USD",
            "100",
            10,
            OptionExerciseStyle::American,
            OptionSettlementType::Physical,
        ));
        differs_by(qualified_candidate(
            symbol("600"),
            "SPY",
            "USD",
            "50",
            100,
            OptionExerciseStyle::American,
            OptionSettlementType::Physical,
        ));
        differs_by(qualified_candidate(
            symbol("600"),
            "SPY",
            "USD",
            "100",
            100,
            OptionExerciseStyle::European,
            OptionSettlementType::Physical,
        ));
        differs_by(qualified_candidate(
            symbol("600"),
            "SPY",
            "USD",
            "100",
            100,
            OptionExerciseStyle::American,
            OptionSettlementType::Cash,
        ));
        assert!(matches!(standard, InstrumentKey::Option(_)));
    }

    #[test]
    fn fully_known_existing_option_terms_are_reused_without_a_second_contract_model() {
        let contract = OptionContract::new(
            symbol("600"),
            ContractMultiplier::new(100).unwrap(),
            ContractCurrency::new("USD").unwrap(),
            deliverable("100"),
        );
        let key = InstrumentKey::option(OptionInstrumentKey::new(
            contract.clone(),
            TradingClass::new("SPY").unwrap(),
            OptionExerciseStyle::American,
            OptionSettlementType::Physical,
        ));
        assert_eq!(key.as_option().contract(), &contract);
        assert_eq!(key.as_option().contract().multiplier().get(), 100);
    }
}
