# Aether Schema Source

This directory is the PostgreSQL schema maintenance workspace. Executable
migrations remain under `../migrations/postgres`. The empty-database bootstrap
snapshot is compiled from the source fragments here during `aether-data`
builds.

## Maintenance flow

```bash
bash crates/aether-data/schema/compose_schema.sh generate
bash crates/aether-data/schema/compose_schema.sh compose
bash crates/aether-data/schema/compose_schema.sh check
```

- `logical/*.toml` is the long-term logical table model.
- `generated/postgres/` is machine-written PostgreSQL DDL used for review and
  drift detection.
- `drivers/postgres/` contains source fragments that compose into executable
  migrations.
- `bootstrap/postgres/` contains fragments for the embedded empty-database
  bootstrap snapshot.
- `overrides/` is reserved for rare PostgreSQL SQL that cannot be represented
  in the logical schema.

Do not edit generated SQL directly. Update the logical model or PostgreSQL
driver fragments, regenerate, and run the check command.

## Targets

| Target | Executable SQL | Source manifest |
|---|---|---|
| PostgreSQL baseline | `migrations/postgres/20260403000000_baseline.sql` | `drivers/postgres/baseline/manifest.txt` |
| PostgreSQL empty-database snapshot | `aether-data` build output | `bootstrap/postgres/manifest.txt` |

The Rust migration tests also compose these manifests so fragment drift is
caught during the data-crate test suite.
