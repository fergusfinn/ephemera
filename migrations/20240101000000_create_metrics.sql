-- Create metrics table
CREATE TABLE metrics (
    id TEXT NOT NULL,
    value REAL NOT NULL,
    timestamp INTEGER NOT NULL,
    PRIMARY KEY (id, timestamp)
);

-- Index for efficient queries by id and timestamp
CREATE INDEX idx_metrics_id_timestamp ON metrics (id, timestamp);