use async_std::stream::StreamExt;
use hypercore_replicator::{Replicator, ReplicatorEvent};
use hyperswarm::{Config, Hyperswarm, HyperswarmStream, TopicConfig};
use std::io;

use crate::Opts;

pub async fn run(mut replicator: Replicator, opts: Opts) -> io::Result<()> {
    let config = swarm_config_from_opts(opts).await?;
    let swarm = Hyperswarm::bind(config).await?;
    let swarm_handle = swarm.handle();
    let replicator_events = replicator.subscribe().await;

    enum Event {
        Replicator(ReplicatorEvent),
        Swarm(io::Result<HyperswarmStream>),
    };

    let mut events = replicator_events
        .map(Event::Replicator)
        .merge(swarm.map(Event::Swarm));

    while let Some(event) = events.next().await {
        match event {
            Event::Replicator(event) => match event {
                ReplicatorEvent::Feed(discovery_key) => {
                    let config = TopicConfig::announce_and_lookup();
                    swarm_handle.configure(discovery_key, config);
                }
                _ => {}
            },
            Event::Swarm(stream) => {
                let stream = stream?;
                let is_initator = stream.is_initiator();
                replicator.add_stream(stream, is_initator).await;
            }
        }
    }
    Ok(())
}

async fn swarm_config_from_opts(opts: Opts) -> io::Result<Config> {
    let config = Config::default();
    let config = if opts.bootstrap.len() > 0 {
        config.set_bootstrap_nodes(opts.bootstrap[..].to_vec())
    } else {
        config
    };
    // TODO: Support these options.
    // let config = config.ephemeral();
    // if let Some(address) = opts.address {
    //     config.bind(address).await.map_err(|(_config, e)| e)
    // } else {
    //     Ok(config)
    // }
    Ok(config)
}
