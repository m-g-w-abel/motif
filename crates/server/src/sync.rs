//! A metadata sync session with a linked engine ([`net::Alpn::Sync`], #239):
//! both ends say what they have ([`Versions`]), send the ops the other lacks
//! a page at a time, and apply what they hear (`library/src/sync.rs`). On
//! one stream each way: versions, pages, an empty page, then an empty page
//! once all the other's are applied, after which the dialler closes. An
//! interrupted session loses nothing: the next one starts where it ended.

use std::time::Duration;

use library::{
    Library,
    sync::{Logged, Versions},
};
use net::{Connection, RecvStream, SendStream};
use serde::{Serialize, de::DeserializeOwned};

/// Ops per page: a few hundred KB of JSON.
const PAGE: u32 = 500;
/// The largest frame read; a page is far smaller.
const FRAME_CAP: usize = 8 << 20;

/// Runs a session on `conn`, which this end dialled or accepted.
pub(crate) async fn session(
    library: &Library,
    conn: &Connection,
    dialled: bool,
) -> Result<(), String> {
    let (mut send, mut recv) = if dialled {
        conn.open_bi().await
    } else {
        conn.accept_bi().await
    }
    .map_err(|e| e.to_string())?;
    let mine = blocking(library, |l| l.versions()).await?;
    write(&mut send, &mine).await?;
    let mut theirs: Versions = read(&mut recv).await?;
    let give = async {
        loop {
            let seen = theirs.clone();
            let page = blocking(library, move |l| l.ops_since(&seen, PAGE)).await?;
            write(&mut send, &page).await?;
            if page.is_empty() {
                return Ok::<_, String>(());
            }
            for op in page {
                theirs.insert(op.device, op.seq);
            }
        }
    };
    let take = async {
        loop {
            let page: Vec<Logged> = read(&mut recv).await?;
            if page.is_empty() {
                return Ok(());
            }
            blocking(library, move |l| l.merge(page)).await?;
        }
    };
    tokio::try_join!(give, take)?;
    write(&mut send, &Vec::<Logged>::new()).await?;
    send.finish().map_err(|e| e.to_string())?;
    if dialled {
        let _: Vec<Logged> = read(&mut recv).await?;
        conn.close(0u32.into(), b"done");
    } else {
        // The dialler closes once it has heard that its ops are applied.
        let _ = tokio::time::timeout(Duration::from_secs(10), conn.closed()).await;
    }
    Ok(())
}

async fn blocking<T: Send + 'static>(
    library: &Library,
    f: impl FnOnce(&Library) -> library::Result<T> + Send + 'static,
) -> Result<T, String> {
    let library = library.clone();
    tokio::task::spawn_blocking(move || f(&library))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// A frame: its length (4 bytes, big-endian), then JSON.
async fn write(send: &mut SendStream, value: &impl Serialize) -> Result<(), String> {
    let json = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    send.write_all(&(json.len() as u32).to_be_bytes())
        .await
        .map_err(|e| e.to_string())?;
    send.write_all(&json).await.map_err(|e| e.to_string())
}

async fn read<T: DeserializeOwned>(recv: &mut RecvStream) -> Result<T, String> {
    let mut len = [0; 4];
    recv.read_exact(&mut len).await.map_err(|e| e.to_string())?;
    let len = u32::from_be_bytes(len) as usize;
    if len > FRAME_CAP {
        return Err(format!("a {len}-byte frame"));
    }
    let mut json = vec![0; len];
    recv.read_exact(&mut json)
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&json).map_err(|e| e.to_string())
}
