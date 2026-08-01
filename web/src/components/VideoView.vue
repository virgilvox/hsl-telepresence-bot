<script setup>
// The camera feed, and the only thing on screen that matters while driving.
// Its own chrome floats on the picture rather than sitting in a strip beneath
// it, and in fullscreen the driving controls come along as a HUD through the
// `hud` slot, so going fullscreen never means giving up the e-stop.
import { computed, ref, watch, onMounted, onUnmounted } from 'vue'
import { useFullscreen } from '../composables/useFullscreen.js'

const props = defineProps({
  stream: { type: Object, default: null },
  state: { type: String, default: 'idle' },
  broadcastState: { type: String, default: 'idle' },
  broadcastError: { type: String, default: null },
  connected: { type: Boolean, default: false },
  connecting: { type: Boolean, default: false },
  attachBroadcast: { type: Function, default: null },
})

const emit = defineEmits(['connect', 'disconnect'])

// The stereoscopic camera sends one wide frame with left and right side by side.
// The operator can view the whole frame or crop to a single eye.
const eye = ref('both') // both | left | right
const video = ref(null)
const canvas = ref(null)
const root = ref(null)

// Which source is actually painting. A peer track wins whenever there is one,
// because it is the same picture a quarter second sooner.
const source = computed(() => {
  if (props.stream && props.state === 'live') return 'peer'
  if (props.broadcastState === 'live') return 'broadcast'
  return 'none'
})

const hasPicture = computed(() => source.value !== 'none')

// Video takes a few seconds on a good day: presence heartbeat, then an offer,
// then ICE. Past this it is worth saying so rather than leaving a spinner
// turning, because the usual causes (robot asleep, peer-to-peer blocked) are
// ones the operator can act on.
const SLOW_AFTER_MS = 12000
const waitingSince = ref(0)
const nowTick = ref(Date.now())
let ticker = null

const slowToConnect = computed(
  () =>
    props.connected &&
    !hasPicture.value &&
    waitingSince.value > 0 &&
    nowTick.value - waitingSince.value > SLOW_AFTER_MS,
)

// The single line under the spinner. Says what is being waited on, not what
// state machine it is in.
const waitLabel = computed(() => {
  if (!props.connected) return props.connecting ? 'Connecting to relay' : ''
  if (props.broadcastState === 'unsupported' && props.state !== 'live') {
    return 'This browser cannot decode the broadcast; waiting for a direct connection'
  }
  if (props.state === 'connecting') return 'Negotiating a direct connection'
  if (props.state === 'failed') return 'Direct connection failed; falling back to the broadcast'
  return 'Waiting for the robot\u2019s camera'
})

watch(
  [hasPicture, () => props.connected],
  ([picture, isConnected]) => {
    waitingSince.value = isConnected && !picture ? Date.now() : 0
  },
  { immediate: true },
)

onMounted(() => {
  ticker = setInterval(() => {
    nowTick.value = Date.now()
  }, 1000)
  if (props.attachBroadcast) props.attachBroadcast(canvas.value)
})

const { isFullscreen, supported: fullscreenSupported, toggle: toggleFullscreen } =
  useFullscreen(root)

defineExpose({ toggleFullscreen, isFullscreen })

watch(
  () => props.stream,
  (stream) => {
    if (video.value) {
      video.value.srcObject = stream || null
      if (stream) video.value.play?.().catch(() => {})
    }
  },
)

onUnmounted(() => {
  if (ticker) clearInterval(ticker)
  if (props.attachBroadcast) props.attachBroadcast(null)
  if (video.value) video.value.srcObject = null
})

// What the corner tag says. Which path the picture is arriving over is worth
// showing: they have visibly different latency, and "broadcast" explains why
// the controls feel ahead of the picture.
const sourceLabel = computed(() => {
  if (source.value === 'peer') return 'live \u00b7 direct'
  if (source.value === 'broadcast') return 'live \u00b7 broadcast'
  if (!props.connected) return 'not connected'
  return 'no picture'
})
</script>

