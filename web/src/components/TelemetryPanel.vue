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

const signed = (v) => ((Number(v) || 0) < 0 ? '' : '+') + (Number(v) || 0).toFixed(2)
</script>

<template>
  <section class="tel" :class="{ compact }">
    <div v-if="!compact" class="rows">
      <div class="row">
        <span class="k">link</span>
        <span class="badge" :class="{ on: online }">{{ online ? 'online' : 'not seen' }}</span>
      </div>
      <div class="row">
        <span class="k">mode</span>
        <span class="badge">{{ mode }}</span>
      </div>
      <div class="row">
        <span class="k">battery</span>
        <span class="badge num">{{ battery }}</span>
      </div>
    </div>

    <div class="motors">
      <div class="motor">
        <span class="ml">L</span>
        <div class="track">
          <div class="centre" />
          <div class="bar" :class="{ rev: motors.left < 0 }" :style="fill(motors.left)" />
        </div>
        <span class="mv num">{{ signed(motors.left) }}</span>
      </div>
      <div class="motor">
        <span class="ml">R</span>
        <div class="track">
          <div class="centre" />
          <div class="bar" :class="{ rev: motors.right < 0 }" :style="fill(motors.right)" />
        </div>
        <span class="mv num">{{ signed(motors.right) }}</span>
      </div>
    </div>
  </section>
</template>

<style scoped>
.tel {
  display: flex;
  flex-direction: column;
  gap: 0.7rem;
}
.compact {
  min-width: 200px;
}
.rows {
  display: flex;
  flex-direction: column;
  gap: 0.3rem;
}
.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.5rem;
  font-size: var(--size-xs);
}
.k {
  color: var(--text-faint);
  letter-spacing: var(--track-wider);
  text-transform: uppercase;
}
.motors {
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
}
.motor {
  display: grid;
  grid-template-columns: 0.9rem 1fr 2.9rem;
  align-items: center;
  gap: 0.5rem;
  font-size: var(--size-xs);
}
.ml {
  color: var(--text-faint);
  font-weight: 700;
}
.track {
  position: relative;
  height: 10px;
  background: var(--ink);
  border: 1px solid var(--line);
  border-radius: 0;
  overflow: hidden;
}
.centre {
  position: absolute;
  left: 50%;
  top: 0;
  bottom: 0;
  width: 1px;
  background: var(--line-strong);
}
.bar {
  position: absolute;
  top: 0;
  bottom: 0;
  background: var(--accent);
  transition: width 80ms linear;
}
.bar.rev {
  background: var(--warn);
}
.mv {
  text-align: right;
  color: var(--text-dim);
}
</style>
