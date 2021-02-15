use async_std::os::unix::net::UnixStream;
use async_std::task;

use hyperspace_common::*;
mod freemap;
mod session;
mod stream;

pub use hyperspace_common::codegen;
pub use session::*;
pub use stream::*;

/// Open a remote corestore
///
/// Example:
/// ```no_run
/// # #[async_std::main]
/// # async fn main () -> anyhow::Result<()> {
/// use hyperspace_client::open_corestore;
/// let mut corestore = open_corestore(None).await?;
/// let mut feed = corestore.open_by_name("somename").await?;
/// let block = "hello, world".as_bytes().to_vec();
/// feed.append(vec![block]).await?;
/// let _block = feed.get(0).await?;
/// Ok(())
/// # }
/// ```
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