<template>
  <section ref="root" class="video" :class="[`eye-${eye}`, { fs: isFullscreen }]">
    <!-- Two sources, one frame. The peer track is shown when it is up; the
         broadcast canvas carries everyone else. -->
    <video v-show="source === 'peer'" ref="video" autoplay playsinline muted />
    <canvas v-show="source === 'broadcast'" ref="canvas" class="broadcast" />

    <!-- Not connected: one obvious thing to do, in the middle of the thing it
         acts on, so a first-time visitor does not have to hunt the top bar. -->
    <div v-if="!connected" class="curtain">
      <svg class="icon big" viewBox="0 0 24 24" aria-hidden="true">
        <path d="M4 7h11v10H4z" />
        <path d="M15 10l5-3v10l-5-3" />
      </svg>
      <button class="connect" :disabled="connecting" @click="emit('connect')">
        {{ connecting ? 'Connecting\u2026' : 'Connect to robot' }}
      </button>
      <span class="hint">{{ waitLabel }}</span>
    </div>

    <!-- Connected but nothing to show yet. -->
    <div v-else-if="!hasPicture" class="curtain">
      <span class="spinner" aria-hidden="true" />
      <span class="hint">{{ waitLabel }}</span>
      <template v-if="slowToConnect">
        <span class="hint warn">
          This is taking longer than usual. The robot may be offline, or a direct
          connection may be blocked by this network.
        </span>
        <button class="retry" @click="emit('connect')">Retry</button>
      </template>
    </div>

    <!-- Driving controls, only while fullscreen: outside it they live in the
         sidebar, where there is room for them. -->
    <div v-if="isFullscreen" class="hud">
      <slot name="hud" />
    </div>

    <div class="chrome">
      <span class="tag">
        <span class="lamp" :class="{ live: hasPicture }" />
        {{ sourceLabel }}
      </span>

      <div class="right">
        <!-- Once connected the action inverts: the big button has done its job
             and steps aside, leaving a quiet way back out. -->
        <button
          v-if="connected"
          class="disconnect"
          title="Disconnect from the robot"
          @click="emit('disconnect')"
        >
          Disconnect
        </button>
        <div class="eyes" role="group" aria-label="Camera view">
          <button :class="{ active: eye === 'left' }" @click="eye = 'left'">L</button>
          <button :class="{ active: eye === 'both' }" @click="eye = 'both'">Both</button>
          <button :class="{ active: eye === 'right' }" @click="eye = 'right'">R</button>
        </div>
        <button
          v-if="fullscreenSupported"
          class="fsbtn"
          :title="isFullscreen ? 'Exit fullscreen (Esc)' : 'Fullscreen (F)'"
          :aria-label="isFullscreen ? 'Exit fullscreen' : 'Enter fullscreen'"
          @click="toggleFullscreen"
        >
          <svg v-if="!isFullscreen" class="icon" viewBox="0 0 24 24" aria-hidden="true">
            <path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5" />
          </svg>
          <svg v-else class="icon" viewBox="0 0 24 24" aria-hidden="true">
            <path d="M9 4v5H4M15 4v5h5M9 20v-5H4M15 20v-5h5" />
          </svg>
        </button>
      </div>
    </div>
  </section>
</template>

<style scoped>
.video {
  position: relative;
  overflow: hidden;
  background: #000;
  border: 1px solid var(--line);
  border-radius: var(--radius);
  box-shadow: var(--lift);
  min-height: 0;
}
.video.fs {
  border: none;
  border-radius: 0;
  box-shadow: none;
}
video {
  width: 100%;
  height: 100%;
  object-fit: contain;
  display: block;
}
/* Crop to a single eye by doubling the width and shifting to the chosen half. */
.eye-left video,
.eye-right video {
  width: 200%;
  object-fit: cover;
}
.eye-left video {
  transform: translateX(-25%);
}
.eye-right video {
  transform: translateX(25%);
}

/* Nothing to look at yet: say so on the same grid the drive pad uses, so an
   empty feed still reads as part of the instrument rather than a broken image. */
.curtain {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 0.7rem;
  color: var(--text-faint);
  background-color: var(--ink);
  background-image: linear-gradient(var(--grid) 1px, transparent 1px),
    linear-gradient(90deg, var(--grid) 1px, transparent 1px);
  background-size: var(--grid-size) var(--grid-size);
  font-size: var(--size-xs);
  letter-spacing: var(--track-wider);
  text-transform: uppercase;
  padding: 1.5rem;
  text-align: center;
}

/* The broadcast paints here. `contain` rather than `cover` so the wide
   side-by-side frame is never cropped by the panel it sits in. */
.broadcast {
  width: 100%;
  height: 100%;
  object-fit: contain;
  display: block;
}

/* The one thing to do on an empty screen, sized to say so. */
.connect {
  font-family: inherit;
  font-size: var(--size-base);
  letter-spacing: var(--track-wide);
  text-transform: uppercase;
  color: var(--ink);
  background: var(--accent);
  border: none;
  padding: 0.85rem 1.9rem;
  cursor: pointer;
}

