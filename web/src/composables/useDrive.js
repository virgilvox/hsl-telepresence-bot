// Teleop input: the drive pad's pointer drag and the keyboard, folded into one
// throttle/steer demand and sent to the robot.
//
// This lives outside the pad component on purpose. The pad is drawn in the
// sidebar normally and inside the video in fullscreen, and only one thing may
// own the key listeners and the send timer: two of them would double every
// command and fight over the knob.
//
// # Why this is not a plain event on press and release
//
// Sending only on the edges is the obvious design and it is not safe here.
// `cmd/drive` is a CLASP Stream, which is best-effort by contract, so a
// dropped release leaves the robot holding its last velocity. Worse, a release
// is never sent at all when the operator's network drops, their machine
// sleeps, or the tab dies mid-drive. The robot's watchdog coasting on silence
// is the only thing that covers those, and it can only do that if silence
// means something.
//
// So the demand is sent the instant it changes, which is what makes the
// controls feel immediate, and then repeated at a low rate for as long as the
// robot is actually moving, purely so that going quiet is meaningful. Holding a
// key steady is a handful of messages a second rather than a continuous
// sampling of the input, and releasing goes silent after one zero frame.
//
// Keyboard rules learned the hard way, all of them safety-relevant on a robot
// that keeps moving while a key is held:
//
//   - Keys are tracked by `event.code`, not `event.key`. `key` changes with
//     Shift ('w' becomes 'W'), so pressing w, then Shift, then releasing gives
//     a keyup that never matches the keydown and leaves the robot driving with
//     nothing held.
//   - Every held key is dropped when the window loses focus or the tab is
//     hidden, because no keyup arrives for a key released while alt-tabbed.
//   - Typing in a text field never drives. Without this, naming yourself
//     "swan" would drive backwards, forwards and left.
//   - Modifier chords are left to the browser, so cmd+W still closes the tab
//     instead of steering.

import { onUnmounted, reactive, ref, watch } from 'vue'

// Keepalive while moving. Comfortably inside the robot's 400 ms drive
// watchdog, so a single dropped frame does not cause a spurious coast, and far
// below the old fixed 15 Hz sampling.
const REPEAT_MS = 150

// Floor on the gap between change-driven sends. A pointer drag fires far
// faster than the robot can care about, and without this a single flick would
// queue a burst of near-identical commands.
const MIN_GAP_MS = 50

// Physical key positions to a unit vector: [steer, throttle].
const DRIVE_KEYS = {
  KeyW: [0, 1],
  KeyS: [0, -1],
  KeyA: [-1, 0],
  KeyD: [1, 0],
  ArrowUp: [0, 1],
  ArrowDown: [0, -1],
  ArrowLeft: [-1, 0],
  ArrowRight: [1, 0],
}

