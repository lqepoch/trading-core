//! Shared, bounded singleflight for local option Greeks.
//!
//! A request key includes the contract, both quote observations, every solver
//! input, the explicit valuation point, and the model version. The owner must
//! advance the current-input fence for every accepted quote or underlying
//! update, including updates that do not submit solver work.
//!
//! ## 简体中文
//!
//! 本模块为本地期权 Greeks 提供共享且有界的 singleflight。请求键包含合约、两路行情观测、全部求解输入、显式估值时点和模型版本。每当接受期权或标的行情更新时，所有者都必须推进 current-input fence，包括本次不提交求解任务的情况。

use std::{
    collections::{HashMap, VecDeque},
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};

use domain::{OptionContract, Price, Underlying};
use tokio::sync::watch;

use crate::{
    GreekSolverPool, ModelUnavailable, OptionMetrics, PoolError, QuoteRevision, SolverInput,
    SolverInputError, SolverOutcome, solver::SolverInputFingerprint,
    worker_pool::increment_counter,
};

/// Maximum number of in-flight or completed entries in one singleflight owner.
/// 单个 singleflight owner 中在途与已完成条目的最大数量。
pub const MAX_SINGLEFLIGHT_ENTRIES: usize = 4_096;

const MAX_SINGLEFLIGHT_WAITERS_PER_KEY: usize = 128;

const MAX_QUOTE_AGE_MS: u64 = 60_000;

/// Stream generation carried by one pricing request.
/// 单个定价请求携带的行情代次。
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PricingGeneration(u64);

impl PricingGeneration {
    /// Creates a generation from the session owner's monotonic sequence.
    /// 使用会话所有者提供的单调序列值创建代次。
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the underlying sequence value.
    /// 返回底层序列值。
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Stable, caller-assigned identity for the source that supplied a quote.
/// 调用方为行情来源分配的稳定身份。
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PricingSourceId(u64);

impl PricingSourceId {
    /// Wraps a source identity without interpreting or logging it.
    /// 包装来源身份；本类型不解释或记录其内容。
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the caller-assigned identity.
    /// 返回调用方分配的身份值。
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Quality carried with one solver quote input.
/// 求解器行情输入携带的质量标记。
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PricingQuoteQuality {
    /// The source explicitly marked this observation as realtime.
    /// 来源明确标记该观测为实时。
    Realtime,
    /// The source explicitly marked this observation as delayed.
    /// 来源明确标记该观测为延迟。
    Delayed,
    /// The source did not provide a reliable quality flag.
    /// 来源未提供可靠的质量标记。
    Unknown,
    /// The source explicitly marked this observation as invalid.
    /// 来源明确标记该观测无效。
    Invalid,
}

/// Provenance and freshness policy for one price input.
/// 单个价格输入的来源信息与新鲜度策略。
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PricingQuoteProvenance {
    source_id: PricingSourceId,
    generation: PricingGeneration,
    revision: QuoteRevision,
    source_timestamp_ms: i64,
    received_at_ms: i64,
    max_age_ms: u64,
    quality: PricingQuoteQuality,
}

impl PricingQuoteProvenance {
    /// Builds quote provenance. Time and freshness bounds are validated when
    /// the key is constructed and checked again each time a result is used.
    /// 创建行情来源信息。构造键时会验证时间和新鲜度上限；每次使用结果时也会重新检查。
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        source_id: PricingSourceId,
        generation: PricingGeneration,
        revision: QuoteRevision,
        source_timestamp_ms: i64,
        received_at_ms: i64,
        max_age_ms: u64,
        quality: PricingQuoteQuality,
    ) -> Self {
        Self {
            source_id,
            generation,
            revision,
            source_timestamp_ms,
            received_at_ms,
            max_age_ms,
            quality,
        }
    }

    /// Returns the stable source identity.
    /// 返回稳定的来源身份。
    pub const fn source_id(self) -> PricingSourceId {
        self.source_id
    }

    /// Returns the source generation.
    /// 返回来源代次。
    pub const fn generation(self) -> PricingGeneration {
        self.generation
    }

    /// Returns the source revision.
    /// 返回来源版本。
    pub const fn revision(self) -> QuoteRevision {
        self.revision
    }

    /// Returns the source observation time in UTC epoch milliseconds.
    /// 返回行情来源观测时刻（UTC Unix 毫秒）。
    pub const fn source_timestamp_ms(self) -> i64 {
        self.source_timestamp_ms
    }

    /// Returns the local receive time in UTC epoch milliseconds.
    /// 返回本地接收时刻（UTC Unix 毫秒）。
    pub const fn received_at_ms(self) -> i64 {
        self.received_at_ms
    }

    /// Returns the maximum accepted observation age in milliseconds.
    /// 返回允许的最大观测年龄（毫秒）。
    pub const fn max_age_ms(self) -> u64 {
        self.max_age_ms
    }

    /// Returns the source quality marker.
    /// 返回来源质量标记。
    pub const fn quality(self) -> PricingQuoteQuality {
        self.quality
    }

    fn validate_configuration(self) -> Result<(), PricingInputError> {
        if self.source_timestamp_ms < 0 || self.received_at_ms < 0 {
            return Err(PricingInputError::InvalidTimestamp);
        }
        if self.max_age_ms == 0 || self.max_age_ms > MAX_QUOTE_AGE_MS {
            return Err(PricingInputError::InvalidFreshnessWindow);
        }
        if self.quality == PricingQuoteQuality::Invalid {
            return Err(PricingInputError::InvalidQuoteQuality);
        }
        Ok(())
    }

