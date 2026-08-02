//! Framing for the video broadcast that rides on CLASP.
//!
//! WebRTC serves the person driving, because teleoperation needs latency a
//! relay cannot give. Everyone else watches this instead: the Pi encodes once,
//! publishes one copy, and the relay fans it out. The robot's cost is then the
//! same for one spectator as for fifty, and, just as importantly, spectators
//! never touch the GStreamer pipeline, so nobody joining can interrupt anyone
//! already watching.
//!
//! The relay accepts at most 65535 bytes in a message, and an H264 keyframe at
//! 1280x480 can exceed that even though almost every other frame is far below
//! it. So access units are cut into chunks that carry enough header to be put
//! back together, and a receiver that misses any chunk of a frame throws the
//! whole frame away rather than handing a decoder something truncated.
//!
//! This module is deliberately free of GStreamer so the wire format can be
//! tested on any machine. `video.rs` supplies the access units; `web/src/
//! composables/useBroadcast.js` reassembles them.

/// Bytes of payload per chunk.
///
/// The relay's ceiling is 65535 for the whole encoded message, which includes
/// CLASP's own framing and the address. This leaves a wide margin rather than
/// creeping up on a limit whose overhead is not ours to predict.
pub const MAX_CHUNK_PAYLOAD: usize = 60_000;

/// Wire format version, so a console can refuse a stream it does not
/// understand instead of rendering noise.
pub const VERSION: u8 = 1;

/// Header bytes ahead of each chunk's payload.
pub const HEADER_LEN: usize = 10;

const FLAG_KEYFRAME: u8 = 0b0000_0001;

/// One chunk's header, as parsed off the wire.
///
/// The robot only ever writes this format; the browser reads it
/// (`web/src/composables/useBroadcast.js`). So the reader here exists to hold
/// the writer honest in tests rather than to be called in anger, and is built
/// only for them.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkHeader {
    /// Which access unit this belongs to. Wraps, and receivers must treat it
    /// as a rolling value rather than an ordering they can trust forever.
    pub seq: u32,
    pub index: u16,
    pub count: u16,
    /// Whether the frame this chunk belongs to is an IDR. A receiver that has
    /// not decoded anything yet waits for one before it starts.
    pub keyframe: bool,
}

/// Cut one access unit into chunks that fit the relay's message ceiling.
///
/// An empty access unit yields no chunks: there is nothing to send, and a
/// zero-length frame would only give the receiver something to discard.
pub fn fragment(seq: u32, keyframe: bool, access_unit: &[u8]) -> Vec<Vec<u8>> {
    if access_unit.is_empty() {
        return Vec::new();
    }

    let count = access_unit.len().div_ceil(MAX_CHUNK_PAYLOAD);
    // A frame needing more chunks than the counter can name cannot be
    // reassembled, so it is better dropped here than sent unreconstructible.
    if count > u16::MAX as usize {
        return Vec::new();
    }

    access_unit
        .chunks(MAX_CHUNK_PAYLOAD)
        .enumerate()
        .map(|(index, payload)| {
            let mut chunk = Vec::with_capacity(HEADER_LEN + payload.len());
            chunk.push(VERSION);
            chunk.push(if keyframe { FLAG_KEYFRAME } else { 0 });
            chunk.extend_from_slice(&seq.to_le_bytes());
            chunk.extend_from_slice(&(index as u16).to_le_bytes());
            chunk.extend_from_slice(&(count as u16).to_le_bytes());
            chunk.extend_from_slice(payload);
            chunk
        })
        .collect()
}

