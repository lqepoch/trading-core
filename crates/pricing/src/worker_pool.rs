//! Bounded CPU worker pool for revision-fenced Greek calculations.
//!
//! The pool never publishes or authorizes results. Callers must recheck the
//! quote revision at the point where a result is consumed.
//!
//! ## 简体中文
//!
//! 本模块为 Greek 计算提供有界 CPU worker pool。线程池不会发布或授权结果；调用方必须在消费结果时重新检查行情 revision。

use std::{
    error::Error,
    fmt,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
};

use tokio::sync::oneshot;

use crate::{
    greeks::OptionMetrics,
    singleflight::SharedJobCompletion,
    solver::{ModelUnavailable, SolverInput, SolverOutcome, solve_option_model},
};

const MAX_WORKERS: usize = 16;
const MAX_QUEUE_CAPACITY: usize = 512;

/// Monotonic quote revision attached to every expensive pricing job.
/// 简体中文：附加到每个高开销定价任务上的单调递增行情版本号。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct QuoteRevision(u64);

impl QuoteRevision {
    /// Creates a revision from its caller-managed sequence value.
    /// Callers should increment it whenever the accepted quote state changes.
    /// 简体中文：使用调用方管理的序列值创建版本号。每当接受的行情状态变化时，调用方应递增该值。
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the underlying sequence value.
    /// 简体中文：返回底层序列值。
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Shared revision fence for one quote stream. Advance this at the point a new
/// quote revision is accepted; jobs and results are then rejected as stale if
/// the fence has moved on. The fence is not an actor or publication cache.
/// 简体中文：单个行情流共享的 revision fence。接受新行情后推进 fence，旧任务或结果会被判为过期；它不是 actor 或结果发布缓存。
#[derive(Clone, Debug)]
pub struct QuoteRevisionFence {
    current: Arc<AtomicU64>,
}

impl QuoteRevisionFence {
    /// Starts a fence at the accepted quote revision.
    /// 简体中文：使用当前已接受的行情版本初始化 fence。
    pub fn new(initial: QuoteRevision) -> Self {
        Self {
            current: Arc::new(AtomicU64::new(initial.0)),
        }
    }

    /// Returns the latest accepted revision.
    /// 简体中文：返回最近接受的版本号。
    pub fn current(&self) -> QuoteRevision {
        QuoteRevision(self.current.load(Ordering::Acquire))
    }

