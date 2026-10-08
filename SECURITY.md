# Security policy

Report security issues privately to the repository owner using the GitHub security advisory feature. Do not include credentials, OAuth tokens, account identifiers, private market archives, or customer data in public issues or pull requests.

This repository contains provider-neutral types, analytical code, schemas, and synthetic tests only. It does not connect to brokers, acquire OAuth tokens, hold credentials, authorize accounts, store private market data, or dispatch orders. Do not add any of those capabilities here.

Public CI, if configured, may build, test, lint, and scan this public source tree and synthetic fixtures. It must not access private repositories, organization trading credentials, private market archives, account data, or broker/trading operations. No workflow is currently configured; local gates are documented in `docs/development.md`.
