-- no-transaction
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_rc_capacity_recovery
ON request_candidates (provider_id, key_id, (COALESCE(extra_data->>'upstream_model', extra_data->>'mapped_model', 'unknown')), (COALESCE(finished_at, created_at)))
WHERE status = 'success';
