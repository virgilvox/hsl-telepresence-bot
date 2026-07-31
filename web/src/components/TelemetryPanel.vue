<script setup>
import { computed } from 'vue'

const props = defineProps({
  status: { type: Object, required: true },
  motors: { type: Object, required: true },
  lastSeen: { type: Number, default: 0 },
  online: { type: Boolean, default: false },
  compact: { type: Boolean, default: false },
})

const battery = computed(() => {
  const v = props.status.battery
  return typeof v === 'number' ? `${(v * 100).toFixed(0)}%` : '--'
})

const mode = computed(() => props.status.mode || '--')

// Wheel demand is signed, so the bars grow out from the centre: forward to the
// right, reverse to the left. A bar anchored at one end would make full reverse
// look like a stop.
function fill(value) {
  const v = Math.max(-1, Math.min(1, Number(value) || 0))
  const width = `${Math.abs(v) * 50}%`
  return v < 0 ? { right: '50%', width } : { left: '50%', width }
}
</script>

<template>
  <section class="telemetry" :class="{ compact }">
    <p v-if="!compact" class="panel-title">Telemetry</p>

    <div v-if="!compact" class="rows">
      <div class="row">
        <span class="k">Robot link</span>
        <span class="v" :class="online ? 'ok' : 'off'">{{ online ? 'Online' : 'Not seen' }}</span>
      </div>
      <div class="row">
        <span class="k">Mode</span>
        <span class="v mono">{{ mode }}</span>
      </div>
      <div class="row">
        <span class="k">Battery</span>
        <span class="v mono">{{ battery }}</span>
      </div>
    </div>

    <div class="motors">
      <div class="motor">
        <span class="ml">L</span>
        <div class="track">
          <div class="centre" />
          <div class="bar" :class="{ rev: motors.left < 0 }" :style="fill(motors.left)" />
        </div>
        <span class="mv mono">{{ (motors.left || 0).toFixed(2) }}</span>
      </div>
      <div class="motor">
        <span class="ml">R</span>
        <div class="track">
          <div class="centre" />
          <div class="bar" :class="{ rev: motors.right < 0 }" :style="fill(motors.right)" />
        </div>
        <span class="mv mono">{{ (motors.right || 0).toFixed(2) }}</span>
      </div>
    </div>
  </section>
</template>

<style scoped>
.telemetry {
  padding: 0.8rem;
}
.telemetry.compact {
  padding: 0;
  min-width: 190px;
}
.rows {
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
  margin-bottom: 0.85rem;
}
.row {
  display: flex;
  justify-content: space-between;
  font-size: 0.85rem;
}
.k {
  color: var(--text-dim);
}
.v.ok {
  color: var(--ok);
}
.v.off {
  color: var(--text-faint);
}
.motors {
  display: flex;
  flex-direction: column;
  gap: 0.45rem;
}
.motor {
  display: grid;
  grid-template-columns: 1rem 1fr 2.6rem;
  align-items: center;
  gap: 0.5rem;
  font-size: 0.75rem;
}
.ml {
  color: var(--text-faint);
  font-family: var(--mono);
}
.compact .ml,
.compact .mv {
  color: var(--hud-dim);
}
.track {
  position: relative;
  height: 8px;
  background: var(--surface-3);
  border-radius: 3px;
  overflow: hidden;
}
.compact .track {
  background: rgba(255, 255, 255, 0.12);
}
.centre {
  position: absolute;
  left: 50%;
  top: 1px;
  bottom: 1px;
  width: 1px;
  background: var(--border-strong);
  transform: translateX(-0.5px);
}
.compact .centre {
  background: rgba(255, 255, 255, 0.25);
}
.bar {
  position: absolute;
  top: 0;
  bottom: 0;
  background: var(--accent);
  transition: width 80ms linear;
}
.bar.rev {
  background: var(--text-dim);
}
.compact .bar.rev {
  background: var(--hud-dim);
}
.mv {
  text-align: right;
  color: var(--text-dim);
}
</style>
