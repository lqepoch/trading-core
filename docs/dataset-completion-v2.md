# Dataset manifest V2 completion evidence

`DatasetManifestV2` is an additive contract in `lqepoch.dataset.v2`. It does not change the V1
message, Rust DTO, JSON fixtures, or Parquet schema fingerprints. V2 keeps immutable-object
readback verification in `storage_verification` and puts input or stream completion in the typed
`completion_evidence` oneof. `readback_sha256` must equal the stored object's content SHA-256;
that proves readback consistency only.

The oneof has three cases with intentionally different meanings:

- `finite_batch` binds a non-empty finite input by immutable identity, exact byte SHA-256, size,
  record count, and a receipt. `consumed_record_count` must equal `input_record_count`.
  `seal_receipt_sha256` must equal the SHA-256 of the core-generated
  `FiniteBatchSealReceiptV2` ProtoJSON projection: the finite-batch fields in protobuf field order,
  excluding `seal_receipt_sha256`, encoded as compact UTF-8 JSON with canonical uint64 strings and
  protobuf timestamps, and no trailing newline. Rust, Python, and TypeScript expose the same
  projection helper and use the shared byte/hash golden in
  `schemas/fixtures/finite-batch-seal-receipt-v2.*`. This binds the claim fields to one another only;
  it does not authenticate an issuer or establish provider completeness. Consumers compare receipt
  artifact bytes with the helper output and verify the manifest's hash before assigning any local
  structural status. A paged
  historical source must include a positive `page_count`, `pages_exhausted: true`, and the exact
  page-set receipt SHA-256. Those page fields must be absent for synthetic replay, non-paged
  history, and local archives. Timestamps are exact UTC protobuf timestamps with nanoseconds;
  `data_cutoff_exclusive <= sealed_at <= completed_at`, including equality. When present, the
  manifest source-time range must end no later than `data_cutoff_exclusive`. A finite seal records
  processing of that exact input. It does not prove that a provider returned all history for a
  query window. `local_archive` means rereading the identified local bytes, not provider-history
  completeness. An empty finite batch cannot be published as a manifest.
- `provider_watermark` is an untrusted structural observation. It binds provider/feed, an immutable
  subscription-instance ID, generation, first/last sequence, a checked `sequence_count`, exclusive
  `complete_up_to_exclusive`, allowed lateness, policy SHA-256, and source/continuity receipts.
  Generation is scoped by subscription instance and may repeat after process restart. Generation
  and sequence numbers start at one. The checked range count proves only arithmetic consistency;
  the receipt verifier must inspect actual observations to establish continuity and detect gaps or
  duplicates. Lateness above 60 seconds is rejected without clamping. A manifest source-time range
  must be covered by the exclusive watermark, with no source timestamps missing.
- `diagnostic_stream` records an immutable local source instance, generation, highest observed
  sequence, local policy cutoff, optional maximum observed source timestamp, and diagnostic
  receipts. The cutoff is not called `complete_up_to`; late timestamps may exceed it. This case is
  diagnostic only.

Hash syntax, identity bounds, oneof presence, timestamp order, and cross-field consistency are
validated in Rust, Python, and TypeScript, including finite receipt hash-to-projection binding.
The parsers do not validate receipt issuers and never mint provider-completeness authority. A fixed
composition-root verifier must retrieve and verify
the exact manifest and referenced receipt bytes before issuing any qualification token. Current
Alpaca streams do not provide the trusted watermark evidence required for that token, so their
stream completion remains unverified/diagnostic. Entitlement text is also only an observation.

ProtoJSON and HTTP JSON represent every `uint64` as a canonical decimal string, including values
above JavaScript's exact-integer range. Rust rejects JSON numbers and non-canonical strings. The
protobuf timestamp projection preserves nanoseconds; consumers that map it to Python `datetime`
must reject timestamps whose nanos are not exactly representable (`nanos % 1000 != 0`).

The shared finite, diagnostic, regular-watermark, and max-`uint64` watermark fixtures are in
`schemas/fixtures/dataset-manifest-v2*.json`. They are parsed by Rust, Python, TypeScript, and the
OpenAPI HTTP JSON validator. The `.textproto` fixture is compiled by `scripts/validate-schemas.sh`.