    /// Advances the fence monotonically; equal revisions are accepted.
    /// A lower revision returns [`PoolError::RevisionRegressed`].
    /// 简体中文：单调推进 fence；允许重复写入相同版本，较低版本会返回 [`PoolError::RevisionRegressed`]。
    pub fn advance_to(&self, revision: QuoteRevision) -> Result<(), PoolError> {
        let mut current = self.current.load(Ordering::Acquire);
        loop {
            if revision.0 < current {
                return Err(PoolError::RevisionRegressed);
            }
            if revision.0 == current {
                return Ok(());
            }
            match self.current.compare_exchange_weak(
                current,
                revision.0,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(()),
                Err(actual) => current = actual,
            }
        }
    }
}

/// One solver request, bound to the quote stream revision fence.
/// 简体中文：绑定到行情流 revision fence 的单个求解请求。
#[derive(Clone, Debug)]
pub struct GreekSolverRequest {
    input: SolverInput,
    revision: QuoteRevision,
    fence: QuoteRevisionFence,
}

impl GreekSolverRequest {
    /// Bind an immutable model input to the accepted revision for its quote
    /// stream. The request is rejected on admission if the fence has advanced.
    /// 简体中文：将不可变模型输入绑定到行情流已接受的版本；若提交时 fence 已推进，请求会被拒绝。
    pub const fn new(
        input: SolverInput,
        revision: QuoteRevision,
        fence: QuoteRevisionFence,
    ) -> Self {
        Self {
            input,
            revision,
            fence,
        }
    }
}

/// Solver completion after freshness and quote-revision fencing.
/// 简体中文：完成行情新鲜度与 revision fence 检查后的求解结果。
#[derive(Clone, Debug, PartialEq)]
pub enum GreekJobOutcome {
    /// The model returned metrics for the still-current quote revision.
    /// 简体中文：模型针对当前行情版本返回了指标。
    Available {
        /// Quote revision used for the calculation.
        /// 简体中文：本次计算使用的行情版本。
        revision: QuoteRevision,
        /// Validated analytical observations; they do not authorize trading.
        /// 简体中文：已验证的解析观测值；不构成交易授权。
        metrics: OptionMetrics,
    },
    /// The option is exactly at expiry; only an exact intrinsic price is returned.
    /// 简体中文：期权恰好到期；只返回精确内在价值。
    AtExpiryIntrinsic {
        /// Quote revision used for the calculation.
        /// 简体中文：本次计算使用的行情版本。
        revision: QuoteRevision,
        /// Exact intrinsic price in the domain money type.
        /// 简体中文：使用领域金额类型表示的精确内在价值。
        value: domain::Price,
    },
    /// The model could not produce a supported result for this revision.
    /// 简体中文：模型无法为此版本生成受支持的结果。
    Unavailable {
        /// Quote revision used for the calculation.
        /// 简体中文：本次计算使用的行情版本。
        revision: QuoteRevision,
        /// Reason the analytical model was unavailable.
        /// 简体中文：解析模型不可用的原因。
        reason: ModelUnavailable,
    },
    /// The fence advanced while the request was queued or computed.
    /// 简体中文：任务排队或计算期间行情版本 fence 已推进。
    Stale {
        /// Revision attached to the submitted job.
        /// 简体中文：提交任务时绑定的版本。
        job_revision: QuoteRevision,
        /// Revision currently accepted by the quote stream.
        /// 简体中文：行情流当前接受的版本。
        current_revision: QuoteRevision,
    },
    /// The worker exited before returning a result.
    /// 简体中文：worker 在返回结果前退出。
    WorkerStopped {
        /// Quote revision attached to the incomplete job.
        /// 简体中文：未完成任务绑定的行情版本。
        revision: QuoteRevision,
    },
}

/// Nonblocking admission handle for a completed or pending solver job.
/// 简体中文：已完成或等待中的求解任务的非阻塞接收句柄。
pub struct GreekJobHandle {
    receiver: oneshot::Receiver<WorkerResult>,
    input: SolverInput,
    revision: QuoteRevision,
    fence: QuoteRevisionFence,
    stale_results: Arc<AtomicU64>,
}

impl GreekJobHandle {
    /// Await one worker result without occupying a Tokio worker thread. The
    /// caller supplies current UTC epoch milliseconds after completion so a
    /// queue delay cannot make an old quote appear fresh.
    /// Rechecks both freshness and the quote revision before returning metrics.
    /// 简体中文：异步等待 worker 结果且不占用 Tokio worker 线程；调用方需在完成后提供当前 UTC Unix 毫秒时刻。返回指标前会重新检查行情新鲜度和 revision，避免排队延迟让旧行情显得新鲜。
    pub async fn resolve(self, checked_at_ms: i64) -> GreekJobOutcome {
        match self.receiver.await {
            Ok(WorkerResult::Stale { current_revision }) => GreekJobOutcome::Stale {
                job_revision: self.revision,
                current_revision,
            },
            Ok(WorkerResult::WorkerStopped { revision }) => {
                GreekJobOutcome::WorkerStopped { revision }
            }
            Ok(WorkerResult::Completed(result)) => {
                let current_revision = self.fence.current();
                if current_revision != self.revision {
                    increment_counter(&self.stale_results);
                    return GreekJobOutcome::Stale {
                        job_revision: self.revision,
                        current_revision,
                    };
                }
                if let Err(error) = self.input.validate_fresh_at(checked_at_ms) {
                    return GreekJobOutcome::Unavailable {
                        revision: self.revision,
                        reason: match error {
                            crate::solver::SolverInputError::Expired => ModelUnavailable::Expired,
                            crate::solver::SolverInputError::DividendEvidenceStale => {
                                ModelUnavailable::DividendEvidenceStale
                            }
                            _ => ModelUnavailable::StaleInput,
                        },
                    };
                }
                match result {
                    SolverOutcome::Available(metrics) => GreekJobOutcome::Available {
                        revision: self.revision,
                        metrics,
                    },
                    SolverOutcome::AtExpiryIntrinsic(value) => GreekJobOutcome::AtExpiryIntrinsic {
                        revision: self.revision,
                        value,
                    },
                    SolverOutcome::Unavailable(reason) => GreekJobOutcome::Unavailable {
                        revision: self.revision,
                        reason,
                    },
                }
            }
            Err(_) => GreekJobOutcome::WorkerStopped {
                revision: self.revision,
            },
        }
    }
}

/// Fixed-thread, bounded-queue CPU solver. `try_submit` is nonblocking: when
/// every worker and queue slot is busy it returns `Saturated` immediately.
/// A pool is computational only; it does not publish results to actors or
/// persistence. Consumers must still compare the returned revision with their
/// quote authority at the point they use a result.
/// 简体中文：固定线程数、有限队列的 CPU 求解器。所有 worker 和队列槽位都繁忙时，`try_submit` 会立即返回 `Saturated`。线程池仅做计算，不会向 actor 或持久化层发布结果；使用结果前，调用方仍须与权威行情 revision 比较。
pub struct GreekSolverPool {
    sender: Option<SyncSender<WorkerJob>>,
    workers: Vec<JoinHandle<()>>,
    pub(super) stale_results: Arc<AtomicU64>,
    worker_count: usize,
    queue_capacity: usize,
}

impl GreekSolverPool {
    /// Creates a fixed worker pool with a bounded waiting queue.
    /// Worker count and queue capacity must be within crate limits.
    /// 简体中文：创建固定数量 worker 和有界等待队列的线程池。worker 数量及队列容量必须处于 crate 限制内。
    pub fn new(worker_count: usize, queue_capacity: usize) -> Result<Self, PoolError> {
        Self::new_with_solver(
            worker_count,
            queue_capacity,
            Arc::new(|input| solve_option_model(input, input.valuation_at_ms())),
        )
    }

