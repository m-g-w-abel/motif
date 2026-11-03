//! ReplayGain for files without gain tags (#84): EBU R128 integrated
//! loudness and sample peak, measured when a track is about to play (never
//! as a library scan) and cached by audio hash. Album gain is made from its
//! tracks' measurements once all of them are known.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::Ordering, mpsc},
    time::Duration,
};

use ebur128::{EbuR128, Mode};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{Shared, hash};

/// ReplayGain 2.0's reference loudness, in LUFS.
pub const REFERENCE: f64 = -18.0;

/// The longest a track about to play waits for its measurement; a long
/// hi-res file plays unmeasured rather than keep the listener waiting.
const WAIT_LIMIT: Duration = Duration::from_secs(5);

/// Integrated loudness in LUFS (`None` if silent) and sample peak of the
/// file at `path`. With `hash`, also its `pcm-b3:` audio hash from the same
/// decode.
pub fn measure(path: &Path, hash: bool) -> Result<(Option<f64>, f64, Option<String>), String> {
    let mut pcm = hash.then(hash::PcmHash::default);
    let mut meter: Option<EbuR128> = None;
    hash::decode(path, |samples, rate, channels| {
        if let Some(pcm) = &mut pcm {
            pcm.update(samples);
        }
        if meter.is_none() {
            meter = EbuR128::new(channels as u32, rate, Mode::I | Mode::SAMPLE_PEAK).ok();
        }
        if let Some(meter) = &mut meter {
            let _ = meter.add_frames_f32(samples);
        }
    })?;
    let meter = meter.ok_or("no audio")?;
    let lufs = meter.loudness_global().map_err(|e| e.to_string())?;
    let peak = (0..meter.channels())
        .filter_map(|c| meter.sample_peak(c).ok())
        .fold(0.0, f64::max);
    Ok((
        lufs.is_finite().then_some(lufs),
        peak,
        pcm.map(|p| p.finish()),
    ))
}

/// Measures `paths` that need it: new requests go first, in their order,
/// ahead of earlier ones still waiting. With `wait`, the first is measured
/// now and this returns once it is cached (or after [`WAIT_LIMIT`]).
pub(crate) fn request(shared: &Arc<Shared>, mut paths: Vec<PathBuf>, wait: bool) {
    {
        let unmeasurable = shared.unmeasurable.lock().unwrap();
        paths.retain(|p| !unmeasurable.contains(p));
    }
    if wait && !paths.is_empty() {
        let (path, (done, finished)) = (paths.remove(0), mpsc::channel());
        let shared = shared.clone();
        std::thread::spawn(move || {
            measure_one(&shared, &path);
            let _ = done.send(());
        });
        let _ = finished.recv_timeout(WAIT_LIMIT);
    }
    if paths.is_empty() {
        return;
    }
    let mut queue = shared.measuring.lock().unwrap();
    let (waiting, running) = &mut *queue;
    waiting.retain(|p| !paths.contains(p));
    for path in paths.into_iter().rev() {
        waiting.push_front(path);
    }
    if !*running {
        *running = true;
        let shared = shared.clone();
        std::thread::Builder::new()
            .name("loudness".into())
            .spawn(move || work(&shared))
            .ok();
    }
}

/// Measures queued files one at a time, then stops.
fn work(shared: &Arc<Shared>) {
    loop {
        let next = {
            let mut queue = shared.measuring.lock().unwrap();
            let next = queue.0.pop_front();
            queue.1 = next.is_some();
            next
        };
        let Some(path) = next else { return };
        measure_one(shared, &path);
    }
}

/// Measures and caches the library file at `path` unless it has gain tags,
/// can't be decoded or is already measured.
fn measure_one(shared: &Shared, path: &Path) {
    let run = || -> rusqlite::Result<()> {
        // A connection of its own: the scanner may hold the write lock for seconds.
        let conn = Connection::open(&shared.db_path)?;
        conn.busy_timeout(Duration::from_secs(30))?;
        let Some(text) = path.to_str() else {
            return Ok(());
        };
        let row: Option<(Option<f64>, Option<String>)> = conn
            .query_row(
                "SELECT rg_track_gain, audio_hash FROM tracks WHERE path = ?1",
                [text],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((None, known)) = row else {
            return Ok(());
        };
        if known.as_deref() == Some("") {
            return Ok(());
        }
        let hash = known.clone().or_else(|| hash::flac_hash(path));
        if let Some(hash) = &hash
            && conn
                .query_row("SELECT 1 FROM loudness WHERE hash = ?1", [hash], |_| Ok(()))
                .optional()?
                .is_some()
        {
            return Ok(());
        }
        let (lufs, peak, pcm_hash) = match measure(path, hash.is_none()) {
            Ok(measured) => measured,
            Err(err) => {
                tracing::debug!(%err, ?path, "can't measure loudness");
                // shortcut: remembered until restart, so a file fixed meanwhile is measured after one.
                shared.unmeasurable.lock().unwrap().insert(path.to_owned());
                return Ok(());
            }
        };
        let Some(hash) = hash.or(pcm_hash) else {
            return Ok(());
        };
        conn.execute(
            "INSERT OR REPLACE INTO loudness (hash, lufs, peak) VALUES (?1, ?2, ?3)",
            params![hash, lufs, peak],
        )?;
        if known.is_none() {
            // The scanner would hash it later; the measurement is found by hash now.
            conn.execute(
                "UPDATE tracks SET audio_hash = ?1 WHERE path = ?2 AND audio_hash IS NULL",
                params![hash, text],
            )?;
        }
        shared.measured.store(true, Ordering::Relaxed);
        Ok(())
    };
    if let Err(err) = run() {
        tracing::warn!(%err, "can't cache loudness");
    }
}

/// Album gain and peak from its tracks' (loudness in LUFS, `None` if
/// silent; peak; duration in ms). The album's loudness is the
/// duration-weighted energy mean of its tracks'.
// shortcut: RG 2.0 gates the album's blocks together; this mean of track loudness can differ by a fraction of a dB on albums with quiet tracks. Upgrade by caching each track's gating histogram if that matters.
pub fn album(tracks: &[(Option<f64>, f64, f64)]) -> Option<(f64, f64)> {
    let (mut energy, mut length, mut peak) = (0.0, 0.0, 0f64);
    for &(lufs, track_peak, ms) in tracks {
        energy += ms * lufs.map_or(0.0, |l| 10f64.powf(l / 10.0));
        length += ms;
        peak = peak.max(track_peak);
    }
    (energy > 0.0).then(|| (REFERENCE - 10.0 * (energy / length).log10(), peak))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn album_loudness_is_the_energy_mean_of_its_tracks() {
        // Two equal-length tracks at -18 and -28 LUFS.
        let (gain, peak) =
            album(&[(Some(-18.0), 0.9, 1000.0), (Some(-28.0), 0.5, 1000.0)]).unwrap();
        // 10·log10((1 + 0.1) / 2) = -2.6 dB below the reference.
        assert!((gain - 2.596).abs() < 0.001, "{gain}");
        assert_eq!(peak, 0.9);
        assert_eq!(album(&[(None, 0.0, 1000.0)]), None, "silence has no gain");
    }

    #[test]
    fn measures_loudness_and_the_hash_in_one_decode() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/Artist B/Second Album/02 Ending.ogg");
        let (lufs, peak, hash) = measure(&path, true).unwrap();
        assert!(peak > 0.0 && peak <= 1.0, "{peak}");
        assert!(lufs.is_none_or(|l| l < 0.0), "{lufs:?}");
        assert_eq!(hash.unwrap(), hash::audio_hash(&path).unwrap());
    }
}
