# aether-data

`aether-data` is the PostgreSQL runtime data-access crate. It owns the
PostgreSQL driver, concrete repository implementations, migrations, backfills,
export/import workflows, and the composition layer used by the application.

Cross-crate DTOs and repository contracts live in `../aether-data-contracts`.

## Directory map

| Path | Responsibility |
|---|---|
| `src/database.rs` | PostgreSQL selection and shared pool configuration. |
| `src/driver/postgres` | Low-level PostgreSQL pool, transaction, and lease primitives. |
| `src/repository` | Domain repository contracts plus PostgreSQL and in-memory implementations. |
| `src/backend` | Composition root and app-facing repository handles. |
| `src/lifecycle` | Migration, backfill, export, and import workflows. |
| `migrations/postgres` | Executable `sqlx` migrations. |
| `schema` | Logical schema, PostgreSQL fragments, bootstrap source, and generated output. |
| `backfills/postgres` | Executable PostgreSQL backfills. |

## Layering rules

1. Shared contracts belong in `aether-data-contracts`.
2. Low-level connection and transaction mechanics belong in
   `driver/postgres`.
3. Domain SQL belongs in `repository/<domain>/postgres.rs`.
4. Driver composition belongs in `backend`.
5. Database lifecycle work belongs in `lifecycle` and `schema`.

Do not add domain queries to pool modules or database selection logic to
individual repositories.

## Schema maintenance

Start structural changes in `schema/logical/*.toml`, then regenerate and check:

```bash
bash crates/aether-data/schema/compose_schema.sh generate
bash crates/aether-data/schema/compose_schema.sh compose
bash crates/aether-data/schema/compose_schema.sh check
```

Generated SQL is checked in for drift review but is not loaded at runtime. Edit
`schema/drivers/postgres` only for migration compatibility, ordering, or
generator gaps.
