# aether-data-schema

`aether-data-schema` is the PostgreSQL logical-schema generator for
`aether-data`.

It parses and validates `crates/aether-data/schema/logical/*.toml`, emits
PostgreSQL DDL, and checks that generated schema artifacts are current. Runtime
migrations, backfills, export/import, and repository SQL remain owned by
`aether-data`.

## Commands

From the workspace root:

```bash
cargo run -p aether-data-schema --bin aether-schema -- check
cargo run -p aether-data-schema --bin aether-schema -- generate
cargo run -p aether-data-schema --bin aether-schema -- print --driver postgres
```

The normal maintenance entrypoint wraps these commands:

```bash
bash crates/aether-data/schema/compose_schema.sh generate
bash crates/aether-data/schema/compose_schema.sh check
```

Input is `crates/aether-data/schema/logical/*.toml`. Generated output is written
to `crates/aether-data/schema/generated/postgres/baseline/`. Generated files
and manifests are checked in for review but must not be edited by hand.
