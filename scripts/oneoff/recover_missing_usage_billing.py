#!/usr/bin/env python3
"""Recover historical zero-settled usage rows from Aether-owned history.

Default mode is a read-only dry run. ``--apply --confirm <batch-id>`` stages
estimated usage records as pending so the existing settlement retry path applies
wallet, plan, unlimited, and provider projection semantics. It never sends a
provider request and it never deletes the original usage row.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from datetime import datetime, timezone

TARGET_DB_HOST = "colocrossing-la-db"
TARGET_PSQL = [
    "sudo",
    "docker",
    "exec",
    "-i",
    "niffler-postgres15",
    "psql",
    "-v",
    "ON_ERROR_STOP=1",
    "-p",
    "55432",
    "-U",
    "postgres",
    "-d",
    "aether",
]
INCIDENT_START = "2026-09-01 07:00:00+00"
BASELINE_START = "2026-08-01 00:00:00+00"
BASELINE_END = INCIDENT_START
ESTIMATOR_VERSION = "historical-p50-v1"


class RecoveryError(RuntimeError):
    pass


def run_psql(sql: str) -> str:
    proc = subprocess.run(
        ["ssh", TARGET_DB_HOST, " ".join(TARGET_PSQL)],
        input=sql,
        text=True,
        capture_output=True,
        check=False,
    )
    if proc.returncode:
        detail = (proc.stderr or proc.stdout).strip()
        raise RecoveryError(f"生产数据库命令失败：{detail}")
    return proc.stdout


def sql_literal(value: str) -> str:
    return "'" + value.replace("'", "''") + "'"


def recovery_ctes(*, exclude_existing_cases: bool = False) -> str:
    existing_case_filter = ""
    if exclude_existing_cases:
        existing_case_filter = """
      AND NOT EXISTS (
          SELECT 1
          FROM public.usage_billing_recovery_cases AS c
          WHERE c.request_id = u.request_id
            AND c.action IN ('staged', 'settled', 'waived', 'manual_review')
      )
"""
    return f"""
