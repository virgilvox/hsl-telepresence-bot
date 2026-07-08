// Single shared CLASP connection for the whole console. All other composables
// read `client` from here rather than opening their own connections.
//
// The SDK's default export is a `clasp(url, options)` factory that returns a
// connected EasyClient. Options we use: name, token, reconnect.

import { ref, shallowRef } from 'vue'
import clasp from '@clasp-to/sdk'

const client = shallowRef(null)
const connected = ref(false)
const connecting = ref(false)
const sessionId = ref(null)
const error = ref(null)

async function connect({ url, name, token }) {
  if (connecting.value) return
  await disconnect()
  connecting.value = true
  error.value = null
  try {
    const c = await clasp(url, {
      name: name || 'operator',
      token: token || undefined,
      reconnect: true,
    })
    client.value = c
    sessionId.value = c.session
    connected.value = true

    c.onDisconnect?.(() => {
      connected.value = false
    })
    c.onReconnect?.(() => {
      connected.value = true
      sessionId.value = c.session
    })
    c.onError?.((err) => {
      error.value = err?.message || String(err)
    })
  } catch (err) {
    error.value = err?.message || String(err)
    connected.value = false
    client.value = null
    throw err
  } finally {
    connecting.value = false
  }
}

async function disconnect() {
  if (client.value) {
    try {
      await client.value.close?.()
    } catch {
      // Closing a dead socket is not worth surfacing.
    }
  }
  client.value = null
  connected.value = false
  sessionId.value = null
}

export function useClasp() {
  return { client, connected, connecting, sessionId, error, connect, disconnect }
}
