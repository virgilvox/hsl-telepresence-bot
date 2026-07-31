<script setup>
// Deliberately not arbitrated: anyone watching can stop the robot, whether or
// not they hold the wheel. The robot mirrors the latched state back, so this
// reflects reality rather than the last thing this console asked for.
//
// The only filled control in the interface. Everything else is outlined, so
// there is never a question about which control is the important one.
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
  gap: 0.5rem;
  font-size: var(--size-sm);
  font-weight: 700;
  letter-spacing: var(--track-wider);
  padding: 0.8rem;
  color: var(--stop);
  border-color: var(--stop-dim);
  background: var(--stop-wash);
  box-shadow: var(--lift);
}
.estop .icon {
  font-size: 1.1rem;
  stroke-width: 2.2;
}
.estop:hover:not(:disabled) {
  background: rgba(239, 68, 68, 0.22);
  border-color: var(--stop);
  color: var(--stop);
}
.estop:active:not(:disabled) {
  transform: translate(4px, 4px);
  box-shadow: none;
}

/* Latched: the robot is actually held. Filled, so it is unmistakable across
   the room. */
.estop.engaged {
  color: #fff;
  background: var(--stop);
  border-color: var(--stop);
}
.estop.engaged:hover:not(:disabled) {
  background: var(--stop-dim);
  border-color: var(--stop-dim);
  color: #fff;
}

.estop.compact {
  width: auto;
  padding: 0.5rem 0.75rem;
  color: #fff;
  background: var(--stop-dim);
  border-color: var(--stop);
}
.estop.compact:hover:not(:disabled) {
  background: var(--stop);
}
.estop.compact .icon {
  font-size: 1rem;
}
</style>
