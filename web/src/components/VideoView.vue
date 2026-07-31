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
  idle: 'no session',
  waiting: 'waiting for robot',
  connecting: 'negotiating',
  live: 'live',
  failed: 'connection failed',
}
</script>

<template>
  <section ref="root" class="video" :class="[`eye-${eye}`, { fs: isFullscreen }]">
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
      <span class="tag">
        <span class="lamp" :class="{ live: state === 'live' }" />
        {{ stateLabel[state] || state }}
      </span>

      <div class="right">
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
.waiting {
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
