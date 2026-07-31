<script setup>
import { computed, onMounted, onUnmounted, reactive, ref, watch } from 'vue'
import ConnectionBar from './components/ConnectionBar.vue'
import VideoView from './components/VideoView.vue'
import DrivePad from './components/DrivePad.vue'
import EStopButton from './components/EStopButton.vue'
import ControlPanel from './components/ControlPanel.vue'
import TelemetryPanel from './components/TelemetryPanel.vue'
import { useClasp } from './composables/useClasp.js'
import { useRobotControl } from './composables/useRobotControl.js'
import { useTelemetry } from './composables/useTelemetry.js'
import { useVideo } from './composables/useVideo.js'
import { useDrive } from './composables/useDrive.js'

const STORAGE_KEY = 'hsl-console-settings'

const defaults = { robotId: 'hslbot', url: 'wss://relay.clasp.to', token: '', name: '' }
const settings = reactive({ ...defaults, ...loadSettings() })

const robotId = computed(() => settings.robotId)
const operatorName = computed(() => settings.name)

const { connected, connecting, sessionId, error, connect, disconnect } = useClasp()
const control = useRobotControl(robotId, operatorName)
const { status, motors, lastSeen } = useTelemetry(robotId)
const { remoteStream, state: videoState } = useVideo(robotId)

const online = computed(() => status.online === true || Date.now() - lastSeen.value < 5000)
const estopEngaged = computed(() => status.estop === true)

// A robot that publishes no protocol version predates multi-operator support:
// it serves one viewer and arbitrates nothing, so the console must let whoever
// is here drive rather than waiting for a lease that will never be granted.
const arbitrated = computed(() => Number(status.protocol) >= 2)
const driver = computed(() =>
  status.driver && typeof status.driver === 'object' ? status.driver : null,
)
const viewers = computed(() => (typeof status.viewers === 'number' ? status.viewers : null))
const isDriver = computed(
  () => Boolean(driver.value?.session) && driver.value.session === sessionId.value,
)
const wheelFree = computed(() => !driver.value?.session)
const mayDrive = computed(
  () => connected.value && (!arbitrated.value || isDriver.value || wheelFree.value),
)

const drive = useDrive(control, {
  enabled: mayDrive,
  // Touching the controls with a free wheel takes it, so a lone operator never
  // has to ask. Taking it from someone else is always an explicit button.
  onEngage: () => {
    if (arbitrated.value && !isDriver.value) control.claimControl()
  },
})

// Hold the wheel while this console is open. The robot expires a lease that
// stops being renewed, which is what frees the wheel when someone closes their
// tab; without this heartbeat an operator who took control and then paused to
// look at something would silently lose it after a few seconds.
let holdTimer = null
watch(
  isDriver,
  (mine) => {
    clearInterval(holdTimer)
    holdTimer = mine ? setInterval(() => control.claimControl(), 3000) : null
  },
  { immediate: true },
)

const video = ref(null)

function onKey(event) {
  if (event.key !== 'f' && event.key !== 'F') return
  if (event.metaKey || event.ctrlKey || event.altKey) return
  const tag = event.target?.tagName?.toLowerCase()
  if (tag === 'input' || tag === 'textarea' || event.target?.isContentEditable) return
  event.preventDefault()
  video.value?.toggleFullscreen()
}

onMounted(() => window.addEventListener('keydown', onKey))
onUnmounted(() => {
  window.removeEventListener('keydown', onKey)
  clearInterval(holdTimer)
})

function onConnect() {
  saveSettings()
  connect({ url: settings.url, name: settings.name || 'operator', token: settings.token }).catch(
    () => {},
  )
}

function onDisconnect() {
  if (arbitrated.value && isDriver.value) control.releaseControl()
  disconnect()
}

function updateSettings(next) {
  Object.assign(settings, next)
  saveSettings()
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
      :online="online"
      :session-id="sessionId"
      :error="error"
      @update:settings="updateSettings"
      @connect="onConnect"
      @disconnect="onDisconnect"
    />

    <main class="layout">
      <VideoView ref="video" class="video-slot" :stream="remoteStream" :state="videoState">
        <!-- In fullscreen the same controls come along, anchored to the
             corners, so going fullscreen never costs you the stop. -->
        <template #hud>
          <div class="hud-tl hud-card">
            <ControlPanel
              compact
              :driver="driver"
              :my-session="sessionId"
              :viewers="viewers"
              :arbitrated="arbitrated"
              :connected="connected"
              :disabled="!connected"
              @claim="control.claimControl"
              @release="control.releaseControl"
            />
          </div>
          <div class="hud-tr">
            <EStopButton
              compact
              :engaged="estopEngaged"
              :disabled="!connected"
              @toggle="control.setEstop"
            />
          </div>
          <div class="hud-bl hud-card">
            <TelemetryPanel
              compact
              :status="status"
              :motors="motors"
              :last-seen="lastSeen"
              :online="online"
            />
          </div>
          <div class="hud-br hud-card">
            <DrivePad compact :drive="drive" :disabled="!mayDrive" />
          </div>
        </template>
      </VideoView>

      <aside class="sidebar">
        <EStopButton :engaged="estopEngaged" :disabled="!connected" @toggle="control.setEstop" />
        <ControlPanel
          class="panel"
          :driver="driver"
          :my-session="sessionId"
          :viewers="viewers"
          :arbitrated="arbitrated"
          :connected="connected"
          :disabled="!connected"
          @claim="control.claimControl"
          @release="control.releaseControl"
        />
        <DrivePad class="panel" :drive="drive" :disabled="!mayDrive" />
        <TelemetryPanel
          class="panel"
          :status="status"
          :motors="motors"
          :last-seen="lastSeen"
          :online="online"
        />
        <p class="hint">
          Drag the pad or hold WASD / arrows. Release to coast. <kbd>F</kbd> for fullscreen.
        </p>
      </aside>
    </main>
  </div>
</template>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  gap: 0.7rem;
  padding: 0.7rem;
  height: 100vh;
  height: 100dvh;
  max-width: 1500px;
  margin: 0 auto;
}
.layout {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 310px;
  gap: 0.7rem;
  flex: 1;
  min-height: 0;
}
.video-slot {
  min-height: 0;
}
.sidebar {
  display: flex;
  flex-direction: column;
  gap: 0.7rem;
  min-height: 0;
  overflow-y: auto;
}
.hint {
  margin: 0;
  color: var(--text-faint);
  font-size: 0.73rem;
  line-height: 1.5;
}
kbd {
  font-family: var(--mono);
  font-size: 0.9em;
  border: 1px solid var(--border);
  border-bottom-width: 2px;
  border-radius: 4px;
  padding: 0 0.25em;
}
@media (max-width: 900px) {
  .app {
    height: auto;
    min-height: 100dvh;
  }
  .layout {
    grid-template-columns: 1fr;
  }
  .video-slot {
    min-height: 46vh;
  }
  .sidebar {
    overflow: visible;
  }
}
</style>
