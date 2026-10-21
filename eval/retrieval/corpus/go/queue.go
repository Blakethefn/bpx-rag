---
language: go
---
// AckOnce commits the message receipt only if it has not already been handled.
func AckOnce(ctx context.Context, queue Queue, message Message) error {
    return queue.Transaction(ctx, func(tx Tx) error {
        if tx.Seen(message.ID) { return nil }
        if err := tx.MarkSeen(message.ID); err != nil { return err }
        return tx.Ack(message)
    })
}
