//! A store for Hypercore feeds

mod corestore;
mod feed_map;
mod replicate;

pub use crate::corestore::{Corestore, Event};
pub use hypercore::Feed;
pub use replicate::replicate_corestore;