.connect:hover:not(:disabled) {
  background: var(--accent-bright);
}

.connect:disabled {
  background: var(--line-strong);
  color: var(--text-dim);
  cursor: default;
}

.retry {
  font-family: inherit;
  font-size: var(--size-xs);
  letter-spacing: var(--track-wider);
  text-transform: uppercase;
  color: var(--text);
  background: transparent;
  border: 1px solid var(--line-strong);
  padding: 0.45rem 1.1rem;
  cursor: pointer;
}

.retry:hover {
  border-color: var(--accent);
  color: var(--accent-bright);
}

/* Once connected, leaving is a minor action and is styled like one. */
.disconnect {
  font-family: inherit;
  font-size: var(--size-xs);
  letter-spacing: var(--track-wider);
  text-transform: uppercase;
  color: var(--text-dim);
  background: transparent;
  border: 1px solid var(--line);
  padding: 0.3rem 0.7rem;
  cursor: pointer;
}

.disconnect:hover {
  color: var(--stop);
  border-color: var(--stop-dim);
}

.hint {
  max-width: 34ch;
  line-height: 1.6;
  text-transform: none;
  letter-spacing: var(--track-wide);
}

.hint.warn {
  color: var(--warn);
}

/* A square that sweeps rather than a spinning circle, to match the panel
   vocabulary the rest of the console uses. */
.spinner {
  width: 22px;
  height: 3px;
  background: var(--line-strong);
  position: relative;
  overflow: hidden;
}

.spinner::after {
  content: '';
  position: absolute;
  inset: 0;
  width: 40%;
  background: var(--accent);
  animation: sweep 1.1s ease-in-out infinite;
}

@keyframes sweep {
  0% {
    transform: translateX(-100%);
  }
  100% {
    transform: translateX(250%);
  }
}
.big {
  font-size: 2rem;
  stroke-width: 1.25;
}

.chrome {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.75rem;
  padding: 0.6rem;
  pointer-events: none;
}
.chrome > * {
  pointer-events: auto;
}
.right {
  display: flex;
  align-items: center;
  gap: 0.4rem;
}
.tag {
  display: inline-flex;
  align-items: center;
  gap: 0.45rem;
  font-size: var(--size-xs);
  letter-spacing: var(--track-wider);
  text-transform: uppercase;
  color: var(--text);
  background: rgba(26, 26, 26, 0.85);
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-sm);
  padding: 0.25rem 0.5rem;
}
.eyes {
  display: inline-flex;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-sm);
  overflow: hidden;
  background: rgba(26, 26, 26, 0.85);
}
.eyes button {
  border: none;
  border-right: 1px solid var(--line-strong);
  border-radius: 0;
  background: transparent;
  color: var(--text-dim);
  padding: 0.3rem 0.5rem;
  box-shadow: none;
}
.eyes button:last-child {
  border-right: none;
}
.eyes button:active:not(:disabled) {
  transform: none;
}
.eyes button.active {
  background: var(--accent-wash);
  color: var(--accent-bright);
}
.fsbtn {
  display: grid;
  place-items: center;
  width: 28px;
  height: 28px;
  padding: 0;
  font-size: 0.9rem;
  color: var(--text-dim);
  background: rgba(26, 26, 26, 0.85);
  border-color: var(--line-strong);
  box-shadow: none;
}

/* The fullscreen HUD: corner-anchored, never covering the middle of the
   picture, and always leaving the stop reachable. */
.hud {
  position: absolute;
  inset: 0;
  padding: 0.9rem;
  /* Clear the chrome bar along the bottom, so the telemetry and the pad never
     sit on top of the live tag and the eye selector. */
  padding-bottom: 3.4rem;
  display: grid;
  grid-template-columns: auto 1fr auto;
  grid-template-rows: auto 1fr auto;
  gap: 0.75rem;
  pointer-events: none;
}
.hud :deep(> *) {
  pointer-events: auto;
}
.hud :deep(.hud-tl) {
  grid-area: 1 / 1;
}
.hud :deep(.hud-tr) {
  grid-area: 1 / 3;
  justify-self: end;
}
.hud :deep(.hud-bl) {
  grid-area: 3 / 1;
  align-self: end;
}
.hud :deep(.hud-br) {
  grid-area: 3 / 3;
  justify-self: end;
  align-self: end;
}
.hud :deep(.hud-card) {
  background: rgba(26, 26, 26, 0.88);
  border: 1px solid var(--line-strong);
  border-radius: var(--radius);
  padding: 0.65rem 0.7rem;
}
</style>
