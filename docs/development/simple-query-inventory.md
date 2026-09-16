# Aether Data Simple Query Inventory

This inventory tracks PostgreSQL repository read paths that use the internal
`aether-data-query` helpers. `SelectQuery` centralizes simple `SELECT`
construction while complex PostgreSQL SQL remains explicit.

## Included paths

- background tasks, announcements, auth modules, OAuth providers, and quotas
- provider catalog reads, filters, search, ordering, and pagination
- proxy node reads and event filters
- management token lookup and listing
- pool score lookup and ranking
- request-candidate listing and finalized counts
- Gemini file-mapping filters and search

## Deferred paths

The following stay hand-written until a dedicated abstraction would materially
reduce duplication:

- usage aggregation, dashboards, leaderboards, rebuilds, and blob reads
- candidate-selection JSON/alias matching and scoring
- wallet ledgers, orders, refunds, callbacks, and redeem codes
- writes, upserts, deletes, transactions, CTEs, window functions, advisory
  locks, and schema probes
- additional user/auth and global-model reads

## Helper coverage

- PostgreSQL identifier quoting and expressions
- simple `SELECT` rendering
- stable `WHERE` / `AND` and bind ordering
- equality, optional equality, `IN`, and case-insensitive search
- whitelisted ordering, limits, and offsets

Repository modules own table-specific projections, joins, and row mapping.
`SelectStatement` owns dynamic filters, ordering, and pagination. Complex SQL
stays hand-written until repeated structure justifies another abstraction.
