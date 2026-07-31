// A stand-in robot for developing the console without hardware. It answers the
// control plane the way the real agent does: status, the driving lease, the
// e-stop, viewer presence, and motor telemetry. There is no camera and no
// WebRTC, so the video panel stays on "Waiting for robot".
//
// This is a test double, not a specification. robot/src/control.rs is the real
// arbiter and the only thing that decides who drives; if the two ever disagree,
// this file is the one that is wrong.
//
//   node tools/sim-robot.mjs my-test-bot
//
// Then point the console at that robot id. Open it in two windows to exercise
// taking over, releasing, and the lease lapsing.
import clasp from '@clasp-to/sdk'

const robotId = process.argv[2] || 'hsl-sim'
const url = process.env.RELAY || 'wss://relay.clasp.to'
const LEASE_MS = 8000
const VIEWER_TIMEOUT_MS = 20000

const base = `/robot/${robotId}`
const A = {
  drive: `${base}/cmd/drive`,
  estop: `${base}/cmd/estop`,
  control: `${base}/cmd/control`,
  hello: `${base}/video/hello`,
  status: (n) => `${base}/status/${n}`,
  tel: (n) => `${base}/tel/${n}`,
}

const c = await clasp(url, { name: `sim-${robotId}`, reconnect: true })
console.log(`sim robot "${robotId}" on ${url}, session ${c.session}`)

let driver = null // { session, name }
let touched = 0
let estopped = false
let wheels = { left: 0, right: 0 }
const viewers = new Map()
let publishedViewers = -1

await c.set(A.status('online'), true)
await c.set(A.status('mode'), 'manual')
await c.set(A.status('estop'), false)
await c.set(A.status('protocol'), 2)
await c.set(A.status('driver'), null)
await c.set(A.status('viewers'), 0)

// Published only when the holder actually changes, like the real arbiter: the
// console renews its claim every few seconds and that must not republish a
// Param every few seconds.
let publishedDriver = undefined
function publishDriver() {
  const key = driver ? `${driver.session}:${driver.name}` : null
  if (key === publishedDriver) return
  publishedDriver = key
  c.set(A.status('driver'), driver ? { session: driver.session, name: driver.name } : null)
  console.log(driver ? `driver: ${driver.name} (${driver.session.slice(0, 8)})` : 'wheel free')
}

function accepts(session) {
  expire()
  if (!driver) {
    driver = { session, name: 'operator' }
    touched = Date.now()
    publishDriver()
    return true
  }
  if (driver.session === session) {
    touched = Date.now()
    return true
  }
  return false
}

function expire() {
  if (driver && Date.now() - touched >= LEASE_MS) {
    driver = null
    publishDriver()
  }
}

c.on(`${base}/cmd/**`, (value, address) => {
  if (address === A.drive) {
    if (!accepts(value?.session || '')) return
    if (estopped) return
    const t = clamp(value?.throttle)
    const s = clamp(value?.steer)
    wheels = { left: clamp(t + s), right: clamp(t - s) }
  } else if (address === A.estop) {
    estopped = Boolean(value)
    if (estopped) wheels = { left: 0, right: 0 }
    c.set(A.status('estop'), estopped)
    console.log(`estop ${estopped ? 'ENGAGED' : 'cleared'}`)
  } else if (address === A.control) {
    if (value?.action === 'claim' && value.session) {
      const name = (value.name || '').trim().slice(0, 32) || 'operator'
      driver = { session: value.session, name }
      touched = Date.now()
      publishDriver()
    } else if (value?.action === 'release' && driver?.session === value.session) {
      driver = null
      publishDriver()
    }
  }
})

c.on(A.hello, (value) => {
  if (value?.session) viewers.set(value.session, Date.now())
})

// A console says goodbye on its way out so its slot frees immediately instead
// of waiting for the presence timeout.
c.on(`${base}/video/signal/**`, (value, address) => {
  if (address.split('/').pop() !== c.session) return
  if (value?.kind === 'bye' && value.from) viewers.delete(value.from)
})

setInterval(() => {
  expire()
  // Coast when nobody has sent a drive recently, like the real watchdog.
  if (Date.now() - touched > 400) wheels = { left: 0, right: 0 }
  c.stream(A.tel('motors'), wheels)

  for (const [session, seen] of viewers) {
    if (Date.now() - seen > VIEWER_TIMEOUT_MS) viewers.delete(session)
  }
  if (viewers.size !== publishedViewers) {
    publishedViewers = viewers.size
    c.set(A.status('viewers'), viewers.size)
    console.log(`viewers: ${viewers.size}`)
  }
}, 200)

function clamp(v) {
  return Math.max(-1, Math.min(1, Number(v) || 0))
}

process.on('SIGINT', async () => {
  await c.set(A.status('online'), false)
  process.exit(0)
})
