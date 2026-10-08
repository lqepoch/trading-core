#![forbid(unsafe_code)]

use std::{env, fs, process::Command, time::Instant};

use domain::{
    ContractCurrency, ContractMultiplier, Deliverable, DeliverableComponent, ExactDecimal,
    ExpirationDate, MarketDataProviderId, MetadataSource, OptionContract, OptionRight,
    OptionSymbol, Price, ProviderMetadataKind, ProviderMetadataRef, ProviderRecordId, Strike,
    Underlying,
};
use pricing::{
    DividendAssumption, DividendCoverageKind, DividendWindowEvidence, ExchangeInstant,
    ExerciseStyle, ExpirationClass, ExpirationContext, GreekJobOutcome, GreekPricingOutcome,
    GreekPricingRequest, GreekSolverPool, GreekSolverRequest, GreekSolverSingleflight, OptionKind,
    OptionPriceInput, PricingGeneration, PricingInputFence, PricingInputKey,
    PricingQuoteProvenance, PricingQuoteQuality, PricingSourceId, QuoteRevision,
    QuoteRevisionFence, SolverInput, UnderlyingPriceInput,
};
use tokio::task::JoinSet;

const VERTICAL_CONSUMERS: usize = 100;
const LEGS_PER_VERTICAL: usize = 2;
const LOGICAL_REQUESTS: usize = VERTICAL_CONSUMERS * LEGS_PER_VERTICAL;
const WORKER_COUNT: usize = 4;
const WORKER_QUEUE_CAPACITY: usize = 512;
const SINGLEFLIGHT_CACHE_CAPACITY: usize = 50;
const GENERATION: PricingGeneration = PricingGeneration::new(1);
const NOW_MS: i64 = 10_000;

#[derive(Clone, Copy)]
enum Mode {
    Direct,
    Singleflight,
}

impl Mode {
    const fn name(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Singleflight => "singleflight",
        }
    }
}

#[derive(Clone, Copy)]
struct Config {
    mode: Mode,
    unique_keys: usize,
    repeats: usize,
}

struct Scenario {
    input: SolverInput,
    key: PricingInputKey,
    input_fence: PricingInputFence,
    input_fence_revision: pricing::PricingFenceRevision,
    quote_revision: QuoteRevision,
    quote_fence: QuoteRevisionFence,
}

#[derive(Default)]
struct PhaseSample {
    accepted_requests: u64,
    worker_jobs: u64,
    in_flight_joins: u64,
    cache_hits: u64,
    saturations: u64,
    in_flight_after: usize,
    completed_after: usize,
    wall_ns: u128,
    request_latency_ns: Vec<u128>,
    process_cpu_ns: Option<u128>,
    process_rss_bytes: Option<u64>,
    process_hwm_bytes: Option<u64>,
}

#[derive(Default)]
struct PhaseAggregate {
    accepted_requests: u64,
    worker_jobs: u64,
    in_flight_joins: u64,
    cache_hits: u64,
    saturations: u64,
    max_in_flight_after: usize,
    max_completed_after: usize,
    wall_ns: Vec<u128>,
    request_latency_ns: Vec<u128>,
    process_cpu_ns: Option<u128>,
    process_cpu_unavailable: bool,
    max_process_rss_bytes: Option<u64>,
    max_process_hwm_bytes: Option<u64>,
}

impl PhaseAggregate {
    fn record(&mut self, sample: PhaseSample) {
        self.accepted_requests += sample.accepted_requests;
        self.worker_jobs += sample.worker_jobs;
        self.in_flight_joins += sample.in_flight_joins;
        self.cache_hits += sample.cache_hits;
        self.saturations += sample.saturations;
        self.max_in_flight_after = self.max_in_flight_after.max(sample.in_flight_after);
        self.max_completed_after = self.max_completed_after.max(sample.completed_after);
        self.wall_ns.push(sample.wall_ns);
        self.request_latency_ns.extend(sample.request_latency_ns);
        match sample.process_cpu_ns {
            Some(next) if !self.process_cpu_unavailable => {
                self.process_cpu_ns = Some(self.process_cpu_ns.unwrap_or_default() + next);
            }
            None => {
                self.process_cpu_ns = None;
                self.process_cpu_unavailable = true;
            }
            Some(_) => {}
        }
        self.max_process_rss_bytes =
            max_option(self.max_process_rss_bytes, sample.process_rss_bytes);
        self.max_process_hwm_bytes =
            max_option(self.max_process_hwm_bytes, sample.process_hwm_bytes);
    }
}

