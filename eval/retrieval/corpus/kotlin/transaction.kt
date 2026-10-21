---
language: kotlin
---
// Save the order and invoice together or roll both writes back.
fun createOrder(command: CreateOrder): Order = database.transaction {
    val order = orders.insert(command)
    invoices.insert(Invoice.forOrder(order))
    order
}
