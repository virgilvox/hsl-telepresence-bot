<script setup>
// Identity and link state, in a fixed-height strip. The connection fields
// matter once, at the start of a session, so they fold away as soon as the
// console is connected and the robot readout takes their place.
import { computed, ref, watch } from 'vue'

const props = defineProps({
  settings: { type: Object, required: true },
  connected: { type: Boolean, default: false },
  connecting: { type: Boolean, default: false },
  online: { type: Boolean, default: false },
  // Tri-state: true responding, false deaf, null not yet known.
  responsive: { type: Boolean, default: null },
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
  if (props.connecting) return 'linking'
  if (!props.connected) return 'offline'
  if (!props.online) return 'relay only'
  // Publishing but not hearing us. Worth its own word, because everything else
  // on screen looks perfectly healthy in this state.
  if (props.responsive === false) return 'not responding'
  return 'robot online'
})

function update(key, value) {
  emit('update:settings', { ...props.settings, [key]: value })
}
</script>

<template>
  <header class="bar">
    <div class="strip">
      <div class="brand">
        <svg class="icon logo" viewBox="0 0 24 24" aria-hidden="true">
          <rect x="4" y="8" width="16" height="10" rx="1" />
          <path d="M9 8V6a3 3 0 0 1 6 0v2" />
          <rect x="8" y="12" width="2" height="2" fill="currentColor" stroke="none" />
          <rect x="14" y="12" width="2" height="2" fill="currentColor" stroke="none" />
        </svg>
        <span class="name">Telepresence</span>
      </div>

      <span class="rule" />

      <div class="readout">
        <span
          class="lamp"
          :class="{
            live: connected && online && responsive !== false,
            busy: connecting || (connected && !online),
            deaf: connected && online && responsive === false,
          }"
        />
        <span class="robot">{{ settings.robotId || 'no robot' }}</span>
        <span class="state">{{ linkState }}</span>
      </div>

      <div class="actions">
        <span v-if="sessionId" class="session num" :title="`Session ${sessionId}`">
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
        <button v-if="!connected" class="primary" :disabled="connecting" @click="emit('connect')">
          {{ connecting ? 'Linking' : 'Connect' }}
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
  border-bottom: 1px solid var(--line);
  background: var(--raised);
}
.strip {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  height: var(--header-h);
  padding: 0 0.75rem;
}
.brand {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  white-space: nowrap;
}
.logo {
  font-size: 1.25rem;
  color: var(--accent);
}
.name {
  font-size: var(--size-sm);
  font-weight: 700;
  letter-spacing: var(--track-wider);
  text-transform: uppercase;
  color: var(--text-bright);
}
.rule {
  width: 1px;
  height: 20px;
  background: var(--line);
  flex: none;
}
.readout {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  min-width: 0;
  font-size: var(--size-xs);
  letter-spacing: var(--track-wide);
  text-transform: uppercase;
}
.robot {
  color: var(--text-bright);
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.state {
  color: var(--text-faint);
  white-space: nowrap;
}
.state::before {
  content: '/ ';
}
.actions {
  display: flex;
  align-items: center;
  gap: 0.4rem;
  margin-left: auto;
}
.session {
  font-size: var(--size-xs);
  color: var(--text-faint);
  letter-spacing: var(--track-wide);
}
.gear {
  display: grid;
  place-items: center;
  width: 28px;
  height: 28px;
  padding: 0;
  font-size: 0.95rem;
}
.gear.on {
  color: var(--accent-bright);
  border-color: var(--accent);
}
.fields {
  display: grid;
  grid-template-columns: minmax(7rem, 1fr) minmax(12rem, 2fr) minmax(7rem, 1fr) minmax(7rem, 1fr);
  gap: 0.6rem;
  padding: 0 0.75rem 0.7rem;
  border-top: 1px solid var(--line);
  padding-top: 0.7rem;
}
label {
  display: flex;
  flex-direction: column;
  gap: 0.3rem;
  font-size: var(--size-xs);
  color: var(--text-faint);
  letter-spacing: var(--track-wider);
  text-transform: uppercase;
  font-weight: 600;
  min-width: 0;
}
.error {
  margin: 0;
  padding: 0.5rem 0.75rem;
  border-top: 1px solid var(--stop-dim);
  background: var(--stop-wash);
  color: var(--stop);
  font-size: var(--size-sm);
}
@media (max-width: 780px) {
  .fields {
    grid-template-columns: 1fr 1fr;
  }
  .state {
    display: none;
  }
}
</style>
