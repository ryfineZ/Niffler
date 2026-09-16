# PostgreSQL Data Schema Inventory

The runtime data layer supports PostgreSQL only. Executable `sqlx` migrations
live under `crates/aether-data/migrations/postgres`.

## Sources

| Path | Role |
|---|---|
| `crates/aether-data/schema/logical/*.toml` | Logical table definitions. |
| `crates/aether-data/schema/drivers/postgres/**` | PostgreSQL executable-SQL fragments. |
| `crates/aether-data/schema/bootstrap/postgres/**` | PostgreSQL empty-database bootstrap source. |
| `crates/aether-data/schema/generated/postgres/**` | Generated PostgreSQL DDL for review and drift detection. |
| `crates/aether-data/migrations/postgres/**` | Runtime migrations embedded by `sqlx`. |

## Maintenance contract

Structural changes start in the logical TOML model. Run:

```bash
bash crates/aether-data/schema/compose_schema.sh generate
bash crates/aether-data/schema/compose_schema.sh compose
bash crates/aether-data/schema/compose_schema.sh check
```

The check verifies generated output, bootstrap source readability, required
logical tables, and byte-for-byte composition of the PostgreSQL baseline.

## Supported logical types

Logical types are rendered directly to PostgreSQL types, including `jsonb`,
`boolean`, `bigint`, `numeric`, timestamps, and PostgreSQL arrays where the
model requires them. New schema work does not need a portability fallback for
another SQL dialect.

References to other database engines in older dated architecture records are
historical and do not describe the current runtime support matrix.
