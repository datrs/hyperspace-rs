use async_std::os::unix::net::{UnixListener, UnixStream};
use futures::stream::StreamExt;
use log::*;
use std::io::Result;
use std::path::{Path, PathBuf};

pub async fn accept<P, S, Cb>(path: P, state: S, mut onconnection: Cb) -> Result<()>
where
    P: AsRef<Path>,
    S: Clone,
    Cb: FnMut(S, UnixStream),
{
    let path = path.as_ref().to_path_buf();
    let sock_remover = SockRemover { path: path.clone() };
    let listener = UnixListener::bind(&path).await?;
    info!("Listening on {}", path.to_str().unwrap());
    let mut incoming = listener.incoming();
    while let Some(stream) = incoming.next().await {
        let stream = stream?;
        onconnection(state.clone(), stream);
    }
    let _ = sock_remover;
    Ok(())
}

struct SockRemover {
    path: PathBuf,
}
impl Drop for SockRemover {
    fn drop(&mut self) {
        if std::fs::remove_file(&self.path).is_err() {
            error!("Could not remove socket {}", self.path.to_str().unwrap());
        } else {
            debug!("Removed socket {}", self.path.to_str().unwrap());
        }
    }
}
