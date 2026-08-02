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
import { useBroadcast, isBroadcastSupported } from './composables/useBroadcast.js'
import { useDrive } from './composables/useDrive.js'

const STORAGE_KEY = 'hsl-console-settings'

const defaults = { robotId: 'hslbot', url: 'wss://relay.clasp.to', token: '', name: '' }
const settings = reactive({ ...defaults, ...loadSettings() })

const robotId = computed(() => settings.robotId)
const operatorName = computed(() => settings.name)

const { connected, connecting, sessionId, error, connect, disconnect } = useClasp()
const control = useRobotControl(robotId, operatorName)
const { status, motors, lastSeen, online, responsive, maxSpeed } = useTelemetry(robotId)

const estopEngaged = computed(() => status.estop === true)

// The robot starts at two thirds because full throttle lurches on this
// chassis, which is unpleasant to drive and hard on the rail the motors share.
// Full speed stays available, but as something you turn on deliberately.
const NORMAL_SPEED = 2 / 3
const fullSpeed = computed(() => (maxSpeed.value ?? NORMAL_SPEED) > 0.9)
function toggleFullSpeed() {
  control.setMaxSpeed(fullSpeed.value ? NORMAL_SPEED : 1)
}

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

// Everyone watches the broadcast the relay fans out. Asking the robot for a
// WebRTC track of its own made it rebuild its pipeline, which froze every other
// watcher for about a second and a half, so the only consoles that still ask
// are the ones that cannot decode the broadcast at all.
const wantsPeer = computed(() => !isBroadcastSupported())
const { remoteStream, state: videoState } = useVideo(robotId, wantsPeer)
const {
  state: broadcastState,
  attach: attachBroadcast,
  error: broadcastError,
} = useBroadcast(robotId)

const mayDrive = computed(
  () => connected.value && (!arbitrated.value || isDriver.value || wheelFree.value),
)

// Driving a free wheel takes it, and the drive command carries the operator's
// name, so nothing has to be claimed up front. The only explicit claim left is
// Take over, for grabbing the wheel from someone mid-drive.
const drive = useDrive(control, { enabled: mayDrive })

const video = ref(null)

const relayHost = computed(() => settings.url.replace(/^wss?:\/\//, '').replace(/\/$/, ''))
const protocolLabel = computed(() =>
  connected.value && arbitrated.value ? `proto ${Number(status.protocol)}` : 'proto 1',
)

function onKey(event) {
  if (event.key !== 'f' && event.key !== 'F') return
  if (event.metaKey || event.ctrlKey || event.altKey) return
  const tag = event.target?.tagName?.toLowerCase()
  if (tag === 'input' || tag === 'textarea' || event.target?.isContentEditable) return
  event.preventDefault()
  video.value?.toggleFullscreen()
}

onMounted(() => window.addEventListener('keydown', onKey))
onUnmounted(() => window.removeEventListener('keydown', onKey))

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
      :responsive="responsive"
      :session-id="sessionId"
      :error="error"
      @update:settings="updateSettings"
      @connect="onConnect"
      @disconnect="onDisconnect"
    />

    <main class="layout">
      <VideoView
        ref="video"
        class="feed"
        :stream="remoteStream"
        :state="videoState"
        :broadcast-state="broadcastState"
        :broadcast-error="broadcastError"
        :connected="connected"
        :connecting="connecting"
        :attach-broadcast="attachBroadcast"
        @connect="onConnect"
        @disconnect="onDisconnect"
      >
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

      <aside class="rail">
        <EStopButton :engaged="estopEngaged" :disabled="!connected" @toggle="control.setEstop" />

        <section class="panel">
          <div class="panel-head">
            <span class="tick" />
            <h2>Control</h2>
          </div>
          <div class="panel-body">
            <ControlPanel
              :driver="driver"
              :my-session="sessionId"
              :viewers="viewers"
              :arbitrated="arbitrated"
              :connected="connected"
              :disabled="!connected"
              @claim="control.claimControl"
            />
          </div>
        </section>

        <section class="panel">
          <div class="panel-head">
            <span class="tick" />
            <h2>Drive</h2>
            <span class="aside">{{ mayDrive ? 'armed' : 'locked' }}</span>
          </div>
          <div class="panel-body">
            <DrivePad :drive="drive" :disabled="!mayDrive" />
            <div class="speed">
              <button
                class="speedbtn"
                :class="{ on: fullSpeed }"
                :disabled="!connected"
                @click="toggleFullSpeed"
              >
                {{ fullSpeed ? 'Full speed on' : 'Full speed' }}
              </button>
              <span class="speednote">
                <template v-if="fullSpeed">
                  Starts hard and can lurch. It also pulls the most current,
                  which is what browns the Pi out.
                </template>
                <template v-else>
                  Limited to two thirds, which starts smoothly.
                </template>
              </span>
            </div>
          </div>
        </section>

        <section class="panel">
          <div class="panel-head">
            <span class="tick" />
            <h2>Telemetry</h2>
          </div>
          <div class="panel-body">
            <TelemetryPanel
              :status="status"
              :motors="motors"
              :last-seen="lastSeen"
              :online="online"
            />
          </div>
        </section>
      </aside>
    </main>

    <footer class="statusbar">
      <span class="lamp" :class="{ live: connected && online, busy: connecting }" />
      <span>{{ relayHost }}</span>
      <span class="sep" />
      <span>{{ protocolLabel }}</span>
      <span class="sep" />
      <span>{{ videoState }}</span>
      <span class="spacer" />
      <span class="hint">drag pad or hold WASD / arrows</span>
      <span class="sep" />
      <span class="hint"><kbd>F</kbd> fullscreen</span>
    </footer>
  </div>
</template>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  height: 100vh;
  height: 100dvh;
}
.layout {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 296px;
  gap: 0.7rem;
  padding: 0.7rem;
  flex: 1;
  min-height: 0;
}
.feed {
  min-height: 0;
}
.rail {
  display: flex;
  flex-direction: column;
  gap: 0.7rem;
  min-height: 0;
  overflow-y: auto;
}

/* Bottom rail of standing facts, the way an editor keeps its status line. */
.statusbar {
  display: flex;
  align-items: center;
  gap: 0.55rem;
  height: var(--statusbar-h);
  flex: none;
  padding: 0 0.75rem;
  border-top: 1px solid var(--line);
  background: var(--raised);
  font-size: var(--size-xs);
  letter-spacing: var(--track-wide);
  text-transform: uppercase;
  color: var(--text-faint);
  white-space: nowrap;
  overflow: hidden;
}
.sep {
  width: 1px;
  height: 12px;
  background: var(--line);
  flex: none;
}
.spacer {
  flex: 1;
}
kbd {
  font-family: inherit;
  font-size: 0.95em;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-sm);
  padding: 0 0.25em;
  color: var(--text-dim);
}

@media (max-width: 900px) {
  .app {
    height: auto;
    min-height: 100dvh;
  }
  .layout {
    grid-template-columns: 1fr;
  }
  .feed {
    min-height: 46vh;
  }
  .rail {
    overflow: visible;
  }
  .statusbar .hint {
    display: none;
  }
}
</style>
