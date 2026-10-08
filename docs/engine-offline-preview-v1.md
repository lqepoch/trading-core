# Engine offline preview read contract v1

`lqepoch.engine.v1.EngineStatusResponseV1` and
`lqepoch.engine.v1.SyntheticOfflinePreviewV1` describe the existing snake_case
JSON responses emitted by the private `offline-persist-preview` reader. The
source contract was checked against `lqepoch/trading-engine` commit
`b6a4a6ca5e3feb8c5fb39f7e781be774629b31e2`, specifically
`apps/offline-persist-preview/src/lib.rs` and
`apps/offline-persist-preview/tests/persistent_preview_http.rs`. The shared
fixtures contain only the source's synthetic one-UNKNOWN response values; they
contain no account, order, provider, market, or credential data.

The Proto fields set explicit `json_name` values so generated clients read the
current snake_case response without changing that wire shape. OpenAPI declares
these response components only; it does not declare an endpoint, authentication
scheme, or server implementation. The engine remains the owner of its routes,
authentication, source reads, and response production.

Both projections are fixed-mode diagnostics. The service and source readiness
labels remain `read_only_ready` and `unknown`; mode/provenance remains synthetic;
projection consistency remains `best_effort_non_transactional`; and every
execution, mutation, account-data, and market-data availability flag is false.
`schema_version` and `source_schema_version` must be positive uint32 values.

The UNKNOWN sample is bounded to 256 rows. The count cannot exceed 256, the cap
flag is true exactly when the count is 256, and the consumed-risk and
unverified-risk counts must sum to the sampled count. The preview disposition
is derived from those counts: any unverified risk state yields
`unknown_reservation_state`, otherwise a nonzero sample yields
`reconciliation_required`, and an empty sample yields
`no_pending_unknown_in_sample`. The empty result says only that this bounded
non-transactional query returned no rows; it does not prove complete
reconciliation.

The exported TypeScript raw-text parsers enforce the exact field set, current
snake_case names, fixed strings, explicit false flags, count bounds, and derived
disposition. They reject duplicate JSON keys and camel/snake aliases before
generated ProtoJSON conversion. The Python bindings are generated from the same
Proto and are checked against the same producer fixtures. These are structural
response checks; they do not grant account, risk, reconciliation, strategy, or
execution authority and do not establish runtime behavior beyond the checked
producer version.

The exact producer fixtures are
[`engine-status-response-v1.json`](../schemas/fixtures/engine-status-response-v1.json)
and
[`synthetic-offline-preview-v1.json`](../schemas/fixtures/synthetic-offline-preview-v1.json).
