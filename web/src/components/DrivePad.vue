<script setup>
// Presentational. All input handling and the send loop live in useDrive so the
// pad can be drawn twice (sidebar and fullscreen HUD) without two of them
// fighting over the keyboard.
//
// The field borrows latch.design's node canvas: the same 20px grid on the same
// near-black, so the knob reads as something sitting on a work surface.
import { computed } from 'vue'

const props = defineProps({
  drive: { type: Object, required: true },
  disabled: { type: Boolean, default: false },
  compact: { type: Boolean, default: false },
})

const knob = computed(() => props.drive.knob)
const lit = (...codes) => codes.some((c) => props.drive.held.has(c))
const fmt = (v) => (v < 0 ? '' : '+') + v.toFixed(2)
</script>

<template>
  <section class="drive" :class="{ compact, disabled }">
    <div
      class="field"
      @pointerdown="drive.onPointerDown"
      @pointermove="drive.onPointerMove"
      @pointerup="drive.onPointerUp"
      @pointercancel="drive.onPointerUp"
    >
      <div class="axis v" />
      <div class="axis h" />
      <!-- Full deflection. The knob's travel stops here rather than running
           off the edge of the field. -->
      <div class="limit" />
      <div
        class="knob"
        :class="{ active: drive.dragging || drive.held.size > 0 }"
        :style="{ left: `${50 + knob.x * 41}%`, top: `${50 - knob.y * 41}%` }"
      />
    </div>

    <div class="keys" aria-hidden="true">
      <span class="key" :class="{ lit: lit('KeyW', 'ArrowUp') }">W</span>
      <span class="key" :class="{ lit: lit('KeyA', 'ArrowLeft') }">A</span>
      <span class="key" :class="{ lit: lit('KeyS', 'ArrowDown') }">S</span>
      <span class="key" :class="{ lit: lit('KeyD', 'ArrowRight') }">D</span>
    </div>

    <dl class="readout num">
      <div><dt>thr</dt><dd :class="{ hot: knob.y !== 0 }">{{ fmt(knob.y) }}</dd></div>
      <div><dt>str</dt><dd :class="{ hot: knob.x !== 0 }">{{ fmt(knob.x) }}</dd></div>
    </dl>
  </section>
</template>

<style scoped>
.drive {
  display: flex;
  flex-direction: column;
  gap: 0.55rem;
}
.field {
  position: relative;
  aspect-ratio: 1;
  width: 100%;
  max-width: 190px;
  margin: 0 auto;
  background-color: var(--ink);
  background-image: linear-gradient(var(--grid) 1px, transparent 1px),
    linear-gradient(90deg, var(--grid) 1px, transparent 1px);
  background-size: var(--grid-size) var(--grid-size);
  background-position: center center;
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  touch-action: none;
  overflow: hidden;
  cursor: crosshair;
}
.compact .field {
  max-width: 132px;
}
.disabled .field {
  opacity: 0.35;
  cursor: not-allowed;
}
.axis {
  position: absolute;
  background: var(--line-strong);
}
.axis.v {
  left: 50%;
  top: 0;
  bottom: 0;
  width: 1px;
}
.axis.h {
  top: 50%;
  left: 0;
  right: 0;
  height: 1px;
}
.limit {
  position: absolute;
  inset: 9%;
  border: 1px dashed var(--line-strong);
  opacity: 0.6;
}
.knob {
  position: absolute;
  width: 18%;
  height: 18%;
  background: var(--accent);
  border: 1px solid var(--accent-bright);
  border-radius: var(--radius-sm);
  transform: translate(-50%, -50%);
  box-shadow: var(--lift-sm);
  transition: box-shadow 100ms ease;
}
.knob.active {
  background: var(--accent-bright);
  box-shadow: 0 0 0 5px var(--accent-wash), var(--lift-sm);
}

.keys {
  display: flex;
  justify-content: center;
  gap: 3px;
}
.key {
  font-size: var(--size-xs);
  font-weight: 700;
  line-height: 1;
  padding: 0.3rem 0;
  width: 1.75rem;
  text-align: center;
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  color: var(--text-faint);
  transition: color 80ms ease, border-color 80ms ease, background 80ms ease;
}
.key.lit {
  color: var(--ink);
  background: var(--accent);
  border-color: var(--accent);
}

.readout {
  display: flex;
  justify-content: center;
  gap: 0.75rem;
  margin: 0;
  font-size: var(--size-xs);
}
.readout > div {
  display: flex;
  align-items: baseline;
  gap: 0.35rem;
}
dt {
  color: var(--text-faint);
  letter-spacing: var(--track-wide);
  text-transform: uppercase;
}
dd {
  margin: 0;
  color: var(--text-dim);
}
dd.hot {
  color: var(--accent-bright);
}
</style>