    fn validate_fresh_at(self, checked_at_ms: i64) -> Result<(), PricingInputError> {
        self.validate_configuration()?;
        if checked_at_ms < 0 {
            return Err(PricingInputError::InvalidTimestamp);
        }
        if self.source_timestamp_ms > checked_at_ms || self.received_at_ms > checked_at_ms {
            return Err(PricingInputError::FutureObservation);
        }
        let age = checked_at_ms
            .checked_sub(self.source_timestamp_ms)
            .ok_or(PricingInputError::InvalidTimestamp)?;
        if u64::try_from(age).map_err(|_| PricingInputError::InvalidTimestamp)? > self.max_age_ms {
            return Err(PricingInputError::StaleObservation);
        }
        Ok(())
    }
}

/// One option quote value and the provenance captured with that value.
///
/// Callers should build this binding from one accepted option quote update and
/// pass it intact to [`PricingInputKey::new`]. The key checks its contract and
/// exact premium against the solver input before admitting work.
///
/// 一份期权报价及其同次采集的来源信息。调用方应从同一次已接受的期权报价更新创建绑定，并整体传入 [`PricingInputKey::new`]。构造键时会校验完整合约身份及精确权利金。
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct OptionPriceInput {
    contract: OptionContract,
    price: Price,
    provenance: PricingQuoteProvenance,
}

impl OptionPriceInput {
    /// Captures a complete option identity, exact quote value, and provenance.
    /// 捕获完整期权身份、精确报价及其来源信息。
    pub const fn new(
        contract: OptionContract,
        price: Price,
        provenance: PricingQuoteProvenance,
    ) -> Self {
        Self {
            contract,
            price,
            provenance,
        }
    }

    /// Returns the option contract associated with this quote.
    /// 返回此报价绑定的期权合约。
    pub const fn contract(&self) -> &OptionContract {
        &self.contract
    }

    /// Returns the exact option premium associated with this quote.
    /// 返回此报价绑定的精确期权权利金。
    pub const fn price(&self) -> Price {
        self.price
    }

    /// Returns the provenance captured with this quote value.
    /// 返回与此报价一同捕获的来源信息。
    pub const fn provenance(&self) -> PricingQuoteProvenance {
        self.provenance
    }
}

/// One underlying quote value and the provenance captured with that value.
///
/// Callers should build this binding from one accepted underlying quote update
/// and pass it intact to [`PricingInputKey::new`]. The key checks the underlying
/// identity against the option contract and its exact price against solver spot.
///
/// 一份标的报价及其同次采集的来源信息。调用方应从同一次已接受的标的报价更新创建绑定，并整体传入 [`PricingInputKey::new`]。构造键时会校验标的身份及精确价格是否与求解器 spot 一致。
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct UnderlyingPriceInput {
    underlying: Underlying,
    price: Price,
    provenance: PricingQuoteProvenance,
}

impl UnderlyingPriceInput {
    /// Captures an underlying identity, exact quote value, and provenance.
    /// 捕获标的身份、精确报价及其来源信息。
    pub const fn new(
        underlying: Underlying,
        price: Price,
        provenance: PricingQuoteProvenance,
    ) -> Self {
        Self {
            underlying,
            price,
            provenance,
        }
    }

    /// Returns the underlying identity associated with this quote.
    /// 返回此报价绑定的标的身份。
    pub const fn underlying(&self) -> &Underlying {
        &self.underlying
    }

    /// Returns the exact underlying price associated with this quote.
    /// 返回此报价绑定的精确标的价格。
    pub const fn price(&self) -> Price {
        self.price
    }

    /// Returns the provenance captured with this quote value.
    /// 返回与此报价一同捕获的来源信息。
    pub const fn provenance(&self) -> PricingQuoteProvenance {
        self.provenance
    }
}

/// Solver model version included in every local pricing key.
/// 每个本地定价键包含的求解模型版本。
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GreekModelVersion {
    /// ACT/365F expiry and complete expiration/dividend evidence model.
    /// 使用 ACT/365F 到期时间及完整到期/股息证据的 V2 模型。
    BlackScholesEuropeanV2,
    /// Compatibility identifier for previously issued V1 keys.
    /// 既有 V1 定价键的兼容标识。
    BlackScholesEuropeanV1,
}

/// Complete hashable identity for one local IV/Greeks calculation.
/// 单次本地 IV/Greeks 计算的完整可哈希身份。
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PricingInputKey {
    contract: OptionContract,
    generation: PricingGeneration,
    option_price_input: OptionPriceInput,
    underlying_price_input: UnderlyingPriceInput,
    valuation_at_ms: i64,
    model_version: GreekModelVersion,
    solver: SolverInputFingerprint,
}

impl PricingInputKey {
    /// Creates a full key from validated option and underlying quote bindings,
    /// a valuation point, and the exact solver input.
    /// 根据已校验的期权与标的报价绑定、估值时点和精确求解输入创建完整键。
    pub fn new(
        contract: OptionContract,
        generation: PricingGeneration,
        option_price_input: OptionPriceInput,
        underlying_price_input: UnderlyingPriceInput,
        valuation_at_ms: i64,
        input: &SolverInput,
    ) -> Result<Self, PricingInputError> {
        if !input.matches_contract(&contract) {
            return Err(PricingInputError::ContractSolverMismatch);
        }
        if option_price_input.contract != contract {
            return Err(PricingInputError::OptionContractMismatch);
        }
        if option_price_input.price != input.option_price() {
            return Err(PricingInputError::OptionPriceMismatch);
        }
        if underlying_price_input.underlying != *contract.symbol().underlying() {
            return Err(PricingInputError::UnderlyingIdentityMismatch);
        }
        if underlying_price_input.price != input.spot() {
            return Err(PricingInputError::UnderlyingPriceMismatch);
        }
        let option_provenance = option_price_input.provenance;
        let underlying_provenance = underlying_price_input.provenance;
        if option_provenance.generation != generation
            || underlying_provenance.generation != generation
        {
            return Err(PricingInputError::GenerationMismatch);
        }
        option_provenance.validate_configuration()?;
        underlying_provenance.validate_configuration()?;
        if option_provenance.source_timestamp_ms != input.observed_at_ms()
            || option_provenance.max_age_ms != input.max_age_ms()
        {
            return Err(PricingInputError::OptionObservationMismatch);
        }
        if valuation_at_ms < 0 {
            return Err(PricingInputError::InvalidTimestamp);
        }
        if valuation_at_ms != input.valuation_at_ms() {
            return Err(PricingInputError::ValuationInstantMismatch);
        }
        Ok(Self {
            contract,
            generation,
            option_price_input,
            underlying_price_input,
            valuation_at_ms,
            model_version: GreekModelVersion::BlackScholesEuropeanV2,
            solver: input.fingerprint(),
        })
    }