/// Read a chunk's header and borrow its payload. `None` for anything too
/// short, of an unknown version, or self-contradictory.
///
/// Test-only, for the reason given on [`ChunkHeader`].
#[cfg(test)]
pub fn parse(chunk: &[u8]) -> Option<(ChunkHeader, &[u8])> {
    if chunk.len() < HEADER_LEN || chunk[0] != VERSION {
        return None;
    }
    let header = ChunkHeader {
        seq: u32::from_le_bytes([chunk[2], chunk[3], chunk[4], chunk[5]]),
        index: u16::from_le_bytes([chunk[6], chunk[7]]),
        count: u16::from_le_bytes([chunk[8], chunk[9]]),
        keyframe: chunk[1] & FLAG_KEYFRAME != 0,
    };
    if header.count == 0 || header.index >= header.count {
        return None;
    }
    Some((header, &chunk[HEADER_LEN..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reassemble(chunks: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        for chunk in chunks {
            let (_, payload) = parse(chunk).expect("chunk should parse");
            out.extend_from_slice(payload);
        }
        out
    }

    #[test]
    fn a_small_frame_is_one_chunk_and_survives_the_round_trip() {
        let frame = vec![7u8; 1024];
        let chunks = fragment(42, false, &frame);
        assert_eq!(chunks.len(), 1);

        let (header, payload) = parse(&chunks[0]).unwrap();
        assert_eq!(header.seq, 42);
        assert_eq!(header.index, 0);
        assert_eq!(header.count, 1);
        assert!(!header.keyframe);
        assert_eq!(payload, &frame[..]);
    }

    #[test]
    fn a_keyframe_too_big_for_one_message_is_split_and_rejoins_identically() {
        // The case the whole module exists for: an IDR past the relay's limit.
        let frame: Vec<u8> = (0..150_000u32).map(|i| (i % 251) as u8).collect();
        let chunks = fragment(7, true, &frame);

        assert_eq!(chunks.len(), 3);
        for chunk in &chunks {
            assert!(
                chunk.len() <= HEADER_LEN + MAX_CHUNK_PAYLOAD,
                "chunk exceeds the relay's message ceiling"
            );
            assert!(parse(chunk).unwrap().0.keyframe);
        }

        let (first, _) = parse(&chunks[0]).unwrap();
        let (last, _) = parse(&chunks[2]).unwrap();
        assert_eq!((first.index, first.count), (0, 3));
        assert_eq!((last.index, last.count), (2, 3));
        assert_eq!(reassemble(&chunks), frame);
    }

    #[test]
    fn a_frame_landing_exactly_on_the_chunk_size_is_not_padded_with_an_empty_one() {
        let frame = vec![1u8; MAX_CHUNK_PAYLOAD];
        let chunks = fragment(1, false, &frame);
        assert_eq!(chunks.len(), 1);
        assert_eq!(reassemble(&chunks), frame);
    }

    #[test]
    fn an_empty_access_unit_produces_nothing_to_send() {
        assert!(fragment(1, true, &[]).is_empty());
    }

    #[test]
    fn garbage_is_rejected_rather_than_decoded() {
        assert!(parse(&[]).is_none());
        assert!(parse(&[VERSION; HEADER_LEN - 1]).is_none());

        // An unknown version. A console must not try to render a format that
        // was changed out from under it.
        let mut wrong_version = fragment(1, false, &[9u8; 32])[0].clone();
        wrong_version[0] = VERSION + 1;
        assert!(parse(&wrong_version).is_none());

        // index >= count could only come from a corrupt or hostile sender, and
        // would index past the end of a reassembly buffer.
        let mut impossible = fragment(1, false, &[9u8; 32])[0].clone();
        impossible[6] = 5;
        assert!(parse(&impossible).is_none());

        // A count of zero names a frame with no chunks in it.
        let mut zero_count = fragment(1, false, &[9u8; 32])[0].clone();
        zero_count[8] = 0;
        zero_count[9] = 0;
        assert!(parse(&zero_count).is_none());
    }

    #[test]
    fn the_sequence_number_survives_the_full_u32_range() {
        // It wraps in normal operation, so the top of the range is not exotic.
        for seq in [0, 1, u32::MAX / 2, u32::MAX - 1, u32::MAX] {
            let chunks = fragment(seq, false, &[3u8; 16]);
            assert_eq!(parse(&chunks[0]).unwrap().0.seq, seq);
        }
    }
}
