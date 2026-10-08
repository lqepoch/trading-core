# Pricing

This crate keeps money exact in the domain types and performs bounded
floating-point calculations only inside analytical solvers. Synthetic vertical
prices are references for analysis, not guaranteed fill prices. IV and Greeks
are observations, not execution or risk authority.

本 crate 在领域类型中精确保存金额，仅在解析求解器内部执行有界浮点计算。合成价差价格仅供分析参考，不保证可成交或成交价。IV 与 Greeks 是观测值，不是执行或风控授权依据。

## Expiry and dividend inputs

`SolverInput::new` receives exact `Price` and `Strike` values, an explicit
exercise style, a typed underlying, a fresh quote observation, and an
`ExpirationContext`. Time to expiry is derived from UTC epoch-millisecond
instants using ACT/365 Fixed; callers cannot independently provide a year
fraction and a potentially conflicting expiry class. Each instant's UTC offset
must arithmetically map to its supplied local-day ordinal. The context retains
the caller's timezone identifier and offsets as evidence, but does not validate
an IANA timezone, exchange calendar, or contract qualification.

`ZeroDaysToExpiry` is derived by comparing the supplied local-day ordinals.
Positive same-day expiry has at most 24 hours remaining. An exact expiry
returns exact intrinsic value only, without time value or Greeks; an already
expired contract is rejected. These input checks do not replace the upstream
contract resolver.

For positive time, `NoDividends` and `ContinuousYield` require a
`DividendWindowEvidence` record for the same typed underlying. Evidence carries
a market-data metadata reference, nonzero revision, observation time, effective
window, and coverage kind. Input admission checks the underlying, coverage, and
that the effective window spans valuation through expiry. Every synchronous
solver requires an explicit `evaluation_at_ms` UTC epoch-millisecond argument
and rechecks quote freshness and dividend-evidence age/window at that time. An
evidence age exactly equal to the configured maximum is accepted; a future
observation or older evidence is unavailable. Effective-window endpoints are
inclusive, so evidence ending exactly at contract expiry covers the interval.
The pool rechecks the same conditions at `GreekJobHandle::resolve` time before
returning a result. This is caller-provided evidence; it does not authenticate
a provider response or qualify an option contract. A continuous yield is a
model approximation, not an equivalent representation of a discrete cash
dividend schedule.

Declaring a known discrete or unknown schedule returns
`DividendAssumptionUnsupported`; evidence that reports an ex-date cannot be
paired with `NoDividends` and is rejected as an input mismatch. Default strategy
underlyings QQQ and SPY do not imply a no-dividend interval: the adapter must
supply timely, same-underlying evidence covering the entire option horizon.
Contract/provider resolution and dividend-schedule acquisition belong to
upstream adapters and are not implemented by this crate. This crate cannot
establish that evidence in production.

`SolverInput::new` 使用精确 `Price`/`Strike`、显式行权方式、类型化标的、新鲜行情
证据和 `ExpirationContext`。它按 ACT/365 Fixed 从 UTC Unix 毫秒派生剩余时间；调用方
不能另传可能与到期分类冲突的年分数。每个时刻的 UTC 偏移必须与提供的本地日期序号
算术一致。上下文会保留调用方时区 ID 和偏移，但不会验证 IANA 时区、交易所日历或
合约资格。

`ZeroDaysToExpiry` 由本地日期序号比较得出。同日到期最多允许 24 小时剩余时间。精确
到期只返回精确内在价值，不含时间价值或 Greeks；已过期合约会被拒绝。这些校验不
替代上游合约解析器。