    /// Returns the option contract identity.
    /// 返回期权合约身份。
    pub fn contract(&self) -> &OptionContract {
        &self.contract
    }

    /// Returns the stream generation.
    /// 返回行情流代次。
    pub const fn generation(&self) -> PricingGeneration {
        self.generation
    }

    /// Returns option quote provenance.
    /// 返回期权行情来源信息。
    pub const fn option_provenance(&self) -> PricingQuoteProvenance {
        self.option_price_input.provenance
    }

    /// Returns underlying quote provenance.
    /// 返回标的行情来源信息。
    pub const fn underlying_provenance(&self) -> PricingQuoteProvenance {
        self.underlying_price_input.provenance
    }

    /// Returns the option quote binding included in this key.
    /// 返回键中包含的期权报价绑定。
    pub const fn option_price_input(&self) -> &OptionPriceInput {
        &self.option_price_input
    }

    /// Returns the underlying quote binding included in this key.
    /// 返回键中包含的标的报价绑定。
    pub const fn underlying_price_input(&self) -> &UnderlyingPriceInput {
        &self.underlying_price_input
    }

    /// Returns the explicit valuation point in UTC epoch milliseconds.
    /// 返回显式估值时点（UTC Unix 毫秒）。
    pub const fn valuation_at_ms(&self) -> i64 {
        self.valuation_at_ms
    }

    /// Returns the local model version.
    /// 返回本地模型版本。
    pub const fn model_version(&self) -> GreekModelVersion {
        self.model_version
    }

    /// Rechecks both quote observations at the caller's current time.
    /// 按调用方传入的当前时刻重新检查两路行情。
    pub fn validate_fresh_at(&self, checked_at_ms: i64) -> Result<(), PricingInputError> {
        self.option_price_input
            .provenance
            .validate_fresh_at(checked_at_ms)?;
        self.underlying_price_input
            .provenance
            .validate_fresh_at(checked_at_ms)?;
        Ok(())
    }

    fn matches_input(&self, input: &SolverInput) -> bool {
        self.solver == input.fingerprint()
    }
}

/// Typed errors for complete pricing key and owner-fence construction.
/// 完整定价键和 owner fence 构造过程中的类型化错误。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PricingInputError {
    /// A timestamp is negative or cannot be represented as an age.
    /// 时间戳为负数，或无法表示为年龄。
    InvalidTimestamp,
    /// A freshness window is zero or exceeds the 60-second bound.
    /// 新鲜度窗口为零或超过 60 秒上限。
    InvalidFreshnessWindow,
    /// An observation timestamp is later than the caller's check time.
    /// 行情观测时间晚于调用方的检查时刻。
    FutureObservation,
    /// An observation exceeds its configured maximum age.
    /// 行情观测已超过配置的最大年龄。
    StaleObservation,
    /// The source explicitly marked the quote invalid.
    /// 来源明确标记行情无效。
    InvalidQuoteQuality,
    /// Quote provenance generation differs from the key generation.
    /// 行情来源代次与键代次不一致。
    GenerationMismatch,
    /// Option quote timestamp or age differs from the solver observation.
    /// 期权行情时间或年龄与求解器观测不一致。
    OptionObservationMismatch,
    /// The contract strike or right differs from the solver input.
    /// 合约行权价或方向与求解器输入不一致。
    ContractSolverMismatch,
    /// The option quote belongs to a different complete contract.
    /// 期权报价属于另一完整合约。
    OptionContractMismatch,
    /// The exact option quote value differs from the solver premium.
    /// 精确期权报价与求解器 premium 不一致。
    OptionPriceMismatch,
    /// The key valuation timestamp differs from the immutable solver valuation.
    /// 定价键估值时刻与求解输入不可变估值时刻不一致。
    ValuationInstantMismatch,
    /// The underlying quote identity differs from the contract underlying.
    /// 标的报价身份与合约标的不一致。
    UnderlyingIdentityMismatch,
    /// The exact underlying quote value differs from the solver spot.
    /// 精确标的报价与求解器 spot 不一致。
    UnderlyingPriceMismatch,
    /// The fence is not current for the complete pricing key.
    /// fence 与完整定价键不一致或已失效。
    FenceMismatch,
    /// The caller attempted to move a generation backwards or repeat it.
    /// 调用方尝试回退或重复设置代次。
    GenerationRegressed,
    /// The monotonic current-input fence revision cannot be advanced.
    /// 单调 current-input fence 版本无法继续推进。
    FenceRevisionExhausted,
    /// The current-input fence lock was poisoned.
    /// current-input fence 锁已中毒。
    FencePoisoned,
}

impl fmt::Display for PricingInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidTimestamp => "PRICING_INPUT_INVALID_TIMESTAMP",
            Self::InvalidFreshnessWindow => "PRICING_INPUT_INVALID_FRESHNESS_WINDOW",
            Self::FutureObservation => "PRICING_INPUT_FUTURE_OBSERVATION",
            Self::StaleObservation => "PRICING_INPUT_STALE_OBSERVATION",
            Self::InvalidQuoteQuality => "PRICING_INPUT_INVALID_QUOTE_QUALITY",
            Self::GenerationMismatch => "PRICING_INPUT_GENERATION_MISMATCH",
            Self::OptionObservationMismatch => "PRICING_INPUT_OPTION_OBSERVATION_MISMATCH",
            Self::ContractSolverMismatch => "PRICING_INPUT_CONTRACT_SOLVER_MISMATCH",
            Self::OptionContractMismatch => "PRICING_INPUT_OPTION_CONTRACT_MISMATCH",
            Self::OptionPriceMismatch => "PRICING_INPUT_OPTION_PRICE_MISMATCH",
            Self::ValuationInstantMismatch => "PRICING_INPUT_VALUATION_INSTANT_MISMATCH",
            Self::UnderlyingIdentityMismatch => "PRICING_INPUT_UNDERLYING_IDENTITY_MISMATCH",
            Self::UnderlyingPriceMismatch => "PRICING_INPUT_UNDERLYING_PRICE_MISMATCH",
            Self::FenceMismatch => "PRICING_INPUT_FENCE_MISMATCH",
            Self::GenerationRegressed => "PRICING_INPUT_GENERATION_REGRESSED",
            Self::FenceRevisionExhausted => "PRICING_INPUT_FENCE_REVISION_EXHAUSTED",
            Self::FencePoisoned => "PRICING_INPUT_FENCE_POISONED",
        })
    }
}

