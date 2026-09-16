-- no-transaction
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_rc_capacity_errors_time
ON request_candidates (provider_id, (COALESCE(finished_at, created_at)))
WHERE extra_data->>'capacity_error' = 'true' OR (status = 'failed' AND (
    error_message ILIKE '%selected model is at capacity%' OR error_message ILIKE '%server\_is\_overloaded%'
    OR error_message ILIKE '%slow\_down%' OR (error_type ILIKE '%server\_is\_overloaded%' OR error_type ILIKE '%slow\_down%' OR error_type ILIKE '%selected model is at capacity%')));
