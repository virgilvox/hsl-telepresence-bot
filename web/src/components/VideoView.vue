<script setup>
// The camera feed, and the only thing on screen that matters while driving.
// Its own chrome floats on the picture rather than sitting in a strip beneath
// it, and in fullscreen the driving controls come along as a HUD through the
// `hud` slot, so going fullscreen never means giving up the e-stop.
import { ref, watch, onUnmounted } from 'vue'
import { useFullscreen } from '../composables/useFullscreen.js'

const props = defineProps({
  stream: { type: Object, default: null },
  state: { type: String, default: 'idle' },
})

// The stereoscopic camera sends one wide frame with left and right side by side.
// The operator can view the whole frame or crop to a single eye.
const eye = ref('both') // both | left | right
const video = ref(null)
const root = ref(null)

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
  if (video.value) video.value.srcObject = null
})

const stateLabel = {
  idle: 'No session',
  waiting: 'Waiting for robot',
  connecting: 'Negotiating',
  live: 'Live',
  failed: 'Connection failed',
}
</script>

<template>
  <section
    ref="root"
    class="video panel"
    :class="[`eye-${eye}`, { fs: isFullscreen }]"
  >
    <video ref="video" autoplay playsinline muted />

    <div v-if="state !== 'live'" class="waiting">
      <svg class="icon big" viewBox="0 0 24 24" aria-hidden="true">
        <path d="M4 7h11v10H4z" />
        <path d="M15 10l5-3v10l-5-3" />
      </svg>
      <span>{{ stateLabel[state] || state }}</span>
    </div>

    <!-- Driving controls, only while fullscreen: outside it they live in the
         sidebar, where there is room for them. -->
    <div v-if="isFullscreen" class="hud">
      <slot name="hud" />
    </div>

    <div class="chrome">
      <span class="badge">
        <span class="dot" :class="{ live: state === 'live' }" />
        {{ stateLabel[state] || state }}
      </span>

      <div class="right">
        <div class="eyes" role="group" aria-label="Camera view">
          <button :class="{ active: eye === 'left' }" @click="eye = 'left'">Left</button>
          <button :class="{ active: eye === 'both' }" @click="eye = 'both'">Both</button>
          <button :class="{ active: eye === 'right' }" @click="eye = 'right'">Right</button>
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
  background: #0b0c0e;
  min-height: 0;
}
.video.fs {
  border: none;
  border-radius: 0;
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
.waiting {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 0.7rem;
  color: #6e727a;
  background: #0b0c0e;
  font-size: 0.9rem;
}
.big {
  font-size: 2.4rem;
  stroke-width: 1.4;
}

/* Floating chrome over the picture. */
.chrome {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.75rem;
  padding: 0.7rem;
  background: linear-gradient(to top, rgba(0, 0, 0, 0.55), transparent);
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
.badge {
  display: inline-flex;
  align-items: center;
  gap: 0.45rem;
  font-size: 0.75rem;
  letter-spacing: 0.02em;
  color: var(--hud-text);
  background: var(--hud);
  border: 1px solid var(--hud-border);
  border-radius: 999px;
  padding: 0.22rem 0.6rem 0.22rem 0.5rem;
  backdrop-filter: blur(6px);
}
.eyes {
  display: inline-flex;
  gap: 2px;
  background: var(--hud);
  border: 1px solid var(--hud-border);
  padding: 2px;
  border-radius: var(--radius-sm);
  backdrop-filter: blur(6px);
}
.eyes button {
  border: none;
  background: transparent;
  color: var(--hud-dim);
  padding: 0.22rem 0.55rem;
  border-radius: 5px;
  font-size: 0.75rem;
}
.eyes button:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.1);
  color: var(--hud-text);
}
.eyes button.active {
  background: rgba(255, 255, 255, 0.14);
  color: var(--hud-text);
}
.fsbtn {
  display: grid;
  place-items: center;
  width: 30px;
  height: 30px;
  padding: 0;
  font-size: 0.95rem;
  color: var(--hud-dim);
  background: var(--hud);
  border-color: var(--hud-border);
  backdrop-filter: blur(6px);
}
.fsbtn:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.14);
  color: var(--hud-text);
  border-color: var(--hud-border);
}

/* The fullscreen HUD: corner-anchored, never covering the middle of the
   picture, and always leaving the stop reachable. */
.hud {
  position: absolute;
  inset: 0;
  padding: 1rem;
  /* Clear the chrome bar along the bottom, so the telemetry and the pad never
     sit on top of the live badge and the eye selector. */
  padding-bottom: 3.6rem;
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
  background: var(--hud);
  border: 1px solid var(--hud-border);
  border-radius: var(--radius);
  padding: 0.7rem 0.8rem;
  color: var(--hud-text);
  backdrop-filter: blur(10px);
}
</style>
