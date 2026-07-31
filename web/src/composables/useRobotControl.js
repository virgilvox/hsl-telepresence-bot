// Turns operator intent into CLASP signals: continuous drive as a Stream, the
// latched e-stop as a Param, config setpoints as Params, and the driving lease
// as one-shot control Events.

import { computed } from 'vue'
import { useClasp } from './useClasp.js'
import { addresses, ControlAction } from '../protocol.js'

export function useRobotControl(robotId, operatorName) {
  const { client, connected, sessionId } = useClasp()
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
      // The robot obeys drive only from the session holding the wheel.
      session: sessionId.value || '',
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

  // Take the wheel. The robot grants this unconditionally, so this doubles as
  // "take over from whoever has it".
  function claimControl() {
    const c = client.value
    if (!c || !connected.value || !sessionId.value) return
    c.emit(addr.value.control, {
      action: ControlAction.Claim,
      session: sessionId.value,
      name: operatorName?.value || '',
    })
  }

  function releaseControl() {
    const c = client.value
    if (!c || !connected.value || !sessionId.value) return
    c.emit(addr.value.control, {
      action: ControlAction.Release,
      session: sessionId.value,
    })
  }

  return { drive, stop, setEstop, setMaxSpeed, claimControl, releaseControl }
}

function clamp(v) {
  return Math.max(-1, Math.min(1, Number(v) || 0))
}

function clamp01(v) {
  return Math.max(0, Math.min(1, Number(v) || 0))
}
