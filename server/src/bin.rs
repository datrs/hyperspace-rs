use async_std::task;
use clap::Clap;
use hyperspace_server::{listen, Opts};
use log::*;

fn main() -> anyhow::Result<()> {
    env_logger::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let opts: Opts = Opts::parse();
    info!("{:?}", opts);
    task::block_on(listen(opts))
}