export function useDrive(control, { enabled, onEngage } = {}) {
  // -1..1 each. y is positive forward, x is positive to the robot's right.
  const knob = reactive({ x: 0, y: 0 })
  const dragging = ref(false)
  const held = reactive(new Set())
  const engaged = ref(false)

  let pointerId = null
  let timer = null
  // True once a zero frame has been sent for the current release, so we send
  // exactly one courtesy stop rather than a stream of them.
  let stopped = true
  // The demand the robot was last told about, and when.
  let sentX = 0
  let sentY = 0
  let sentAt = 0

  const allowed = () => (enabled ? Boolean(enabled.value) : true)

  // Geometry comes from the element the event was bound to, never from a
  // remembered one: the same pad is drawn in the sidebar and again inside the
  // video in fullscreen, and a stored reference would leave one of them
  // computing its knob from the other's rectangle.
  function setFromPointer(event) {
    const rect = event.currentTarget?.getBoundingClientRect()
    if (!rect || !rect.width || !rect.height) return
    knob.x = clamp(((event.clientX - rect.left) / rect.width) * 2 - 1)
    knob.y = clamp(-(((event.clientY - rect.top) / rect.height) * 2 - 1))
  }

  function onPointerDown(event) {
    if (!allowed()) return
    engage()
    dragging.value = true
    pointerId = event.pointerId
    event.currentTarget?.setPointerCapture?.(event.pointerId)
    setFromPointer(event)
    pump()
  }

  function onPointerMove(event) {
    if (!dragging.value || event.pointerId !== pointerId) return
    setFromPointer(event)
    pump()
  }

  function onPointerUp(event) {
    if (event.pointerId !== pointerId) return
    dragging.value = false
    pointerId = null
    settle()
    pump()
  }

  function onKeyDown(event) {
    if (!DRIVE_KEYS[event.code]) return
    if (isTyping(event.target)) return
    if (event.metaKey || event.ctrlKey || event.altKey) return
    // Claim the keys whether or not we may drive, so arrow keys never scroll
    // the console out from under the operator.
    event.preventDefault()
    if (!allowed()) return
    engage()
    held.add(event.code)
    applyKeys()
    pump()
  }

  function onKeyUp(event) {
    if (!DRIVE_KEYS[event.code]) return
    if (held.delete(event.code)) {
      settle()
      pump()
    }
  }

  // A key released while the window is not focused never reports a keyup, so
  // treat losing focus as releasing everything.
  function releaseAll() {
    if (held.size === 0) return
    held.clear()
    settle()
    pump()
  }

  function onVisibility() {
    if (document.hidden) releaseAll()
  }

  function applyKeys() {
    if (dragging.value) return // the pointer wins while it is down
    let x = 0
    let y = 0
    for (const code of held) {
      const [dx, dy] = DRIVE_KEYS[code]
      x += dx
      y += dy
    }
    knob.x = clamp(x)
    knob.y = clamp(y)
  }

  function engage() {
    if (engaged.value) return
    engaged.value = true
    onEngage?.()
  }

  // Called whenever an input source lets go. Hands the knob back to whatever
  // is still held (releasing the pointer with W down keeps driving forward),
  // and only goes idle once nothing is driving at all.
  function settle() {
    applyKeys()
    if (dragging.value || held.size > 0) return
    engaged.value = false
  }

  // The one place a drive command leaves the console. Called straight from
  // every input handler, so a press is on the wire in the same event rather
  // than waiting for a tick, and from a timer, so a steady demand keeps
  // feeding the robot's watchdog.
  function pump() {
    const driving = allowed() && (dragging.value || held.size > 0)

    if (!driving) {
      // One zero frame on release so the robot stops promptly instead of
      // waiting out its watchdog. After that, silence.
      if (!stopped) {
        stopped = true
        sentAt = 0
        control.stop()
      }
      return
    }

    const now = performance.now()
    const changed = knob.x !== sentX || knob.y !== sentY
    const due = changed ? MIN_GAP_MS : REPEAT_MS
    if (now - sentAt < due) return

    stopped = false
    sentX = knob.x
    sentY = knob.y
    sentAt = now
    control.drive(knob.y, knob.x)
  }

  timer = setInterval(pump, MIN_GAP_MS)
  window.addEventListener('keydown', onKeyDown)
  window.addEventListener('keyup', onKeyUp)
  window.addEventListener('blur', releaseAll)
  document.addEventListener('visibilitychange', onVisibility)
  // Belt and braces for the drag: pointer capture should route the release back
  // to the pad, but if the pad ever goes away mid-drag its own handler never
  // runs and the robot would keep the last demand. onPointerUp ignores a
  // pointer it is not tracking, so the duplicate is harmless.
  window.addEventListener('pointerup', onPointerUp)
  window.addEventListener('pointercancel', onPointerUp)

  // Losing permission to drive mid-command (someone took the wheel, the relay
  // dropped) must not leave the robot with a stale demand.
  if (enabled) {
    watch(enabled, (may) => {
      if (!may) {
        held.clear()
        dragging.value = false
        pointerId = null
        knob.x = 0
        knob.y = 0
        engaged.value = false
        pump()
      }
    })
  }

  onUnmounted(() => {
    clearInterval(timer)
    window.removeEventListener('keydown', onKeyDown)
    window.removeEventListener('keyup', onKeyUp)
    window.removeEventListener('blur', releaseAll)
    window.removeEventListener('pointerup', onPointerUp)
    window.removeEventListener('pointercancel', onPointerUp)
    document.removeEventListener('visibilitychange', onVisibility)
  })

  return {
    knob,
    held,
    dragging,
    engaged,
    onPointerDown,
    onPointerMove,
    onPointerUp,
  }
}

function isTyping(target) {
  if (!target || !target.tagName) return false
  const tag = target.tagName.toLowerCase()
  return tag === 'input' || tag === 'textarea' || tag === 'select' || target.isContentEditable
}

function clamp(v) {
  return Math.max(-1, Math.min(1, v))
}
