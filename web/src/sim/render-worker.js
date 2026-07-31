// The simulation clock, and everything that has to keep running on it.
//
// This is in a worker for one reason: browsers clamp timers on a hidden page to
// roughly one a second, and the simulator spends most of its life behind the
// console window. On the page's own thread that meant a one-frame-a-second
// camera and, worse, a robot that crawled, because the physics stepped on the
// same starved clock. A worker's timers are not throttled, so the simulation
// keeps real time no matter which window is in front.
//
// The worker owns the clock for the whole page. It steps the physics and the
// drive watchdog, and posts every tick to the page, which draws the frame and
// publishes telemetry from that message rather than from a timer of its own.
// Message handlers are not throttled the way timers are, so both keep full rate
// while hidden.
//
// Drawing stays on the page's thread on purpose. A canvas handed to a worker
// only reaches the MediaStream that captures it when the page composites, and
// a hidden page does not composite: measured, that path delivers exactly zero
// frames. Drawing on the page still produces captured frames while hidden, so
// the split is clock here, pixels there.

import { createWorld } from './world.js'

const TICK_MS = 50 // 20 Hz, the rate the camera is captured at
const STEP = 0.02 // fixed physics step
const MAX_CATCHUP = 0.5 // never advance more than this in one tick

// Mirrors DRIVE_TIMEOUT_MS on the robot: silence coasts the motors. The
// simulator has to have this or it would quietly excuse a console that stops
// sending.
const WATCHDOG_MS = 400

let timer = null
const world = createWorld()

let demand = { throttle: 0, steer: 0 }
let lastDemandAt = -Infinity
let estopped = false

let accumulator = 0
let last = 0
let frames = 0
let fpsAt = 0
let fps = 0

self.onmessage = ({ data }) => {
  switch (data.type) {
    case 'start':
      start()
      break
    case 'demand':
      demand = { throttle: data.throttle, steer: data.steer }
      lastDemandAt = performance.now()
      break
    case 'estop':
      estopped = Boolean(data.engaged)
      break
    case 'stop':
      clearInterval(timer)
      timer = null
      break
  }
}

function start() {
  if (timer) return
  last = performance.now()
  fpsAt = last
  timer = setInterval(tick, TICK_MS)
}

function tick() {
  const now = performance.now()
  let elapsed = (now - last) / 1000
  last = now
  // A long stall (the machine slept, the tab was frozen outright) should not
  // be replayed as motion.
  if (elapsed > MAX_CATCHUP) elapsed = MAX_CATCHUP

  const stale = now - lastDemandAt >= WATCHDOG_MS
  const wheels = estopped || stale ? { left: 0, right: 0 } : mix(demand.throttle, demand.steer)

  // Fixed steps, so the robot covers the same ground per second whatever rate
  // this loop happens to be running at. Clamping a variable dt instead, which
  // is the obvious thing to write, silently turns a throttled tab into slow
  // motion.
  accumulator += elapsed
  while (accumulator >= STEP) {
    world.step(STEP, wheels.left, wheels.right)
    accumulator -= STEP
  }

  frames++
  if (now - fpsAt >= 1000) {
    fps = Math.round((frames * 1000) / (now - fpsAt))
    frames = 0
    fpsAt = now
  }

  self.postMessage({ type: 'tick', pose: { ...world.pose }, wheels, fps })
}

// The same mix the motion plane applies: positive steer turns the robot right.
function mix(throttle, steer) {
  const t = clamp(throttle)
  const s = clamp(steer)
  return { left: clamp(t + s), right: clamp(t - s) }
}

function clamp(v) {
  const n = Number(v)
  return Number.isFinite(n) ? Math.max(-1, Math.min(1, n)) : 0
}
