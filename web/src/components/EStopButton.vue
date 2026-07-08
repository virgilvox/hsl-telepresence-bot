<script setup>
import { computed } from 'vue'

const props = defineProps({
  control: { type: Object, required: true },
  engaged: { type: Boolean, default: false },
  disabled: { type: Boolean, default: false },
})

const label = computed(() => (props.engaged ? 'Release stop' : 'Emergency stop'))

function toggle() {
  props.control.setEstop(!props.engaged)
}
</script>

<template>
  <button class="estop" :class="{ engaged }" :disabled="disabled" @click="toggle">
    <svg class="icon" viewBox="0 0 24 24" aria-hidden="true">
      <circle cx="12" cy="12" r="9" />
      <line x1="8" y1="8" x2="16" y2="16" />
      <line x1="16" y1="8" x2="8" y2="16" />
    </svg>
    <span>{{ label }}</span>
  </button>
</template>

<style scoped>
.estop {
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 0.6rem;
  font-size: 1.05rem;
  font-weight: 600;
  padding: 0.9rem;
  color: var(--stop);
  border-color: var(--stop);
  background: var(--stop-soft);
}
.estop .icon {
  font-size: 1.3rem;
  stroke-width: 2.4;
}
.estop.engaged {
  color: #fff;
  background: var(--stop);
  border-color: var(--stop);
}
.estop:hover:not(:disabled) {
  background: color-mix(in srgb, var(--stop) 22%, var(--surface));
}
.estop.engaged:hover:not(:disabled) {
  background: color-mix(in srgb, var(--stop) 88%, black);
}
</style>