fn main() {
    if let Err(error) = entry() {
        eprintln!("greek_singleflight benchmark: {error}");
        std::process::exit(2);
    }
}

fn entry() -> Result<(), String> {
    let config = parse_config()?;
    let ticks_per_second = process_clock_ticks_per_second();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("cannot create Tokio runtime: {error}"))?;
    runtime.block_on(run(config, ticks_per_second))
}

fn parse_config() -> Result<Config, String> {
    let mut mode = None;
    let mut unique_keys = None;
    let mut repeats = 30;
    let mut args = env::args().skip(1);
    while let Some(argument) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {argument}; {USAGE}"))?;
        match argument.as_str() {
            "--mode" => {
                mode = Some(match value.as_str() {
                    "direct" => Mode::Direct,
                    "singleflight" => Mode::Singleflight,
                    _ => return Err(format!("invalid mode {value}; {USAGE}")),
                });
            }
            "--unique-keys" => {
                unique_keys = Some(
                    value
                        .parse::<usize>()
                        .map_err(|_| format!("invalid unique-key count {value}; {USAGE}"))?,
                );
            }
            "--repeats" => {
                repeats = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid repeat count {value}; {USAGE}"))?;
            }
            _ => return Err(format!("unknown argument {argument}; {USAGE}")),
        }
    }
    let mode = mode.ok_or_else(|| format!("--mode is required; {USAGE}"))?;
    let unique_keys = unique_keys.ok_or_else(|| format!("--unique-keys is required; {USAGE}"))?;
    if !matches!(unique_keys, 2 | 10 | 50) {
        return Err(format!("unique keys must be 2, 10, or 50; {USAGE}"));
    }
    if repeats == 0 || repeats > 1_000 {
        return Err(format!("repeats must be in 1..=1000; {USAGE}"));
    }
    Ok(Config {
        mode,
        unique_keys,
        repeats,
    })
}

const USAGE: &str =
    "usage: greek_singleflight --mode direct|singleflight --unique-keys 2|10|50 [--repeats N]";

async fn run(config: Config, ticks_per_second: Option<u64>) -> Result<(), String> {
    let mut first_wave = PhaseAggregate::default();
    let mut second_wave = PhaseAggregate::default();

    for _ in 0..config.repeats {
        let pool = GreekSolverPool::new(WORKER_COUNT, WORKER_QUEUE_CAPACITY)
            .map_err(|error| format!("cannot create worker pool: {error}"))?;
        let scenarios = build_scenarios(config.unique_keys);
        let order = workload_order(config.unique_keys);
        match config.mode {
            Mode::Direct => {
                first_wave
                    .record(run_direct_wave(&pool, &scenarios, &order, ticks_per_second).await?);
                second_wave
                    .record(run_direct_wave(&pool, &scenarios, &order, ticks_per_second).await?);
            }
            Mode::Singleflight => {
                let owner =
                    GreekSolverSingleflight::new(&pool, SINGLEFLIGHT_CACHE_CAPACITY, GENERATION)
                        .map_err(|error| format!("cannot create singleflight owner: {error}"))?;
                first_wave.record(
                    run_singleflight_wave(&owner, &scenarios, &order, ticks_per_second).await?,
                );
                second_wave.record(
                    run_singleflight_wave(&owner, &scenarios, &order, ticks_per_second).await?,
                );
            }
        }
        pool.shutdown()
            .await
            .map_err(|error| format!("worker shutdown failed: {error}"))?;
    }

    print_phase("first_wave", config, &first_wave);
    print_phase("second_wave_same_keys", config, &second_wave);
    println!(
        "CONFIG mode={} unique_complete_keys={} vertical_consumers={} legs_per_vertical={} logical_requests_per_wave={} worker_count={} worker_queue_capacity={} repeats={}",
        config.mode.name(),
        config.unique_keys,
        VERTICAL_CONSUMERS,
        LEGS_PER_VERTICAL,
        LOGICAL_REQUESTS,
        WORKER_COUNT,
        WORKER_QUEUE_CAPACITY,
        config.repeats,
    );
    println!(
        "GENERATION_FENCE_LATE_COMPLETION=NOT_AVAILABLE note=benchmark_uses_production_solver_without_a_controlled_completion_gate"
    );
    Ok(())
}

