use async_std::os::unix::net::UnixStream;
use async_std::task;
use std::io::Result;
use std::path::Path;

use crate::State;

mod session;
pub mod socket;

pub use session::Session;

pub async fn run_rpc<P>(socket_path: P, state: State) -> Result<()>
where
    P: AsRef<Path>,
{
    socket::accept(socket_path, state, on_rpc_connection).await
}

pub fn on_rpc_connection(state: State, stream: UnixStream) {
    log::info!("new connection from {:?}", stream.peer_addr().unwrap());
    let mut rpc = hrpc::Rpc::new();
    let _session = Session::new(&mut rpc, state);
    task::spawn(async move {
        rpc.connect(stream).await.unwrap();
    });
}
