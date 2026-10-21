---
language: lua
---
-- Count requests in a fixed window and reject after the configured maximum.
function allow_fixed_window(counter, maximum)
  counter.count = counter.count + 1
  return counter.count <= maximum
end
