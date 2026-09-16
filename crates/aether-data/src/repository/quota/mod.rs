mod memory;
mod postgres;

use aether_data_query::{DialectSql, SelectColumn, SelectQuery};

#[allow(unused_imports)]
pub(crate) use aether_data_contracts::repository::quota::{
    ProviderQuotaReadRepository, ProviderQuotaRepository, ProviderQuotaWriteRepository,
    StoredProviderQuotaSnapshot,
};
pub use memory::InMemoryProviderQuotaRepository;
pub use postgres::SqlxProviderQuotaRepository;

fn quota_snapshot_select() -> SelectQuery<'static> {
    SelectQuery::new("providers").select_columns([
        SelectColumn::expr("id").alias("provider_id"),
        SelectColumn::expr(
            DialectSql::common("billing_type").with_postgres("CAST(billing_type AS TEXT)"),
        )
        .alias("billing_type"),
        SelectColumn::expr(DialectSql::postgres(
            "CAST(monthly_quota_usd AS DOUBLE PRECISION)",
        ))
        .alias("monthly_quota_usd"),
        SelectColumn::expr(DialectSql::postgres(
            "CAST(COALESCE(monthly_used_usd, 0) AS DOUBLE PRECISION)",
        ))
        .alias("monthly_used_usd"),
        SelectColumn::expr("quota_reset_day"),
        SelectColumn::expr(DialectSql::postgres(
            "CAST(EXTRACT(EPOCH FROM quota_last_reset_at) AS BIGINT)",
        ))
        .alias("quota_last_reset_at_unix_secs"),
        SelectColumn::expr(DialectSql::postgres(
            "CAST(EXTRACT(EPOCH FROM quota_expires_at) AS BIGINT)",
        ))
        .alias("quota_expires_at_unix_secs"),
        SelectColumn::expr("is_active"),
    ])
}
