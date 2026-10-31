//! Waveforms for the seek bar (#16): a track's loudness along its length,
//! decoded when first asked for and cached by audio hash.

use std::path::Path;

use crate::hash;

/// Points in a waveform, about one per pixel of a wide seek bar.
pub const POINTS: usize = 1000;

/// Samples averaged per block before blocks are grouped into points: small
/// enough that a short track still has about a block per point, large
/// enough that an hour-long file's blocks stay a few megabytes.
const BLOCK: usize = 512;

/// The waveform of the file at `path`: RMS loudness per point, scaled so the
/// loudest point is 255. With `hash`, also its `pcm-b3:` audio hash, made
/// from the same decode.
pub fn compute(path: &Path, hash: bool) -> Result<(Vec<u8>, Option<String>), String> {
    let mut pcm = hash.then(hash::PcmHash::default);
    // Mean square of each block of samples.
    let mut blocks = Vec::new();
    let (mut sum, mut n) = (0f64, 0usize);
    hash::decode(path, |samples, _, _| {
        if let Some(pcm) = &mut pcm {
            pcm.update(samples);
        }
        for &s in samples {
            sum += f64::from(s * s);
            n += 1;
            if n == BLOCK {
                blocks.push((sum / BLOCK as f64) as f32);
                (sum, n) = (0.0, 0);
            }
        }
    })?;
    if n > 0 {
        blocks.push((sum / n as f64) as f32);
    }
    if blocks.is_empty() {
        return Err("no audio".into());
    }
    Ok((levels(&blocks), pcm.map(|p| p.finish())))
}

/// Groups blocks' mean squares into [`POINTS`] RMS levels, scaled to 0–255.
fn levels(blocks: &[f32]) -> Vec<u8> {
    let rms: Vec<f32> = (0..POINTS)
        .map(|i| {
            let start = i * blocks.len() / POINTS;
            let end = ((i + 1) * blocks.len() / POINTS).max(start + 1);
            let part = &blocks[start..end];
            (part.iter().sum::<f32>() / part.len() as f32).sqrt()
        })
        .collect();
    let loudest = rms.iter().copied().fold(0.0, f32::max);
    rms.iter()
        .map(|&r| match loudest > 0.0 {
            true => (r / loudest * 255.0).round() as u8,
            false => 0,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_scale_to_the_loudest_point() {
        // A quiet first half, a loud second half, fewer blocks than points.
        let blocks: Vec<f32> = (0..600)
            .map(|i| if i < 300 { 0.01 } else { 0.25 })
            .collect();
        let wave = levels(&blocks);
        assert_eq!(wave.len(), POINTS);
        assert_eq!(wave[0], 51); // sqrt(0.01) / sqrt(0.25) * 255
        assert_eq!(wave[POINTS - 1], 255);
        assert_eq!(levels(&[0.0; 5000]), vec![0; POINTS]);
    }

    #[test]
    fn the_hash_made_in_passing_is_the_audio_hash() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/Artist B/Second Album/02 Ending.ogg");
        let (levels, hash) = compute(&path, true).unwrap();
        assert_eq!(levels.len(), POINTS);
        assert_eq!(hash.unwrap(), hash::audio_hash(&path).unwrap());
    }
}
