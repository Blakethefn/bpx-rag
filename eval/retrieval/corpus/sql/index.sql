---
language: sql
---
-- Speed tenant-scoped queries ordered by newest creation time.
CREATE INDEX idx_events_tenant_created
    ON events (tenant_id, created_at DESC);