impl std::error::Error for PricingInputError {}

/// Monotonic version token for one owner-maintained complete-input fence.
/// 单个 owner 维护的完整输入 fence 的单调版本令牌。
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PricingFenceRevision {
    generation: PricingGeneration,
    revision: u64,
}

impl PricingFenceRevision {
    /// Returns the generation this token belongs to.
    /// 返回令牌所属代次。
    pub const fn generation(self) -> PricingGeneration {
        self.generation
    }

    /// Returns the monotonic owner-input revision.
    /// 返回单调递增的 owner 输入版本。
    pub const fn revision(self) -> u64 {
        self.revision
    }
}

#[derive(Debug)]
struct PricingFenceState {
    generation: PricingGeneration,
    revision: u64,
    current_key: Option<PricingInputKey>,
}

/// Owner-updated fence holding exactly one current complete pricing key.
///
/// Clone this fence for all consumers of one option leg. Call [`Self::advance`]
/// as soon as either accepted quote changes, even when no solver request is
/// made; then call [`Self::accept_key`] when a complete new input is available.
/// The fence retains no history.
///
/// owner 更新的 fence，只保存一个当前完整定价键。
///
/// 同一 option leg 的所有消费者应克隆并共享此 fence。任一路已接受的行情发生变化时，应立即调用 [`Self::advance`]，即使本次不提交求解；新输入完整后再调用 [`Self::accept_key`]。fence 不保留历史记录。
#[derive(Clone, Debug)]
pub struct PricingInputFence {
    state: Arc<Mutex<PricingFenceState>>,
}

impl PricingInputFence {
    /// Creates an empty current-input fence for one generation.
    /// 为一个行情代次创建空的 current-input fence。
    pub fn new(generation: PricingGeneration) -> Self {
        Self {
            state: Arc::new(Mutex::new(PricingFenceState {
                generation,
                revision: 0,
                current_key: None,
            })),
        }
    }

    /// Invalidates the current key after any accepted input update.
    ///
    /// This method is intended for owner-side quote updates when a full
    /// replacement key is not yet available.
    ///
    /// 任一输入更新被接受后使当前键失效。当新完整键尚不可用时，由 owner 在行情更新处调用。
    pub fn advance(&self) -> Result<PricingFenceRevision, PricingInputError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| PricingInputError::FencePoisoned)?;
        state.revision = state
            .revision
            .checked_add(1)
            .ok_or(PricingInputError::FenceRevisionExhausted)?;
        state.current_key = None;
        Ok(PricingFenceRevision {
            generation: state.generation,
            revision: state.revision,
        })
    }

    /// Moves this fence to a newer stream generation and invalidates its key.
    /// 将 fence 推进到更新的行情代次并使当前键失效。
    pub fn begin_generation(
        &self,
        generation: PricingGeneration,
    ) -> Result<PricingFenceRevision, PricingInputError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| PricingInputError::FencePoisoned)?;
        if generation <= state.generation {
            return Err(PricingInputError::GenerationRegressed);
        }
        state.revision = state
            .revision
            .checked_add(1)
            .ok_or(PricingInputError::FenceRevisionExhausted)?;
        state.generation = generation;
        state.current_key = None;
        Ok(PricingFenceRevision {
            generation: state.generation,
            revision: state.revision,
        })
    }

    /// Accepts the current full key and returns its fence token.
    ///
    /// A different key advances the fence. Reaccepting the same key is
    /// idempotent, allowing multiple verticals to join one owner's current
    /// input without changing its revision.
    ///
    /// 接受当前完整键并返回 fence 令牌。键变化会推进 fence；重复接受同一键不改变版本，允许多个 vertical 使用同一 owner 当前输入。
    pub fn accept_key(
        &self,
        key: &PricingInputKey,
    ) -> Result<PricingFenceRevision, PricingInputError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| PricingInputError::FencePoisoned)?;
        if key.generation != state.generation {
            return Err(PricingInputError::GenerationMismatch);
        }
        if state.current_key.as_ref() != Some(key) {
            state.revision = state
                .revision
                .checked_add(1)
                .ok_or(PricingInputError::FenceRevisionExhausted)?;
            state.current_key = Some(key.clone());
        }
        Ok(PricingFenceRevision {
            generation: state.generation,
            revision: state.revision,
        })
    }

    fn is_current(&self, key: &PricingInputKey, revision: PricingFenceRevision) -> bool {
        let Ok(state) = self.state.lock() else {
            return false;
        };
        state.generation == revision.generation
            && state.revision == revision.revision
            && revision.generation == key.generation
            && state.current_key.as_ref() == Some(key)
    }

    fn same_owner(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }
}

/// Request for one complete local option pricing input.
/// 单个完整本地期权定价输入请求。
#[derive(Clone, Debug)]
pub struct GreekPricingRequest {
    input: SolverInput,
    key: PricingInputKey,
    fence: PricingInputFence,
    fence_revision: PricingFenceRevision,
}

impl GreekPricingRequest {
    /// Binds a solver input to the owner's accepted complete key and fence token.
    /// 将求解输入绑定到 owner 已接受的完整键和 fence 令牌。
    pub fn new(
        input: SolverInput,
        key: PricingInputKey,
        fence: PricingInputFence,
        fence_revision: PricingFenceRevision,
    ) -> Result<Self, PricingInputError> {
        if !key.matches_input(&input) {
            return Err(PricingInputError::ContractSolverMismatch);
        }
        if !fence.is_current(&key, fence_revision) {
            return Err(PricingInputError::FenceMismatch);
        }
        Ok(Self {
            input,
            key,
            fence,
            fence_revision,
        })
    }
}

