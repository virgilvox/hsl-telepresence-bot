<script setup>
// Who has the wheel, and how to take it. Several operators can watch at once;
// the robot grants driving to one of them and publishes its decision, so this
// panel reports the robot's answer rather than guessing locally.
import { computed } from 'vue'

const props = defineProps({
  driver: { type: Object, default: null },
  mySession: { type: String, default: null },
  viewers: { type: Number, default: null },
  // False when the robot predates multi-operator support: it serves one viewer
  // and arbitrates nothing, so offering to take control would be a lie.
  arbitrated: { type: Boolean, default: false },
  connected: { type: Boolean, default: false },
  disabled: { type: Boolean, default: false },
  compact: { type: Boolean, default: false },
})

const emit = defineEmits(['claim', 'release'])

const isMine = computed(
  () => Boolean(props.driver?.session) && props.driver.session === props.mySession,
)
const isFree = computed(() => !props.driver?.session)

const label = computed(() => {
  if (!props.connected) return 'Not connected'
  if (!props.arbitrated) return 'Single operator robot'
  if (isMine.value) return 'You have control'
  if (isFree.value) return 'Wheel is free'
  return `${props.driver.name || 'Someone'} has control`
})

const action = computed(() => {
  if (isMine.value) return { text: 'Release', event: 'release' }
  if (isFree.value) return { text: 'Take control', event: 'claim' }
  return { text: 'Take over', event: 'claim' }
})

const watchers = computed(() =>
  typeof props.viewers === 'number' ? `${props.viewers} watching` : null,
)
</script>

<template>
  <section class="control" :class="{ compact, mine: isMine, free: isFree }">
    <p v-if="!compact" class="panel-title">Control</p>

    <div class="line">
      <span class="dot" :class="{ live: isMine, busy: !isMine && !isFree }" />
      <span class="who">{{ label }}</span>
      <span v-if="watchers" class="watchers mono">{{ watchers }}</span>
    </div>

    <button
      v-if="arbitrated"
      class="act"
      :class="{ primary: !isMine }"
      :disabled="disabled"
      @click="emit(action.event)"
    >
      {{ action.text }}
    </button>
  </section>
</template>

<style scoped>
.control {
  padding: 0.8rem;
}
.control.compact {
  padding: 0;
  display: flex;
  align-items: center;
  gap: 0.6rem;
}
.line {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  font-size: 0.88rem;
  min-width: 0;
}
.compact .line {
  font-size: 0.8rem;
  color: var(--hud-text);
}
.who {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.watchers {
  margin-left: auto;
  font-size: 0.72rem;
  color: var(--text-faint);
  white-space: nowrap;
}
.compact .watchers {
  color: var(--hud-dim);
}
.act {
  width: 100%;
  margin-top: 0.65rem;
}
.compact .act {
  width: auto;
  margin: 0;
  padding: 0.25rem 0.55rem;
  font-size: 0.75rem;
  background: transparent;
  color: var(--hud-text);
  border-color: var(--hud-border);
}
.compact .act:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.12);
}
.act.primary {
  border-color: var(--accent);
  color: var(--accent);
}
.act.primary:hover:not(:disabled) {
  background: var(--accent-soft);
}
.compact .act.primary {
  color: var(--accent);
  border-color: color-mix(in srgb, var(--accent) 65%, transparent);
}
</style>
