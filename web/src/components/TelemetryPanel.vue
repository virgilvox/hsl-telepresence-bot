<script setup>
import { computed } from 'vue'

const props = defineProps({
  status: { type: Object, required: true },
  motors: { type: Object, required: true },
  lastSeen: { type: Number, default: 0 },
  online: { type: Boolean, default: false },
})

const battery = computed(() => {
  const v = props.status.battery
  return typeof v === 'number' ? `${(v * 100).toFixed(0)}%` : '--'
})

const mode = computed(() => props.status.mode || '--')

function bar(value) {
  return `${Math.min(100, Math.abs(Number(value) || 0) * 100).toFixed(0)}%`
}
</script>

<template>
  <section class="panel telemetry">
    <p class="panel-title">Telemetry</p>

    <div class="rows">
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
        <span class="ml">Left</span>
        <div class="track">
          <div class="fill" :class="{ rev: motors.left < 0 }" :style="{ width: bar(motors.left) }" />
        </div>
        <span class="mv mono">{{ (motors.left || 0).toFixed(2) }}</span>
      </div>
      <div class="motor">
        <span class="ml">Right</span>
        <div class="track">
          <div class="fill" :class="{ rev: motors.right < 0 }" :style="{ width: bar(motors.right) }" />
        </div>
        <span class="mv mono">{{ (motors.right || 0).toFixed(2) }}</span>
      </div>
    </div>
  </section>
</template>

<style scoped>
.telemetry {
  padding: 0.85rem;
}
.rows {
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
  margin-bottom: 0.9rem;
}
.row {
  display: flex;
  justify-content: space-between;
  font-size: 0.88rem;
}
.k {
  color: var(--text-dim);
}
.v.ok {
  color: var(--ok);
}
.v.off {
  color: var(--text-dim);
}
.motors {
  display: flex;
  flex-direction: column;
  gap: 0.55rem;
}
.motor {
  display: grid;
  grid-template-columns: 3rem 1fr 3rem;
  align-items: center;
  gap: 0.5rem;
  font-size: 0.8rem;
}
.ml {
  color: var(--text-dim);
}
.track {
  height: 8px;
  background: var(--surface-2);
  border-radius: 4px;
  overflow: hidden;
}
.fill {
  height: 100%;
  background: var(--accent);
  border-radius: 4px;
}
.fill.rev {
  background: var(--text-dim);
}
.mv {
  text-align: right;
  color: var(--text-dim);
}
</style>
