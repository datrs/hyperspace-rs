//! A store for Hypercore feeds

mod corestore;
mod feed_map;
mod replicate;

pub use crate::corestore::{ArcFeed, Corestore, Event};
pub use hypercore::{Event as FeedEvent, Feed};
pub use replicate::replicate_corestore;