WITH bad AS MATERIALIZED (
    SELECT
        u.request_id,
        u.user_id,
        u.wallet_id,
        u.provider_id,
        u.provider_api_key_id,
        u.provider_name,
        u.model,
        u.created_at,
        u.status,
        u.status_code,
        regexp_replace(
            regexp_replace(lower(trim(u.model)), '[._]', '-', 'g'),
            '-+', '-', 'g'
        ) AS canonical_model,
        COALESCE(
            NULLIF(u.request_metadata::jsonb->>'sales_multiplier', '')::numeric,
            1.0
        ) AS sales_multiplier,
        a.funding_source
    FROM public.usage AS u
    JOIN public.billing_request_admissions AS a
      ON a.request_id = u.request_id
    WHERE u.created_at >= {sql_literal(INCIDENT_START)}
      AND u.provider_name <> 'Niffler 平台'
      AND u.status = 'completed'
      AND u.status_code = 200
      AND u.billing_status = 'settled'
      AND COALESCE(u.total_tokens, 0) = 0
      AND COALESCE(u.total_cost_usd, 0) = 0
      AND NOT EXISTS (
          SELECT 1
          FROM public.usage_settlement_snapshots AS s
          WHERE s.request_id = u.request_id
            AND COALESCE(s.billing_total_cost_usd, 0) > 0
      )
{existing_case_filter}),
baseline AS MATERIALIZED (
    SELECT
        regexp_replace(
            regexp_replace(lower(trim(u.model)), '[._]', '-', 'g'),
            '-+', '-', 'g'
        ) AS canonical_model,
        u.provider_name,
        percentile_cont(0.5) WITHIN GROUP (ORDER BY u.actual_total_cost_usd)
            FILTER (WHERE u.actual_total_cost_usd > 0) AS p50_actual_cost,
        percentile_cont(0.5) WITHIN GROUP (ORDER BY u.input_tokens)
            FILTER (WHERE u.input_tokens > 0) AS p50_input_tokens,
        percentile_cont(0.5) WITHIN GROUP (ORDER BY u.output_tokens)
            FILTER (WHERE u.output_tokens > 0) AS p50_output_tokens,
        COUNT(*) FILTER (WHERE u.actual_total_cost_usd > 0) AS sample_count
    FROM public.usage AS u
    WHERE u.created_at >= {sql_literal(BASELINE_START)}
      AND u.created_at < {sql_literal(BASELINE_END)}
      AND u.provider_name <> 'Niffler 平台'
      AND u.status = 'completed'
      AND u.status_code = 200
    GROUP BY 1, 2
),
provider_baseline AS MATERIALIZED (
    SELECT
        u.provider_name,
        percentile_cont(0.5) WITHIN GROUP (ORDER BY u.actual_total_cost_usd)
            FILTER (WHERE u.actual_total_cost_usd > 0) AS p50_actual_cost,
        percentile_cont(0.5) WITHIN GROUP (ORDER BY u.input_tokens)
            FILTER (WHERE u.input_tokens > 0) AS p50_input_tokens,
        percentile_cont(0.5) WITHIN GROUP (ORDER BY u.output_tokens)
            FILTER (WHERE u.output_tokens > 0) AS p50_output_tokens,
        COUNT(*) FILTER (WHERE u.actual_total_cost_usd > 0) AS sample_count
    FROM public.usage AS u
    WHERE u.created_at >= {sql_literal(BASELINE_START)}
      AND u.created_at < {sql_literal(BASELINE_END)}
      AND u.provider_name <> 'Niffler 平台'
      AND u.status = 'completed'
      AND u.status_code = 200
    GROUP BY u.provider_name
),
estimated AS (
    SELECT
        b.*,
        COALESCE(pm.p50_actual_cost, pp.p50_actual_cost) AS base_cost_usd,
        COALESCE(pm.p50_input_tokens, pp.p50_input_tokens, 0)::bigint AS input_tokens,
        COALESCE(pm.p50_output_tokens, pp.p50_output_tokens, 0)::bigint AS output_tokens,
        CASE
            WHEN pm.p50_actual_cost IS NOT NULL THEN 'provider_model_p50'
            WHEN pp.p50_actual_cost IS NOT NULL THEN 'provider_p50'
            ELSE 'unrecoverable'
        END AS evidence_status,
        COALESCE(pm.sample_count, pp.sample_count, 0) AS baseline_sample_count
    FROM bad AS b
    LEFT JOIN baseline AS pm
      ON pm.provider_name = b.provider_name
     AND pm.canonical_model = b.canonical_model
    LEFT JOIN provider_baseline AS pp
      ON pp.provider_name = b.provider_name
)
"""


def dry_run() -> None:
    sql = recovery_ctes() + """
SELECT
    CASE WHEN GROUPING(provider_name) = 1 THEN 'TOTAL' ELSE provider_name END AS provider_name,
    CASE WHEN GROUPING(evidence_status) = 1 THEN 'TOTAL' ELSE evidence_status END AS evidence_status,
    CASE WHEN GROUPING(funding_source) = 1 THEN 'TOTAL' ELSE funding_source END AS funding_source,
    COUNT(*) AS records,
    COUNT(DISTINCT user_id) AS users,
    COUNT(*) FILTER (WHERE base_cost_usd IS NOT NULL) AS estimable_records,
    ROUND(COALESCE(SUM(ROUND((base_cost_usd * sales_multiplier)::numeric, 8)), 0)::numeric, 8) AS estimated_user_charge_usd,
    ROUND(COALESCE(SUM(base_cost_usd), 0)::numeric, 8) AS estimated_provider_cost_usd,
    MIN(baseline_sample_count) AS min_baseline_samples,
    MAX(baseline_sample_count) AS max_baseline_samples
FROM estimated
GROUP BY GROUPING SETS ((provider_name, evidence_status, funding_source), ())
ORDER BY GROUPING(provider_name), provider_name, evidence_status, funding_source;
"""
    print(run_psql(sql).strip())


def apply(batch_id: str) -> None:
    sql = f"""
