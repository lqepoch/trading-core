# Notices and source provenance

`SOURCE-MANIFEST.json` records every imported source file, immutable upstream commit, source/target path, source and target SHA-256, and any local adaptation.

- Selected self-authored files from `lqepoch/schwab_auto_bot` at `c907d18bc31790ede4cf36a4312a6813467506f0` are included only under the explicit owner authorization for CHG-2026-001. The source repository had no repository-wide open-source license; that repository is unchanged. The authorization covers only paths listed in the manifest.
- `crates/market-contracts/src/legacy.rs` adapts the legacy DTO module from `lqepoch/EqoBoard` at `faf17c0c4a6a127996b66c192d9518b9fe86d6ef`, originally distributed under MIT, Copyright (c) 2026 LQ Epoch.
- Third-party dependency notices and license texts are not relicensed. The dependency license inventory is maintained in the SPDX SBOM.
- The optional Python `arrow` extra pins PyArrow 25.0.1, licensed Apache-2.0; the exact release metadata and included upstream license/notice files were reviewed. PyArrow is used only by schema-validation helpers and is not a required runtime dependency.

The private Schwab `quote_exit.json` fixture was reviewed and intentionally excluded because it contains private strategy thresholds and inventory/exit-persistence traces. Pricing tests use newly authored synthetic values instead.
