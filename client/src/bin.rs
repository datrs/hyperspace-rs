// TODO: Cleanup
#![allow(dead_code)]
#![allow(unused_imports)]

use anyhow::{anyhow, Context, Result};
use async_std::io;
use async_std::io::BufReader;
use async_std::os::unix::net::UnixStream;
use async_std::prelude::*;
use async_std::sync::Arc;
use async_std::task;
use async_trait::async_trait;
use clap::Clap;
use env_logger::Env;
use futures::io::{AsyncRead, AsyncWrite};
use futures::stream::{StreamExt, TryStreamExt};
use hrpc::{Client, Decoder, Rpc};
use log::*;
use std::collections::HashMap;
use std::env;
use std::io::Error;
use std::sync::Mutex;

use hyperspace_client::codegen;
use hyperspace_client::{RemoteCorestore, RemoteHypercore};
use hyperspace_common::socket_path;

#[derive(Clap, Debug)]
pub struct Opts {
    /// Hypercore key
    #[clap(short, long)]
    pub key: Option<String>,
    /// Hypercore name
    #[clap(short, long)]
    pub name: Option<String>,

    /// Override socket name to connect to
    #[clap(short, long)]
    pub host: Option<String>,

    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Clap, Debug)]
pub enum Command {
    /// Read from a Hypercore
    Read,
    /// Write to a Hypercore
    Write,
}

fn usage() {
    eprintln!("usage: hyperspace-client <name> [read|write]");
    std::process::exit(1);
}

fn exit<T>(message: T)
where
    T: ToString,
{
    eprintln!("{}", message.to_string());
    std::process::exit(1);
}

fn main() -> Result<()> {
    env_logger::from_env(Env::default().default_filter_or("info")).init();
    let opts: Opts = Opts::parse();
    task::block_on(async_main(opts))
}

pub async fn async_main(opts: Opts) -> Result<()> {
    debug!("{:?}", opts);
    let socket_path = socket_path(opts.host);
    let socket = UnixStream::connect(socket_path).await?;
    let mut rpc = Rpc::new();
    let mut corestore = RemoteCorestore::new(&mut rpc);
    task::spawn(async move {
        rpc.connect(socket).await.unwrap();
    });

    let core = match (opts.key, opts.name) {
        (Some(key), _) => {
            let key = hex::decode(key).with_context(|| "Invalid key")?;
            if key.len() != 32 {
                Err(anyhow!(
                    "Invalid key: Must be 32 bytes long, not {}.",
                    key.len()
                ))
            } else {
                Ok(corestore.open_by_key(key).await?)
            }
        }
        (None, Some(name)) => Ok(corestore.open_by_name(name).await?),
        _ => Err(anyhow!("Either key or name is required")),
    };
    let core = core?;

    // TODO: The mutex/inner of RemoteHypercore is not yet pleasant to work with..
    info!(
        "Opened feed: {}",
        hex::encode(core.read().key.clone().unwrap())
    );
    debug!("Opened feed: {:?}", core);

    match opts.command {
        Command::Read => {
            let output = io::stdout();
            read(core, output).await?;
        }
        Command::Write => {
            let input = BufReader::new(io::stdin());
            write(core, input).await?;
        }
    };

    Ok(())
}

async fn read(mut core: RemoteHypercore, mut writer: impl AsyncWrite + Send + Unpin) -> Result<()> {
    use std::io::SeekFrom;
    let stream = core.to_stream(0, None, true);
    let mut async_read = stream.into_async_read();
    // let mut async_read = core.to_reader();
    // async_read.seek(SeekFrom::Start(8)).await?;
    async_std::io::copy(&mut async_read, &mut writer).await?;
    info!("finished");
    Ok(())
}

async fn write(mut core: RemoteHypercore, mut reader: impl AsyncRead + Send + Unpin) -> Result<()> {
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = reader.read(&mut buf).await?;
        if n > 0 {
            let vec = (&buf[..n]).to_vec();
            core.append(vec![vec]).await?;
            info!("written n {}", n);
        }
    }
}
