<script setup>
// Identity and link state. The connection fields matter once, at the start of a
// session, so they fold away as soon as the console is connected and the robot
// chip takes their place.
import { computed, ref, watch } from 'vue'

const props = defineProps({
  settings: { type: Object, required: true },
  connected: { type: Boolean, default: false },
  connecting: { type: Boolean, default: false },
  online: { type: Boolean, default: false },
  sessionId: { type: String, default: null },
  error: { type: String, default: null },
})

const emit = defineEmits(['connect', 'disconnect', 'update:settings'])

const expanded = ref(!props.connected)

watch(
  () => props.connected,
  (isConnected) => {
    expanded.value = !isConnected
  },
)

const linkState = computed(() => {
  if (props.connecting) return 'Connecting'
  if (!props.connected) return 'Offline'
  return props.online ? 'Robot online' : 'Relay only'
})

function update(key, value) {
  emit('update:settings', { ...props.settings, [key]: value })
}
</script>

<template>
  <header class="bar panel">
    <div class="top">
      <div class="brand">
        <svg class="icon logo" viewBox="0 0 24 24" aria-hidden="true">
          <rect x="4" y="8" width="16" height="10" rx="2" />
          <path d="M9 8V6a3 3 0 0 1 6 0v2" />
          <circle cx="9" cy="13" r="1.2" fill="currentColor" stroke="none" />
          <circle cx="15" cy="13" r="1.2" fill="currentColor" stroke="none" />
        </svg>
        <span class="title">Telepresence</span>
      </div>

      <div class="chip" :title="linkState">
        <span
          class="dot"
          :class="{ live: connected && online, busy: connecting || (connected && !online) }"
        />
        <span class="robot mono">{{ settings.robotId || 'no robot' }}</span>
        <span class="sep">/</span>
        <span class="state">{{ linkState }}</span>
      </div>

      <div class="actions">
        <span v-if="sessionId" class="session mono" :title="`Session ${sessionId}`">
          {{ sessionId.slice(0, 8) }}
        </span>
        <button
          class="gear"
          :class="{ on: expanded }"
          :aria-expanded="expanded"
          aria-label="Connection settings"
          title="Connection settings"
          @click="expanded = !expanded"
        >
          <svg class="icon" viewBox="0 0 24 24" aria-hidden="true">
            <circle cx="12" cy="12" r="3" />
            <path
              d="M12 3v2M12 19v2M4.2 7.5l1.7 1M18.1 15.5l1.7 1M4.2 16.5l1.7-1M18.1 8.5l1.7-1"
            />
          </svg>
        </button>
        <button v-if="!connected" class="go" :disabled="connecting" @click="emit('connect')">
          {{ connecting ? 'Connecting' : 'Connect' }}
        </button>
        <button v-else @click="emit('disconnect')">Disconnect</button>
      </div>
    </div>

    <div v-if="expanded" class="fields">
      <label>
        <span>Robot</span>
        <input
          :value="settings.robotId"
          :disabled="connected || connecting"
          spellcheck="false"
          @input="update('robotId', $event.target.value)"
        />
      </label>
      <label class="wide">
        <span>Relay</span>
        <input
          :value="settings.url"
          :disabled="connected || connecting"
          spellcheck="false"
          @input="update('url', $event.target.value)"
        />
      </label>
      <label>
        <span>Your name</span>
        <input
          :value="settings.name"
          placeholder="operator"
          maxlength="32"
          @input="update('name', $event.target.value)"
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

    <p v-if="error" class="error">{{ error }}</p>
  </header>
</template>

<style scoped>
.bar {
  padding: 0.55rem 0.7rem;
}
.top {
  display: flex;
  align-items: center;
  gap: 0.8rem;
}
.brand {
  display: flex;
  align-items: center;
  gap: 0.45rem;
  font-weight: 600;
  font-size: 0.92rem;
  white-space: nowrap;
}
.logo {
  font-size: 1.3rem;
  color: var(--accent);
}
.chip {
  display: flex;
  align-items: center;
  gap: 0.45rem;
  font-size: 0.78rem;
  padding: 0.25rem 0.6rem;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: var(--surface-2);
  min-width: 0;
}
.robot {
  font-size: 0.78rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.sep {
  color: var(--text-faint);
}
.state {
  color: var(--text-dim);
  white-space: nowrap;
}
.actions {
  display: flex;
  align-items: center;
  gap: 0.4rem;
  margin-left: auto;
}
.session {
  font-size: 0.72rem;
  color: var(--text-faint);
}
.gear {
  display: grid;
  place-items: center;
  width: 30px;
  height: 30px;
  padding: 0;
  font-size: 1rem;
  color: var(--text-dim);
}
.gear.on {
  color: var(--accent);
  border-color: var(--accent);
}
.go {
  border-color: var(--accent);
  color: var(--accent);
}
.go:hover:not(:disabled) {
  background: var(--accent-soft);
}
.fields {
  display: grid;
  grid-template-columns: minmax(7rem, 1fr) minmax(12rem, 2fr) minmax(7rem, 1fr) minmax(7rem, 1fr);
  gap: 0.6rem;
  margin-top: 0.6rem;
  padding-top: 0.6rem;
  border-top: 1px solid var(--border);
}
label {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  font-size: 0.66rem;
  color: var(--text-faint);
  letter-spacing: 0.06em;
  text-transform: uppercase;
  font-weight: 600;
  min-width: 0;
}
.error {
  margin: 0.55rem 0 0;
  color: var(--stop);
  font-size: 0.82rem;
}
@media (max-width: 760px) {
  .fields {
    grid-template-columns: 1fr 1fr;
  }
  .chip .state {
    display: none;
  }
}
</style>
