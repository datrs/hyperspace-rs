mod corestore;
mod feed_map;
mod replicate;

pub use corestore::{Corestore, Event};
pub use hypercore::Feed;
pub use replicate::replicate_corestore;