正剩余时间下，`NoDividends` 与 `ContinuousYield` 必须提供同一类型化标的的
`DividendWindowEvidence`。证据包含行情元数据引用、非零 revision、观测时刻、有效
窗口和覆盖类型。输入校验会核对标的、覆盖语义，以及有效窗口是否覆盖整个估值至
到期区间。每个同步求解入口都要求显式传入 `evaluation_at_ms`（UTC Unix 毫秒），并在
该时刻重新检查行情新鲜度、股息证据年龄和有效窗口。证据年龄恰好达到最大值时接受；
时间戳来自未来或证据更旧时返回不可用。有效窗口端点包含在覆盖范围内，因此恰好
结束于合约到期时刻的证据可以覆盖整个窗口。线程池在 `GreekJobHandle::resolve` 时刻
重新核验这些条件后才返回结果。这只是调用方提供的证据，不认证供应商响应，也不
完成期权合约资格核验。连续收益率是模型近似，不等同于离散现金股息日程。

声明已知离散或未知股息日程会返回 `DividendAssumptionUnsupported`；若证据报告除息
事件，则不能与 `NoDividends` 一起使用，会作为输入不匹配拒绝。默认策略标的 QQQ
与 SPY 不代表区间内无股息；适配器必须提供及时、标的相符并覆盖完整期限的证据。
合约/供应商解析与股息日程获取属于上游适配层，不由本 crate 实现。本 crate 不会在
生产中自行证明这些证据。

## European Black-Scholes

`solve_european_black_scholes(input, evaluation_at_ms)` supports European
exercise with either an evidence-backed no-cash-dividend interval or an
explicitly supplied continuous-yield approximation. The bounded IV search uses
96 bisection iterations,
volatility bounds `[0.0001, 5.0]`, and a `1e-10` model-price stopping
tolerance. The stopping tolerance bounds the numerical search only; it is not
a market-data or model-accuracy guarantee. At exact expiry the solver returns
intrinsic value without model Greeks.

At expiry, a snapshot valued at an earlier instant is expired; intrinsic-only
output applies only when valuation and evaluation are exactly at expiry.

Theta is reported per ACT/365F model-year fraction per underlying unit. Delta
and gamma are per underlying unit and per squared underlying-dollar move;
vega is per `1.0` volatility fraction and underlying unit; rho is per `1.0`
annual rate unit and underlying unit. Every model value is finite and range
checked. Per-leg IV remains separate; `LongMinusShortIvDifference` names the
signed IV difference, and `aggregate_greek` combines directional quantities
and validated multipliers without aggregating IV.

`solve_european_black_scholes(input, evaluation_at_ms)` 支持欧式行权，并要求有证据覆盖的
无现金股息区间，或明确提供连续收益率近似。IV 搜索固定为 96 次二分，波动率范围为
`[0.0001, 5.0]`，模型价格停止容差为 `1e-10`。该容差只约束数值搜索，不保证行情
精度或模型准确度。精确到期只返回内在价值，不返回模型 Greeks。
在到期时刻，估值于更早时刻的快照会被视为已到期；仅当估值与检查时刻都恰好为
到期时，才允许只返回内在价值。

Theta 单位是每 ACT/365F 模型年分数、每标的单位。Delta 按每标的单位计量，Gamma
按每标的美元变动平方计量；Vega 按波动率小数增加 `1.0`、每标的单位计量；Rho
按年化利率增加 `1.0`、每标的单位计量。所有模型数值均检查有限性和范围。每腿 IV
独立保存；`LongMinusShortIvDifference` 明确表示有方向的 IV 差值；`aggregate_greek`
按腿方向、数量和经过验证的乘数汇总，不聚合 IV。

## American pricing status: PARTIAL

The public `solve_american_crr(input, evaluation_at_ms)` entrypoint does not
publish an American price, IV, or Greek for positive remaining time. Once
freshness is rechecked at the explicit evaluation time and the one-minute
resolution boundary passes, it returns the typed
`AmericanPricingAccuracyUnverified` result. Less than one minute returns
`TimeBelowResolution`; unknown or discrete dividend schedules return
`DividendAssumptionUnsupported`. At exact expiry, the shared exact intrinsic
path is still available.

