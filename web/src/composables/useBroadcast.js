// Watching the robot without a peer connection.
//
// The robot encodes once and publishes one copy of the stream to the relay,
// which fans it out. So the robot's cost is the same for one viewer as for
// fifty, and, just as importantly, joining here does not touch the robot's
// GStreamer pipeline, so arriving cannot interrupt anyone already watching.
// The price is latency: roughly a quarter second against WebRTC's tenth. That
// is fine for watching and wrong for driving, which is why whoever holds the
// wheel is given a peer connection instead.
//
// The relay caps a message at 65535 bytes and a keyframe can exceed that, so
// the robot cuts access units into chunks (robot/src/broadcast.rs). This
// reassembles them, decodes with WebCodecs, and paints onto a canvas.

import { ref, shallowRef, watch, onUnmounted } from 'vue'
import { useClasp } from './useClasp.js'
import { addresses } from '../protocol.js'

// Mirrors robot/src/broadcast.rs. Keep the two in step.
const VERSION = 1
const HEADER_LEN = 10
const FLAG_KEYFRAME = 0x01

// How far behind the newest frame an incomplete one is still worth holding.
// Chunks of one frame arrive together, so anything older than this lost a chunk
// and is never going to be finished.
const REASSEMBLY_DEPTH = 8

// Frames carry no presentation time, so timestamps are synthesised from the
// sequence number. Nothing here buffers for playback, so they only have to
// increase; the exact rate does not matter.
const FRAME_INTERVAL_US = 33_333

export function isBroadcastSupported() {
  return typeof window !== 'undefined' && typeof window.VideoDecoder === 'function'
}

/** Read one chunk's header. Null for anything malformed. */
function parseChunk(bytes) {
  if (!bytes || bytes.length < HEADER_LEN || bytes[0] !== VERSION) return null
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  const count = view.getUint16(8, true)
  const index = view.getUint16(6, true)
  if (count === 0 || index >= count) return null
  return {
    keyframe: (bytes[1] & FLAG_KEYFRAME) !== 0,
    seq: view.getUint32(2, true),
    index,
    count,
    payload: bytes.subarray(HEADER_LEN),
  }
}

/**
 * Derive the codec string from the stream's own SPS.
 *
 * Hard-coding one means guessing what the encoder produced, and a decoder
 * configured for the wrong profile fails outright rather than degrading. The
 * SPS is in-band ahead of every keyframe precisely so this is possible.
 */
function codecFromSps(accessUnit) {
  for (let i = 0; i + 4 < accessUnit.length; i++) {
    const startCode =
      accessUnit[i] === 0 && accessUnit[i + 1] === 0 && accessUnit[i + 2] === 1
        ? 3
        : accessUnit[i] === 0 &&
            accessUnit[i + 1] === 0 &&
            accessUnit[i + 2] === 0 &&
            accessUnit[i + 3] === 1
          ? 4
          : 0
    if (!startCode) continue
    const nal = i + startCode
    if ((accessUnit[nal] & 0x1f) !== 7) continue // not an SPS
    if (nal + 3 >= accessUnit.length) return null
    const hex = (n) => n.toString(16).padStart(2, '0')
    return `avc1.${hex(accessUnit[nal + 1])}${hex(accessUnit[nal + 2])}${hex(accessUnit[nal + 3])}`
  }
  return null
}