/// Errors returned by the independent shared-pricing API. Legacy
/// [`GreekSolverPool`] errors remain unchanged; underlying pool failures are
/// available through [`Self::Pool`].
/// 独立共享定价 API 返回的错误。旧 [`GreekSolverPool`] 错误类型保持不变；线程池错误通过 [`Self::Pool`] 返回。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GreekSingleflightError {
    /// Configured cache capacity is zero or exceeds the crate bound.
    /// 配置容量为零或超过 crate 上限。
    InvalidCacheCapacity,
    /// The caller attempted to move the active generation backwards.
    /// 调用方尝试回退当前代次。
    GenerationRegressed,
    /// A complete key was submitted with a different current-input owner.
    /// 完整键由不同的 current-input owner 提交。
    FenceOwnerMismatch,
    /// The bounded table is full and no completed entry without live handles can be evicted.
    /// 有界表已满，且没有可淘汰的已完成无活动句柄条目。
    CacheSaturated,
    /// A complete key already has the maximum number of live result handles.
    /// 完整键已有达到上限的活动结果句柄。
    WaiterSaturated,
    /// Option or underlying quote provenance is stale at admission.
    /// 接收时的期权或标的行情来源信息已过期。
    StaleInput,
    /// The input fence or generation changed before admission.
    /// 接收前输入 fence 或行情代次已变化。
    CurrentInputChanged,
    /// The singleflight state lock was poisoned.
    /// singleflight 状态锁已中毒。
    StatePoisoned,
    /// The monotonic job identifier was exhausted.
    /// 单调任务标识已耗尽。
    FlightIdExhausted,
    /// The shared job could not enter the existing bounded worker pool.
    /// 共享任务无法进入现有的有界 worker 线程池。
    Pool(PoolError),
}

impl fmt::Display for GreekSingleflightError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCacheCapacity => {
                formatter.write_str("PRICING_SINGLEFLIGHT_INVALID_CACHE_CAPACITY")
            }
            Self::GenerationRegressed => {
                formatter.write_str("PRICING_SINGLEFLIGHT_GENERATION_REGRESSED")
            }
            Self::FenceOwnerMismatch => {
                formatter.write_str("PRICING_SINGLEFLIGHT_FENCE_OWNER_MISMATCH")
            }
            Self::CacheSaturated => formatter.write_str("PRICING_SINGLEFLIGHT_CACHE_SATURATED"),
            Self::WaiterSaturated => formatter.write_str("PRICING_SINGLEFLIGHT_WAITER_SATURATED"),
            Self::StaleInput => formatter.write_str("PRICING_SINGLEFLIGHT_STALE_INPUT"),
            Self::CurrentInputChanged => {
                formatter.write_str("PRICING_SINGLEFLIGHT_CURRENT_INPUT_CHANGED")
            }
            Self::StatePoisoned => formatter.write_str("PRICING_SINGLEFLIGHT_STATE_POISONED"),
            Self::FlightIdExhausted => {
                formatter.write_str("PRICING_SINGLEFLIGHT_FLIGHT_ID_EXHAUSTED")
            }
            Self::Pool(error) => write!(formatter, "PRICING_SINGLEFLIGHT_POOL({error})"),
        }
    }
}

impl std::error::Error for GreekSingleflightError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Pool(error) => Some(error),
            _ => None,
        }
    }
}

/// Result of a shared local pricing calculation.
/// 共享本地定价计算的结果。
#[derive(Clone, Debug, PartialEq)]
pub enum GreekPricingOutcome {
    /// Current local-model observations are available.
    /// 当前本地模型观测可用。
    Available {
        /// Complete key used for the solver.
        /// 本次求解使用的完整键。
        key: PricingInputKey,
        /// Local-model metrics with their original source and units.
        /// 保留原有来源和单位的本地模型指标。
        metrics: OptionMetrics,
    },
    /// The solver evaluated an input exactly at expiration and returned only its intrinsic value.
    /// 求解器在合约精确到期时估值，只返回内在价值。
    AtExpiryIntrinsic {
        /// Complete key used for the solver.
        /// 本次求解使用的完整键。
        key: PricingInputKey,
        /// Exact intrinsic option value.
        /// 精确期权内在价值。
        value: Price,
    },
    /// The local solver could not produce a supported result.
    /// 本地求解器无法生成受支持的结果。
    Unavailable {
        /// Complete key used for the solver.
        /// 本次求解使用的完整键。
        key: PricingInputKey,
        /// Model limitation or convergence reason.
        /// 模型限制或收敛原因。
        reason: ModelUnavailable,
    },
    /// The key was superseded by a new input or generation.
    /// 当前输入或行情代次已变化，该键已过期。
    Stale {
        /// Superseded complete key.
        /// 已过期的完整键。
        key: PricingInputKey,
    },
    /// A worker exited before returning a result.
    /// worker 在返回结果前退出。
    WorkerStopped {
        /// Complete key that was being computed.
        /// 正在计算的完整键。
        key: PricingInputKey,
    },
}

/// Bounded singleflight owner sharing a fixed worker pool.
///
/// Create one owner per solver pool and clone it for consumers of shared option
/// legs. The table limit counts both in-flight and completed entries. In-flight
/// work is never evicted. A completed entry is evictable only after all of its
/// result handles are resolved or dropped. If the table is full with no such
/// victim, admission returns [`GreekSingleflightError::CacheSaturated`]. Each
/// complete key has at most 128 live handles;
/// further joins or cache hits return [`GreekSingleflightError::WaiterSaturated`].
/// Completed entries use bounded LRU eviction.
/// Each [`PricingInputFence`] holds one current key and no history; the market
/// owner controls the number of fences by its bounded contract set.
///
/// 绑定固定 worker pool 的有界 singleflight owner。
///
/// 每个 solver pool 创建一个 owner，并克隆给共享同一 option leg 的消费者。缓存上限同时计入在途与已完成条目；在途工作不淘汰，已完成条目在所有结果句柄都 resolve 或被丢弃前也不会淘汰。若表满且没有这样的淘汰候选则返回 [`GreekSingleflightError::CacheSaturated`]。每个完整键最多有 128 个活动句柄；后续加入或缓存命中返回 [`GreekSingleflightError::WaiterSaturated`]。已完成条目采用有界 LRU 淘汰。每个 [`PricingInputFence`] 只持有一个当前键且不留历史；行情 owner 以其有界合约集合控制 fence 数量。
pub struct GreekSolverSingleflight<'pool> {
    pool: &'pool GreekSolverPool,
    state: Arc<Mutex<SingleflightState>>,
}