BEGIN;
CREATE TABLE IF NOT EXISTS public.usage_billing_recovery_cases (
    request_id character varying(128) PRIMARY KEY REFERENCES public.usage(request_id) ON DELETE CASCADE,
    recovery_batch_id character varying(128) NOT NULL,
    user_id character varying(64),
    wallet_id character varying(64),
    provider_id character varying(64),
    provider_api_key_id character varying(64),
    provider_name character varying(255) NOT NULL,
    model character varying(255) NOT NULL,
    evidence_status character varying(32) NOT NULL,
    estimator_version character varying(64) NOT NULL,
    baseline_start timestamp with time zone NOT NULL,
    baseline_end timestamp with time zone NOT NULL,
    baseline_sample_count bigint NOT NULL DEFAULT 0,
    estimated_input_tokens bigint,
    estimated_output_tokens bigint,
    estimated_total_tokens bigint,
    estimated_base_cost_usd double precision,
    sales_multiplier double precision,
    estimated_user_cost_usd double precision,
    funding_source character varying(32),
    action character varying(32) NOT NULL DEFAULT 'staged',
    applied_at timestamp with time zone,
    created_at timestamp with time zone NOT NULL DEFAULT now(),
    updated_at timestamp with time zone NOT NULL DEFAULT now(),
    notes jsonb NOT NULL DEFAULT '{{}}'::jsonb,
    CONSTRAINT usage_billing_recovery_cases_evidence_check CHECK (
        evidence_status IN ('provider_model_p50', 'model_p50', 'provider_p50', 'unrecoverable')
    ),
    CONSTRAINT usage_billing_recovery_cases_action_check CHECK (
        action IN ('staged', 'settled', 'waived', 'manual_review')
    )
);
CREATE INDEX IF NOT EXISTS ix_usage_billing_recovery_cases_batch
    ON public.usage_billing_recovery_cases (recovery_batch_id, action);
CREATE INDEX IF NOT EXISTS ix_usage_billing_recovery_cases_user
    ON public.usage_billing_recovery_cases (user_id, created_at);

CREATE TEMP TABLE tmp_usage_billing_recovery ON COMMIT DROP AS
""" + recovery_ctes(exclude_existing_cases=True) + f"""
SELECT
    e.*,
    ROUND((e.base_cost_usd * e.sales_multiplier)::numeric, 8)::double precision AS user_cost_usd,
    e.input_tokens + e.output_tokens AS total_tokens
FROM estimated AS e;

INSERT INTO public.usage_billing_recovery_cases (
    request_id, recovery_batch_id, user_id, wallet_id, provider_id, provider_api_key_id,
    provider_name, model, evidence_status, estimator_version, baseline_start, baseline_end,
    baseline_sample_count, estimated_input_tokens, estimated_output_tokens, estimated_total_tokens,
    estimated_base_cost_usd, sales_multiplier, estimated_user_cost_usd, funding_source, action,
    notes
)
SELECT
    request_id, {sql_literal(batch_id)}, user_id, wallet_id, provider_id, provider_api_key_id,
    provider_name, model, evidence_status, {sql_literal(ESTIMATOR_VERSION)},
    {sql_literal(BASELINE_START)}::timestamptz, {sql_literal(BASELINE_END)}::timestamptz,
    baseline_sample_count, input_tokens, output_tokens, total_tokens,
    base_cost_usd, sales_multiplier, user_cost_usd, funding_source,
    CASE WHEN base_cost_usd IS NULL THEN 'manual_review' ELSE 'staged' END,
    jsonb_build_object('canonical_model', canonical_model, 'status', status, 'status_code', status_code)
FROM tmp_usage_billing_recovery
ON CONFLICT (request_id) DO NOTHING;

UPDATE public.usage_settlement_snapshots AS s
SET billing_status = 'pending',
    billing_snapshot_schema_version = 'v2',
    billing_snapshot_status = 'estimated',
    settlement_snapshot_schema_version = 'v2',
    settlement_snapshot = jsonb_build_object(
        'base_cost_usd', t.base_cost_usd,
        'sales_multiplier', t.sales_multiplier,
        'estimated', true,
        'recovery_batch_id', {sql_literal(batch_id)},
        'estimator_version', {sql_literal(ESTIMATOR_VERSION)}
    ),
    billing_dimensions = jsonb_build_object(
        'input_tokens', t.input_tokens,
        'output_tokens', t.output_tokens,
        'total_tokens', t.total_tokens,
        'estimated', true
    ),
    billing_input_tokens = t.input_tokens,
    billing_effective_input_tokens = t.input_tokens,
    billing_output_tokens = t.output_tokens,
    billing_total_input_context = t.input_tokens,
    billing_total_cost_usd = t.user_cost_usd,
    billing_actual_total_cost_usd = t.base_cost_usd,
    billing_pricing_source = 'historical_estimate',
    billing_rule_id = 'historical-usage-recovery-p50',
    billing_rule_version = 'v1',
    finalized_at = NULL,
    updated_at = NOW()
