use crate::{Corestore, Event as CorestoreEvent};
use async_std::stream::StreamExt;
use async_std::sync::{Arc, Mutex};
use hypercore_replicator::{Replicator, ReplicatorEvent};
use std::io;

// pub fn replicate_corestore(corestore: Arc<Mutex<Corestore>>) -> (Replicator, JoinHandle<()>) {
//     let replicator = Replicator::new();
//     let task = task::spawn(task_replicate(corestore, replicator.clone()));
//     (replicator, task)
// }

/// Replicate all feeds in a corestore with peers
pub async fn replicate_corestore(
    mut corestore: Corestore,
    mut replicator: Replicator,
) -> io::Result<()> {
    // Add all feeds in the corestore to the replicator.
    for feed in &corestore.feeds().await {
        replicator.add_feed(feed.clone()).await;
    }

    // Wait for events from the corestore and the replicator.
    let corestore_events = corestore.subscribe().await;
    let replicator_events = replicator.subscribe().await;
    enum Event {
        Corestore(CorestoreEvent),
        Replicator(ReplicatorEvent),
    }

    let mut events = replicator_events
        .map(Event::Replicator)
        .merge(corestore_events.map(Event::Corestore));

    while let Some(event) = events.next().await {
        match event {
            Event::Replicator(ReplicatorEvent::DiscoveryKey(dkey)) => {
                // Try to open the feed from the corestore. If it exists,
                // it will be added to the protocol through
                // CorestoreEvent::Feed.
                let _ = corestore.get_by_dkey(dkey).await;
            }
            Event::Corestore(CorestoreEvent::Feed(feed)) => {
                replicator.add_feed(feed).await;
            }
            _ => {}
        }
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[async_std::test]
    async fn test_corestore_replicate() -> anyhow::Result<()> {
        use piper::pipe;
        use tempdir::TempDir;

        init();

        let dir1 = TempDir::new("corestore-test1")?;
        let dir2 = TempDir::new("corestore-test2")?;

        let store1 = Corestore::open(&dir1).await?;
        let store1 = Arc::new(Mutex::new(store1));
        let store2 = Corestore::open(&dir2).await?;
        let store2 = Arc::new(Mutex::new(store2));

        let mut rep1 = Replicator::new();
        let mut rep2 = Replicator::new();

        let feed1 = store1.lock().await.get_by_name("foo").await?;
        let key1 = {
            let mut feed1 = feed1.lock().await;
            feed1.append("hello".as_bytes()).await?;
            feed1.public_key().as_bytes().to_vec()
        };

        let _ = store2.lock().await.get_by_key(&key1).await?;

        let cap = 1024 * 1024 * 4;

        let (r1, w1) = pipe(cap);
        let (r2, w2) = pipe(cap);
        rep1.add_io(r1, w2, true).await;
        rep2.add_io(r2, w1, false).await;

        task::spawn(replicate_corestore(store1.clone(), rep1.clone()));
        task::spawn(replicate_corestore(store2.clone(), rep2.clone()));

        let feed2 = store2.lock().await.get_by_key(&key1).await?;

        // TODO: Don't await a timeout but an event.
        timeout(100).await;

        let block = feed2.lock().await.get(0).await?;
        assert!(block == Some("hello".as_bytes().to_vec()));

        Ok(())
    }

    async fn timeout(ms: u64) {
        let _ = async_std::future::timeout(
            std::time::Duration::from_millis(ms),
            futures::future::pending::<()>(),
        )
        .await;
    }

    fn init() {
        let _ = env_logger::builder().is_test(true).try_init();
    }
}
