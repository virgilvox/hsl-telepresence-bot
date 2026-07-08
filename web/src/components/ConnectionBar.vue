<script setup>
import { computed } from 'vue'

const props = defineProps({
  settings: { type: Object, required: true },
  connected: { type: Boolean, default: false },
  connecting: { type: Boolean, default: false },
  sessionId: { type: String, default: null },
  error: { type: String, default: null },
})

const emit = defineEmits(['connect', 'disconnect', 'update:settings'])

const statusText = computed(() => {
  if (props.connecting) return 'Connecting'
  if (props.connected) return 'Connected'
  return 'Offline'
})

function update(key, value) {
  emit('update:settings', { ...props.settings, [key]: value })
}
</script>

<template>
  <header class="bar panel">
    <div class="brand">
      <svg class="icon logo" viewBox="0 0 24 24" aria-hidden="true">
        <rect x="4" y="8" width="16" height="10" rx="2" />
        <path d="M9 8V6a3 3 0 0 1 6 0v2" />
        <circle cx="9" cy="13" r="1.2" fill="currentColor" stroke="none" />
        <circle cx="15" cy="13" r="1.2" fill="currentColor" stroke="none" />
      </svg>
      <span class="title">Telepresence Console</span>
    </div>

    <div class="fields">
      <label>
        <span>Robot</span>
        <input
          :value="settings.robotId"
          :disabled="connected || connecting"
          @input="update('robotId', $event.target.value)"
        />
      </label>
      <label class="wide">
        <span>Relay</span>
        <input
          :value="settings.url"
          :disabled="connected || connecting"
          @input="update('url', $event.target.value)"
        />
      </label>
      <label>
        <span>Token</span>
        <input
          type="password"
          placeholder="optional"
          :value="settings.token"
          :disabled="connected || connecting"
          @input="update('token', $event.target.value)"
        />
      </label>
    </div>

    <div class="status">
      <span class="dot" :class="{ live: connected, busy: connecting }" />
      <span class="state">{{ statusText }}</span>
      <span v-if="sessionId" class="mono session">{{ sessionId.slice(0, 8) }}</span>
      <button v-if="!connected" :disabled="connecting" @click="emit('connect')">Connect</button>
      <button v-else @click="emit('disconnect')">Disconnect</button>
    </div>

    <p v-if="error" class="error">{{ error }}</p>
  </header>
</template>

<style scoped>
.bar {
  display: grid;
  grid-template-columns: auto 1fr auto;
  align-items: center;
  gap: 1.25rem;
  padding: 0.75rem 1rem;
}
.brand {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  font-weight: 600;
}
.logo {
  font-size: 1.4rem;
  color: var(--accent);
}
.title {
  white-space: nowrap;
}
.fields {
  display: flex;
  gap: 0.75rem;
  min-width: 0;
}
label {
  display: flex;
  flex-direction: column;
  gap: 0.2rem;
  font-size: 0.7rem;
  color: var(--text-dim);
  letter-spacing: 0.04em;
  text-transform: uppercase;
}
label.wide {
  flex: 1;
  min-width: 12rem;
}
.status {
  display: flex;
  align-items: center;
  gap: 0.6rem;
  white-space: nowrap;
}
.dot {
  width: 9px;
  height: 9px;
  border-radius: 50%;
  background: var(--text-dim);
}
.dot.live {
  background: var(--ok);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--ok) 25%, transparent);
}
.dot.busy {
  background: var(--accent);
}
.session {
  color: var(--text-dim);
  font-size: 0.8rem;
}
.error {
  grid-column: 1 / -1;
  margin: 0;
  color: var(--stop);
  font-size: 0.85rem;
}
@media (max-width: 860px) {
  .bar {
    grid-template-columns: 1fr;
  }
  .fields {
    flex-wrap: wrap;
  }
}
</style>