    /// Returns the number of operating-system workers.
    /// 简体中文：返回操作系统 worker 的数量。
    pub fn worker_count(&self) -> usize {
        self.worker_count
    }

    /// Returns the number of queued jobs the pool can hold.
    /// 简体中文：返回队列可容纳的等待任务数量。
    pub fn queue_capacity(&self) -> usize {
        self.queue_capacity
    }

    /// Number of stale revision submissions or results discarded. Counter
    /// saturation is sticky at `u64::MAX`.
    /// 简体中文：返回因 revision 过期而丢弃的提交或结果数量；计数饱和后保持为 `u64::MAX`。
    pub fn stale_result_count(&self) -> u64 {
        self.stale_results.load(Ordering::Relaxed)
    }

    /// Attempts nonblocking job admission after checking the request revision.
    /// A full queue returns [`PoolError::Saturated`] immediately.
    /// 简体中文：检查请求版本后尝试非阻塞地接收任务；队列已满时立即返回 [`PoolError::Saturated`]。
    pub fn try_submit(&self, request: GreekSolverRequest) -> Result<GreekJobHandle, PoolError> {
        if request.fence.current() != request.revision {
            increment_counter(&self.stale_results);
            return Err(PoolError::StaleRevision);
        }
        let sender = self.sender.as_ref().ok_or(PoolError::Closed)?;
        let (result_sender, result_receiver) = oneshot::channel();
        let job = WorkerJob {
            input: request.input.clone(),
            completion: WorkerJobCompletion::Legacy {
                revision: request.revision,
                fence: request.fence.clone(),
                result_sender,
            },
        };
        match sender.try_send(job) {
            Ok(()) => Ok(GreekJobHandle {
                receiver: result_receiver,
                input: request.input,
                revision: request.revision,
                fence: request.fence,
                stale_results: Arc::clone(&self.stale_results),
            }),
            Err(TrySendError::Full(_)) => Err(PoolError::Saturated),
            Err(TrySendError::Disconnected(_)) => Err(PoolError::Closed),
        }
    }

    /// Close admission and asynchronously join the fixed workers. Joining runs
    /// on Tokio's blocking pool so shutdown does not occupy an async executor
    /// thread while bounded in-flight calculations drain.
    /// 简体中文：关闭新任务接收，并在 Tokio blocking pool 上异步等待固定 worker 退出，避免在有界任务排空期间占用 async executor 线程。
    pub async fn shutdown(mut self) -> Result<(), PoolError> {
        let runtime =
            tokio::runtime::Handle::try_current().map_err(|_| PoolError::RuntimeUnavailable)?;
        self.sender.take();
        let workers = std::mem::take(&mut self.workers);
        runtime
            .spawn_blocking(move || {
                for worker in workers {
                    if worker.join().is_err() {
                        return Err(PoolError::WorkerShutdownFailed);
                    }
                }
                Ok(())
            })
            .await
            .map_err(|_| PoolError::WorkerShutdownFailed)?
    }

