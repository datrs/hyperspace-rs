use async_std::task;
use clap::Clap;
use hyperspace_server::{listen, run_bootstrap_node, Opts};

fn main() -> anyhow::Result<()> {
    env_logger::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let opts: Opts = Opts::parse();
    task::block_on(async_main(opts))
}

async fn async_main(opts: Opts) -> anyhow::Result<()> {
    if opts.dht {
        let (addr, task) = run_bootstrap_node(opts).await?;
        log::info!("bootstrap node address: {}", addr);
        Ok(task.await)
    } else {
        listen(opts).await
    }
}
