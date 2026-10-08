# 美式定价 QuantLib oracle 证据

## 状态

本次离线实验验证了测试专用 CRR 候选在有限合成矩阵上的价格、IV 反解和有限差分 Greeks。它**没有通过一般美式价格/IV 精度验收**，也不改变 `AmericanPricingAccuracyUnverified`。生产入口仍拒绝正剩余时间的美式价格、IV 和 Greeks。

测试数据只包含人为构造的参数；没有真实市场行情、账户、凭据、OAuth、券商调用或交易活动。QuantLib 只在样本生成工具中使用，不是生产或 Cargo 依赖，也不是第二套生产定价权威。

## 固定来源与构建

上游来源为 [QuantLib v1.43](https://github.com/lballabio/QuantLib/tree/6b57206e04598f092efee66e3b367efc84771995)，已用远程 tag 查询确认 peeled commit 为 `6b57206e04598f092efee66e3b367efc84771995`；本地构建 checkout 与之相同。上游 `LICENSE.TXT` SHA-256 为 `85c8dd19077a479f7591a7dcc25194cd58c6f5d98690d2f865d0fe870b9d0eb2`。被检查的美式测试、FD 引擎头文件及黑舒尔斯 FD solver 哈希见 [来源清单](../crates/pricing/tests/fixtures/american_quantlib_v143_manifest.json)。

本次在隔离 `/tmp` 目录从源码编译，启用 `QL_HIGH_RESOLUTION_DATE` 以保留毫秒级 expiry。构建参数为 Release、OpenMP/样例/上游 test suite 关闭、最多两个并行 jobs；没有把库复制进仓库。完整 QuantLib 编译未用资源计时器包裹，约 22 分钟是运行观察值，准确耗时和 peak RSS 均未采集，不作为 SLA。

```sh
git clone https://github.com/lballabio/QuantLib.git /tmp/quantlib-v143
git -C /tmp/quantlib-v143 checkout --detach 6b57206e04598f092efee66e3b367efc84771995
git -C /tmp/quantlib-v143 rev-parse HEAD

cmake -S /tmp/quantlib-v143 -B /tmp/quantlib-v143-build \
  -DCMAKE_BUILD_TYPE=Release \
  -DQL_HIGH_RESOLUTION_DATE=ON \
  -DQL_ENABLE_OPENMP=OFF \
  -DQL_BUILD_EXAMPLES=OFF \
  -DQL_BUILD_TEST_SUITE=OFF
cmake --build /tmp/quantlib-v143-build --parallel 2

PRICING_REPO="$(git rev-parse --show-toplevel)"
g++ -std=c++17 -O2 -Wall -Wextra \
  -I/tmp/quantlib-v143-build -isystem /tmp/quantlib-v143 \
  "$PRICING_REPO/crates/pricing/tests/oracle/american_quantlib_v143.cpp" \
  -L/tmp/quantlib-v143-build/ql \
  -Wl,-rpath,/tmp/quantlib-v143-build/ql -lQuantLib \
  -o /tmp/pricing-quantlib-v143
/tmp/pricing-quantlib-v143 > /tmp/quantlib-v143-grid.jsonl
python3 "$PRICING_REPO/crates/pricing/tests/oracle/convert_quantlib_jsonl.py" \
  /tmp/quantlib-v143-grid.jsonl > /tmp/american_quantlib_v143_grid.csv
cmp "$PRICING_REPO/crates/pricing/tests/fixtures/american_quantlib_v143_grid.csv" \
  /tmp/american_quantlib_v143_grid.csv

g++ -std=c++17 -O2 -Wall -Wextra \
  -I/tmp/quantlib-v143-build -isystem /tmp/quantlib-v143 \
  "$PRICING_REPO/crates/pricing/tests/oracle/american_quantlib_v143_fd.cpp" \
  -L/tmp/quantlib-v143-build/ql \
  -Wl,-rpath,/tmp/quantlib-v143-build/ql -lQuantLib \
  -o /tmp/pricing-quantlib-v143-fd
/tmp/pricing-quantlib-v143-fd \
  "$PRICING_REPO/crates/pricing/tests/fixtures/american_quantlib_v143_grid.csv" \
  > /tmp/quantlib-v143-fd.jsonl
python3 "$PRICING_REPO/crates/pricing/tests/oracle/convert_quantlib_fd_jsonl.py" \
  /tmp/quantlib-v143-fd.jsonl > /tmp/american_quantlib_v143_fd.csv
cmp "$PRICING_REPO/crates/pricing/tests/fixtures/american_quantlib_v143_fd.csv" \
  /tmp/american_quantlib_v143_fd.csv
```

The fixture was regenerated from the committed C++ harness using this command sequence; `cmp` succeeded byte-for-byte. The runtime rows report `QuantLib 1.43`. Harness and converter SHA-256 values are included in the manifest.

## Same-model comparison

QuantLib `FdBlackScholesVanillaEngine` is the reference engine. Its grids are `(100,400)`, `(400,800)`, `(800,1600)`, and `(1600,3200)` time and price points, with two damping steps and the default Douglas scheme. Synthetic valuation begins at `2026-06-15 13:30:00`; each expiry is an exact elapsed-millisecond offset. Both calculations use ACT/365F, flat continuously compounded risk-free rate, flat continuous dividend yield, constant volatility, and American exercise. QuantLib uses high-resolution `Date`; the curves use a synthetic `NullCalendar`. Reported prices and Greeks are per one underlying unit; no option multiplier or currency conversion is applied. Delta is per underlying unit, Gamma per squared underlying-dollar move, and Theta per ACT/365F model year per underlying unit. There is no exchange calendar, settlement convention, contract resolution, or market-price input in this comparison.

The matrix contains 36 synthetic cases and 144 QuantLib rows. It covers calls and puts; `S/K` from `0.80` to `1.20`; remaining time from `59,999 ms` through 365 days; rates from `-0.15` to `0.15`; continuous yields from `-0.02` to `0.08`; and volatility from `0.05` to `2.0`. ATM and ITM puts include `59,999`, `60,000`, and `60,001 ms` boundary cases. The two fixed-cash-dividend rows are deliberately separated from the 34 no-cash/continuous-yield CRR comparisons.

For direct price comparison, the current test-only CRR result is the mean of its 256-step and 257-step trees. It rejects the pair when their absolute gap exceeds `$0.05`. Of the 34 same-model cases, 31 emitted a candidate pair and three were rejected by that parity check: `matrix_put_100_365d` (gap `$0.092796`), `matrix_call_120_90d` (`$0.061654`), and `matrix_put_120_180d` (`$0.075056`). Among the 31 emitted values, maximum absolute price error was `$0.011081806` (`matrix_call_105_30d`); maximum relative error was `18.6801%` (`matrix_call_95_1d`, a low-premium case). Relative error is `abs(CRR-QuantLib)/max(abs(QuantLib),1e-12)`.

The raw CRR `N/(N+1)` price-pair maximum absolute errors across the 34 eligible cases were:

| Steps | Maximum absolute error |
|---:|---:|
| 64 / 65 | `$0.073003` |
| 128 / 129 | `$0.035678` |
| 256 / 257 | `$0.015719` |
| 512 / 513 | `$0.007853` |
| 1024 / 1025 | `$0.004356` |

This sequence is a finite-sample convergence observation. It does not qualify a production step count or tolerance. The current public solver does not publish any of these candidate values.

The QuantLib change from grid `800x1600` to `1600x3200` was at most `$0.001744911` for price (`matrix_put_95_365d`), `8.8234e-6` for Delta (`matrix_put_95_365d`), `2.5374e-5` for Gamma (`matrix_call_100_60s`), and `1.7348` for annualized Theta (`matrix_call_100_60s`). This is observed grid convergence for this matrix, not a general error bound.

## IV experiment

The experiment takes each of ten fixed synthetic volatility inputs, uses the finest-grid QuantLib price at that volatility as the target, then recovers volatility with a test-only CRR price bisection. The bisection runs 48 iterations on the mean of the `N/(N+1)` CRR prices; it expands the lower trial volatility from `0.0001` until the tree is numerically valid and lowers the upper trial volatility from `5.0` until valid. It does not represent an implemented or exposed American IV solver.

All ten cases returned roots at each tested pair. Maximum absolute and relative recovered-volatility errors were:

| CRR step pair | Maximum absolute IV error | Maximum relative IV error |
|---:|---:|---:|
| 64 / 65 | `0.00353947` | `0.18623%` |
| 128 / 129 | `0.00042792` | `0.08815%` |
| 256 / 257 | `0.00099123` | `0.05797%` |
| 512 / 513 | `0.00018461` | `0.03190%` |

The maximum absolute error does not decrease monotonically at every step pair: for `matrix_call_105_30d` with generating volatility `2.0`, it is `0.00042792` at 128/129, `0.00099123` at 256/257, and `0.00018461` at 512/513. This finite study does not establish an IV error budget.

## Greek experiment

QuantLib `delta()`, `gamma()`, and `theta()` at grid `1600x3200` are compared with central finite differences of the test-only CRR pair across 31 emitted cases. Spot bump fractions for Delta and Gamma are `0.01%`, `0.1%`, and `1%` of spot. Theta uses `-(P(T+h)-P(T-h))/(2h)` with `h` equal to `0.1%`, `1%`, and `5%` of ACT/365F remaining time, giving the same per-year-fraction unit as the reference. These are cross-convention comparisons: QuantLib's native Delta/Gamma come from derivatives of its finite-difference solution interpolation, and its native Theta uses the solver's snapshot convention rather than the test's local central difference.

Maximum absolute errors across this sample were:

| Metric | Bump sweep | Maximum absolute error | Case |
|---|---|---:|---|
| Delta | `0.01%`, `0.1%`, `1%` of spot | `0.00829935` | `matrix_call_120_30d` |
| Gamma | `0.01%`, `0.1%`, `1%` of spot | `10.5971` | `0dte_atm_put_59999ms_no_div` |
| Theta | `0.1%`, `1%`, `5%` of remaining time | `2958.60` | `matrix_call_100_60s` |

Delta/Gamma/Theta bump-size results are also emitted by the regression test. Relative Greek error is not used as a gate: near-zero reference Greeks make that ratio unstable, and the finite differences are only experimental. The large gaps in this table are not treated as production numerical errors because the native QuantLib and finite-difference conventions differ.

### Convention-matched price probes

To separate model differences from Greek-definition differences, `american_quantlib_v143_fd.cpp` independently reprices each of the 34 no-cash-dividend cases at the same `1600x3200` grid. It reads model parameters from the original finest-grid rows, recomputes each base price, and checks that price against the original row within `$1e-10`; it ignores the original Delta, Gamma, and Theta columns. The original C++ harness, converter, 36-case/144-row fixture, and its manifest remain byte-for-byte unchanged. See [`american_quantlib_v143_fd_manifest.json`](../crates/pricing/tests/fixtures/american_quantlib_v143_fd_manifest.json) for the new harness, converter, output, upstream library, and preserved-artifact SHA-256 values.

For each case the probe records prices at `S±h` for `h/S = 0.01%, 0.1%, 1%`, and at `T±h` for `h/T = 0.1%, 1%, 5%`. Time bumps are rounded to the nearest integer millisecond before both QuantLib repricing and the CRR comparison. The resulting Delta and Gamma use central spot differences; Theta uses `-(P(T+h)-P(T-h))/(2h)`. This directly matches the test-only CRR finite-difference definitions. The new fixture contains 204 rows. All 31 cases with an accepted 256/257 CRR pair are compared; the three parity-rejected price cases remain unavailable.

The maximum absolute differences between those matched finite differences were:

| Metric | Bump sweep | Maximum absolute difference | Case |
|---|---|---:|---|
| Delta | `0.01%`, `0.1%`, `1%` of spot | `0.00829955` | `matrix_call_120_30d` |
| Gamma | `0.01%`, `0.1%`, `1%` of spot | `2.43431` | `american_call_90d_atm_no_div` |
| Theta | `0.1%`, `1%`, `5%` of remaining time | `7.79605` | `matrix_put_120_1d` |

The smaller bump does not always give the closer Gamma match, consistent with finite differences amplifying each engine's spatial-grid/interpolation noise. The Theta result also confirms that the prior maximum gap of about `2958.6` came from comparing a local central difference with QuantLib's different native snapshot convention; it is not evidence of a production American solver error. The matched figures remain finite-sample diagnostics, do not establish Greek accuracy or a production budget, and do not change the public unavailable result.

QuantLib v1.43's finite-difference engine delegates native Theta to `Fdm1DimSolver::thetaAt`; that method derives Theta from a value snapshot near the current time rather than from symmetric `T±h` reprices. The pinned sources are [`Fdm1DimSolver.cpp`](https://github.com/lballabio/QuantLib/blob/6b57206e04598f092efee66e3b367efc84771995/ql/methods/finitedifferences/solvers/fdm1dimsolver.cpp) and [`FdBlackScholesVanillaEngine.cpp`](https://github.com/lballabio/QuantLib/blob/6b57206e04598f092efee66e3b367efc84771995/ql/pricingengines/vanilla/fdblackscholesvanillaengine.cpp).

## Discrete-dividend boundary

The CRR-comparable cases use either no dividends or a continuous yield. QuantLib separately prices a fixed `$1` cash dividend ten days after valuation on a 30-day American call and put, using the same spot/rate/volatility as a no-dividend baseline. The cash-dividend price changes are `-$0.509632540` for the call and `+$0.625554977` for the put. These are different payoff models; they are not CRR pricing errors, and a continuous yield is not substituted for a discrete schedule. The production solver continues to return `DividendAssumptionUnsupported` for unknown/discrete schedules.

## Offline regression and limits

The checked-in CSV fixture is read by the pricing crate's unit tests; running Cargo tests does not download or link QuantLib. The tests pin the QuantLib version/grid rows, verify grid convergence within this sample, measure CRR price/IV/Greek differences, and assert explicit sample-only regression envelopes. Those envelopes prevent accidental changes to this recorded experiment; they do not authorize candidate values or replace a broader frozen acceptance plan. The existing public `AmericanPricingAccuracyUnverified` result remains unchanged.

Not established or not run:

- a broad/random or exchange-specific parameter distribution, market calibration, or real quote error;
- American settlement, expiration calendars, exercise cutoffs, contract qualification, or timezone/DST behavior;
- a production American IV/Greek implementation or a general price/IV/Greek error budget;
- discrete-dividend CRR pricing or provider-validated dividend schedules;
- full QuantLib upstream test-suite execution (`QL_BUILD_TEST_SUITE=OFF` for this build);
- cross-platform/C++ compiler reproducibility, a clean cold-build peak RSS, or full-workspace Cargo checks.

The pricing crate's targeted offline validation is `CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/tmp/pricing-oracle-target cargo +1.98.1 test --locked -p pricing`; formatting and Clippy are run separately during this change. The temporary target directory keeps build output outside the worktree.
