use crossbeam_channel::Receiver;

/// Adapter that bridges a crossbeam receiver into a Tokio mpsc sender.
pub fn spawn_bridge<T: Send + 'static>(
    rx: Receiver<T>,
    tx: tokio::sync::mpsc::Sender<T>,
) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        while let Ok(msg) = rx.recv() {
            if tx.blocking_send(msg).is_err() {
                break;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::spawn_bridge;
    use tokio::time::{Duration, timeout};

    #[tokio::test]
    async fn bridge_forwards_messages_in_order() {
        let (crossbeam_tx, crossbeam_rx) = crossbeam_channel::bounded(4);
        let (tokio_tx, mut tokio_rx) = tokio::sync::mpsc::channel(4);

        let handle = spawn_bridge(crossbeam_rx, tokio_tx);

        crossbeam_tx.send(10).expect("first send should work");
        crossbeam_tx.send(20).expect("second send should work");
        drop(crossbeam_tx);

        let first = timeout(Duration::from_millis(200), tokio_rx.recv())
            .await
            .expect("first recv should not time out");
        let second = timeout(Duration::from_millis(200), tokio_rx.recv())
            .await
            .expect("second recv should not time out");

        assert_eq!(first, Some(10));
        assert_eq!(second, Some(20));

        handle.await.expect("bridge task should complete");
    }

    #[tokio::test]
    async fn bridge_stops_when_tokio_receiver_closes() {
        let (crossbeam_tx, crossbeam_rx) = crossbeam_channel::bounded(2);
        let (tokio_tx, tokio_rx) = tokio::sync::mpsc::channel::<u8>(1);

        let handle = spawn_bridge(crossbeam_rx, tokio_tx);
        drop(tokio_rx);

        crossbeam_tx
            .send(1)
            .expect("send should succeed before close propagation");
        drop(crossbeam_tx);

        handle.await.expect("bridge task should stop cleanly");
    }
}
