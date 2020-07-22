use clap::Clap;
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Clap, Debug)]
pub struct Opts {
    /// Set storage path
    #[clap(short, long)]
    pub storage: Option<PathBuf>,

    /// Set unix hostname
    #[clap(short, long)]
    pub host: Option<String>,

    /// Address to which Hyperswarm binds
    #[clap(short, long, default_value = "127.0.0.1:3401")]
    pub address: SocketAddr,

    /// Override default bootstrapp addresses
    #[clap(short, long)]
    pub bootstrap: Vec<SocketAddr>,

    /// Set a default port to announce and listen on.
    #[clap(short, long, default_value = "12345")]
    pub port: u32,

    /// Run a local bootstrapping dht node
    #[clap(long)]
    pub dht: bool,

    /// A level of verbosity, and can be used multiple times
    #[clap(short, long, parse(from_occurrences))]
    pub verbose: i32,
}
