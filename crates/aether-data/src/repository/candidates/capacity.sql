WITH failures AS MATERIALIZED (
    SELECT id, request_id, provider_id, key_id,
        COALESCE(extra_data->>'upstream_model', extra_data->>'mapped_model', 'unknown') AS model,
        COALESCE(finished_at, created_at) AS occurred_at
    FROM request_candidates
    WHERE provider_id = ANY($1) AND key_id IS NOT NULL
        AND COALESCE(finished_at, created_at) >= TO_TIMESTAMP($2::double precision / 1000)
        AND COALESCE(finished_at, created_at) <= TO_TIMESTAMP($3::double precision / 1000)
        AND (extra_data->>'capacity_error' = 'true' OR (status = 'failed' AND (
            error_message ILIKE '%selected model is at capacity%'
            OR error_message ILIKE '%server\_is\_overloaded%'
            OR error_message ILIKE '%slow\_down%'
            OR (error_type ILIKE '%server\_is\_overloaded%' OR error_type ILIKE '%slow\_down%' OR error_type ILIKE '%selected model is at capacity%'))))
), ranked AS (
    SELECT *, ROW_NUMBER() OVER (PARTITION BY provider_id, key_id, model ORDER BY occurred_at DESC, id) AS rn
    FROM failures
), grouped AS (
    SELECT provider_id, key_id, model, COUNT(*) AS count_24h, MAX(occurred_at) AS last_occurred_at,
        JSONB_AGG(JSONB_BUILD_OBJECT('candidate_id', id, 'request_id', request_id,
            'occurred_at_ms', (EXTRACT(EPOCH FROM occurred_at) * 1000)::bigint)
            ORDER BY occurred_at DESC, id) FILTER (WHERE rn <= 20) AS recent
    FROM ranked GROUP BY provider_id, key_id, model
)
SELECT JSONB_BUILD_OBJECT('provider_id', f.provider_id, 'key_id', f.key_id, 'model', f.model,
    'count_24h', f.count_24h, 'last_occurred_at_ms', (EXTRACT(EPOCH FROM f.last_occurred_at) * 1000)::bigint,
    'last_success_at_ms', (EXTRACT(EPOCH FROM s.at) * 1000)::bigint, 'recent', f.recent) AS summary
FROM grouped f
LEFT JOIN LATERAL (
    SELECT MAX(COALESCE(finished_at, created_at)) AS at FROM request_candidates s
    WHERE s.provider_id = f.provider_id AND s.key_id = f.key_id AND s.status = 'success'
        AND COALESCE(s.extra_data->>'upstream_model', s.extra_data->>'mapped_model', 'unknown') = f.model
        AND COALESCE(s.finished_at, s.created_at) > f.last_occurred_at
        AND COALESCE(s.finished_at, s.created_at) <= TO_TIMESTAMP($3::double precision / 1000)
) s ON TRUE