FROM tmp_usage_billing_recovery AS t
WHERE s.request_id = t.request_id
  AND t.base_cost_usd IS NOT NULL;

UPDATE public.usage AS u
SET input_tokens = t.input_tokens,
    output_tokens = t.output_tokens,
    input_output_total_tokens = t.total_tokens,
    total_tokens = t.total_tokens,
    total_cost_usd = t.user_cost_usd,
    actual_total_cost_usd = t.base_cost_usd,
    billing_status = 'pending',
    finalized_at = NULL,
    request_metadata = (
        COALESCE(u.request_metadata::jsonb, '{{}}'::jsonb)
        || jsonb_build_object(
            'base_cost_usd', t.base_cost_usd,
            'sales_multiplier', t.sales_multiplier,
            'billing_snapshot_schema_version', 'v2',
            'billing_snapshot_status', 'estimated',
            'billing_snapshot', jsonb_build_object(
                'base_cost_usd', t.base_cost_usd,
                'sales_multiplier', t.sales_multiplier,
                'estimated', true,
                'recovery_batch_id', {sql_literal(batch_id)},
                'estimator_version', {sql_literal(ESTIMATOR_VERSION)}
            ),
            'historical_usage_recovery', jsonb_build_object(
                'batch_id', {sql_literal(batch_id)},
                'evidence_status', t.evidence_status,
                'baseline_sample_count', t.baseline_sample_count,
                'estimated_input_tokens', t.input_tokens,
                'estimated_output_tokens', t.output_tokens
            )
        )
    )::json,
    updated_at_unix_secs = EXTRACT(EPOCH FROM NOW())::bigint
FROM tmp_usage_billing_recovery AS t
WHERE u.request_id = t.request_id
  AND t.base_cost_usd IS NOT NULL;

COMMIT;
"""
    print(run_psql(sql).strip())
    print(f"已将批次 {batch_id} 分阶段为 pending；等待现有 settlement retry 结算。")


def sync(batch_id: str) -> None:
    sql = f"""
UPDATE public.usage_billing_recovery_cases AS c
SET action = CASE
        WHEN COALESCE(s.billing_status, u.billing_status) = 'settled' THEN 'settled'
        WHEN COALESCE(s.billing_status, u.billing_status) = 'insufficient_quota' THEN 'manual_review'
        ELSE c.action
    END,
    applied_at = CASE
        WHEN COALESCE(s.billing_status, u.billing_status) IN ('settled', 'insufficient_quota')
        THEN COALESCE(c.applied_at, NOW())
        ELSE c.applied_at
    END,
    updated_at = NOW()
FROM public.usage AS u
LEFT JOIN public.usage_settlement_snapshots AS s ON s.request_id = u.request_id
WHERE c.request_id = u.request_id
  AND c.recovery_batch_id = {sql_literal(batch_id)};

SELECT action, COUNT(*)
FROM public.usage_billing_recovery_cases
WHERE recovery_batch_id = {sql_literal(batch_id)}
GROUP BY action
ORDER BY action;
"""
    print(run_psql(sql).strip())


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true", help="写入恢复批次并将可估算记录置为 pending")
    parser.add_argument("--sync", action="store_true", help="根据 usage/snapshot 状态同步 recovery case")
    parser.add_argument("--confirm", help="必须与 batch id 完全一致，防止误执行")
    parser.add_argument("--batch-id", help="恢复批次 ID；apply 时默认使用 UTC 时间生成")
    args = parser.parse_args()

    try:
        if args.sync:
            if not args.batch_id:
                raise RecoveryError("sync 需要 --batch-id")
            sync(args.batch_id)
            return 0
        if not args.apply:
            dry_run()
            return 0
        batch_id = args.batch_id or f"{ESTIMATOR_VERSION}-{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}"
        if args.confirm != batch_id:
            raise RecoveryError(
                f"apply 需要 --confirm {batch_id!r}；当前未执行任何写入。"
            )
        apply(batch_id)
        return 0
    except RecoveryError as exc:
        print(str(exc), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
