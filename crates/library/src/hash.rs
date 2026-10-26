//! Audio hashes identify the same recording file across devices even when
//! its tags differ, because only the audio is hashed.

use std::{fs::File, path::Path};

use symphonia::core::{
    codecs::CodecParameters, errors::Error as SymError, formats::TrackType, io::MediaSourceStream,
};

use crate::tags;

/// `flac-md5:<hex>` from the FLAC stream's own MD5 when present (free to
/// read), else `pcm-b3:<hex>`, a BLAKE3 hash of the decoded samples.
pub fn audio_hash(path: &Path) -> Result<String, String> {
    if let Some(md5) = tags::flac_md5(path) {
        return Ok(format!("flac-md5:{md5:032x}"));
    }
    let stream = MediaSourceStream::new(
        Box::new(File::open(path).map_err(|e| e.to_string())?),
        Default::default(),
    );
    let mut reader = symphonia::default::get_probe()
        .probe(
            &Default::default(),
            stream,
            Default::default(),
            Default::default(),
        )
        .map_err(|e| e.to_string())?;
    let track = reader
        .default_track(TrackType::Audio)
        .ok_or("no audio track")?;
    let Some(CodecParameters::Audio(params)) = &track.codec_params else {
        return Err("no audio parameters".into());
    };
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &Default::default())
        .map_err(|e| e.to_string())?;
    let mut hasher = blake3::Hasher::new();
    let (mut samples, mut bytes) = (Vec::<f32>::new(), Vec::<u8>::new());
    loop {
        let packet = match reader.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.to_string()),
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(buffer) => buffer.copy_to_vec_interleaved(&mut samples),
            Err(SymError::DecodeError(_)) => continue,
            Err(e) => return Err(e.to_string()),
        }
        bytes.clear();
        bytes.extend(samples.iter().flat_map(|s| s.to_le_bytes()));
        hasher.update(&bytes);
    }
    Ok(format!("pcm-b3:{}", &hasher.finalize().to_hex()[..32]))
}