async fn run_direct_wave(
    pool: &GreekSolverPool,
    scenarios: &[Scenario],
    order: &[usize],
    ticks_per_second: Option<u64>,
) -> Result<PhaseSample, String> {
    let cpu_before = process_cpu_ns(ticks_per_second);
    let started = Instant::now();
    let mut waiters = JoinSet::new();
    for &scenario_index in order {
        let scenario = &scenarios[scenario_index];
        let submitted_at = Instant::now();
        let request = GreekSolverRequest::new(
            scenario.input.clone(),
            scenario.quote_revision,
            scenario.quote_fence.clone(),
        );
        let handle = pool
            .try_submit(request)
            .map_err(|error| format!("direct pool rejected a request: {error}"))?;
        waiters.spawn(async move {
            let outcome = handle.resolve(NOW_MS).await;
            (submitted_at.elapsed().as_nanos(), outcome)
        });
    }

    let mut sample = PhaseSample {
        accepted_requests: order.len() as u64,
        worker_jobs: order.len() as u64,
        request_latency_ns: Vec::with_capacity(order.len()),
        ..PhaseSample::default()
    };
    while let Some(joined) = waiters.join_next().await {
        let (request_latency_ns, outcome) =
            joined.map_err(|error| format!("direct result waiter failed: {error}"))?;
        if matches!(
            outcome,
            GreekJobOutcome::Stale { .. } | GreekJobOutcome::WorkerStopped { .. }
        ) {
            return Err(format!(
                "direct pool returned unexpected outcome: {outcome:?}"
            ));
        }
        sample.request_latency_ns.push(request_latency_ns);
    }
    sample.wall_ns = started.elapsed().as_nanos();
    sample.process_cpu_ns = cpu_delta(cpu_before, process_cpu_ns(ticks_per_second));
    (sample.process_rss_bytes, sample.process_hwm_bytes) = process_memory_bytes();
    Ok(sample)
}

async fn run_singleflight_wave(
    owner: &GreekSolverSingleflight<'_>,
    scenarios: &[Scenario],
    order: &[usize],
    ticks_per_second: Option<u64>,
) -> Result<PhaseSample, String> {
    let before = admission_counts(owner);
    let cpu_before = process_cpu_ns(ticks_per_second);
    let started = Instant::now();
    let mut waiters = JoinSet::new();
    for &scenario_index in order {
        let scenario = &scenarios[scenario_index];
        let submitted_at = Instant::now();
        let request = GreekPricingRequest::new(
            scenario.input.clone(),
            scenario.key.clone(),
            scenario.input_fence.clone(),
            scenario.input_fence_revision,
        )
        .map_err(|error| format!("cannot create complete pricing request: {error}"))?;
        let handle = owner
            .try_submit(request, NOW_MS)
            .map_err(|error| format!("singleflight rejected a request: {error}"))?;
        waiters.spawn(async move {
            let outcome = handle.resolve(NOW_MS).await;
            (submitted_at.elapsed().as_nanos(), outcome)
        });
    }

    let mut sample = PhaseSample {
        accepted_requests: order.len() as u64,
        request_latency_ns: Vec::with_capacity(order.len()),
        ..PhaseSample::default()
    };
    while let Some(joined) = waiters.join_next().await {
        let (request_latency_ns, outcome) =
            joined.map_err(|error| format!("singleflight result waiter failed: {error}"))?;
        if matches!(
            outcome,
            GreekPricingOutcome::Stale { .. } | GreekPricingOutcome::WorkerStopped { .. }
        ) {
            return Err(format!(
                "singleflight returned unexpected outcome: {outcome:?}"
            ));
        }
        sample.request_latency_ns.push(request_latency_ns);
    }
    sample.wall_ns = started.elapsed().as_nanos();
    sample.process_cpu_ns = cpu_delta(cpu_before, process_cpu_ns(ticks_per_second));
    (sample.process_rss_bytes, sample.process_hwm_bytes) = process_memory_bytes();
    let after = admission_counts(owner);
    sample.worker_jobs = after.0.saturating_sub(before.0);
    sample.in_flight_joins = after.1.saturating_sub(before.1);
    sample.cache_hits = after.2.saturating_sub(before.2);
    sample.saturations = after.3.saturating_sub(before.3);
    (sample.in_flight_after, sample.completed_after) = owner.occupancy_counts();
    Ok(sample)
}

