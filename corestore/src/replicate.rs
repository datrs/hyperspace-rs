use crate::{Corestore, Event as CorestoreEvent};
use async_std::sync::{Arc, Mutex};
use async_std::{task, task::JoinHandle};
use futures::stream::StreamExt;
use hypercore_replicator::{Replicator, ReplicatorEvent};
use std::io;

// pub fn replicate_corestore(corestore: Arc<Mutex<Corestore>>) -> (Replicator, JoinHandle<()>) {
//     let replicator = Replicator::new();
//     let task = task::spawn(task_replicate(corestore, replicator.clone()));
//     (replicator, task)
// }

pub async fn replicate_corestore(
    corestore: Arc<Mutex<Corestore>>,
    mut replicator: Replicator,
) -> io::Result<()> {
    let mut corestore_events = corestore.lock().await.subscribe().await;
    let mut replicator_events = replicator.subscribe().await;
    for feed in corestore.lock().await.feeds() {
        replicator.add_feed(feed.clone()).await;
    }

    let corestore_clone = corestore.clone();
    let task = task::spawn(async move {
        while let Some(event) = replicator_events.next().await {
            match event {
                ReplicatorEvent::DiscoveryKey(dkey) => {
                    // Try to open the feed from the corestore. If it exists,
                    // it will be added to the protocol through
                    // CorestoreEvent::Feed.
                    let _ = corestore_clone.lock().await.get_by_dkey(dkey).await;
                }
                _ => {}
            }
        }
    });

    while let Some(event) = corestore_events.next().await {
        match event {
            CorestoreEvent::Feed(feed) => {
                replicator.add_feed(feed).await;
            }
        }
    }

    task.await;
    Ok(())
}
