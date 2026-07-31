<script setup>
// Deliberately not arbitrated: anyone watching can stop the robot, whether or
// not they hold the wheel. The robot mirrors the latched state back, so this
// reflects reality rather than the last thing this console asked for.
import { computed } from 'vue'

const props = defineProps({
  engaged: { type: Boolean, default: false },
  disabled: { type: Boolean, default: false },
  compact: { type: Boolean, default: false },
})

const emit = defineEmits(['toggle'])

const label = computed(() => (props.engaged ? 'Release stop' : 'Emergency stop'))
</script>

<template>
  <button
    class="estop"
    :class="{ engaged, compact }"
    :disabled="disabled"
    @click="emit('toggle', !engaged)"
  >
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
  gap: 0.55rem;
  font-size: 1rem;
  font-weight: 600;
  letter-spacing: 0.01em;
  padding: 0.85rem;
  color: var(--stop);
  border-color: var(--stop);
  background: var(--stop-soft);
}
.estop .icon {
  font-size: 1.25rem;
  stroke-width: 2.2;
}
.estop.engaged {
  color: #fff;
  background: var(--stop);
  border-color: var(--stop);
}
.estop:hover:not(:disabled) {
  background: color-mix(in srgb, var(--stop) 20%, var(--surface));
}
.estop.engaged:hover:not(:disabled) {
  background: color-mix(in srgb, var(--stop) 85%, black);
}

/* Over the video the stop must read as the most solid thing on screen, so it
   stays filled rather than tinted. */
.estop.compact {
  width: auto;
  padding: 0.5rem 0.8rem;
  font-size: 0.85rem;
  color: #fff;
  background: color-mix(in srgb, var(--stop) 88%, black);
  border-color: color-mix(in srgb, var(--stop) 70%, black);
}
.estop.compact:hover:not(:disabled) {
  background: var(--stop);
}
.estop.compact .icon {
  font-size: 1.05rem;
}
</style>