fn admission_counts(owner: &GreekSolverSingleflight<'_>) -> (u64, u64, u64, u64) {
    (
        owner.admitted_flight_count(),
        owner.in_flight_join_count(),
        owner.cache_hit_count(),
        owner.saturation_count(),
    )
}

fn build_scenarios(unique_keys: usize) -> Vec<Scenario> {
    (0..unique_keys)
        .map(|index| {
            let strike = Strike::from_mills(100_000 + index as u32 * 1_000)
                .expect("synthetic strike within OCC range");
            let underlying = Underlying::new("QQQ").expect("synthetic underlying");
            let symbol = OptionSymbol::new(
                underlying.clone(),
                ExpirationDate::new(2027, 12, 17).expect("synthetic expiration"),
                OptionRight::Call,
                strike,
            );
            let contract = OptionContract::new(
                symbol,
                ContractMultiplier::new(100).expect("contract multiplier"),
                ContractCurrency::new("USD").expect("contract currency"),
                Deliverable::new(vec![
                    DeliverableComponent::equity(
                        underlying.clone(),
                        ExactDecimal::from_integer(100),
                    )
                    .expect("equity deliverable"),
                ])
                .expect("single-leg deliverable"),
            );
            let spot = Price::parse_json_number("100").expect("synthetic spot");
            let premium = Price::parse_json_number("6.12").expect("synthetic premium");
            let expiry_at_ms = NOW_MS + 182 * 86_400_000 + 43_200_000;
            let expiry_context = ExpirationContext::new(
                ExpirationClass::NonZeroDaysToExpiry,
                "UTC",
                ExchangeInstant::new(NOW_MS, 0, 0).expect("valuation instant"),
                ExchangeInstant::new(expiry_at_ms, 0, 182).expect("expiry instant"),
            )
            .expect("synthetic expiration context");
            let dividend_evidence = DividendWindowEvidence::new(
                underlying.clone(),
                ProviderMetadataRef::new(
                    MetadataSource::MarketData(MarketDataProviderId::Alpaca),
                    ProviderMetadataKind::MarketData,
                    ProviderRecordId::new(format!("synthetic-dividend-window-{index}"))
                        .expect("synthetic dividend record"),
                ),
                1,
                9_900,
                0,
                expiry_at_ms,
                DividendCoverageKind::NoCashDividendInWindow,
            )
            .expect("synthetic dividend evidence");
            let input = SolverInput::new(
                OptionKind::Call,
                underlying.clone(),
                spot,
                strike,
                premium,
                0.02,
                ExerciseStyle::European,
                expiry_context,
                DividendAssumption::NoDividends,
                Some(dividend_evidence),
                9_900,
                NOW_MS,
                3_000,
            )
            .expect("valid synthetic solver input");
            let revision = QuoteRevision::new(index as u64 + 1);
            let provenance = |source, max_age| {
                PricingQuoteProvenance::new(
                    PricingSourceId::new(source),
                    GENERATION,
                    revision,
                    9_900,
                    NOW_MS,
                    max_age,
                    PricingQuoteQuality::Realtime,
                )
            };
            let key = PricingInputKey::new(
                contract.clone(),
                GENERATION,
                OptionPriceInput::new(contract.clone(), premium, provenance(11, 3_000)),
                UnderlyingPriceInput::new(underlying, spot, provenance(22, 3_000)),
                NOW_MS,
                &input,
            )
            .expect("complete synthetic pricing key");
            let input_fence = PricingInputFence::new(GENERATION);
            let input_fence_revision = input_fence
                .accept_key(&key)
                .expect("accept synthetic input key");
            Scenario {
                input,
                key,
                input_fence,
                input_fence_revision,
                quote_revision: revision,
                quote_fence: QuoteRevisionFence::new(revision),
            }
        })
        .collect()
}

fn workload_order(unique_keys: usize) -> Vec<usize> {
    let mut order = Vec::with_capacity(LOGICAL_REQUESTS);
    let mut per_key = vec![0; unique_keys];
    for vertical_index in 0..VERTICAL_CONSUMERS {
        for leg_index in 0..LEGS_PER_VERTICAL {
            let key_index = (vertical_index * LEGS_PER_VERTICAL + leg_index) % unique_keys;
            order.push(key_index);
            per_key[key_index] += 1;
        }
    }
    assert!(
        per_key
            .iter()
            .all(|count| *count == LOGICAL_REQUESTS / unique_keys)
    );
    order
}

