---
language: lua
---
-- Limit requests with tokens that refill steadily over time.
function allow_request(bucket, now)
  refill(bucket, now)
  if bucket.tokens < 1 then return false end
  bucket.tokens = bucket.tokens - 1
  return true
end
