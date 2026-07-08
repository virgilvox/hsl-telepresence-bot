<script setup>
import { computed, reactive } from 'vue'
import ConnectionBar from './components/ConnectionBar.vue'
import VideoView from './components/VideoView.vue'
import DrivePad from './components/DrivePad.vue'
import EStopButton from './components/EStopButton.vue'
import TelemetryPanel from './components/TelemetryPanel.vue'
import { useClasp } from './composables/useClasp.js'
import { useRobotControl } from './composables/useRobotControl.js'
import { useTelemetry } from './composables/useTelemetry.js'
import { useVideo } from './composables/useVideo.js'

const STORAGE_KEY = 'hsl-console-settings'

const defaults = { robotId: 'hslbot', url: 'wss://relay.clasp.to', token: '' }
const settings = reactive({ ...defaults, ...loadSettings() })

const robotId = computed(() => settings.robotId)

const { connected, connecting, sessionId, error, connect, disconnect } = useClasp()
const control = useRobotControl(robotId)
const { status, motors, lastSeen } = useTelemetry(robotId)
const { remoteStream, state: videoState } = useVideo(robotId)

const online = computed(() => status.online === true || Date.now() - lastSeen.value < 5000)
const estopEngaged = computed(() => status.estop === true)
const controlsDisabled = computed(() => !connected.value)

function onConnect() {
  saveSettings()
  connect({ url: settings.url, name: 'operator', token: settings.token }).catch(() => {})
}

function updateSettings(next) {
  Object.assign(settings, next)
}

function loadSettings() {
  try {
    return JSON.parse(localStorage.getItem(STORAGE_KEY)) || {}
  } catch {
    return {}
  }
}

function saveSettings() {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(settings))
  } catch {
    // Storage is a convenience, not a requirement.
  }
}
</script>

<template>
  <div class="app">
    <ConnectionBar
      :settings="settings"
      :connected="connected"
      :connecting="connecting"
      :session-id="sessionId"
      :error="error"
      @update:settings="updateSettings"
      @connect="onConnect"
      @disconnect="disconnect"
    />

    <main class="layout">
      <VideoView class="video-slot" :stream="remoteStream" :state="videoState" />

      <aside class="sidebar">
        <EStopButton :control="control" :engaged="estopEngaged" :disabled="controlsDisabled" />
        <DrivePad :control="control" :disabled="controlsDisabled" />
        <TelemetryPanel :status="status" :motors="motors" :last-seen="lastSeen" :online="online" />
      </aside>
    </main>
  </div>
</template>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  gap: 0.9rem;
  padding: 0.9rem;
  min-height: 100vh;
  max-width: 1400px;
  margin: 0 auto;
}
.layout {
  display: grid;
  grid-template-columns: 1fr 340px;
  gap: 0.9rem;
  flex: 1;
  min-height: 0;
}
.video-slot {
  min-height: 420px;
}
.sidebar {
  display: flex;
  flex-direction: column;
  gap: 0.9rem;
}
@media (max-width: 900px) {
  .layout {
    grid-template-columns: 1fr;
  }
}
</style>
