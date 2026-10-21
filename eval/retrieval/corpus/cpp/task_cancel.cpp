---
language: cpp
---
// Ask pending work items to cancel when shutdown begins.
void WorkerPool::cancelPending() {
    std::lock_guard<std::mutex> lock(mutex_);
    for (auto& task : tasks_) task.cancel();
    tasks_.clear();
}