impl<'pool> Clone for GreekSolverSingleflight<'pool> {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool,
            state: Arc::clone(&self.state),
        }
    }
}

impl<'pool> GreekSolverSingleflight<'pool> {
    /// Creates an owner with a bounded in-flight plus completed-key table.
    /// 创建具有有界在途与已完成键表的 owner。
    pub fn new(
        pool: &'pool GreekSolverPool,
        cache_capacity: usize,
        initial_generation: PricingGeneration,
    ) -> Result<Self, GreekSingleflightError> {
        if cache_capacity == 0 || cache_capacity > MAX_SINGLEFLIGHT_ENTRIES {
            return Err(GreekSingleflightError::InvalidCacheCapacity);
        }
        Ok(Self {
            pool,
            state: Arc::new(Mutex::new(SingleflightState {
                active_generation: initial_generation,
                capacity: cache_capacity,
                next_flight_id: 1,
                admitted_flights: 0,
                in_flight_joins: 0,
                cache_hits: 0,
                saturations: 0,
                entries: HashMap::with_capacity(cache_capacity),
                lru: VecDeque::with_capacity(cache_capacity),
            })),
        })
    }

    /// Returns the configured maximum number of entries.
    /// 返回配置的最大条目数。
    pub fn capacity(&self) -> usize {
        match self.state.lock() {
            Ok(state) => state.capacity,
            Err(poisoned) => poisoned.into_inner().capacity,
        }
    }

    /// Number of distinct worker jobs successfully admitted since creation.
    ///
    /// This counts only newly admitted flights. It does not count callers that
    /// join an in-flight job or use its completed cache entry.
    /// 创建以来成功接收的不同 worker 作业数；不含加入在途作业或命中完成缓存的调用。
    pub fn admitted_flight_count(&self) -> u64 {
        match self.state.lock() {
            Ok(state) => state.admitted_flights,
            Err(poisoned) => poisoned.into_inner().admitted_flights,
        }
    }

    /// Number of callers that joined a not-yet-completed flight.
    /// 创建以来加入尚未完成 flight 的调用数。
    pub fn in_flight_join_count(&self) -> u64 {
        match self.state.lock() {
            Ok(state) => state.in_flight_joins,
            Err(poisoned) => poisoned.into_inner().in_flight_joins,
        }
    }

    /// Number of callers that found a completed entry in the bounded cache.
    /// 创建以来命中有界完成缓存条目的调用数。
    pub fn cache_hit_count(&self) -> u64 {
        match self.state.lock() {
            Ok(state) => state.cache_hits,
            Err(poisoned) => poisoned.into_inner().cache_hits,
        }
    }

    /// Number of rejected submissions due to waiter, cache, or worker-queue saturation.
    /// 因 waiter、缓存容量或 worker 队列饱和而被拒绝的提交数。
    pub fn saturation_count(&self) -> u64 {
        match self.state.lock() {
            Ok(state) => state.saturations,
            Err(poisoned) => poisoned.into_inner().saturations,
        }
    }

    /// Returns current `(in_flight, completed)` entry occupancy.
    ///
    /// The snapshot scans at most the configured cache capacity and does not
    /// alter the cache or its eviction order.
    /// 返回当前 `(在途, 已完成)` 条目数。此快照最多扫描配置的缓存容量，不改变缓存或淘汰顺序。
    pub fn occupancy_counts(&self) -> (usize, usize) {
        let state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        let in_flight = state
            .entries
            .values()
            .filter(|entry| !entry.completed)
            .count();
        (in_flight, state.entries.len() - in_flight)
    }

