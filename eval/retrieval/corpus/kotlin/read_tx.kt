---
language: kotlin
---
// Read a consistent snapshot of an order and its invoice.
fun readOrder(orderId: Long): OrderView = database.readTransaction {
    OrderView(orders.find(orderId), invoices.findForOrder(orderId))
}
