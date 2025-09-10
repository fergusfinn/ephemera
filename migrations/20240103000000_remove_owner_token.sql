-- Remove owner_token column (SQLite doesn't support DROP COLUMN directly)
-- We'll create a new table without the column and copy data over

CREATE TABLE metrics_new (
    namespace TEXT NOT NULL,
    id TEXT NOT NULL,
    value REAL NOT NULL,
    timestamp INTEGER NOT NULL
);

-- Copy data from old table to new table
INSERT INTO metrics_new (namespace, id, value, timestamp)
SELECT namespace, id, value, timestamp FROM metrics;

-- Drop old table and rename new table
DROP TABLE metrics;
ALTER TABLE metrics_new RENAME TO metrics;

-- Recreate index if it existed
CREATE INDEX idx_namespace_id_timestamp ON metrics(namespace, id, timestamp);