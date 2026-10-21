---
language: cpp
---
// Join every worker before destroying shared state during shutdown.
void WorkerPool::shutdown() {
    {
        std::lock_guard<std::mutex> lock(mutex_);
        stopping_ = true;
    }
    ready_.notify_all();
    for (auto& worker : workers_) worker.join();
}
