//! Tiny, dependency-free PCM WAV reader/writer for the voice cache and the Windows-voice safety net.
//!
//! Reads 16-bit PCM or 32-bit float (any channel count, mixed down to mono); writes 16-bit mono PCM.

/// Encode mono f32 samples (-1..1) as a 16-bit PCM WAV file.
pub fn encode_wav_i16(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// Decode a WAV file to mono f32 + sample rate. `None` for anything that is not 16-bit PCM or 32-bit float.
pub fn decode_wav(bytes: &[u8]) -> Option<(Vec<f32>, u32)> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let (mut channels, mut rate, mut bits, mut format) = (0u16, 0u32, 0u16, 0u16);
    let mut pos = 12usize;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32_at(bytes, pos + 4)? as usize;
        let body = pos + 8;
        if id == b"fmt " {
            format = u16_at(bytes, body)?;
            channels = u16_at(bytes, body + 2)?;
            rate = u32_at(bytes, body + 4)?;
            bits = u16_at(bytes, body + 14)?;
        } else if id == b"data" {
            let end = (body + size).min(bytes.len());
            let data = &bytes[body..end];
            if channels == 0 || rate == 0 {
                return None;
            }
            let ch = channels as usize;
            let mono: Vec<f32> = match (format, bits) {
                (1, 16) => data
                    .chunks_exact(2 * ch)
                    .map(|f| {
                        let sum: f32 = f
                            .chunks_exact(2)
                            .map(|s| i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0)
                            .sum();
                        sum / ch as f32
                    })
                    .collect(),
                (3, 32) => data
                    .chunks_exact(4 * ch)
                    .map(|f| {
                        let sum: f32 = f
                            .chunks_exact(4)
                            .map(|s| f32::from_le_bytes([s[0], s[1], s[2], s[3]]))
                            .sum();
                        sum / ch as f32
                    })
                    .collect(),
                _ => return None,
            };
            return Some((mono, rate));
        }
        // chunks are word aligned
        pos = body.checked_add(size)?.checked_add(size & 1)?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_rate_length_and_shape() {
        let src: Vec<f32> = (0..2400).map(|i| ((i as f32) * 0.05).sin() * 0.5).collect();
        let wav = encode_wav_i16(&src, 24_000);
        assert_eq!(wav.len(), 44 + src.len() * 2);
        let (back, rate) = decode_wav(&wav).expect("decodes");
        assert_eq!(rate, 24_000);
        assert_eq!(back.len(), src.len());
        let worst = src.iter().zip(&back).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(worst < 1.0 / 16_000.0, "16-bit quantisation only: {worst}");
    }

    #[test]
    fn stereo_is_mixed_down_to_mono() {
        // L = +0.5, R = -0.5 -> 0
        let mut wav = encode_wav_i16(&[], 8_000);
        wav[22] = 2; // channels = 2
        let data: Vec<u8> = [16384i16, -16384, 16384, -16384].iter().flat_map(|v| v.to_le_bytes()).collect();
        wav.truncate(40);
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        let (mono, _) = decode_wav(&wav).expect("decodes");
        assert_eq!(mono.len(), 2);
        assert!(mono.iter().all(|s| s.abs() < 1e-4));
    }

    #[test]
    fn float_wav_and_extra_chunks_are_handled() {
        let samples = [0.25f32, -0.5, 0.75];
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&0u32.to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"LIST");
        wav.extend_from_slice(&3u32.to_le_bytes());
        wav.extend_from_slice(&[1, 2, 3, 0]); // 3 bytes + pad byte
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&3u16.to_le_bytes()); // float
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&16_000u32.to_le_bytes());
        wav.extend_from_slice(&(16_000u32 * 4).to_le_bytes());
        wav.extend_from_slice(&4u16.to_le_bytes());
        wav.extend_from_slice(&32u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&12u32.to_le_bytes());
        for s in samples {
            wav.extend_from_slice(&s.to_le_bytes());
        }
        let (out, rate) = decode_wav(&wav).expect("decodes");
        assert_eq!(rate, 16_000);
        assert_eq!(out, samples);
    }

    #[test]
    fn garbage_and_unsupported_formats_are_rejected_not_panicked() {
        assert!(decode_wav(&[]).is_none());
        assert!(decode_wav(b"RIFFxxxxWAVE").is_none());
        assert!(decode_wav(&[0u8; 64]).is_none());
        let mut wav = encode_wav_i16(&[0.1; 4], 8_000);
        wav[34] = 24; // 24-bit: unsupported
        assert!(decode_wav(&wav).is_none());
        // truncated data chunk must not panic
        let wav = encode_wav_i16(&[0.1; 100], 8_000);
        let _ = decode_wav(&wav[..60]);
    }
}