The CRR tree retained in `solver/american_crr.rs` is compiled only for unit
tests. It has fixed 256/257 steps, bounded terminal storage (257 and 258
values), and rejects invalid risk-neutral probabilities and non-finite values.
These invariants do not establish price or IV accuracy and the production
entrypoint cannot expose its candidate values. The provenance identifier
`AmericanCrrV1` is reserved, and metric validation rejects it until the
accuracy gate passes. The negative-rate American put
upper bound uses `K * exp(max(-r, 0) * T)` rather than assuming the bound is
always `K`.

An independent reference source is pinned to QuantLib v1.43 commit
[`6b57206e04598f092efee66e3b367efc84771995`](https://github.com/lballabio/QuantLib/blob/6b57206e04598f092efee66e3b367efc84771995/test-suite/americanoption.cpp),
`test-suite/americanoption.cpp`. Its `FdBlackScholesVanillaEngine(process,
100, 400)` constructor uses `tGrid=100` time points and `xGrid=400` price
points, with `Actual360` and American exercise dates. Its price cases use
rounded Ju (1999) published values and a `$0.08` upstream tolerance. Those
values are historical cross-method references only: they are not captured
QuantLib PDE outputs, and QuantLib has not been installed or run in this
environment. They are not used as a local pass gate.

That pinned test does not provide fixed portable CRR Greek outputs. Its delta
and gamma checks compare bumps from the same finite-difference engine, and its
Theta test is disabled. Bump stability is not an independent Greek oracle.
Independent price/IV/Greek fixtures, convergence studies, frozen unit-specific
error budgets, and validated dividend schedules remain BLOCKED. Model accuracy,
provider evidence resolution, and strategy/coordinator integration are not
complete.

公共入口 `solve_american_crr(input, evaluation_at_ms)` 在正剩余时间下不会发布美式价格、
IV 或 Greek。按显式检查时刻重新核验输入新鲜度，并通过一分钟分辨率边界后，它仍返回类型化的
`AmericanPricingAccuracyUnverified`。不足一分钟返回 `TimeBelowResolution`；未知或
离散股息日程返回 `DividendAssumptionUnsupported`。精确到期仍可通过共享路径返回
精确内在价值。

`solver/american_crr.rs` 中保留的 CRR 树仅在单元测试中编译。它固定使用 256/257
步，终端存储最多为 257 和 258 个值，并拒绝无效风险中性概率和非有限数值。这些
不变量不能证明价格或 IV 准确度，生产入口也不能暴露候选结果。来源标识
`AmericanCrrV1` 仅作预留；精度门通过前，指标校验会拒绝此来源。负利率美式看跌期权
的上界使用 `K * exp(max(-r, 0) * T)`，不再一律假设上界为 `K`。

独立参考来源固定为 QuantLib v1.43 commit
[`6b57206e04598f092efee66e3b367efc84771995`](https://github.com/lballabio/QuantLib/blob/6b57206e04598f092efee66e3b367efc84771995/test-suite/americanoption.cpp)
中的 `test-suite/americanoption.cpp`。`FdBlackScholesVanillaEngine(process,
100, 400)` 构造函数参数是 `tGrid=100` 个时间点、`xGrid=400` 个价格点，并使用
`Actual360` 和美式行权日期。其价格样例使用 Ju (1999) 已发表的舍入数值及上游
`$0.08` 容差。这些仅是历史跨方法参考，不是保存的 QuantLib PDE 输出；当前环境
没有安装或运行 QuantLib，也不使用这些数值作为本地通过门槛。

该固定测试未提供可移植的 CRR Greek 数值。其 Delta/Gamma 检查把同一有限差分引擎
的输出与自身价格 bump 比较，Theta 测试已禁用；bump 稳定性不是独立 Greek oracle。
独立价格/IV/Greeks fixtures、收敛研究、按单位冻结的误差预算和经验证的股息日程仍为
BLOCKED。模型准确度、供应商证据解析以及策略/coordinator 接线尚未完成。

## Shared local singleflight

`GreekSolverSingleflight` is a bounded API layered on the compatible
`GreekSolverPool`. Build each `OptionPriceInput` and `UnderlyingPriceInput`
from one accepted quote update and pass the bindings intact to
`PricingInputKey::new`. Each binding holds its exact price, identity, source,
revision, quality, observation/receive times, and freshness limit together.
Key construction checks the full option contract, the solver underlying and
spot, the exact option premium, and the option observation against the solver
input. The immutable valuation timestamp must match `SolverInput` exactly.

The V2 key fingerprint includes every solver input and assumption: underlying,
exact spot/strike/premium, normalized floating-point rate and derived ACT/365F
time, exercise style, timezone identifier, both UTC expiry instants including
their offsets and local-day ordinals, exact valuation time, and the full
dividend evidence identity, revision, observation time, effective interval,
and coverage. The selected key model is `BlackScholesEuropeanV2`. The caller's
current `checked_at_ms` is used for freshness checks and does not become part of
the calculation key. `AtExpiryIntrinsic` remains a separate typed outcome and
never carries model Greeks. Provider-native metrics retain their original
source and unit; this local solver path does not merge or relabel them.

The market owner creates one `PricingInputFence` per option contract and
advances it immediately after accepting either an option or underlying update,
even when the update schedules no solver work. A generation change also
advances the singleflight generation. The worker only caches a completion when
the entry still belongs to the active generation and the owner's current key.
Every admission, in-flight join, completed-cache hit, and
`GreekPricingJobHandle::resolve(now_ms)` rechecks both quote bindings and the
solver quote, expiry, and dividend evidence at the time supplied by the caller.
Expired or stale evidence returns a typed unavailable outcome; a cache hit
never renews its freshness. A stale resolution removes its matching cache
entry. Runtime coordinator/strategy publication remains unwired and is not
claimed by this crate API.

Cache capacity (1–4,096) bounds in-flight plus completed key entries. In-flight
entries cannot be evicted. Completed entries use LRU eviction only after all
handles resolve or drop; if no completed entry is available, admission returns
`CacheSaturated`. Each key admits at most 128 live handles, including the
initial request, in-flight joins, and cache hits; further requests return
`WaiterSaturated`. RAII releases each slot on resolution or cancellation.
Dropping all waiters does not cancel the worker, and the bounded worker may
retain its completed result for a later fresh cache hit. Worker queue saturation
remains the separate `Pool(PoolError::Saturated)` result. Each fence retains
one current key and no history; its owner must keep its contract set bounded.
The decimal `synthetic_debit_vertical` path is not part of this solver cache.

`benches/greek_singleflight.rs` compares 100 synthetic vertical consumers with
two legs each (200 logical requests) and 2, 10, or 50 unique complete keys. The
direct pool path admits 200 jobs; singleflight admits 2, 10, or 50 jobs in the
first wave. A separate second wave measures completed-cache hits. The harness
reports admission/join/cache/saturation counts, occupancy, p50/p95/p99, wave
time, process CPU/RSS, and labels generation-fence late-completion timing
`NOT_AVAILABLE` because it has no controlled benchmark completion gate.
Deterministic stale-completion behavior is covered by tests, not measured as a
performance result.

`GreekSolverSingleflight` 是建立在兼容 `GreekSolverPool` 上的有界 API。每个
`OptionPriceInput` 和 `UnderlyingPriceInput` 都应从同一次已接受的行情更新创建，并将
完整绑定传给 `PricingInputKey::new`。绑定在同一对象中保存精确价格、身份、来源、版本、
质量、观测/接收时刻和新鲜度上限。构造键时会校验完整期权合约、求解输入标的与 spot、
精确 option premium，以及求解输入与期权行情观测。不可变估值时刻必须与 `SolverInput`
完全一致。

V2 键指纹包含全部求解输入和假设：标的、精确 spot/strike/premium、规范化浮点利率与
ACT/365F 派生时间、行权方式、时区标识、估值和到期两个 UTC 时刻及其偏移/本地日期序号、
精确估值时刻，以及完整股息证据身份、revision、观测时间、有效区间和覆盖类型。所选模型
版本为 `BlackScholesEuropeanV2`。调用方传入的当前 `checked_at_ms` 只用于新鲜度核验，
不会进入计算键。`AtExpiryIntrinsic` 是独立的类型化结果，不含模型 Greeks。Provider-native
指标保留原来源和单位；本地求解路径不合并或重标这些指标。

行情 owner 为每个期权合约创建一个 `PricingInputFence`，接受期权或标的任一更新后立即推进，
即使本次没有提交求解任务。代次变化也会推进 singleflight generation。只有条目仍属于当前
代次且 owner 的当前键匹配时，worker 才缓存完成结果。每次接收、加入在途任务、命中完成缓存，
以及 `GreekPricingJobHandle::resolve(now_ms)` 都会按调用方提供的当前时刻重新核验两路行情和
solver 的行情、到期及股息证据。过期输入或陈旧证据返回类型化不可用结果；缓存命中不会续鲜。
陈旧 resolve 会移除匹配缓存条目。运行时 coordinator/strategy 发布仍未接线，本 crate 不声称
已完成该集成。

缓存容量（1–4,096）同时限制在途与完成键条目。在途项不淘汰；完成项仅在所有句柄 resolve 或
drop 后参加 LRU 淘汰。没有可淘汰完成项时返回 `CacheSaturated`。每键最多接收 128 个活动句柄，
包括首次请求、在途加入与缓存命中；超过上限返回 `WaiterSaturated`。句柄通过 RAII 在 resolve
或取消时释放名额。丢弃全部 waiter 不取消 worker；有界 worker 可以保留完成结果，供之后重新
验证新鲜度的缓存命中使用。worker 队列饱和仍返回独立的 `Pool(PoolError::Saturated)`。每个 fence
仅保留一个当前键且不留历史；owner 必须限制其合约集合。Decimal `synthetic_debit_vertical`
路径不进入该 solver 缓存。

`benches/greek_singleflight.rs` 对比 100 个 synthetic vertical consumers、每个 2 条腿（200 个
逻辑请求）和 2、10、50 个不同完整键。direct pool 首波接收 200 个作业；singleflight 首波接收
2、10、50 个作业。第二波单独测量完成缓存命中。harness 输出接收/加入/缓存命中/饱和数、占用、
p50/p95/p99、波次耗时、进程 CPU/RSS；generation fence 延迟完成时延因无可控 benchmark gate
标记为 `NOT_AVAILABLE`。确定性旧结果隔离由测试覆盖，不作为性能测量结果。

## Greek solver pool

`GreekSolverPool` is the existing single bounded CPU pool: 1–16 workers and a
1–512 waiting-job queue. `try_submit` is nonblocking and returns `Saturated`
when full. Requests and results carry quote revisions; stale results contain
no metrics. `GreekJobHandle::resolve(evaluation_at_ms)` rechecks quote
freshness, dividend-evidence freshness/window, and revision at the actual
result-check time before returning a result.
The pool performs computation only. It does not publish into actors,
persistence, execution, or strategies. `GreekSolverSingleflight` shares work
and caches results within its explicitly bounded owner; it is not a runtime
coordinator or strategy publication path. Runtime wiring is outside this crate
and is not claimed here.

`GreekSolverPool` 是现有唯一有界 CPU 池：1–16 个 worker，等待队列容量 1–512。
`try_submit` 非阻塞；队列满时返回 `Saturated`。请求与结果携带行情 revision；旧
结果不含指标。`GreekJobHandle::resolve(evaluation_at_ms)` 会在实际结果检查时刻重新核验
行情新鲜度、股息证据新鲜度/窗口和 revision 后才返回结果。线程池只执行计算，不向
actor、持久化、执行或策略发布结果。`GreekSolverSingleflight` 在其显式有界 owner 中共享并缓存
任务；它不是运行时 coordinator 或策略发布路径。运行时接线不属于本 crate，本 crate 不
声称已完成此集成。