fn print_phase(phase: &str, config: Config, aggregate: &PhaseAggregate) {
    let repeats = aggregate.wall_ns.len() as u64;
    let requests_per_run = aggregate.accepted_requests / repeats;
    let jobs_per_run = aggregate.worker_jobs / repeats;
    let joins_per_run = aggregate.in_flight_joins / repeats;
    let hits_per_run = aggregate.cache_hits / repeats;
    let saturations_per_run = aggregate.saturations / repeats;
    println!(
        "BENCH mode={} keys={} phase={} repeats={} logical_requests_per_run={} accepted_per_run={} worker_jobs_per_run={} worker_jobs_total={} in_flight_joins_per_run={} cache_hits_per_run={} saturations_per_run={} in_flight_after_max={} completed_after_max={} wall_ns_p50={} wall_ns_p95={} wall_ns_p99={} request_latency_ns_p50={} request_latency_ns_p95={} request_latency_ns_p99={} process_cpu_ns_total={} process_rss_bytes_max={} process_hwm_bytes_max={}",
        config.mode.name(),
        config.unique_keys,
        phase,
        repeats,
        LOGICAL_REQUESTS,
        requests_per_run,
        jobs_per_run,
        aggregate.worker_jobs,
        joins_per_run,
        hits_per_run,
        saturations_per_run,
        aggregate.max_in_flight_after,
        aggregate.max_completed_after,
        percentile(&aggregate.wall_ns, 50),
        percentile(&aggregate.wall_ns, 95),
        percentile(&aggregate.wall_ns, 99),
        percentile(&aggregate.request_latency_ns, 50),
        percentile(&aggregate.request_latency_ns, 95),
        percentile(&aggregate.request_latency_ns, 99),
        aggregate
            .process_cpu_ns
            .map_or_else(|| "NOT_AVAILABLE".to_owned(), |value| value.to_string()),
        aggregate
            .max_process_rss_bytes
            .map_or_else(|| "NOT_AVAILABLE".to_owned(), |value| value.to_string()),
        aggregate
            .max_process_hwm_bytes
            .map_or_else(|| "NOT_AVAILABLE".to_owned(), |value| value.to_string()),
    );
}

fn percentile(samples: &[u128], percentile: usize) -> u128 {
    if samples.is_empty() {
        return 0;
    }
    let mut ordered = samples.to_vec();
    ordered.sort_unstable();
    let rank = ordered
        .len()
        .saturating_mul(percentile)
        .div_ceil(100)
        .max(1);
    ordered[rank - 1]
}

fn max_option<T: Ord>(left: Option<T>, right: Option<T>) -> Option<T> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn process_clock_ticks_per_second() -> Option<u64> {
    let output = Command::new("getconf").arg("CLK_TCK").output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()?.trim().parse().ok()
}

fn process_cpu_ns(ticks_per_second: Option<u64>) -> Option<u128> {
    let ticks = process_cpu_ticks()?;
    let ticks_per_second = ticks_per_second?;
    Some(u128::from(ticks) * 1_000_000_000 / u128::from(ticks_per_second))
}

fn process_cpu_ticks() -> Option<u64> {
    let tasks = fs::read_dir("/proc/self/task").ok()?;
    let mut total_ticks = 0_u64;
    for task in tasks.flatten() {
        let stat = fs::read_to_string(task.path().join("stat")).ok()?;
        let command_end = stat.rfind(')')?;
        let fields: Vec<&str> = stat.get(command_end + 2..)?.split_whitespace().collect();
        let user_ticks = fields.get(11)?.parse::<u64>().ok()?;
        let system_ticks = fields.get(12)?.parse::<u64>().ok()?;
        total_ticks = total_ticks.checked_add(user_ticks.checked_add(system_ticks)?)?;
    }
    Some(total_ticks)
}

fn cpu_delta(before: Option<u128>, after: Option<u128>) -> Option<u128> {
    after?.checked_sub(before?)
}

fn process_memory_bytes() -> (Option<u64>, Option<u64>) {
    let Ok(status) = fs::read_to_string("/proc/self/status") else {
        return (None, None);
    };
    let rss = status_value_bytes(&status, "VmRSS:");
    let high_water = status_value_bytes(&status, "VmHWM:");
    (rss, high_water)
}

fn status_value_bytes(status: &str, key: &str) -> Option<u64> {
    let value = status.lines().find_map(|line| line.strip_prefix(key))?;
    value
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()?
        .checked_mul(1024)
}
