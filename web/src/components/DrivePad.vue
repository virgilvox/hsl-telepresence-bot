<script setup>
// Presentational. All input handling and the send loop live in useDrive so the
// pad can be drawn twice (sidebar and fullscreen HUD) without two of them
// fighting over the keyboard.
import { computed } from 'vue'

const props = defineProps({
  drive: { type: Object, required: true },
  disabled: { type: Boolean, default: false },
  compact: { type: Boolean, default: false },
})

const knob = computed(() => props.drive.knob)
const lit = (code) => props.drive.held.has(code)
</script>

<template>
  <section class="drive" :class="{ compact, disabled }">
    <p v-if="!compact" class="panel-title">Drive</p>

    <div
      class="pad"
      @pointerdown="drive.onPointerDown"
      @pointermove="drive.onPointerMove"
      @pointerup="drive.onPointerUp"
      @pointercancel="drive.onPointerUp"
    >
      <div class="ring" />
      <div class="crosshair v" />
      <div class="crosshair h" />
      <div
        class="knob"
        :class="{ active: drive.dragging || drive.held.size > 0 }"
        :style="{ left: `${(knob.x + 1) * 50}%`, top: `${(1 - knob.y) * 50}%` }"
      />
    </div>

    <div class="keys" aria-hidden="true">
      <span class="key" :class="{ lit: lit('KeyA') || lit('ArrowLeft') }">A</span>
      <span class="key" :class="{ lit: lit('KeyS') || lit('ArrowDown') }">S</span>
      <span class="key" :class="{ lit: lit('KeyW') || lit('ArrowUp') }">W</span>
      <span class="key" :class="{ lit: lit('KeyD') || lit('ArrowRight') }">D</span>
    </div>

    <div class="readout mono">
      <span><i>thr</i>{{ knob.y.toFixed(2).padStart(5, ' ') }}</span>
      <span><i>str</i>{{ knob.x.toFixed(2).padStart(5, ' ') }}</span>
    </div>
  </section>
</template>

<style scoped>
.drive {
  padding: 0.8rem;
}
.drive.compact {
  padding: 0;
}
.pad {
  position: relative;
  aspect-ratio: 1;
  width: 100%;
  max-width: 200px;
  margin: 0 auto;
  background: var(--surface-2);
  border: 1px solid var(--border);
  border-radius: 12px;
  touch-action: none;
  overflow: hidden;
  cursor: grab;
}
.compact .pad {
  max-width: 150px;
  background: rgba(0, 0, 0, 0.3);
  border-color: var(--hud-border);
}
.disabled .pad {
  opacity: 0.4;
  cursor: not-allowed;
}
.pad:active {
  cursor: grabbing;
}
.ring {
  position: absolute;
  inset: 16%;
  border: 1px solid var(--border);
  border-radius: 50%;
  opacity: 0.7;
}
.compact .ring,
.compact .crosshair {
  border-color: var(--hud-border);
  background: var(--hud-border);
}
.compact .ring {
  background: none;
}
.crosshair {
  position: absolute;
  background: var(--border);
}
.crosshair.v {
  left: 50%;
  top: 6%;
  bottom: 6%;
  width: 1px;
  transform: translateX(-0.5px);
}
.crosshair.h {
  top: 50%;
  left: 6%;
  right: 6%;
  height: 1px;
  transform: translateY(-0.5px);
}
.knob {
  position: absolute;
  width: 19%;
  height: 19%;
  border-radius: 50%;
  background: var(--accent);
  transform: translate(-50%, -50%);
  transition: box-shadow 120ms ease;
}
.knob.active {
  box-shadow: 0 0 0 7px var(--accent-soft);
}
.compact .knob.active {
  box-shadow: 0 0 0 6px color-mix(in srgb, var(--accent) 30%, transparent);
}
.keys {
  display: flex;
  justify-content: center;
  gap: 3px;
  margin-top: 0.6rem;
}
.key {
  font-family: var(--mono);
  font-size: 0.65rem;
  line-height: 1;
  padding: 0.28rem 0.4rem;
  min-width: 1.5rem;
  text-align: center;
  border: 1px solid var(--border);
  border-radius: 4px;
  color: var(--text-faint);
  transition: color 90ms ease, border-color 90ms ease, background 90ms ease;
}
.compact .key {
  border-color: var(--hud-border);
  color: var(--hud-dim);
}
.key.lit {
  color: var(--accent);
  border-color: var(--accent);
  background: var(--accent-soft);
}
.compact .key.lit {
  background: transparent;
}
.readout {
  display: flex;
  justify-content: center;
  gap: 1rem;
  margin-top: 0.5rem;
  color: var(--text-dim);
  font-size: 0.8rem;
  white-space: pre;
}
.compact .readout {
  color: var(--hud-dim);
  font-size: 0.72rem;
}
.readout i {
  font-style: normal;
  color: var(--text-faint);
  margin-right: 0.4rem;
}
.compact .readout i {
  color: var(--hud-dim);
  opacity: 0.7;
}
</style>
