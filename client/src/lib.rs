use async_std::os::unix::net::UnixStream;
use async_std::task;

pub use hyperspace_common::*;
mod freemap;
mod session;
pub mod stream;

pub use session::*;
pub use stream::*;

pub async fn open_corestore(host: Option<String>) -> std::io::Result<RemoteCorestore> {
    let socket_path = socket_path(host);
    let socket = UnixStream::connect(socket_path).await?;
    let mut rpc = hrpc::Rpc::new();
    let corestore = RemoteCorestore::new(&mut rpc);
    task::spawn(async move {
        rpc.connect(socket).await.unwrap();
    });
    Ok(corestore)
}
