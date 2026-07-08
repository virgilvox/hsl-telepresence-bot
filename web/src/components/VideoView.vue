<script setup>
import { ref, watch, onUnmounted } from 'vue'

const props = defineProps({
  stream: { type: Object, default: null },
  state: { type: String, default: 'idle' },
})

// The stereoscopic camera sends one wide frame with left and right side by side.
// The operator can view the whole frame or crop to a single eye.
const eye = ref('both') // both | left | right
const video = ref(null)

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
  <section class="video panel" :class="`eye-${eye}`">
    <div class="stage">
      <video ref="video" autoplay playsinline muted />
      <div v-if="state !== 'live'" class="overlay">
        <svg class="icon big" viewBox="0 0 24 24" aria-hidden="true">
          <path d="M4 7h11v10H4z" />
          <path d="M15 10l5-3v10l-5-3" />
        </svg>
        <span>{{ stateLabel[state] || state }}</span>
      </div>
    </div>

    <div class="controls">
      <span class="live-tag" :class="{ on: state === 'live' }">
        <span class="dot" />{{ state === 'live' ? 'Live' : stateLabel[state] }}
      </span>
      <div class="eyes" role="group" aria-label="Camera view">
        <button :class="{ active: eye === 'left' }" @click="eye = 'left'">Left</button>
        <button :class="{ active: eye === 'both' }" @click="eye = 'both'">Both</button>
        <button :class="{ active: eye === 'right' }" @click="eye = 'right'">Right</button>
      </div>
    </div>
  </section>
</template>

<style scoped>
.video {
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.stage {
  position: relative;
  flex: 1;
  min-height: 260px;
  background: #0b0c0e;
  overflow: hidden;
  display: grid;
  place-items: center;
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
.overlay {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 0.6rem;
  color: #7b7f88;
  background: #0b0c0e;
}
.big {
  font-size: 2.6rem;
}
.controls {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0.6rem 0.85rem;
  border-top: 1px solid var(--border);
}
.live-tag {
  display: inline-flex;
  align-items: center;
  gap: 0.4rem;
  font-size: 0.78rem;
  color: var(--text-dim);
}
.live-tag .dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--text-dim);
}
.live-tag.on {
  color: var(--ok);
}
.live-tag.on .dot {
  background: var(--ok);
}
.eyes {
  display: inline-flex;
  gap: 2px;
  background: var(--surface-2);
  padding: 2px;
  border-radius: 8px;
}
.eyes button {
  border: none;
  background: transparent;
  padding: 0.3rem 0.7rem;
  border-radius: 6px;
  font-size: 0.8rem;
}
.eyes button.active {
  background: var(--surface);
  color: var(--accent);
  box-shadow: var(--shadow);
}
</style>
