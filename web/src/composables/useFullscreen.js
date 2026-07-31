// Fullscreen for one element, with the state read back from the document
// rather than assumed. The user can leave fullscreen with Escape or the
// browser's own chrome, which never tells us directly, so tracking a boolean of
// our own would drift out of sync with what is actually on screen.

import { onMounted, onUnmounted, ref } from 'vue'

export function useFullscreen(target) {
  const isFullscreen = ref(false)
  const supported =
    typeof document !== 'undefined' &&
    Boolean(document.fullscreenEnabled || document.webkitFullscreenEnabled)

  function currentElement() {
    return document.fullscreenElement || document.webkitFullscreenElement || null
  }

  function sync() {
    isFullscreen.value = Boolean(target.value) && currentElement() === target.value
  }

  async function enter() {
    const el = target.value
    if (!el) return
    try {
      // Safari still only has the prefixed form.
      await (el.requestFullscreen?.() ?? el.webkitRequestFullscreen?.())
    } catch (err) {
      // Rejected when the call did not come from a user gesture. Nothing to
      // recover, and it must not take the console down.
      console.warn('fullscreen request refused', err)
    }
    sync()
  }

  async function exit() {
    if (!currentElement()) return
    try {
      await (document.exitFullscreen?.() ?? document.webkitExitFullscreen?.())
    } catch (err) {
      console.warn('leaving fullscreen failed', err)
    }
    sync()
  }

  function toggle() {
    return isFullscreen.value ? exit() : enter()
  }

  onMounted(() => {
    document.addEventListener('fullscreenchange', sync)
    document.addEventListener('webkitfullscreenchange', sync)
    sync()
  })

  onUnmounted(() => {
    document.removeEventListener('fullscreenchange', sync)
    document.removeEventListener('webkitfullscreenchange', sync)
  })

  return { isFullscreen, supported, enter, exit, toggle }
}
