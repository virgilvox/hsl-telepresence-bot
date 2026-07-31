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

const emit = defineEmits(['claim'])

const isMine = computed(
  () => Boolean(props.driver?.session) && props.driver.session === props.mySession,
)
const isFree = computed(() => !props.driver?.session)

const label = computed(() => {
  if (!props.connected) return 'not connected'
  if (!props.arbitrated) return 'single operator'
  if (isMine.value) return 'you are driving'
  if (isFree.value) return 'wheel is free'
  return `${props.driver.name || 'someone'} is driving`
})

// The only button here, and only when there is somebody to take the wheel
// from. A free wheel needs no ceremony: just drive. Handing it back needs none
// either, because the lease lapses a moment after you stop.
const canTakeOver = computed(
  () => props.arbitrated && props.connected && !isMine.value && !isFree.value,
)
</script>

<template>
  <section class="control" :class="{ compact }">
    <div class="who">
      <span class="lamp" :class="{ live: isMine, busy: !isMine && !isFree && connected }" />
      <span class="label">{{ label }}</span>
      <span v-if="typeof viewers === 'number'" class="badge num watchers">
        {{ viewers }} watching
      </span>
    </div>

    <button v-if="canTakeOver" class="act primary" :disabled="disabled" @click="emit('claim')">
      Take over
    </button>
    <p v-else-if="arbitrated && connected && isFree && !compact" class="hint">
      hold WASD or drag the pad to take the wheel
    </p>
  </section>
</template>

<style scoped>
.control {
  display: flex;
  flex-direction: column;
  gap: 0.6rem;
}
.compact {
  flex-direction: row;
  align-items: center;
  gap: 0.6rem;
}
.who {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  min-width: 0;
}
.label {
  font-size: var(--size-xs);
  letter-spacing: var(--track-wide);
  text-transform: uppercase;
  color: var(--text-bright);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.watchers {
  margin-left: auto;
  color: var(--text-faint);
}
.compact .watchers {
  margin-left: 0.25rem;
}
.act {
  width: 100%;
}
.compact .act {
  width: auto;
  margin-left: auto;
}
.hint {
  margin: 0;
  font-size: var(--size-xs);
  color: var(--text-faint);
  letter-spacing: var(--track-wide);
}
</style>
