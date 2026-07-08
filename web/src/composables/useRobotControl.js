// Turns operator intent into CLASP signals: continuous drive as a Stream, the
// latched e-stop as a Param, and config setpoints as Params.

import { computed } from 'vue'
import { useClasp } from './useClasp.js'
import { addresses } from '../protocol.js'

export function useRobotControl(robotId) {
  const { client, connected } = useClasp()
  const addr = computed(() => addresses(robotId.value))

  let seq = 0

  function drive(throttle, steer) {
    const c = client.value
    if (!c || !connected.value) return
    seq += 1
    c.stream(addr.value.drive, {
      throttle: clamp(throttle),
      steer: clamp(steer),
      seq,
      ts: Date.now(),
    })
  }

  // Sending a single zero frame is a courtesy stop; the robot's watchdog will
  // coast on its own if frames simply stop arriving.
  function stop() {
    drive(0, 0)
  }

  function setEstop(engaged) {
    const c = client.value
    if (!c || !connected.value) return
    c.set(addr.value.estop, Boolean(engaged))
  }

  function setMaxSpeed(value) {
    const c = client.value
    if (!c || !connected.value) return
    c.set(addr.value.cfg('max_speed'), clamp01(value))
  }

  return { drive, stop, setEstop, setMaxSpeed }
}

function clamp(v) {
  return Math.max(-1, Math.min(1, Number(v) || 0))
}

function clamp01(v) {
  return Math.max(0, Math.min(1, Number(v) || 0))
}
