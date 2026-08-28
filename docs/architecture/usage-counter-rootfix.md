# Usage Counter Root Fix

This worktree implements the first production cut of the root fix for
usage-related lock contention and hot-row pressure.

## Problem

The current Postgres write path mixes:

- request facts (`usage`, audit snapshots, settlement state)
- shared counters (`api_keys`, `provider_api_keys`, `global_models`)
- provider quota window JSON updates
- wallet settlement writes

That means a single request can hold a transaction while touching multiple shared rows, and high-frequency traffic can serialize on the same `api_key_id` / `provider_api_key_id` / wallet rows.

Relevant code paths:

- `crates/aether-data/src/repository/usage/postgres/mod.rs`
- `crates/aether-data/src/repository/settlement/postgres.rs`
- `crates/aether-usage-runtime/src/runtime.rs`

## Goal

Remove shared-hot-row updates from the request path without losing correctness.

The request path must become:

1. write immutable request facts
2. write durable delta records
3. commit

All shared counters must be derived later by a worker.

## Target Architecture

### 1. Facts first

Keep `usage` as the source of truth for per-request facts:

- request identity
- status transitions
- billing state
- token / cost / latency payloads
- audit snapshots

### 2. Durable counter outbox

Add a new append-only delta table, for example:

`usage_counter_deltas`

Each row should represent one logical contribution:

- `kind` (`api_key`, `provider_api_key`, `model`, `provider_monthly`,
  `proxy_node`, `management_token`, `api_key_last_used`, `window`)
- `target_id`
- delta fields
- request id / revision
- `processed_at`

This table is the bridge between request facts and derived counters.

### 3. Flush worker

Add a background worker that:

1. reads unprocessed deltas with `FOR UPDATE SKIP LOCKED`
2. aggregates them in memory by `(kind, target_id)`
3. applies one UPDATE per target
4. marks the source delta rows processed in the same transaction

This keeps memory useful as a buffer, but never as the only source of truth.

### 4. Read models

Move high-read counters to separate tables or compact materialized read models:

- `api_key_usage_counters`
- `provider_api_key_usage_counters`
- `model_usage_counters`
- `provider_monthly_usage_counters`
- `provider_api_key_window_usage_counters`

Keep the original business tables (`api_keys`, `provider_api_keys`, `global_models`) for configuration and compatibility only.

## Implementation Boundary

This branch should land the fix in phases, but the direction must not change:

1. Postgres gets the durable outbox and counter flush path first, because the
   reported production lock wait is on Postgres row locks.
2. Request-path Postgres writes may still write `usage`, audit blobs, routing
   snapshots, and settlement pricing snapshots. They must not directly update
   shared aggregate rows.
3. Compatibility mirror columns may be updated by the flush worker only, never
   by the request transaction.
4. Dashboard/statistics read paths should be migrated after the write pressure
   is removed, otherwise we risk mixing a large read refactor with the lock fix.

The first executable migration creates the outbox. The first code patch makes
`upsert_usage_record` enqueue deltas in the same transaction as the usage fact
write, then a background worker batches those deltas with
`FOR UPDATE SKIP LOCKED`. Dedicated counter read-model tables remain a follow-up
after the request-path lock pressure is removed.

## Locking Model

### Keep

- advisory lock per `request_id` for idempotent request transitions
- wallet row lock only inside settlement, where correctness depends on it

### Stale pending cleanup

The stale pending cleanup worker may recover a stuck `pending` or `streaming`
usage row as `completed` when the matching request candidate already succeeded.
Recovered rows do not have a terminal usage payload, so they cannot be charged
from token or price facts that were never written. They must still be finalized
as zero-charge settled records:

- set `usage.status = 'completed'`
- set `usage.billing_status = 'settled'`
- set `usage.finalized_at`
- upsert `usage_settlement_snapshots.billing_status = 'settled'`
- keep existing token, cost, wallet, and pricing snapshot fields unchanged

This prevents successful recovered rows from remaining indefinitely as
`completed/pending` and keeps admin usage records, readiness checks, and wallet
debit displays aligned with the persisted billing state.

### Remove from request path

- direct `UPDATE api_keys`
- direct `UPDATE provider_api_keys`
- direct `UPDATE global_models`
- direct `UPDATE providers.monthly_used_usd`
- direct `FOR UPDATE` on provider quota JSON for request-level usage windows

## Memory Cache Rules

Allowed:

- short TTL snapshot cache for read-only admin UI data
- short TTL + single-flight cache for provider/model candidate selection rows
- worker-side delta aggregation buffer
- per-key last-used-at max tracking before flush

Not allowed:

- using memory as the only accounting source
- using memory as the only settlement source
- depending on a clipped Redis stream as the only record of usage

## Rollout Plan

1. Stop counting `pending` / `streaming` as shared counter contributions.
2. Introduce the delta outbox and worker.
3. Redirect request path to facts + outbox only.
4. Migrate reads to the new counter tables.
5. Decommission synchronous hot-row updates.
6. Move provider quota windows out of request transactions.

## Implemented In This Branch

- `usage_counter_deltas` durable outbox for api key, provider api key, model,
  provider monthly, proxy node, management token, and api key last-used counters.
