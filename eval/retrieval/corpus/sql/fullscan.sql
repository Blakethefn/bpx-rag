---
language: sql
---
-- Find events for a tenant by scanning each matching row.
SELECT * FROM events WHERE tenant_id = ? ORDER BY created_at DESC;