    pub(crate) fn new_with_solver(
        worker_count: usize,
        queue_capacity: usize,
        solver: Arc<dyn Fn(&SolverInput) -> SolverOutcome + Send + Sync>,
    ) -> Result<Self, PoolError> {
        if worker_count == 0 || worker_count > MAX_WORKERS {
            return Err(PoolError::InvalidWorkerCount);
        }
        if queue_capacity == 0 || queue_capacity > MAX_QUEUE_CAPACITY {
            return Err(PoolError::InvalidQueueCapacity);
        }

        let (sender, receiver) = mpsc::sync_channel::<WorkerJob>(queue_capacity);
        let receiver = Arc::new(Mutex::new(receiver));
        let stale_results = Arc::new(AtomicU64::new(0));
        let mut workers = Vec::with_capacity(worker_count);
        for index in 0..worker_count {
            let receiver = Arc::clone(&receiver);
            let solver = Arc::clone(&solver);
            let stale_results = Arc::clone(&stale_results);
            let name = format!("pricing-greeks-{index}");
            match thread::Builder::new()
                .name(name)
                .spawn(move || worker_loop(receiver, solver, stale_results))
            {
                Ok(worker) => workers.push(worker),
                Err(_) => {
                    drop(sender);
                    drop(workers);
                    return Err(PoolError::WorkerStartFailed);
                }
            }
        }

        Ok(Self {
            sender: Some(sender),
            workers,
            stale_results,
            worker_count,
            queue_capacity,
        })
    }

    pub(super) fn try_enqueue_shared(
        &self,
        input: SolverInput,
        completion: SharedJobCompletion,
    ) -> Result<(), PoolError> {
        let sender = self.sender.as_ref().ok_or(PoolError::Closed)?;
        sender
            .try_send(WorkerJob {
                input,
                completion: WorkerJobCompletion::Shared(Box::new(completion)),
            })
            .map_err(|error| match error {
                TrySendError::Full(_) => PoolError::Saturated,
                TrySendError::Disconnected(_) => PoolError::Closed,
            })
    }
}

impl Drop for GreekSolverPool {
    fn drop(&mut self) {
        self.sender.take();
        // Dropping JoinHandles detaches them; synchronous drop may run on a
        // Tokio worker thread. They observe the closed channel and drain only
        // the bounded queue. Call `shutdown().await` to join deterministically.
        self.workers.clear();
    }
}

fn worker_loop(
    receiver: Arc<Mutex<Receiver<WorkerJob>>>,
    solver: Arc<dyn Fn(&SolverInput) -> SolverOutcome + Send + Sync>,
    stale_results: Arc<AtomicU64>,
) {
    loop {
        let received = match receiver.lock() {
            Ok(receiver) => receiver.recv(),
            Err(_) => return,
        };
        let job = match received {
            Ok(job) => job,
            Err(_) => return,
        };

        let may_compute = match &job.completion {
            WorkerJobCompletion::Legacy {
                revision, fence, ..
            } => fence.current() == *revision,
            WorkerJobCompletion::Shared(completion) => completion.is_current(),
        };
        if !may_compute {
            match job.completion {
                WorkerJobCompletion::Legacy {
                    fence,
                    result_sender,
                    ..
                } => {
                    increment_counter(&stale_results);
                    let current_revision = fence.current();
                    let _ = result_sender.send(WorkerResult::Stale { current_revision });
                }
                WorkerJobCompletion::Shared(completion) => {
                    completion.complete(SolverOutcome::Unavailable(ModelUnavailable::StaleInput));
                }
            }
            continue;
        }

        match catch_unwind(AssertUnwindSafe(|| solver(&job.input))) {
            Ok(computed) => match job.completion {
                WorkerJobCompletion::Legacy {
                    revision,
                    fence,
                    result_sender,
                } => {
                    let current_revision = fence.current();
                    let result = if current_revision != revision {
                        increment_counter(&stale_results);
                        WorkerResult::Stale { current_revision }
                    } else {
                        WorkerResult::Completed(computed)
                    };
                    let _ = result_sender.send(result);
                }
                WorkerJobCompletion::Shared(completion) => completion.complete(computed),
            },
            Err(_) => match job.completion {
                WorkerJobCompletion::Legacy {
                    revision,
                    result_sender,
                    ..
                } => {
                    let _ = result_sender.send(WorkerResult::WorkerStopped { revision });
                }
                WorkerJobCompletion::Shared(completion) => completion.stopped(),
            },
        }
    }
}

pub(super) fn increment_counter(counter: &AtomicU64) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
        Some(value.saturating_add(1))
    });
}

