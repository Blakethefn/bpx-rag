---
language: go
---
// Report queue depth and delivery throughput for monitoring dashboards.
func QueueMetrics(queue Queue) Metrics {
    return Metrics{Depth: queue.Len(), Delivered: queue.DeliveredCount()}
}