export function useBroadcast(robotId) {
  const { client, connected } = useClasp()

  // idle | waiting | live | unsupported | error
  const state = ref('idle')
  const error = ref(null)
  const framesDecoded = ref(0)
  const canvas = shallowRef(null)

  let unsub = null
  let decoder = null
  let context = null
  // Until the first keyframe there is nothing a decoder can start from, and
  // after a gap the frames in between are gone, so deltas would decode into
  // visible corruption. Both cases wait for the next keyframe.
  let needKeyframe = true
  let expectedSeq = null
  const pending = new Map()

  function attach(element) {
    canvas.value = element || null
    context = element ? element.getContext('2d') : null
  }

  function resetDecoder() {
    if (decoder) {
      try {
        if (decoder.state !== 'closed') decoder.close()
      } catch {
        // Closing an already-failed decoder is not worth surfacing.
      }
    }
    decoder = null
    needKeyframe = true
  }

  function paint(frame) {
    const el = canvas.value
    if (!el || !context) {
      frame.close()
      return
    }
    if (el.width !== frame.displayWidth || el.height !== frame.displayHeight) {
      el.width = frame.displayWidth
      el.height = frame.displayHeight
    }
    context.drawImage(frame, 0, 0)
    // VideoFrames hold real memory and are not garbage collected promptly.
    // Missing this stalls the decoder within a second or two.
    frame.close()
    framesDecoded.value++
    if (state.value !== 'live') state.value = 'live'
  }

  function ensureDecoder(accessUnit) {
    if (decoder) return true
    const codec = codecFromSps(accessUnit)
    if (!codec) return false
    try {
      decoder = new VideoDecoder({
        output: paint,
        error: (err) => {
          // Start over rather than showing a frozen or corrupt picture. The
          // next keyframe is at most a second or two away.
          error.value = err?.message || String(err)
          resetDecoder()
        },
      })
      // No `description`, which is what puts the decoder in Annex-B mode: the
      // form the robot's h264parse emits.
      decoder.configure({ codec, optimizeForLatency: true })
      return true
    } catch (err) {
      error.value = err?.message || String(err)
      state.value = 'error'
      decoder = null
      return false
    }
  }

  function onAccessUnit(seq, keyframe, accessUnit) {
    // A missing frame means the decoder's reference is gone, so wait it out.
    if (expectedSeq !== null && seq !== expectedSeq) needKeyframe = true
    expectedSeq = (seq + 1) >>> 0

    if (needKeyframe && !keyframe) return
    if (!ensureDecoder(accessUnit)) return
    if (keyframe) needKeyframe = false

    try {
      decoder.decode(
        new EncodedVideoChunk({
          type: keyframe ? 'key' : 'delta',
          timestamp: seq * FRAME_INTERVAL_US,
          data: accessUnit,
        }),
      )
    } catch (err) {
      error.value = err?.message || String(err)
      resetDecoder()
    }
  }

  function onChunk(value) {
    const bytes = value instanceof Uint8Array ? value : value ? new Uint8Array(value) : null
    const chunk = parseChunk(bytes)
    if (!chunk) return

    // The common case by a wide margin: everything but a keyframe fits in one
    // message, so skip the bookkeeping entirely.
    if (chunk.count === 1) {
      onAccessUnit(chunk.seq, chunk.keyframe, chunk.payload)
      return
    }

    let entry = pending.get(chunk.seq)
    if (!entry) {
      entry = { parts: new Array(chunk.count), have: 0, bytes: 0, keyframe: chunk.keyframe }
      pending.set(chunk.seq, entry)
    }
    if (entry.parts[chunk.index]) return // duplicate
    entry.parts[chunk.index] = chunk.payload
    entry.have++
    entry.bytes += chunk.payload.length

    if (entry.have === chunk.count) {
      pending.delete(chunk.seq)
      const whole = new Uint8Array(entry.bytes)
      let offset = 0
      for (const part of entry.parts) {
        whole.set(part, offset)
        offset += part.length
      }
      onAccessUnit(chunk.seq, entry.keyframe, whole)
    }

    // Anything this far behind lost a chunk and will never complete.
    for (const seq of pending.keys()) {
      if (((chunk.seq - seq) >>> 0) > REASSEMBLY_DEPTH) pending.delete(seq)
    }
  }

  function start() {
    stop()
    if (!isBroadcastSupported()) {
      state.value = 'unsupported'
      return
    }
    const c = client.value
    if (!c || !connected.value || !robotId.value) return
    state.value = 'waiting'
    error.value = null
    unsub = c.on(addresses(robotId.value).videoBroadcast, onChunk)
  }

  function stop() {
    if (unsub) {
      try {
        unsub()
      } catch {
        // Unsubscribe handles are best-effort.
      }
      unsub = null
    }
    resetDecoder()
    pending.clear()
    expectedSeq = null
    framesDecoded.value = 0
    if (state.value !== 'unsupported') state.value = 'idle'
  }

  watch([connected, robotId], start, { immediate: true })
  onUnmounted(stop)

  return { state, error, framesDecoded, attach, start, stop }
}
