// Wiring for the simulator page.
//
// The worker owns the clock and the physics; this thread owns the pixels, the
// protocol and the UI. Everything here happens in response to either a CLASP
// message or a worker tick, and neither of those is throttled when the page is
// hidden, so the simulator keeps running at full rate behind the console.

import '../styles.css'
import './sim.css'
import { drawMap, renderCamera, FRAME_W, FRAME_H } from './world.js'
import { startAgent } from './agent.js'

const el = (id) => document.getElementById(id)

const camera = el('camera')
camera.width = FRAME_W
camera.height = FRAME_H

const cameraCtx = camera.getContext('2d', { alpha: false })
const stream = camera.captureStream(20)

const worker = new Worker(new URL('./render-worker.js', import.meta.url), { type: 'module' })
worker.postMessage({ type: 'start' })

const map = el('map')
const mapCtx = map.getContext('2d')

let agent = null
let pose = { x: 0, y: 0, th: 0 }

worker.onmessage = ({ data }) => {
  if (data.type !== 'tick') return
  pose = data.pose
  // Telemetry, the driving lease and viewer expiry all run off this.
  agent?.tick(data.wheels)
  paint(data)
}

function paint({ wheels, fps }) {
  // Drawing here rather than in the worker is what keeps frames flowing to the
  // captured stream while this page is in the background.
  renderCamera(cameraCtx, pose)
  drawMap(mapCtx, map.width, map.height, pose)
  el('pose').textContent = `${pose.x.toFixed(2)} ${pose.y.toFixed(2)} ${((pose.th * 180) / Math.PI)
    .toFixed(0)
    .padStart(4)}°`
  el('wheels').textContent = `${signed(wheels.left)} ${signed(wheels.right)}`
  el('fps').textContent = `${fps}`
}

// Default to an id nobody else is using, so opening this page can never
// impersonate a robot that is actually on the bench.
const saved = load()
el('robotId').value = saved.robotId || `sim-${Math.random().toString(36).slice(2, 7)}`
el('relay').value = saved.url || 'wss://relay.clasp.to'
el('token').value = saved.token || ''

async function start() {
  const robotId = el('robotId').value.trim()
  if (!robotId) return
  setBusy(true)
  save()
  try {
    agent = await startAgent({
      robotId,
      url: el('relay').value.trim(),
      token: el('token').value.trim(),
      stream,
      onDemand: (throttle, steer) => worker.postMessage({ type: 'demand', throttle, steer }),
      onEstop: (engaged) => worker.postMessage({ type: 'estop', engaged }),
      onState: render,
    })
    el('sessionId').textContent = agent.session.slice(0, 8)
    setRunning(true)
  } catch (err) {
    el('error').textContent = String(err?.message || err)
    setRunning(false)
  }
  setBusy(false)
}

async function stop() {
  setBusy(true)
  const going = agent
  agent = null
  worker.postMessage({ type: 'demand', throttle: 0, steer: 0 })
  await going?.stop()
  render(null)
  setRunning(false)
  setBusy(false)
}

function render(state) {
  el('driver').textContent = state?.driver ? state.driver.name : 'wheel free'
  el('driver').className = state?.driver ? 'v on' : 'v'
  el('estop').textContent = state?.estopped ? 'ENGAGED' : 'clear'
  el('estop').className = state?.estopped ? 'v alarm' : 'v'
  el('viewers').textContent = state ? String(state.viewers.length) : '0'
  el('peers').textContent = state?.peers?.length ? state.peers.join(' ') : '--'
}

function signed(v) {
  return ((v < 0 ? '' : '+') + v.toFixed(2)).padStart(5)
}

function setRunning(on) {
  el('start').hidden = on
  el('stop').hidden = !on
  el('lamp').className = on ? 'lamp live' : 'lamp'
  el('state').textContent = on ? 'broadcasting' : 'stopped'
  for (const id of ['robotId', 'relay', 'token']) el(id).disabled = on
  if (on) el('error').textContent = ''
}

function setBusy(busy) {
  el('start').disabled = busy
  el('stop').disabled = busy
}

function save() {
  try {
    localStorage.setItem(
      'hsl-sim-settings',
      JSON.stringify({
        robotId: el('robotId').value,
        url: el('relay').value,
        token: el('token').value,
      }),
    )
  } catch {
    // Storage is a convenience.
  }
}

function load() {
  try {
    return JSON.parse(localStorage.getItem('hsl-sim-settings')) || {}
  } catch {
    return {}
  }
}

el('start').addEventListener('click', start)
el('stop').addEventListener('click', stop)
// A tab that goes away should take its robot off the relay with it, rather
// than leaving a latched "online" behind for the next console to believe.
window.addEventListener('pagehide', () => {
  agent?.stop()
})
render(null)
setRunning(false)