struct WorkerJob {
    input: SolverInput,
    completion: WorkerJobCompletion,
}

enum WorkerJobCompletion {
    Legacy {
        revision: QuoteRevision,
        fence: QuoteRevisionFence,
        result_sender: oneshot::Sender<WorkerResult>,
    },
    Shared(Box<SharedJobCompletion>),
}

enum WorkerResult {
    Completed(SolverOutcome),
    Stale { current_revision: QuoteRevision },
    WorkerStopped { revision: QuoteRevision },
}

/// Pool admission, revision, and lifecycle errors. These codes reveal no
/// inputs, quote values, account data, or provider response content.
/// 简体中文：线程池接收、revision 和生命周期错误；错误码不包含输入、行情、账户或 provider 响应内容。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PoolError {
    /// Worker count is zero or exceeds the configured maximum.
    /// 简体中文：worker 数为零或超过配置上限。
    InvalidWorkerCount,
    /// Queue capacity is zero or exceeds the configured maximum.
    /// 简体中文：队列容量为零或超过配置上限。
    InvalidQueueCapacity,
    /// A worker operating-system thread could not be started.
    /// 简体中文：无法启动 worker 操作系统线程。
    WorkerStartFailed,
    /// The bounded queue has no free slot.
    /// 简体中文：有界队列没有空闲槽位。
    Saturated,
    /// The pool no longer accepts work.
    /// 简体中文：线程池已关闭，不再接收任务。
    Closed,
    /// The request revision does not match the current fence.
    /// 简体中文：请求版本与当前 fence 不匹配。
    StaleRevision,
    /// A caller attempted to move a fence to an older revision.
    /// 简体中文：调用方尝试将 fence 回退到更旧的版本。
    RevisionRegressed,
    /// Shutdown was requested outside a Tokio runtime.
    /// 简体中文：当前不在 Tokio runtime 中，无法执行关闭流程。
    RuntimeUnavailable,
    /// A worker failed to join during shutdown.
    /// 简体中文：关闭过程中 worker join 失败。
    WorkerShutdownFailed,
}

impl fmt::Display for PoolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidWorkerCount => "PRICING_POOL_INVALID_WORKER_COUNT",
            Self::InvalidQueueCapacity => "PRICING_POOL_INVALID_QUEUE_CAPACITY",
            Self::WorkerStartFailed => "PRICING_POOL_WORKER_START_FAILED",
            Self::Saturated => "PRICING_POOL_SATURATED",
            Self::Closed => "PRICING_POOL_CLOSED",
            Self::StaleRevision => "PRICING_POOL_STALE_REVISION",
            Self::RevisionRegressed => "PRICING_POOL_REVISION_REGRESSED",
            Self::RuntimeUnavailable => "PRICING_POOL_RUNTIME_UNAVAILABLE",
            Self::WorkerShutdownFailed => "PRICING_POOL_WORKER_SHUTDOWN_FAILED",
        })
    }
}

