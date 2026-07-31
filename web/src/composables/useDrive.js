// Teleop input: the drive pad's pointer drag and the keyboard, folded into one
// throttle/steer demand and sent to the robot at a fixed rate.
//
// This lives outside the pad component on purpose. The pad is drawn in the
// sidebar normally and inside the video in fullscreen, and only one thing may
// own the key listeners and the send timer: two of them would double every
// command and fight over the knob.
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

const SEND_HZ = 15

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
  }

  function onPointerMove(event) {
    if (dragging.value && event.pointerId === pointerId) setFromPointer(event)
  }

  function onPointerUp(event) {
    if (event.pointerId !== pointerId) return
    dragging.value = false
    pointerId = null
    settle()
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
  }

  function onKeyUp(event) {
    if (!DRIVE_KEYS[event.code]) return
    if (held.delete(event.code)) settle()
  }

  // A key released while the window is not focused never reports a keyup, so
  // treat losing focus as releasing everything.
  function releaseAll() {
    if (held.size === 0) return
    held.clear()
    settle()
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

  function tick() {
    const driving = allowed() && (dragging.value || held.size > 0)
    if (driving) {
      stopped = false
      control.drive(knob.y, knob.x)
      return
    }
    // One zero frame on release so the robot stops promptly instead of waiting
    // out its watchdog. After that, silence.
    if (!stopped) {
      stopped = true
      control.stop()
    }
  }

  timer = setInterval(tick, 1000 / SEND_HZ)
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
      }
    })
  }

  onUnmounted(() => {
    clearInterval(timer)
    window.removeEventListener('keydown', onKeyDown)
    window.removeEventListener('keyup', onKeyUp)
    window.removeEventListener('blur', releaseAll)
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
