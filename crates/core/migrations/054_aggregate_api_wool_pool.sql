ALTER TABLE aggregate_apis ADD COLUMN pool TEXT NOT NULL DEFAULT 'primary';
ALTER TABLE aggregate_apis ADD COLUMN wool_max_inflight INTEGER;
ALTER TABLE aggregate_apis ADD COLUMN wool_cooldown_until INTEGER;
ALTER TABLE aggregate_apis ADD COLUMN wool_failure_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE aggregate_apis ADD COLUMN wool_last_preflight_at INTEGER;

UPDATE aggregate_apis
SET pool = 'primary'
WHERE pool IS NULL OR TRIM(pool) = '' OR pool NOT IN ('primary', 'wool');

UPDATE aggregate_apis
SET wool_failure_count = COALESCE(wool_failure_count, 0)
WHERE wool_failure_count IS NULL;