- Postgres usage upsert writes request facts plus outbox rows, not shared counter
  rows.
- Postgres settlement enqueues provider monthly usage deltas instead of updating
  `providers.monthly_used_usd` in the request transaction.
- Gateway request-adjacent proxy node, management token, and api key last-used
  writes enqueue durable deltas and fall back to direct writes only when the
  usage writer is unavailable.
- Gateway provider/model candidate selection reads use a 5 second in-memory TTL
  cache with per-key single-flight. Provider/routing catalog writes invalidate
  this cache alongside provider transport and scheduler affinity caches.
- Gateway maintenance worker flushes deltas in batches and aggregates in memory
  inside the worker before applying compatibility counter updates.
- Daily quota lookup index on `(user_entitlement_id, usage_date)` removes the
  avoidable aggregate scan in quota settlement checks.
- Hot counter columns are widened to `bigint` where old bootstrap schemas still
  used `integer`, preventing long-running counter overflow.

## Integration Pressure Tests

The hotspot benchmarks start a managed local Postgres instance, run the
migrations, seed a single hot target, then monitor `pg_stat_activity` while the
load is running. Use a separate target directory when the main worktree target
lock is not writable.

Usage write path, one hot `api_key` / `provider_api_key` / `global_model`:

```sh
CARGO_TARGET_DIR=/tmp/aether-rootfix-target \
cargo run -p aether-testkit --bin usage_counter_hotspot_baseline -- \
  --requests 5000 \
  --concurrency 200 \
  --flush-interval-ms 50 \
  --monitor-interval-ms 20 \
  --output /tmp/usage_counter_hotspot_after_5000.json
```

Settlement path, one hot provider monthly counter:

```sh
CARGO_TARGET_DIR=/tmp/aether-rootfix-target \
cargo run -p aether-testkit --bin usage_settlement_hotspot_baseline -- \
  --requests 5000 \
  --concurrency 200 \
  --flush-interval-ms 50 \
  --monitor-interval-ms 20 \
  --output /tmp/usage_settlement_hotspot_after_5000.json
```

Auxiliary hot counters, one hot proxy node / management token / api key
last-used target:

```sh
CARGO_TARGET_DIR=/tmp/aether-rootfix-target \
cargo run -p aether-testkit --bin usage_aux_counter_hotspot_baseline -- \
  --requests 5000 \
  --concurrency 200 \
  --flush-interval-ms 50 \
  --monitor-interval-ms 20 \
  --output /tmp/usage_aux_counter_hotspot_after_5000.json
```

Latest local run on this worktree:

- usage hotspot: 5000 requests, 200 concurrency, p95 173 ms, 0 failures,
  15000 outbox rows processed, 0 pending rows, 0 `api_keys` /
  `provider_api_keys` / `global_models` update waiters.
- settlement hotspot: 5000 requests, 200 concurrency, p95 39 ms, 0 failures,
  5000 provider monthly deltas processed, `providers.monthly_used_usd = 5.0`,
  0 provider update waiters.

Run the auxiliary counter hotspot after changing outbox schemas or gateway
fallback routing; it should drain all pending outbox rows and report zero
request-path waiters for `proxy_nodes`, `management_tokens`, and `api_keys`.

## Runtime Observability

The same outbox health signals used by the pressure tools are exposed through
admin runtime endpoints:

- `GET /api/admin/system/stats`
- `GET /api/admin/monitoring/system-status`
- `GET /api/admin/stats/performance/providers`

These responses include `usage_counter`:

- `status`: `idle`, `catching_up`, or `backlogged`
- `outbox_pending_rows`
- `outbox_processed_rows`
- `oldest_pending_created_at_unix_secs`
- `oldest_pending_age_secs`
- `latest_processed_at_unix_secs`
- `pending_by_kind`

Operational alerting should page when `status = backlogged`, when pending rows
continue growing across several flush intervals, or when the oldest pending age
stays above one minute. A transient non-zero backlog is acceptable during catch
up bursts.

## Remaining Correctness Locks

Wallet debit settlement and daily quota consumption still use database locks
because they protect money/quota correctness, not derived counters. Removing
those locks safely requires a separate wallet debit ledger/reservation worker:

1. request settlement writes an immutable debit intent keyed by `request_id`
2. a per-wallet worker claims intents with `FOR UPDATE SKIP LOCKED`
3. the worker applies balance changes and writes final settlement snapshots
4. request-facing APIs read `pending/settled/insufficient_quota` from the
   settlement snapshot

Do not replace this with memory-only balance caches. A cache may accelerate
read-side availability estimates, but the durable ledger must remain the source
of truth.

## Acceptance Criteria

- request transactions no longer update shared counter rows
- counter updates become batchable and replayable
- lock wait time on `api_keys` / `provider_api_keys` drops sharply under concurrency
- wallet settlement remains correct and isolated
- dashboard/statistics reads stay fast from dedicated read models

## Open Decisions

- table names for counter read models
- whether provider window counters live in Postgres only or are dual-written into Redis for display latency
- flush cadence and batch size defaults
- whether to keep compatibility snapshot columns as low-frequency mirrors