impl Error for PoolError {}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc, Condvar, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use domain::{
        MarketDataProviderId, MetadataSource, Price, ProviderMetadataKind, ProviderMetadataRef,
        ProviderRecordId, Strike, Underlying,
    };

    use crate::{
        DividendAssumption, DividendCoverageKind, DividendWindowEvidence, ExchangeInstant,
        ExerciseStyle, ExpirationClass, ExpirationContext, OptionKind, SolverInput, SolverOutcome,
    };

    use super::{
        GreekJobOutcome, GreekSolverPool, GreekSolverRequest, ModelUnavailable, PoolError,
        QuoteRevision, QuoteRevisionFence,
    };

    fn input() -> SolverInput {
        input_with_style(ExerciseStyle::European)
    }

    fn input_with_style(exercise_style: ExerciseStyle) -> SolverInput {
        input_with_style_and_dividend_observation(exercise_style, 10_000)
    }

    fn input_with_style_and_dividend_observation(
        exercise_style: ExerciseStyle,
        dividend_observed_at_ms: i64,
    ) -> SolverInput {
        let checked_at_ms = 10_000;
        let expiry_at_ms = checked_at_ms + 182 * 86_400_000 + 43_200_000;
        let expiry_context = ExpirationContext::new(
            ExpirationClass::NonZeroDaysToExpiry,
            "UTC",
            ExchangeInstant::new(checked_at_ms, 0, 0).expect("valuation"),
            ExchangeInstant::new(expiry_at_ms, 0, 182).expect("expiry"),
        )
        .expect("expiry context");
        let dividend_evidence = DividendWindowEvidence::new(
            Underlying::new("QQQ").expect("underlying"),
            ProviderMetadataRef::new(
                MetadataSource::MarketData(MarketDataProviderId::Alpaca),
                ProviderMetadataKind::MarketData,
                ProviderRecordId::new("synthetic-dividend-window").expect("record id"),
            ),
            1,
            dividend_observed_at_ms,
            0,
            expiry_at_ms,
            DividendCoverageKind::NoCashDividendInWindow,
        )
        .expect("dividend evidence");
        SolverInput::new(
            OptionKind::Call,
            Underlying::new("QQQ").expect("underlying"),
            Price::parse_json_number("100").expect("spot"),
            Strike::parse_json_number("100").expect("strike"),
            Price::parse_json_number("6.12").expect("premium"),
            0.02,
            exercise_style,
            expiry_context,
            DividendAssumption::NoDividends,
            Some(dividend_evidence),
            checked_at_ms,
            checked_at_ms,
            2_000,
        )
        .expect("solver input")
    }

    fn exact_expiry_input() -> SolverInput {
        let expiry_at_ms = 10_000;
        let expiry_context = ExpirationContext::new(
            ExpirationClass::ZeroDaysToExpiry,
            "UTC",
            ExchangeInstant::new(expiry_at_ms, 0, 0).expect("valuation"),
            ExchangeInstant::new(expiry_at_ms, 0, 0).expect("expiry"),
        )
        .expect("expiry context");
        SolverInput::new(
            OptionKind::Call,
            Underlying::new("QQQ").expect("underlying"),
            Price::parse_json_number("100").expect("spot"),
            Strike::parse_json_number("100").expect("strike"),
            Price::parse_json_number("0").expect("premium"),
            0.02,
            ExerciseStyle::European,
            expiry_context,
            DividendAssumption::NoDividends,
            None,
            expiry_at_ms,
            expiry_at_ms,
            2_000,
        )
        .expect("exact-expiry solver input")
    }

    fn gate() -> Arc<(Mutex<(usize, bool)>, Condvar)> {
        Arc::new((Mutex::new((0, false)), Condvar::new()))
    }

    fn blocking_solver(
        gate: Arc<(Mutex<(usize, bool)>, Condvar)>,
    ) -> Arc<dyn Fn(&SolverInput) -> SolverOutcome + Send + Sync> {
        Arc::new(move |_| {
            let (lock, condition) = &*gate;
            let mut state = lock.lock().expect("test gate lock");
            state.0 += 1;
            condition.notify_all();
            while !state.1 {
                state = condition.wait(state).expect("test gate wait");
            }
            SolverOutcome::Unavailable(ModelUnavailable::NonConvergent)
        })
    }

    fn wait_started(gate: &Arc<(Mutex<(usize, bool)>, Condvar)>, count: usize) {
        let (lock, condition) = &**gate;
        let state = lock.lock().expect("test gate lock");
        let (mut state, timeout) = condition
            .wait_timeout_while(state, Duration::from_secs(2), |state| state.0 < count)
            .expect("test gate wait");
        assert!(!timeout.timed_out());
        assert!(state.0 >= count);
        state.1 = true;
        condition.notify_all();
    }

    fn request(revision: u64, fence: QuoteRevisionFence) -> GreekSolverRequest {
        GreekSolverRequest::new(input(), QuoteRevision::new(revision), fence)
    }

    #[tokio::test]
    async fn full_queue_returns_saturated_without_waiting_for_worker() {
        let gate = gate();
        let solver = blocking_solver(Arc::clone(&gate));
        let pool = GreekSolverPool::new_with_solver(1, 1, solver).expect("pool");
        let fence = QuoteRevisionFence::new(QuoteRevision::new(1));
        let first = pool
            .try_submit(request(1, fence.clone()))
            .expect("first queued");
        {
            let (lock, condition) = &*gate;
            let state = lock.lock().expect("test gate lock");
            let (state, timeout) = condition
                .wait_timeout_while(state, Duration::from_secs(2), |state| state.0 < 1)
                .expect("test gate wait");
            assert!(!timeout.timed_out());
            assert_eq!(state.0, 1);
        }
        let queued = pool
            .try_submit(request(1, fence.clone()))
            .expect("one bounded slot");
        assert!(matches!(
            pool.try_submit(request(1, fence)),
            Err(PoolError::Saturated)
        ));
        wait_started(&gate, 1);
        assert_eq!(
            first.resolve(10_000).await,
            GreekJobOutcome::Unavailable {
                revision: QuoteRevision::new(1),
                reason: ModelUnavailable::NonConvergent,
            }
        );
        assert_eq!(
            queued.resolve(10_000).await,
            GreekJobOutcome::Unavailable {
                revision: QuoteRevision::new(1),
                reason: ModelUnavailable::NonConvergent,
            }
        );
        assert_eq!(pool.shutdown().await, Ok(()));
    }

    #[tokio::test]
    async fn old_revision_is_discarded_after_quote_fence_advances() {
        let gate = gate();
        let solver = blocking_solver(Arc::clone(&gate));
        let pool = GreekSolverPool::new_with_solver(1, 1, solver).expect("pool");
        let fence = QuoteRevisionFence::new(QuoteRevision::new(7));
        let old = pool
            .try_submit(request(7, fence.clone()))
            .expect("old job queued");
        let (lock, condition) = &*gate;
        let state = lock.lock().expect("test gate lock");
        let (state, timeout) = condition
            .wait_timeout_while(state, Duration::from_secs(2), |state| state.0 < 1)
            .expect("test gate wait");
        assert!(!timeout.timed_out());
        drop(state);

        fence
            .advance_to(QuoteRevision::new(8))
            .expect("monotonic revision");
        assert!(matches!(
            pool.try_submit(request(7, fence.clone())),
            Err(PoolError::StaleRevision)
        ));
        let current = pool
            .try_submit(request(8, fence.clone()))
            .expect("current revision accepted");
        wait_started(&gate, 1);
        assert_eq!(
            old.resolve(10_000).await,
            GreekJobOutcome::Stale {
                job_revision: QuoteRevision::new(7),
                current_revision: QuoteRevision::new(8),
            }
        );
        assert_eq!(pool.stale_result_count(), 2);
        assert_eq!(
            current.resolve(10_000).await,
            GreekJobOutcome::Unavailable {
                revision: QuoteRevision::new(8),
                reason: ModelUnavailable::NonConvergent,
            }
        );
        assert_eq!(pool.shutdown().await, Ok(()));
    }

    #[test]
    fn rejects_unbounded_worker_configuration_and_revision_regression() {
        assert!(matches!(
            GreekSolverPool::new(0, 1),
            Err(PoolError::InvalidWorkerCount)
        ));
        assert!(matches!(
            GreekSolverPool::new(1, 0),
            Err(PoolError::InvalidQueueCapacity)
        ));
        let fence = QuoteRevisionFence::new(QuoteRevision::new(4));
        assert_eq!(
            fence.advance_to(QuoteRevision::new(3)),
            Err(PoolError::RevisionRegressed)
        );
    }

    #[tokio::test]
    async fn queued_result_that_ages_out_is_unavailable() {
        let pool = GreekSolverPool::new(1, 1).expect("pool");
        let fence = QuoteRevisionFence::new(QuoteRevision::new(1));
        let handle = pool.try_submit(request(1, fence)).expect("job admitted");
        assert_eq!(
            handle.resolve(12_001).await,
            GreekJobOutcome::Unavailable {
                revision: QuoteRevision::new(1),
                reason: ModelUnavailable::StaleInput,
            }
        );
        assert_eq!(pool.shutdown().await, Ok(()));
    }

    #[tokio::test]
    async fn identical_inputs_are_admitted_as_independent_pool_jobs() {
        let gate = gate();
        let solver_calls = Arc::new(AtomicUsize::new(0));
        let solver_calls_for_job = Arc::clone(&solver_calls);
        let solve = blocking_solver(Arc::clone(&gate));
        let solver = Arc::new(move |input: &SolverInput| {
            solver_calls_for_job.fetch_add(1, Ordering::Relaxed);
            solve(input)
        });
        let solver: Arc<dyn Fn(&SolverInput) -> SolverOutcome + Send + Sync> = solver;
        let pool = GreekSolverPool::new_with_solver(2, 4, solver).expect("pool");
        let fence = QuoteRevisionFence::new(QuoteRevision::new(1));

        let first = pool
            .try_submit(request(1, fence.clone()))
            .expect("first input admitted");
        let second = pool
            .try_submit(request(1, fence))
            .expect("same input admitted as a separate pool job");

        wait_started(&gate, 2);
        assert_eq!(
            solver_calls.load(Ordering::Relaxed),
            2,
            "the unkeyed worker pool runs one solver job per submission"
        );
        let expected = GreekJobOutcome::Unavailable {
            revision: QuoteRevision::new(1),
            reason: ModelUnavailable::NonConvergent,
        };
        assert_eq!(first.resolve(10_000).await, expected);
        assert_eq!(second.resolve(10_000).await, expected);
        assert_eq!(pool.shutdown().await, Ok(()));
    }

    #[tokio::test]
    async fn result_resolution_rechecks_dividend_evidence_at_exact_age_boundary() {
        let solver: Arc<dyn Fn(&SolverInput) -> SolverOutcome + Send + Sync> =
            Arc::new(|_| SolverOutcome::Unavailable(ModelUnavailable::NonConvergent));
        let pool = GreekSolverPool::new_with_solver(1, 1, Arc::clone(&solver)).expect("pool");
        let fence = QuoteRevisionFence::new(QuoteRevision::new(3));
        let fresh_at_boundary =
            input_with_style_and_dividend_observation(ExerciseStyle::European, 9_000);
        let accepted = pool
            .try_submit(GreekSolverRequest::new(
                fresh_at_boundary,
                QuoteRevision::new(3),
                fence.clone(),
            ))
            .expect("input fresh at admission");
        assert_eq!(
            accepted.resolve(11_000).await,
            GreekJobOutcome::Unavailable {
                revision: QuoteRevision::new(3),
                reason: ModelUnavailable::NonConvergent,
            }
        );
        assert_eq!(pool.shutdown().await, Ok(()));

        let pool = GreekSolverPool::new_with_solver(1, 1, solver).expect("pool");
        let fence = QuoteRevisionFence::new(QuoteRevision::new(4));
        let stale_dividend_but_fresh_quote =
            input_with_style_and_dividend_observation(ExerciseStyle::European, 9_000);
        assert_eq!(stale_dividend_but_fresh_quote.observed_at_ms(), 10_000);
        let rejected = pool
            .try_submit(GreekSolverRequest::new(
                stale_dividend_but_fresh_quote,
                QuoteRevision::new(4),
                fence,
            ))
            .expect("input fresh at admission");
        assert_eq!(
            rejected.resolve(11_001).await,
            GreekJobOutcome::Unavailable {
                revision: QuoteRevision::new(4),
                reason: ModelUnavailable::DividendEvidenceStale,
            }
        );
        assert_eq!(pool.shutdown().await, Ok(()));
    }

    #[tokio::test]
    async fn exact_expiry_is_a_distinct_intrinsic_only_result() {
        let pool = GreekSolverPool::new(1, 1).expect("pool");
        let revision = QuoteRevision::new(5);
        let handle = pool
            .try_submit(GreekSolverRequest::new(
                exact_expiry_input(),
                revision,
                QuoteRevisionFence::new(revision),
            ))
            .expect("exact-expiry job admitted");
        assert_eq!(
            handle.resolve(10_000).await,
            GreekJobOutcome::AtExpiryIntrinsic {
                revision,
                value: Price::parse_json_number("0").expect("zero intrinsic"),
            }
        );
        assert_eq!(pool.shutdown().await, Ok(()));
    }

    #[tokio::test]
    async fn pool_does_not_publish_unverified_american_candidate_metrics() {
        let pool = GreekSolverPool::new(1, 1).expect("pool");
        let fence = QuoteRevisionFence::new(QuoteRevision::new(9));
        let handle = pool
            .try_submit(GreekSolverRequest::new(
                input_with_style(ExerciseStyle::American),
                QuoteRevision::new(9),
                fence,
            ))
            .expect("American model request admitted");
        assert_eq!(
            handle.resolve(10_000).await,
            GreekJobOutcome::Unavailable {
                revision: QuoteRevision::new(9),
                reason: ModelUnavailable::AmericanPricingAccuracyUnverified,
            }
        );
        assert_eq!(pool.shutdown().await, Ok(()));
    }
}
