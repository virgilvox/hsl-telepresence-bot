// Subscribes to the robot's status Params and telemetry Streams. Status Params
// snapshot on subscribe, so the panel shows correct state the instant it loads.

import { computed, reactive, ref, watch, onUnmounted } from 'vue'
import { useClasp } from './useClasp.js'
import { addresses } from '../protocol.js'

// How long the robot may go quiet before the console stops calling it online.
// It publishes motor telemetry five times a second, so this is many missed
// messages, not a marginal one.
const SILENCE_LIMIT_MS = 6000

export function useTelemetry(robotId) {
  const { client, connected } = useClasp()

  const status = reactive({})
  const motors = reactive({ left: 0, right: 0 })
  const lastSeen = ref(0)

  // Liveness has to be derived from the passage of time, and time is not
  // reactive. Without this tick, `online` would only be recomputed when a
  // message arrived, so a robot that stopped sending would stay "online"
  // forever: exactly the case the check exists to catch.
  const now = ref(Date.now())
  const clock = setInterval(() => {
    now.value = Date.now()
  }, 1000)

  // `status/online` is a latched Param, so it survives the robot that set it.
  // A robot killed uncleanly, unplugged, or crashed never clears it, and the
  // relay keeps serving `true` to every console that connects afterwards. It
  // is necessary but not sufficient: the robot must also still be talking.
  const online = computed(
    () => status.online === true && now.value - lastSeen.value < SILENCE_LIMIT_MS,
  )

  let unsubs = []

  function tail(address) {
    const i = address.lastIndexOf('/')
    return i >= 0 ? address.slice(i + 1) : address
  }

  function subscribe() {
    unsubscribe()
    // Everything here mirrors one robot, so it cannot be carried across to the
    // next one. Point the console at an older robot after a newer one and the
    // stale `protocol` would convince it the new robot arbitrates the wheel,
    // while the stale `driver` named somebody else holding it: the pad locks
    // and the robot cannot be driven at all. Params re-snapshot on subscribe,
    // so anything still true is about to arrive again anyway.
    for (const key of Object.keys(status)) delete status[key]
    motors.left = 0
    motors.right = 0
    lastSeen.value = 0

    const c = client.value
    if (!c || !connected.value || !robotId.value) return
    const addr = addresses(robotId.value)

    unsubs.push(
      c.on(addr.statusPattern, (value, address) => {
        status[tail(address)] = value
        lastSeen.value = Date.now()
      }),
    )
    unsubs.push(
      c.on(
        addr.telPattern,
        (value, address) => {
          lastSeen.value = Date.now()
          if (tail(address) === 'motors' && value && typeof value === 'object') {
            motors.left = value.left ?? 0
            motors.right = value.right ?? 0
          }
        },
        { maxRate: 10 },
      ),
    )
  }

  function unsubscribe() {
    for (const u of unsubs) {
      try {
        u?.()
      } catch {
        // Unsubscribe handles are best-effort.
      }
    }
    unsubs = []
  }

  watch([connected, robotId], subscribe, { immediate: true })
  onUnmounted(() => {
    clearInterval(clock)
    unsubscribe()
  })

  return { status, motors, lastSeen, online }
}
