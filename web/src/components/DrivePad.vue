<script setup>
import { ref, onMounted, onUnmounted, reactive } from 'vue'

const props = defineProps({
  control: { type: Object, required: true },
  disabled: { type: Boolean, default: false },
})

const SEND_HZ = 15

const pad = ref(null)
const knob = reactive({ x: 0, y: 0 }) // -1..1, y positive is up (forward)
const active = ref(false)
const keys = new Set()

let pointerId = null
let timer = null

function setFromPointer(event) {
  const el = pad.value
  if (!el) return
  const rect = el.getBoundingClientRect()
  const nx = ((event.clientX - rect.left) / rect.width) * 2 - 1
  const ny = ((event.clientY - rect.top) / rect.height) * 2 - 1
  knob.x = clamp(nx)
  knob.y = clamp(-ny)
}

function onDown(event) {
  if (props.disabled) return
  active.value = true
  pointerId = event.pointerId
  pad.value.setPointerCapture?.(event.pointerId)
  setFromPointer(event)
}

function onMove(event) {
  if (active.value && event.pointerId === pointerId) setFromPointer(event)
}

function onUp(event) {
  if (event.pointerId !== pointerId) return
  active.value = false
  pointerId = null
  recenter()
}

function recenter() {
  if (keys.size > 0) return // keyboard still driving
  knob.x = 0
  knob.y = 0
  props.control.stop()
}

function tick() {
  applyKeys()
  if (props.disabled) return
  if (active.value || keys.size > 0) {
    props.control.drive(knob.y, knob.x)
  }
}

function applyKeys() {
  if (keys.size === 0) return
  let y = 0
  let x = 0
  if (keys.has('w') || keys.has('ArrowUp')) y += 1
  if (keys.has('s') || keys.has('ArrowDown')) y -= 1
  if (keys.has('a') || keys.has('ArrowLeft')) x -= 1
  if (keys.has('d') || keys.has('ArrowRight')) x += 1
  knob.x = clamp(x)
  knob.y = clamp(y)
}

function onKeyDown(event) {
  if (props.disabled) return
  if (isDriveKey(event.key)) {
    keys.add(event.key)
    event.preventDefault()
  }
}

function onKeyUp(event) {
  if (isDriveKey(event.key)) {
    keys.delete(event.key)
    if (keys.size === 0 && !active.value) recenter()
  }
}

function isDriveKey(key) {
  return ['w', 'a', 's', 'd', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'].includes(key)
}

function clamp(v) {
  return Math.max(-1, Math.min(1, v))
}

onMounted(() => {
  timer = setInterval(tick, 1000 / SEND_HZ)
  window.addEventListener('keydown', onKeyDown)
  window.addEventListener('keyup', onKeyUp)
})

onUnmounted(() => {
  clearInterval(timer)
  window.removeEventListener('keydown', onKeyDown)
  window.removeEventListener('keyup', onKeyUp)
})
</script>

<template>
  <section class="panel drive">
    <p class="panel-title">Drive</p>
    <div
      ref="pad"
      class="pad"
      :class="{ disabled }"
      @pointerdown="onDown"
      @pointermove="onMove"
      @pointerup="onUp"
      @pointercancel="onUp"
    >
      <div class="crosshair v" />
      <div class="crosshair h" />
      <div
        class="knob"
        :class="{ active }"
        :style="{
          left: `${(knob.x + 1) * 50}%`,
          top: `${(1 - knob.y) * 50}%`,
        }"
      />
    </div>
    <div class="readout mono">
      <span>thr {{ knob.y.toFixed(2) }}</span>
      <span>str {{ knob.x.toFixed(2) }}</span>
    </div>
    <p class="hint">Drag the pad or use WASD / arrow keys. Release to coast.</p>
  </section>
</template>

<style scoped>
.drive {
  padding: 0.85rem;
}
.pad {
  position: relative;
  aspect-ratio: 1;
  width: 100%;
  max-width: 240px;
  margin: 0 auto;
  background: var(--surface-2);
  border: 1px solid var(--border);
  border-radius: 14px;
  touch-action: none;
  overflow: hidden;
}
.pad.disabled {
  opacity: 0.5;
}
.crosshair {
  position: absolute;
  background: var(--border);
}
.crosshair.v {
  left: 50%;
  top: 8%;
  bottom: 8%;
  width: 1px;
  transform: translateX(-0.5px);
}
.crosshair.h {
  top: 50%;
  left: 8%;
  right: 8%;
  height: 1px;
  transform: translateY(-0.5px);
}
.knob {
  position: absolute;
  width: 20%;
  height: 20%;
  border-radius: 50%;
  background: var(--accent);
  transform: translate(-50%, -50%);
  transition: box-shadow 100ms ease;
}
.knob.active {
  box-shadow: 0 0 0 6px var(--accent-soft);
}
.readout {
  display: flex;
  justify-content: center;
  gap: 1.25rem;
  margin-top: 0.7rem;
  color: var(--text-dim);
  font-size: 0.85rem;
}
.hint {
  margin: 0.5rem 0 0;
  text-align: center;
  color: var(--text-dim);
  font-size: 0.75rem;
}
</style>
