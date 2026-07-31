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
  if (!props.connected) return 'not connected'
  if (!props.arbitrated) return 'single operator'
  if (isMine.value) return 'you have control'
  if (isFree.value) return 'wheel is free'
  return `${props.driver.name || 'someone'} has control`
})

const action = computed(() => {
  if (isMine.value) return { text: 'Release', event: 'release', primary: false }
  if (isFree.value) return { text: 'Take control', event: 'claim', primary: true }
  return { text: 'Take over', event: 'claim', primary: true }
})
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

    <button
      v-if="arbitrated"
      class="act"
      :class="{ primary: action.primary }"
      :disabled="disabled"
      @click="emit(action.event)"
    >
      {{ action.text }}
    </button>
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
</style>
