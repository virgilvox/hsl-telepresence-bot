// Subscribes to the robot's status Params and telemetry Streams. Status Params
// snapshot on subscribe, so the panel shows correct state the instant it loads.

import { reactive, ref, watch, onUnmounted } from 'vue'
import { useClasp } from './useClasp.js'
import { addresses } from '../protocol.js'

export function useTelemetry(robotId) {
  const { client, connected } = useClasp()

  const status = reactive({})
  const motors = reactive({ left: 0, right: 0 })
  const lastSeen = ref(0)

  let unsubs = []

  function tail(address) {
    const i = address.lastIndexOf('/')
    return i >= 0 ? address.slice(i + 1) : address
  }

  function subscribe() {
    unsubscribe()
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
  onUnmounted(unsubscribe)

  return { status, motors, lastSeen }
}