    /// Invalidates every entry and moves the owner to a newer stream generation.
    /// 使所有条目失效并将 owner 推进到更新的行情代次。
    pub fn begin_generation(
        &self,
        generation: PricingGeneration,
    ) -> Result<(), GreekSingleflightError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| GreekSingleflightError::StatePoisoned)?;
        if generation <= state.active_generation {
            return Err(GreekSingleflightError::GenerationRegressed);
        }
        state.active_generation = generation;
        state.entries.clear();
        state.lru.clear();
        Ok(())
    }

    /// Attempts to join an existing flight/cache entry or admit one bounded job.
    /// `checked_at_ms` is used only for freshness checks; it is not part of the key.
    /// 尝试加入现有 flight/缓存，或接收一个有界求解任务。`checked_at_ms` 只用于新鲜度检查，不进入键。
    pub fn try_submit(
        &self,
        request: GreekPricingRequest,
        checked_at_ms: i64,
    ) -> Result<GreekPricingJobHandle, GreekSingleflightError> {
        request
            .key
            .validate_fresh_at(checked_at_ms)
            .map_err(|_| GreekSingleflightError::StaleInput)?;
        request
            .input
            .validate_fresh_at(checked_at_ms)
            .map_err(|_| GreekSingleflightError::StaleInput)?;

        let key = EntryKey {
            input: request.key.clone(),
            fence_revision: request.fence_revision,
        };
        let mut state = self
            .state
            .lock()
            .map_err(|_| GreekSingleflightError::StatePoisoned)?;
        if state.active_generation != request.key.generation {
            return Err(GreekSingleflightError::CurrentInputChanged);
        }
        if !request
            .fence
            .is_current(&request.key, request.fence_revision)
        {
            return Err(GreekSingleflightError::CurrentInputChanged);
        }

        let existing = state.entries.get(&key).map(|entry| {
            (
                entry.fence.same_owner(&request.fence),
                entry.completed,
                entry.flight_id,
                Arc::clone(&entry.waiters),
                entry.result_sender.clone(),
            )
        });
        if let Some((same_owner, completed, flight_id, waiters, result_sender)) = existing {
            if !same_owner {
                return Err(GreekSingleflightError::FenceOwnerMismatch);
            }
            if !try_acquire_waiter(&waiters) {
                state.saturations = state.saturations.saturating_add(1);
                return Err(GreekSingleflightError::WaiterSaturated);
            }
            let receiver = result_sender.subscribe();
            if completed {
                state.cache_hits = state.cache_hits.saturating_add(1);
            } else {
                state.in_flight_joins = state.in_flight_joins.saturating_add(1);
            }
            touch_lru(&mut state.lru, &key);
            return Ok(GreekPricingJobHandle {
                receiver,
                key: request.key,
                input: request.input,
                fence: request.fence,
                fence_revision: request.fence_revision,
                entry_key: key,
                flight_id,
                state: Arc::clone(&self.state),
                _waiter_lease: WaiterLease { waiters },
            });
        }

        let victim = if state.entries.len() >= state.capacity {
            match find_oldest_completed(&state) {
                Some(victim) => Some(victim),
                None => {
                    state.saturations = state.saturations.saturating_add(1);
                    return Err(GreekSingleflightError::CacheSaturated);
                }
            }
        } else {
            None
        };
        let flight_id = state.next_flight_id;
        let next_flight_id = flight_id
            .checked_add(1)
            .ok_or(GreekSingleflightError::FlightIdExhausted)?;
        let (result_sender, receiver) = watch::channel(None);
        let waiters = Arc::new(AtomicUsize::new(1));
        let completion = SharedJobCompletion {
            state: Arc::clone(&self.state),
            key: key.clone(),
            flight_id,
            fence: request.fence.clone(),
            input: request.input.clone(),
            result_sender: result_sender.clone(),
            stale_results: Arc::clone(&self.pool.stale_results),
        };
        if let Err(error) = self
            .pool
            .try_enqueue_shared(request.input.clone(), completion)
        {
            if error == PoolError::Saturated {
                state.saturations = state.saturations.saturating_add(1);
            }
            return Err(GreekSingleflightError::Pool(error));
        }

        if let Some(victim) = victim {
            remove_entry(&mut state, &victim);
        }
        state.next_flight_id = next_flight_id;
        state.entries.insert(
            key.clone(),
            SingleflightEntry {
                flight_id,
                fence: request.fence.clone(),
                result_sender,
                completed: false,
                waiters: Arc::clone(&waiters),
            },
        );
        state.lru.push_back(key.clone());
        state.admitted_flights = state.admitted_flights.saturating_add(1);
        Ok(GreekPricingJobHandle {
            receiver,
            key: request.key,
            input: request.input,
            fence: request.fence,
            fence_revision: request.fence_revision,
            entry_key: key,
            flight_id,
            state: Arc::clone(&self.state),
            _waiter_lease: WaiterLease { waiters },
        })
    }

    #[cfg(test)]
    fn entry_counts(&self) -> (usize, usize) {
        self.state
            .lock()
            .map(|state| {
                let in_flight = state
                    .entries
                    .values()
                    .filter(|entry| !entry.completed)
                    .count();
                (in_flight, state.entries.len() - in_flight)
            })
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct EntryKey {
    input: PricingInputKey,
    fence_revision: PricingFenceRevision,
}

struct SingleflightEntry {
    flight_id: u64,
    fence: PricingInputFence,
    result_sender: watch::Sender<Option<SharedWorkerResult>>,
    completed: bool,
    waiters: Arc<AtomicUsize>,
}

struct WaiterLease {
    waiters: Arc<AtomicUsize>,
}

impl Drop for WaiterLease {
    fn drop(&mut self) {
        let previous = self
            .waiters
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                count.checked_sub(1)
            });
        debug_assert!(previous.is_ok(), "singleflight waiter count underflow");
    }
}

fn try_acquire_waiter(waiters: &AtomicUsize) -> bool {
    let mut current = waiters.load(Ordering::Relaxed);
    loop {
        if current >= MAX_SINGLEFLIGHT_WAITERS_PER_KEY {
            return false;
        }
        match waiters.compare_exchange_weak(
            current,
            current + 1,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return true,
            Err(observed) => current = observed,
        }
    }
}

struct SingleflightState {
    active_generation: PricingGeneration,
    capacity: usize,
    next_flight_id: u64,
    admitted_flights: u64,
    in_flight_joins: u64,
    cache_hits: u64,
    saturations: u64,
    entries: HashMap<EntryKey, SingleflightEntry>,
    lru: VecDeque<EntryKey>,
}

#[derive(Clone, Debug)]
enum SharedWorkerResult {
    Completed(SolverOutcome),
    Stale,
    WorkerStopped,
}

/// Shared result handle. Dropping a receiver never cancels the bounded owner job.
/// 共享结果句柄；丢弃接收端不会取消 owner 的有界任务。
pub struct GreekPricingJobHandle {
    receiver: watch::Receiver<Option<SharedWorkerResult>>,
    key: PricingInputKey,
    input: SolverInput,
    fence: PricingInputFence,
    fence_revision: PricingFenceRevision,
    entry_key: EntryKey,
    flight_id: u64,
    state: Arc<Mutex<SingleflightState>>,
    _waiter_lease: WaiterLease,
}

