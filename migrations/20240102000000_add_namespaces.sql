-- Add namespace and owner_token columns
ALTER TABLE metrics ADD COLUMN namespace TEXT NOT NULL DEFAULT '';
ALTER TABLE metrics ADD COLUMN owner_token TEXT;

-- Create new composite primary key and index
DROP INDEX idx_metrics_id_timestamp;
CREATE INDEX idx_metrics_namespace_id_timestamp ON metrics (namespace, id, timestamp);

-- Create index for namespace ownership lookups
CREATE INDEX idx_metrics_namespace_owner ON metrics (namespace, owner_token);