# Trading Core repository rules

This repository owns provider-neutral financial values, deterministic models, and versioned cross-language data contracts.

## Ownership boundaries

- exact-decimal and domain own exact numeric values and canonical instrument, order-intent, routing, and account identifiers.
- pricing owns deterministic analytical models and bounded model execution. Model outputs are observations, not market facts, execution prices, or risk authority.
- market-contracts owns serializable market DTOs, legacy compatibility exports, and the trusted Parquet logical-schema registry. Protobuf, OpenAPI, and Parquet registry resources under proto/ and schemas/ are versioned here; raw-frame DTOs describe storage rows but do not implement transport or persistence.
- Broker SDKs, provider adapters, account-authority coordinators, request schedulers, OMS, persistence, and order dispatch belong to their owning repositories. Do not create dependencies on them here.

The [nine-repository target architecture](https://github.com/lqepoch/platform-ops/blob/main/docs/NINE-REPOSITORY-ARCHITECTURE.md) was published on `platform-ops/main` on 2026-10-17. It governs target responsibilities and migration gates; implemented authority and readiness still require source, exact dependency locks and head-bound acceptance evidence.

## Safety

- Parsing an OCC or provider symbol creates a candidate. A qualified contract also requires trading class, currency, multiplier, deliverable, exercise style, settlement type, and provider evidence.
- Control JSON consumers must use the strict uint64 entry points before protobuf conversion; direct generated parsers can accept rounded numeric JSON values. Parsing does not authenticate provider ACKs.
- Preserve exact decimal values at API boundaries. Do not silently convert invalid or missing values to zero.
- Keep unverified models fail closed. Positive-time American IV/Greeks remain unavailable until the independent accuracy gate is accepted.
- This repository must not add broker network clients, OAuth, account credentials, mutation transports, or order dispatch.

## Validation

- Use the toolchain in rust-toolchain.toml, locked dependencies, and synthetic fixtures.
- Run the narrowest relevant package tests, formatting and strict Clippy for changed crates, and scripts/validate-schemas.sh for schema changes.
- Record exact commands and any unavailable language generators as NOT RUN or NOT IMPLEMENTED.
- `.github/workflows/public-validation.yml` runs the Rust and cross-language schema validation scripts on pull requests targeting `main` and pushes to `main`; PR jobs explicitly check out and assert the PR head SHA, and main jobs assert the event commit. Actions are pinned to full commit SHAs, permissions are read-only, and tool versions are fixed. The workflow consumes no repository or organization secrets; checkout uses only the ephemeral read-only GitHub token and does not persist it. It uses no provider clients, private data, Docker, or artifact uploads. Do not report local results as a substitute for the matching head's GitHub checks.
- Public workflows may build, test, lint, and scan only public source and synthetic fixtures. They must not read private repositories, credentials, private market archives, account data, or invoke any broker or trading operation.

## Provenance and licensing

- Every imported source file must be listed in SOURCE-MANIFEST.json with its upstream repository, immutable commit, source path, target path, and content digest.
- The root MIT and Apache-2.0 licenses apply only to source explicitly authorized for this repository. Preserve third-party notices and licenses. Never copy private source without explicit owner authorization.
- Update NOTICE.md and the source manifest when importing or changing upstream-derived code.

- OpenAPI custom UTF-8 byte formats must be registered as assertions before schema validation; generic annotation-only tools do not enforce domain byte bounds. Keep DatasetObject transport identity conditions aligned with Rust and the shared negative fixtures.

## Owner self-review and self-merge

The user has explicitly authorized the repository owner to review and merge their
own PRs for user-authorized work in this repository. This authorization persists;
do not request it again per task or PR. The author may perform the final diff
review without waiting for a non-author. Record it as `SELF_REVIEW`, bound to the
final head SHA, with scope, findings and validation; never call it independent.

GitHub does not accept an author's own `APPROVED` review. Record self-review in a
PR comment and use existing Owner/Admin/Team approval exceptions when formal
approval is the only remaining gate. Do not fabricate approval, remove blocking
reviews, change organization rulesets, or grant additional credential privileges.

Before merging, refresh remote `main`, freeze the exact head, check the full diff,
conflicts and blocking reviews/threads, and pass the repository's applicable local
validation and current required checks. Honor this repository's CodeQL policy;
never import another repository's CI requirements or report an unrun check as PASS.
Existing failures and release blockers remain recorded. Self-review authorization
does not waive failed or unknown mandatory checks or financial safety gates.

Merge with an expected head SHA, verify the returned merge and remote `main`, and
clean only the task's clean, merged branch after checking private-state filenames.
Use the existing checkout when the task prohibits extra worktrees or clones.
Merging a scoped fix is separate from release acceptance, deployment, provider
entitlement and Paper/Live authorization; it grants none of those capabilities.