impl GreekPricingJobHandle {
    /// Awaits a shared result and rechecks both quote ages and the current key.
    /// 等待共享结果，并重新检查两路行情年龄与当前键。
    pub async fn resolve(mut self, checked_at_ms: i64) -> GreekPricingOutcome {
        let result = loop {
            let current = { self.receiver.borrow().clone() };
            if let Some(result) = current {
                break result;
            }
            if self.receiver.changed().await.is_err() {
                break SharedWorkerResult::WorkerStopped;
            }
        };

        if !self.is_current() {
            return GreekPricingOutcome::Stale { key: self.key };
        }
        let key_freshness = self.key.validate_fresh_at(checked_at_ms);
        let solver_freshness = self.input.validate_fresh_at(checked_at_ms);
        if key_freshness.is_err() || solver_freshness.is_err() {
            if let Ok(mut state) = self.state.lock() {
                remove_entry_if_flight_id(&mut state, &self.entry_key, self.flight_id);
            }
            return GreekPricingOutcome::Unavailable {
                key: self.key,
                reason: key_freshness
                    .err()
                    .map(|_| ModelUnavailable::StaleInput)
                    .or_else(|| solver_freshness.err().map(solver_input_unavailable))
                    .unwrap_or(ModelUnavailable::StaleInput),
            };
        }
        match result {
            SharedWorkerResult::Completed(SolverOutcome::Available(metrics)) => {
                GreekPricingOutcome::Available {
                    key: self.key,
                    metrics,
                }
            }
            SharedWorkerResult::Completed(SolverOutcome::AtExpiryIntrinsic(value)) => {
                GreekPricingOutcome::AtExpiryIntrinsic {
                    key: self.key,
                    value,
                }
            }
            SharedWorkerResult::Completed(SolverOutcome::Unavailable(reason)) => {
                GreekPricingOutcome::Unavailable {
                    key: self.key,
                    reason,
                }
            }
            SharedWorkerResult::Stale => GreekPricingOutcome::Stale { key: self.key },
            SharedWorkerResult::WorkerStopped => {
                GreekPricingOutcome::WorkerStopped { key: self.key }
            }
        }
    }

    fn is_current(&self) -> bool {
        let Ok(state) = self.state.lock() else {
            return false;
        };
        state.active_generation == self.key.generation
            && self.fence.is_current(&self.key, self.fence_revision)
    }
}

fn find_oldest_completed(state: &SingleflightState) -> Option<EntryKey> {
    state.lru.iter().find_map(|key| {
        state
            .entries
            .get(key)
            .filter(|entry| entry.completed && entry.waiters.load(Ordering::Relaxed) == 0)
            .map(|_| key.clone())
    })
}

fn touch_lru(lru: &mut VecDeque<EntryKey>, key: &EntryKey) {
    if let Some(position) = lru.iter().position(|candidate| candidate == key) {
        lru.remove(position);
    }
    lru.push_back(key.clone());
}

fn remove_entry(state: &mut SingleflightState, key: &EntryKey) {
    state.entries.remove(key);
    if let Some(position) = state.lru.iter().position(|candidate| candidate == key) {
        state.lru.remove(position);
    }
}

fn remove_entry_if_flight(state: &mut SingleflightState, key: &EntryKey, flight_id: u64) {
    if state
        .entries
        .get(key)
        .is_some_and(|entry| entry.flight_id == flight_id && !entry.completed)
    {
        remove_entry(state, key);
    }
}

fn remove_entry_if_flight_id(state: &mut SingleflightState, key: &EntryKey, flight_id: u64) {
    if state
        .entries
        .get(key)
        .is_some_and(|entry| entry.flight_id == flight_id)
    {
        remove_entry(state, key);
    }
}

fn solver_input_unavailable(error: SolverInputError) -> ModelUnavailable {
    match error {
        SolverInputError::Expired => ModelUnavailable::Expired,
        SolverInputError::DividendEvidenceStale => ModelUnavailable::DividendEvidenceStale,
        _ => ModelUnavailable::StaleInput,
    }
}

pub(super) struct SharedJobCompletion {
    state: Arc<Mutex<SingleflightState>>,
    key: EntryKey,
    flight_id: u64,
    fence: PricingInputFence,
    input: SolverInput,
    result_sender: watch::Sender<Option<SharedWorkerResult>>,
    stale_results: Arc<AtomicU64>,
}

impl SharedJobCompletion {
    pub(super) fn is_current(&self) -> bool {
        let Ok(state) = self.state.lock() else {
            return false;
        };
        state.active_generation == self.key.input.generation
            && self.key.input.matches_input(&self.input)
            && self
                .input
                .validate_fresh_at(self.input.valuation_at_ms())
                .is_ok()
            && self
                .key
                .input
                .validate_fresh_at(self.input.valuation_at_ms())
                .is_ok()
            && state.entries.get(&self.key).is_some_and(|entry| {
                !entry.completed
                    && entry.flight_id == self.flight_id
                    && entry.fence.same_owner(&self.fence)
            })
            && self
                .fence
                .is_current(&self.key.input, self.key.fence_revision)
    }

    pub(super) fn complete(self, result: SolverOutcome) {
        let mut completed_result = SharedWorkerResult::Completed(result);
        let Ok(mut state) = self.state.lock() else {
            increment_counter(&self.stale_results);
            self.result_sender
                .send_replace(Some(SharedWorkerResult::Stale));
            return;
        };
        let current = state.active_generation == self.key.input.generation
            && self.key.input.matches_input(&self.input)
            && self
                .input
                .validate_fresh_at(self.input.valuation_at_ms())
                .is_ok()
            && self
                .key
                .input
                .validate_fresh_at(self.input.valuation_at_ms())
                .is_ok()
            && self
                .fence
                .is_current(&self.key.input, self.key.fence_revision)
            && state.entries.get(&self.key).is_some_and(|entry| {
                !entry.completed
                    && entry.flight_id == self.flight_id
                    && entry.fence.same_owner(&self.fence)
            });
        if current {
            if let Some(entry) = state.entries.get_mut(&self.key) {
                entry.completed = true;
            }
        } else {
            increment_counter(&self.stale_results);
            remove_entry_if_flight(&mut state, &self.key, self.flight_id);
            completed_result = SharedWorkerResult::Stale;
        }
        self.result_sender.send_replace(Some(completed_result));
        drop(state);
    }

    pub(super) fn stopped(self) {
        if let Ok(mut state) = self.state.lock() {
            remove_entry_if_flight(&mut state, &self.key, self.flight_id);
            self.result_sender
                .send_replace(Some(SharedWorkerResult::WorkerStopped));
        } else {
            self.result_sender
                .send_replace(Some(SharedWorkerResult::WorkerStopped));
        }
    }
}

#[cfg(test)]
#[path = "singleflight/tests.rs"]
mod tests;
