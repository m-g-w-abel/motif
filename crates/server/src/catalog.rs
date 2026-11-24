//! A catalog exchange with a linked engine ([`net::Alpn::Catalog`], #241):
//! both ends say what they hold of the other's catalog ([`Seen`]), then send
//! the changes to their own after that a page at a time, and apply what
//! they hear (`library/src/catalog.rs`). Frames and the ending are sync's.

use library::{
    Library,
    catalog::{Page, Seen},
};
use net::Connection;

use crate::sync::{blocking, done, read, write};

/// Changes per page: a few hundred KB of JSON.
const PAGE: u32 = 1000;

/// Runs an exchange on `conn`, which this end dialled or accepted.
pub(crate) async fn session(
    library: &Library,
    conn: &Connection,
    dialled: bool,
) -> Result<(), String> {
    let peer = net::key(conn);
    let (mut send, mut recv) = if dialled {
        conn.open_bi().await
    } else {
        conn.accept_bi().await
    }
    .map_err(|e| e.to_string())?;
    let of = peer.clone();
    let mine = blocking(library, move |l| l.catalog_seen(&of)).await?;
    write(&mut send, &mine).await?;
    let mut theirs: Seen = read(&mut recv).await?;
    let give = async {
        loop {
            let seen = theirs.clone();
            let page = blocking(library, move |l| l.catalog_page(&seen, PAGE)).await?;
            write(&mut send, &page).await?;
            let Some(last) = page.changes.last() else {
                return Ok::<_, String>(());
            };
            theirs = Seen {
                epoch: Some(page.epoch.clone()),
                version: last.version,
            };
        }
    };
    let take = async {
        loop {
            let page: Page = read(&mut recv).await?;
            let end = page.changes.is_empty();
            // The last, empty page too: from a new library, it clears the old.
            let of = peer.clone();
            blocking(library, move |l| l.merge_catalog(&of, page)).await?;
            if end {
                return Ok(());
            }
        }
    };
    tokio::try_join!(give, take)?;
    done(conn, send, recv, dialled).await
}
