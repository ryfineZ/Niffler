use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use sqlx::Row;

use crate::error::SqlResultExt;
use crate::{DataLayerError, DatabaseDriver, SqlDatabaseConfig};

pub const EXPORT_FORMAT_VERSION: u32 = 1;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ExportDomain {
    Users,
    ApiKeys,
    Providers,
    ProviderKeys,
    Endpoints,
    GlobalModels,
    Models,
    AuthModules,
    OAuthProviders,
    UserOAuthLinks,
    UserGroups,
    UserGroupMembers,
    ProxyNodes,
    SystemConfigs,
    Wallets,
    Usage,
    Billing,
}
impl ExportDomain {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Users => "users",
            Self::ApiKeys => "api_keys",
            Self::Providers => "providers",
            Self::ProviderKeys => "provider_keys",
            Self::Endpoints => "endpoints",
            Self::Models => "models",
            Self::GlobalModels => "global_models",
            Self::AuthModules => "auth_modules",
            Self::OAuthProviders => "oauth_providers",
            Self::UserOAuthLinks => "user_oauth_links",
            Self::UserGroups => "user_groups",
            Self::UserGroupMembers => "user_group_members",
            Self::ProxyNodes => "proxy_nodes",
            Self::SystemConfigs => "system_configs",
            Self::Wallets => "wallets",
            Self::Usage => "usage",
            Self::Billing => "billing",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DataExportManifest {
    pub format_version: u32,
    pub created_at_unix_secs: u64,
    pub source_driver: Option<DatabaseDriver>,
    pub domains: Vec<ExportDomain>,
}

impl DataExportManifest {
    pub fn new(
        created_at_unix_secs: u64,
        source_driver: Option<DatabaseDriver>,
        domains: Vec<ExportDomain>,
    ) -> Self {
        let mut domains = domains;
        domains.sort();
        domains.dedup();
        Self {
            format_version: EXPORT_FORMAT_VERSION,
            created_at_unix_secs,
            source_driver,
            domains,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "record_type", rename_all = "snake_case")]
pub enum DataExportRecord {
    Manifest {
        manifest: DataExportManifest,
    },
    Row {
        domain: ExportDomain,
        id: String,
        payload: Value,
    },
}

impl DataExportRecord {
    pub fn manifest(manifest: DataExportManifest) -> Self {
        Self::Manifest { manifest }
    }

