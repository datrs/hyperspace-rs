use async_std::os::unix::net::{UnixListener, UnixStream};
use futures::stream::StreamExt;
use log::*;
use std::io::Result;
use std::path::Path;

pub async fn accept<P, S, Cb>(path: P, state: S, mut onconnection: Cb) -> Result<()>
where
    P: AsRef<Path>,
    S: Clone,
    Cb: FnMut(S, UnixStream),
{
    let path = path.as_ref().to_path_buf();
    // let sock_remover = SockRemover { path: path.clone() };
    let listener = match UnixListener::bind(&path).await {
        Ok(listener) => listener,
        Err(_) => {
            std::fs::remove_file(&path)?;
            UnixListener::bind(&path).await?
        }
    };
    info!("Listening on {}", path.to_str().unwrap());
    let mut incoming = listener.incoming();
    while let Some(stream) = incoming.next().await {
        let stream = stream?;
        onconnection(state.clone(), stream);
    }
    // TODO: On ctrl-c this is not run I think.
    std::fs::remove_file(&path).unwrap();
    debug!("Deleted socket file {:?}", path);
    // let _ = sock_remover;
    Ok(())
}

// TODO: This also did not run always.
// struct SockRemover {
//     path: PathBuf,
// }
// impl Drop for SockRemover {
//     fn drop(&mut self) {
//         if std::fs::remove_file(&self.path).is_err() {
//             error!("Could not remove socket {}", self.path.to_str().unwrap());
//         } else {
//             debug!("Removed socket {}", self.path.to_str().unwrap());
//         }
//     }
// }
