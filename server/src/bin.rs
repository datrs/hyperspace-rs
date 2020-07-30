use async_std::task;
use clap::Clap;
use hyperspace_server::{listen, Opts};

fn main() -> anyhow::Result<()> {
    env_logger::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let opts: Opts = Opts::parse();
    task::block_on(listen(opts))
}