    pub fn row(domain: ExportDomain, id: impl Into<String>, payload: Value) -> Self {
        Self::Row {
            domain,
            id: id.into(),
            payload,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DataImportPlan {
    pub manifest: DataExportManifest,
    pub rows_by_domain: BTreeMap<ExportDomain, Vec<ExportRow>>,
}

impl DataImportPlan {
    pub fn rows(&self, domain: ExportDomain) -> &[ExportRow] {
        self.rows_by_domain
            .get(&domain)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportRow {
    pub id: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DataCopyOptions {
    pub omit_request_body_details: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PostgresImportColumn {
    data_type: String,
    udt_name: String,
    is_nullable: bool,
    has_default: bool,
}

type PostgresImportColumns = BTreeMap<String, PostgresImportColumn>;

const USAGE_REQUEST_BODY_DETAIL_COLUMNS: &[&str] = &[
    "request_body",
    "response_body",
    "provider_request_body",
    "client_response_body",
    "request_body_compressed",
    "response_body_compressed",
    "provider_request_body_compressed",
    "client_response_body_compressed",
];

pub fn encode_jsonl(records: &[DataExportRecord]) -> Result<String, DataLayerError> {
    validate_export_records(records)?;

    let mut output = String::new();
    for record in records {
        let line = serde_json::to_string(record)
            .map_err(|err| DataLayerError::UnexpectedValue(err.to_string()))?;
        output.push_str(&line);
        output.push('\n');
    }
    Ok(output)
}

pub fn decode_jsonl(input: &str) -> Result<Vec<DataExportRecord>, DataLayerError> {
    let mut records = Vec::new();
    for (line_index, line) in input.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let record = serde_json::from_str::<DataExportRecord>(line).map_err(|err| {
            DataLayerError::InvalidInput(format!(
                "invalid export JSONL record on line {}: {err}",
                line_index + 1
            ))
        })?;
        records.push(record);
    }
    validate_export_records(&records)?;
    Ok(records)
}

pub fn build_import_plan(input: &str) -> Result<DataImportPlan, DataLayerError> {
    let records = decode_jsonl(input)?;
    let manifest = match records.first() {
        Some(DataExportRecord::Manifest { manifest }) => manifest.clone(),
        _ => unreachable!("decode_jsonl validates the manifest record"),
    };
    let mut rows_by_domain = BTreeMap::<ExportDomain, Vec<ExportRow>>::new();
    for record in records.into_iter().skip(1) {
        let DataExportRecord::Row {
            domain,
            id,
            payload,
        } = record
        else {
            return Err(DataLayerError::InvalidInput(
                "export manifest must appear only as the first record".to_string(),
            ));
        };
        rows_by_domain
            .entry(domain)
            .or_default()
            .push(ExportRow { id, payload });
    }
    Ok(DataImportPlan {
        manifest,
        rows_by_domain,
    })
}

pub fn validate_export_records(records: &[DataExportRecord]) -> Result<(), DataLayerError> {
    let Some(DataExportRecord::Manifest { manifest }) = records.first() else {
        return Err(DataLayerError::InvalidInput(
            "export JSONL must start with a manifest record".to_string(),
        ));
    };
    if manifest.format_version != EXPORT_FORMAT_VERSION {
        return Err(DataLayerError::InvalidInput(format!(
            "unsupported export format version {}; expected {}",
            manifest.format_version, EXPORT_FORMAT_VERSION
        )));
    }

    let allowed_domains = manifest.domains.iter().copied().collect::<BTreeSet<_>>();
    let mut seen_ids = BTreeSet::<(ExportDomain, String)>::new();
    for (index, record) in records.iter().enumerate().skip(1) {
        match record {
            DataExportRecord::Manifest { .. } => {
                return Err(DataLayerError::InvalidInput(format!(
                    "export manifest appears more than once at record {}",
                    index + 1
                )));
            }
            DataExportRecord::Row {
                domain,
                id,
                payload: _,
            } => {
                if !allowed_domains.contains(domain) {
                    return Err(DataLayerError::InvalidInput(format!(
                        "record {} uses domain '{}' not declared in manifest",
                        index + 1,
                        domain.as_str()
                    )));
                }
                if id.trim().is_empty() {
                    return Err(DataLayerError::InvalidInput(format!(
                        "record {} has an empty id",
                        index + 1
                    )));
                }
                let key = (*domain, id.clone());
                if !seen_ids.insert(key) {
                    return Err(DataLayerError::InvalidInput(format!(
                        "duplicate '{}' export id '{}' at record {}",
                        domain.as_str(),
                        id,
                        index + 1
                    )));
                }
            }
        }
    }
    Ok(())
}

pub fn postgres_core_export_domains() -> Vec<ExportDomain> {
    vec![
        ExportDomain::Users,
        ExportDomain::ApiKeys,
        ExportDomain::Providers,
        ExportDomain::ProviderKeys,
        ExportDomain::Endpoints,
        ExportDomain::GlobalModels,
        ExportDomain::Models,
        ExportDomain::AuthModules,
        ExportDomain::OAuthProviders,
        ExportDomain::UserOAuthLinks,
        ExportDomain::UserGroups,
        ExportDomain::UserGroupMembers,
        ExportDomain::ProxyNodes,
        ExportDomain::SystemConfigs,
        ExportDomain::Wallets,
        ExportDomain::Usage,
        ExportDomain::Billing,
    ]
}

pub async fn export_database_jsonl(
    database: SqlDatabaseConfig,
    domains: Vec<ExportDomain>,
    created_at_unix_secs: u64,
) -> Result<String, DataLayerError> {
    let pool = crate::driver::postgres::PostgresPoolFactory::new(database.to_postgres_config()?)?
        .connect_lazy()?;
    if domains.is_empty() {
        export_postgres_core_jsonl(&pool, created_at_unix_secs).await
    } else {
        export_postgres_jsonl(&pool, domains, created_at_unix_secs).await
    }
}

pub async fn import_database_jsonl(
    database: SqlDatabaseConfig,
    input: &str,
) -> Result<usize, DataLayerError> {
    let pool = crate::driver::postgres::PostgresPoolFactory::new(database.to_postgres_config()?)?
        .connect_lazy()?;
    import_postgres_jsonl(&pool, input).await
}

pub async fn copy_database_records(
    source: SqlDatabaseConfig,
    target: SqlDatabaseConfig,
    domains: Vec<ExportDomain>,
    created_at_unix_secs: u64,
    options: DataCopyOptions,
) -> Result<usize, DataLayerError> {
    let mut records =
        decode_jsonl(&export_database_jsonl(source, domains, created_at_unix_secs).await?)?;
    if options.omit_request_body_details {
        omit_request_body_details_from_records(&mut records);
    }
    import_database_jsonl(target, &encode_jsonl(&records)?).await
}

fn omit_request_body_details_from_records(records: &mut [DataExportRecord]) {
    for record in records {
        let DataExportRecord::Row {
            domain: ExportDomain::Usage,
            payload,
            ..
        } = record
        else {
            continue;
        };

        if let Some(object) = payload.as_object_mut() {
            for column_name in USAGE_REQUEST_BODY_DETAIL_COLUMNS {
                object.remove(*column_name);
            }
        }
    }
}

pub async fn export_postgres_core_jsonl(
    pool: &crate::driver::postgres::PostgresPool,
    created_at_unix_secs: u64,
) -> Result<String, DataLayerError> {
    export_postgres_jsonl(pool, postgres_core_export_domains(), created_at_unix_secs).await
}

pub async fn export_postgres_jsonl(
    pool: &crate::driver::postgres::PostgresPool,
    domains: Vec<ExportDomain>,
    created_at_unix_secs: u64,
) -> Result<String, DataLayerError> {
    let manifest = DataExportManifest::new(
        created_at_unix_secs,
        Some(DatabaseDriver::Postgres),
        domains.clone(),
    );
    let mut records = vec![DataExportRecord::manifest(manifest)];

    for domain in domains {
        if domain == ExportDomain::Billing {
            export_postgres_billing_records(pool, &mut records).await?;
            continue;
        }
        if domain == ExportDomain::Wallets {
            export_postgres_wallet_records(pool, &mut records).await?;
            continue;
        }
        let (table_name, id_column) = postgres_domain_table(domain)?;
        let export_id_sql = postgres_export_id_sql(domain, id_column);
        let order_by = export_order_by(domain, id_column);
        let sql = format!(
            "SELECT {export_id_sql} AS export_id, to_jsonb(t) AS payload FROM {table_name} AS t ORDER BY {order_by}"
        );
        let rows = sqlx::query(&sql).fetch_all(pool).await.map_sql_err()?;
        for row in rows {
            let id = row.try_get::<String, _>("export_id").map_sql_err()?;
            let payload = row.try_get::<Value, _>("payload").map_sql_err()?;
            records.push(DataExportRecord::row(domain, id, payload));
        }
    }

    encode_jsonl(&records)
}

pub async fn import_postgres_jsonl(
    pool: &crate::driver::postgres::PostgresPool,
    input: &str,
) -> Result<usize, DataLayerError> {
    let plan = build_import_plan(input)?;
    import_postgres_plan(pool, &plan).await
}

pub async fn import_postgres_plan(
    pool: &crate::driver::postgres::PostgresPool,
    plan: &DataImportPlan,
) -> Result<usize, DataLayerError> {
    let mut imported = 0usize;
    let mut column_cache = BTreeMap::<String, PostgresImportColumns>::new();
    for domain in &plan.manifest.domains {
        if *domain == ExportDomain::Billing {
            for row in plan.rows(*domain) {
                import_postgres_billing_row(pool, row, &mut column_cache).await?;
                imported = imported.saturating_add(1);
            }
            continue;
        }
        if *domain == ExportDomain::Wallets {
            for row in plan.rows(*domain) {
                import_postgres_wallet_row(pool, row, &mut column_cache).await?;
                imported = imported.saturating_add(1);
            }
            continue;
        }
        let (table_name, id_column) = postgres_domain_table(*domain)?;
        let conflict_columns = postgres_conflict_columns(*domain, id_column);
        let rows = plan.rows(*domain);
        if rows.is_empty() {
            continue;
        }
        let target_columns =
            postgres_import_columns_cached(pool, &mut column_cache, table_name).await?;
        for row in rows {
            import_postgres_row(
                pool,
                table_name,
                &conflict_columns,
                *domain,
                row,
                &target_columns,
            )
            .await?;
            imported = imported.saturating_add(1);
        }
    }
    Ok(imported)
}

fn export_order_by(domain: ExportDomain, id_column: &str) -> String {
    if domain == ExportDomain::UserGroupMembers {
        "group_id ASC, user_id ASC".to_string()
    } else {
        format!("{id_column} ASC")
    }
}

fn postgres_domain_table(
    domain: ExportDomain,
) -> Result<(&'static str, &'static str), DataLayerError> {
    match domain {
        ExportDomain::Users => Ok(("public.users", "id")),
        ExportDomain::ApiKeys => Ok(("public.api_keys", "id")),
        ExportDomain::Providers => Ok(("public.providers", "id")),
        ExportDomain::ProviderKeys => Ok(("public.provider_api_keys", "id")),
        ExportDomain::Endpoints => Ok(("public.provider_endpoints", "id")),
        ExportDomain::Models => Ok(("public.models", "id")),
        ExportDomain::GlobalModels => Ok(("public.global_models", "id")),
        ExportDomain::AuthModules => Ok(("public.auth_modules", "id")),
        ExportDomain::OAuthProviders => Ok(("public.oauth_providers", "provider_type")),
        ExportDomain::UserOAuthLinks => Ok(("public.user_oauth_links", "id")),
        ExportDomain::UserGroups => Ok(("public.user_groups", "id")),
        ExportDomain::UserGroupMembers => Ok(("public.user_group_members", "group_id")),
        ExportDomain::ProxyNodes => Ok(("public.proxy_nodes", "id")),
        ExportDomain::SystemConfigs => Ok(("public.system_configs", "id")),
        ExportDomain::Wallets => Err(DataLayerError::InvalidInput(
            "postgres wallet export uses multiple tables and must be handled as a domain"
                .to_string(),
        )),
        ExportDomain::Usage => Ok(("public.usage", "request_id")),
        ExportDomain::Billing => Err(DataLayerError::InvalidInput(
            "postgres billing export uses multiple tables and must be handled as a domain"
                .to_string(),
        )),
    }
}

fn postgres_export_id_sql(domain: ExportDomain, id_column: &str) -> String {
    if domain == ExportDomain::UserGroupMembers {
        "group_id::text || ':' || user_id::text".to_string()
    } else {
        format!("{id_column}::text")
    }
}

fn postgres_conflict_columns(domain: ExportDomain, id_column: &str) -> Vec<&str> {
    if domain == ExportDomain::UserGroupMembers {
        vec!["group_id", "user_id"]
    } else {
        vec![id_column]
    }
}

async fn postgres_import_columns_cached(
    pool: &crate::driver::postgres::PostgresPool,
    cache: &mut BTreeMap<String, PostgresImportColumns>,
    table_name: &str,
) -> Result<PostgresImportColumns, DataLayerError> {
    if let Some(columns) = cache.get(table_name) {
        return Ok(columns.clone());
    }

    let columns = load_postgres_import_columns(pool, table_name).await?;
    cache.insert(table_name.to_string(), columns.clone());
    Ok(columns)
}

async fn load_postgres_import_columns(
    pool: &crate::driver::postgres::PostgresPool,
    table_name: &str,
) -> Result<PostgresImportColumns, DataLayerError> {
    let (schema_name, relation_name) = postgres_table_parts(table_name)?;
    let rows = sqlx::query(
        r#"
SELECT column_name, data_type, udt_name, is_nullable, column_default IS NOT NULL AS has_default
FROM information_schema.columns
WHERE table_schema = $1
  AND table_name = $2
"#,
    )
    .bind(schema_name)
    .bind(relation_name)
    .fetch_all(pool)
    .await
    .map_sql_err()?;

    let mut columns = PostgresImportColumns::new();
    for row in rows {
        let column_name = row.try_get::<String, _>("column_name").map_sql_err()?;
        let data_type = row
            .try_get::<String, _>("data_type")
            .map_sql_err()?
            .to_ascii_lowercase();
        let udt_name = row
            .try_get::<String, _>("udt_name")
            .map_sql_err()?
            .to_ascii_lowercase();
        let is_nullable = row.try_get::<String, _>("is_nullable").map_sql_err()? == "YES";
        let has_default = row.try_get::<bool, _>("has_default").map_sql_err()?;
        columns.insert(
            column_name,
            PostgresImportColumn {
                data_type,
                udt_name,
                is_nullable,
                has_default,
            },
        );
    }

    if columns.is_empty() {
        return Err(DataLayerError::UnexpectedValue(format!(
            "postgres import target table '{table_name}' has no visible columns"
        )));
    }

    Ok(columns)
}

fn postgres_table_parts(table_name: &str) -> Result<(&str, &str), DataLayerError> {
    let Some((schema_name, relation_name)) = table_name.split_once('.') else {
        return Err(DataLayerError::InvalidInput(format!(
            "postgres import target table '{table_name}' must include a schema"
        )));
    };
    Ok((
        schema_name.trim_matches('"'),
        relation_name.trim_matches('"'),
    ))
}

async fn export_postgres_billing_records(
    pool: &crate::driver::postgres::PostgresPool,
    records: &mut Vec<DataExportRecord>,
) -> Result<(), DataLayerError> {
    for (table_name, export_table, id_column) in [
        ("public.billing_rules", "billing_rules", "id"),
        ("public.dimension_collectors", "dimension_collectors", "id"),
        (
            "public.usage_settlement_snapshots",
            "usage_settlement_snapshots",
            "request_id",
        ),
    ] {
        let sql = format!(
            "SELECT {id_column}::text AS export_id, to_jsonb(t) || jsonb_build_object('__table', '{export_table}') AS payload FROM {table_name} AS t ORDER BY {id_column} ASC"
        );
        let rows = sqlx::query(&sql).fetch_all(pool).await.map_sql_err()?;
        for row in rows {
            let id = row.try_get::<String, _>("export_id").map_sql_err()?;
            let payload = row.try_get::<Value, _>("payload").map_sql_err()?;
            records.push(DataExportRecord::row(
                ExportDomain::Billing,
                format!("{export_table}:{id}"),
                payload,
            ));
        }
    }
    Ok(())
}

async fn export_postgres_wallet_records(
    pool: &crate::driver::postgres::PostgresPool,
    records: &mut Vec<DataExportRecord>,
) -> Result<(), DataLayerError> {
    for (table_name, export_table, id_column) in postgres_wallet_tables() {
        let sql = format!(
            "SELECT {id_column}::text AS export_id, to_jsonb(t) || jsonb_build_object('__table', '{export_table}') AS payload FROM {table_name} AS t ORDER BY {id_column} ASC"
        );
        let rows = sqlx::query(&sql).fetch_all(pool).await.map_sql_err()?;
        for row in rows {
            let id = row.try_get::<String, _>("export_id").map_sql_err()?;
            let payload = row.try_get::<Value, _>("payload").map_sql_err()?;
            records.push(DataExportRecord::row(
                ExportDomain::Wallets,
                format!("{export_table}:{id}"),
                payload,
            ));
        }
    }
    Ok(())
}

async fn import_postgres_row(
    pool: &crate::driver::postgres::PostgresPool,
    table_name: &str,
    conflict_columns: &[&str],
    domain: ExportDomain,
    row: &ExportRow,
    target_columns: &PostgresImportColumns,
) -> Result<(), DataLayerError> {
    let object = normalize_postgres_import_payload(table_name, domain, row, target_columns)?;

    let columns = object.keys().map(String::as_str).collect::<Vec<_>>();
    let column_sql = columns
        .iter()
        .map(|column| postgres_quote_identifier(column))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    let update_sql = columns
        .iter()
        .filter(|column| !conflict_columns.contains(column))
        .map(|column| {
            let quoted = postgres_quote_identifier(column)?;
            Ok(format!("{quoted} = EXCLUDED.{quoted}"))
        })
        .collect::<Result<Vec<_>, DataLayerError>>()?
        .join(", ");
    let conflict_target_sql = conflict_columns
        .iter()
        .map(|column| postgres_quote_identifier(column))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    let conflict_sql = if update_sql.is_empty() {
        format!("ON CONFLICT ({conflict_target_sql}) DO NOTHING")
    } else {
        format!("ON CONFLICT ({conflict_target_sql}) DO UPDATE SET {update_sql}")
    };
    let sql = format!(
        "INSERT INTO {table_name} ({column_sql}) SELECT {column_sql} FROM jsonb_populate_record(NULL::{table_name}, $1::jsonb) {conflict_sql}"
    );
    let payload = Value::Object(object);

    sqlx::query(&sql)
        .bind(&payload)
        .execute(pool)
        .await
        .map_sql_err()?;
    Ok(())
}

fn normalize_postgres_import_payload(
    table_name: &str,
    domain: ExportDomain,
    row: &ExportRow,
    target_columns: &PostgresImportColumns,
) -> Result<serde_json::Map<String, Value>, DataLayerError> {
    let object = row.payload.as_object().ok_or_else(|| {
        DataLayerError::InvalidInput(format!(
            "{} export row '{}' payload must be a JSON object",
            domain.as_str(),
            row.id
        ))
    })?;
    if object.is_empty() {
        return Err(DataLayerError::InvalidInput(format!(
            "{} export row '{}' payload cannot be empty",
            domain.as_str(),
            row.id
        )));
    }

    let mut normalized = serde_json::Map::new();
    for (column_name, value) in object {
        if let Some(target_column) = target_columns.get(column_name) {
            if value.is_null() && !target_column.is_nullable && target_column.has_default {
                continue;
            }
            normalized.insert(
                column_name.clone(),
                normalize_postgres_import_value(column_name, target_column, value)?,
            );
            continue;
        }
        if value.is_null() {
            continue;
        }
        return Err(DataLayerError::InvalidInput(format!(
            "{} export row '{}' contains column '{}' that does not exist in postgres table '{}'",
            domain.as_str(),
            row.id,
            column_name,
            table_name
        )));
    }

    if normalized.is_empty() {
        return Err(DataLayerError::InvalidInput(format!(
            "{} export row '{}' has no columns supported by postgres table '{}'",
            domain.as_str(),
            row.id,
            table_name
        )));
    }

    Ok(normalized)
}

fn normalize_postgres_import_value(
    column_name: &str,
    target_column: &PostgresImportColumn,
    value: &Value,
) -> Result<Value, DataLayerError> {
    if value.is_null() {
        return Ok(Value::Null);
    }

    if is_postgres_boolean_column(target_column) {
        return normalize_postgres_boolean_value(column_name, value);
    }
    if is_postgres_timestamp_column(target_column) {
        return normalize_postgres_timestamp_value(column_name, value);
    }
    if is_postgres_json_column(target_column) {
        return normalize_postgres_json_value(value);
    }

    Ok(value.clone())
}

fn is_postgres_boolean_column(target_column: &PostgresImportColumn) -> bool {
    target_column.data_type == "boolean" || target_column.udt_name == "bool"
}

fn is_postgres_timestamp_column(target_column: &PostgresImportColumn) -> bool {
    matches!(
        target_column.data_type.as_str(),
        "timestamp with time zone" | "timestamp without time zone"
    ) || matches!(target_column.udt_name.as_str(), "timestamptz" | "timestamp")
}

fn is_postgres_json_column(target_column: &PostgresImportColumn) -> bool {
    matches!(target_column.data_type.as_str(), "json" | "jsonb")
        || matches!(target_column.udt_name.as_str(), "json" | "jsonb")
}

fn normalize_postgres_boolean_value(
    column_name: &str,
    value: &Value,
) -> Result<Value, DataLayerError> {
    match value {
        Value::Bool(_) => Ok(value.clone()),
        Value::Number(number) => {
            let Some(value) = number
                .as_i64()
                .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            else {
                return Err(DataLayerError::InvalidInput(format!(
                    "postgres boolean import column '{column_name}' has non-integer value {number}"
                )));
            };
            match value {
                0 => Ok(Value::Bool(false)),
                1 => Ok(Value::Bool(true)),
                other => Err(DataLayerError::InvalidInput(format!(
                    "postgres boolean import column '{column_name}' has unsupported integer value {other}"
                ))),
            }
        }
        Value::String(value) => match value.trim().to_ascii_lowercase().as_str() {
            "0" | "false" => Ok(Value::Bool(false)),
            "1" | "true" => Ok(Value::Bool(true)),
            _ => Ok(Value::String(value.clone())),
        },
        _ => Ok(value.clone()),
    }
}

fn normalize_postgres_timestamp_value(
    column_name: &str,
    value: &Value,
) -> Result<Value, DataLayerError> {
    let Value::Number(number) = value else {
        return Ok(value.clone());
    };
    let Some(timestamp) = number
        .as_i64()
        .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
    else {
        return Err(DataLayerError::InvalidInput(format!(
            "postgres timestamp import column '{column_name}' has non-integer value {number}"
        )));
    };

    let datetime = if column_name.ends_with("_unix_ms")
        || timestamp >= 100_000_000_000
        || timestamp <= -100_000_000_000
    {
        chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp)
    } else {
        chrono::DateTime::<chrono::Utc>::from_timestamp(timestamp, 0)
    }
    .ok_or_else(|| {
        DataLayerError::InvalidInput(format!(
            "postgres timestamp import column '{column_name}' has out-of-range unix value {timestamp}"
        ))
    })?;

    Ok(Value::String(datetime.to_rfc3339()))
}

fn normalize_postgres_json_value(value: &Value) -> Result<Value, DataLayerError> {
    let Value::String(raw) = value else {
        return Ok(value.clone());
    };
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(value.clone());
    }
    match serde_json::from_str::<Value>(raw) {
        Ok(parsed) => Ok(parsed),
        Err(_) => Ok(value.clone()),
    }
}

async fn import_postgres_billing_row(
    pool: &crate::driver::postgres::PostgresPool,
    row: &ExportRow,
    column_cache: &mut BTreeMap<String, PostgresImportColumns>,
) -> Result<(), DataLayerError> {
    let (export_table_name, payload) = billing_payload_table(row)?;
    let table_name = postgres_billing_table_name(&export_table_name)?;
    let target_columns = postgres_import_columns_cached(pool, column_cache, table_name).await?;
    import_postgres_row(
        pool,
        table_name,
        &["id"],
        ExportDomain::Billing,
        &ExportRow {
            id: row.id.clone(),
            payload,
        },
        &target_columns,
    )
    .await
}

fn postgres_billing_table_name(table_name: &str) -> Result<&'static str, DataLayerError> {
    match table_name {
        "billing_rules" => Ok("public.billing_rules"),
        "dimension_collectors" => Ok("public.dimension_collectors"),
        "usage_settlement_snapshots" => Ok("public.usage_settlement_snapshots"),
        other => Err(DataLayerError::InvalidInput(format!(
            "unsupported postgres billing export table '{other}'"
        ))),
    }
}

async fn import_postgres_wallet_row(
    pool: &crate::driver::postgres::PostgresPool,
    row: &ExportRow,
    column_cache: &mut BTreeMap<String, PostgresImportColumns>,
) -> Result<(), DataLayerError> {
    let (export_table_name, payload) = domain_payload_table(row, "wallet", Some("wallets"))?;
    let (table_name, id_column) = postgres_wallet_table_name(&export_table_name)?;
    let target_columns = postgres_import_columns_cached(pool, column_cache, table_name).await?;
    import_postgres_row(
        pool,
        table_name,
        &[id_column],
        ExportDomain::Wallets,
        &ExportRow {
            id: row.id.clone(),
            payload,
        },
        &target_columns,
    )
    .await
}

fn postgres_wallet_tables() -> &'static [(&'static str, &'static str, &'static str)] {
    &[
        ("public.wallets", "wallets", "id"),
        ("public.wallet_transactions", "wallet_transactions", "id"),
        (
            "public.wallet_daily_usage_ledgers",
            "wallet_daily_usage_ledgers",
            "id",
        ),
        ("public.payment_orders", "payment_orders", "id"),
        ("public.payment_callbacks", "payment_callbacks", "id"),
        ("public.refund_requests", "refund_requests", "id"),
        ("public.redeem_code_batches", "redeem_code_batches", "id"),
        ("public.redeem_codes", "redeem_codes", "id"),
    ]
}

fn postgres_wallet_table_name(
    table_name: &str,
) -> Result<(&'static str, &'static str), DataLayerError> {
    postgres_wallet_tables()
        .iter()
        .find(|(_, export_table, _)| *export_table == table_name)
        .map(|(table, _, id_column)| (*table, *id_column))
        .ok_or_else(|| {
            DataLayerError::InvalidInput(format!(
                "unsupported postgres wallet export table '{table_name}'"
            ))
        })
}

fn postgres_quote_identifier(identifier: &str) -> Result<String, DataLayerError> {
    if identifier.trim().is_empty() {
        return Err(DataLayerError::InvalidInput(
            "postgres import column name cannot be empty".to_string(),
        ));
    }
    if !identifier
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return Err(DataLayerError::InvalidInput(format!(
            "postgres import column name '{identifier}' contains unsupported characters"
        )));
    }
    Ok(format!(r#""{identifier}""#))
}

fn billing_payload_table(row: &ExportRow) -> Result<(String, Value), DataLayerError> {
    domain_payload_table(row, "billing", None)
}

fn domain_payload_table(
    row: &ExportRow,
    domain_label: &str,
    default_table: Option<&str>,
) -> Result<(String, Value), DataLayerError> {
    let mut object = row.payload.as_object().cloned().ok_or_else(|| {
        DataLayerError::InvalidInput(format!(
            "{domain_label} export row '{}' payload must be a JSON object",
            row.id,
        ))
    })?;
    let table_name = match object.remove("__table") {
        Some(value) => value.as_str().map(ToOwned::to_owned).ok_or_else(|| {
            DataLayerError::InvalidInput(format!(
                "{domain_label} export row '{}' has non-string __table",
                row.id
            ))
        })?,
        None => default_table.map(str::to_string).ok_or_else(|| {
            DataLayerError::InvalidInput(format!(
                "{domain_label} export row '{}' is missing string __table",
                row.id
            ))
        })?,
    };
    Ok((table_name, Value::Object(object)))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::{
        build_import_plan, decode_jsonl, encode_jsonl, export_postgres_core_jsonl,
        normalize_postgres_import_payload, postgres_core_export_domains, DataExportManifest,
        DataExportRecord, ExportDomain, ExportRow, PostgresImportColumn,
    };
    use crate::driver::postgres::{PostgresPoolConfig, PostgresPoolFactory};
    use crate::lifecycle::migrate::run_migrations as run_postgres_migrations;
    use crate::DatabaseDriver;

    #[test]
    fn jsonl_round_trips_manifest_and_domain_rows() {
        let records = vec![
            DataExportRecord::manifest(DataExportManifest::new(
                1_700_000_000,
                Some(DatabaseDriver::Postgres),
                vec![ExportDomain::Users, ExportDomain::ApiKeys],
            )),
            DataExportRecord::row(
                ExportDomain::Users,
                "user-1",
                json!({"id": "user-1", "email": "owner@example.com"}),
            ),
        ];

        let encoded = encode_jsonl(&records).expect("records should encode");
        let decoded = decode_jsonl(&encoded).expect("records should decode");
        assert_eq!(decoded, records);

        let import_plan = build_import_plan(&encoded).expect("import plan should build");
        assert_eq!(
            import_plan.manifest.source_driver,
            Some(DatabaseDriver::Postgres)
        );
        assert_eq!(import_plan.rows(ExportDomain::Users).len(), 1);
    }

    #[test]
    fn postgres_core_export_domains_are_complete_and_unique() {
        let domains = postgres_core_export_domains();
        let unique = domains
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(domains.len(), unique.len());
        assert!(unique.contains(&ExportDomain::Users));
        assert!(unique.contains(&ExportDomain::Billing));
        assert!(unique.contains(&ExportDomain::Usage));
    }

    #[test]
    fn jsonl_rejects_missing_manifest() {
        let error =
            decode_jsonl(r#"{"record_type":"row","domain":"users","id":"user-1","payload":{}}"#)
                .expect_err("missing manifest should fail");
        assert!(error.to_string().contains("must start with a manifest"));
    }

    #[test]
    fn jsonl_rejects_rows_outside_manifest_domains() {
        let records = vec![
            DataExportRecord::manifest(DataExportManifest::new(
                1_700_000_000,
                Some(DatabaseDriver::Postgres),
                vec![ExportDomain::Users],
            )),
            DataExportRecord::row(ExportDomain::Wallets, "wallet-1", json!({"id": "wallet-1"})),
        ];

        let error = encode_jsonl(&records).expect_err("undeclared domain should fail");
        assert!(error.to_string().contains("not declared in manifest"));
    }

    #[test]
    fn postgres_import_normalizes_legacy_values() {
        let columns = BTreeMap::from([
            (
                "is_active".to_string(),
                PostgresImportColumn {
                    data_type: "boolean".to_string(),
                    udt_name: "bool".to_string(),
                    is_nullable: false,
                    has_default: false,
                },
            ),
            (
                "created_at".to_string(),
                PostgresImportColumn {
                    data_type: "timestamp with time zone".to_string(),
                    udt_name: "timestamptz".to_string(),
                    is_nullable: false,
                    has_default: false,
                },
            ),
            (
                "metadata".to_string(),
                PostgresImportColumn {
                    data_type: "jsonb".to_string(),
                    udt_name: "jsonb".to_string(),
                    is_nullable: true,
                    has_default: false,
                },
            ),
        ]);
        let row = ExportRow {
            id: "row-1".to_string(),
            payload: json!({
                "is_active": 1,
                "created_at": 1_700_000_000,
                "metadata": "{\"source\":\"legacy\"}"
            }),
        };

        let normalized = normalize_postgres_import_payload(
            "public.example",
            ExportDomain::Users,
            &row,
            &columns,
        )
        .expect("legacy values should normalize");

        assert_eq!(normalized["is_active"], true);
        assert!(normalized["created_at"]
            .as_str()
            .is_some_and(|value| value.starts_with("2023-")));
        assert_eq!(normalized["metadata"], json!({"source": "legacy"}));
    }

    #[tokio::test]
    async fn postgres_core_export_reads_migrated_database_rows_when_url_is_set() {
        let Some(database_url) = std::env::var("AETHER_TEST_POSTGRES_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
        else {
            eprintln!(
                "skipping postgres core export smoke test because AETHER_TEST_POSTGRES_URL is unset"
            );
            return;
        };

        let pool = PostgresPoolFactory::new(PostgresPoolConfig {
            database_url,
            min_connections: 1,
            max_connections: 1,
            acquire_timeout_ms: 1_000,
            idle_timeout_ms: 5_000,
            max_lifetime_ms: 30_000,
            statement_cache_capacity: 64,
            require_ssl: false,
        })
        .expect("postgres factory should build")
        .connect_lazy()
        .expect("postgres pool should build");
        run_postgres_migrations(&pool)
            .await
            .expect("postgres migrations should run");

        let suffix = unique_suffix();
        let user_id = format!("export-user-{suffix}");
        let api_key_id = format!("export-api-key-{suffix}");
        sqlx::query(
            "INSERT INTO users (id, email, username, auth_source, email_verified, created_at, updated_at) VALUES ($1, $2, $3, 'local', TRUE, to_timestamp(1), to_timestamp(2))",
        )
        .bind(&user_id)
        .bind(format!("{user_id}@example.com"))
        .bind(format!("owner-{suffix}"))
        .execute(&pool)
        .await
        .expect("user should seed");
        sqlx::query(
            "INSERT INTO api_keys (id, user_id, key_hash, key_encrypted, name, created_at, updated_at) VALUES ($1, $2, $3, 'ciphertext-1', 'Default', to_timestamp(1), to_timestamp(2))",
        )
        .bind(&api_key_id)
        .bind(&user_id)
        .bind(format!("hash-{api_key_id}"))
        .execute(&pool)
        .await
        .expect("api key should seed");

        let encoded = export_postgres_core_jsonl(&pool, 1_700_000_000)
            .await
            .expect("postgres export should encode");
        let import_plan = build_import_plan(&encoded).expect("postgres export should decode");

        assert_eq!(
            import_plan.manifest.source_driver,
            Some(DatabaseDriver::Postgres)
        );
        assert_eq!(import_plan.manifest.domains, postgres_core_export_domains());
        assert!(import_plan
            .rows(ExportDomain::Users)
            .iter()
            .any(|row| row.id == user_id));
        assert!(import_plan
            .rows(ExportDomain::ApiKeys)
            .iter()
            .any(|row| row.id == api_key_id && row.payload["key_encrypted"] == "ciphertext-1"));
    }

    fn unique_suffix() -> String {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        format!("{:016x}", nanos ^ counter.rotate_left(17))
    }
}
