// A room, a robot in it, and a camera view out of its eyes.
//
// Deliberately crude: segment raycasting against a handful of walls, drawn
// twice with a horizontal offset to make the same side-by-side stereo frame the
// real camera produces. It exists so the console has something to drive and
// something to look at, not to be a simulator worth believing.

// Metres. The robot is roughly the footprint of the real chassis.
const MAX_SPEED = 0.65 // m/s at full throttle
const TRACK = 0.26 // wheel separation, sets how fast it spins
const RADIUS = 0.2

// The camera: 1280x480 side by side, matching the mode the Pi captures.
export const FRAME_W = 1280
export const FRAME_H = 480
const EYE_W = FRAME_W / 2
const FOV = 1.2 // radians, roughly 69 degrees
const EYE_SEP = 0.06

// Walls, as coloured segments. The colours are only there so that turning is
// obvious at a glance.
const ROOM = 8
const walls = [
  seg(0, 0, ROOM, 0, [58, 62, 70]),
  seg(ROOM, 0, ROOM, ROOM * 0.75, [42, 171, 138]),
  seg(ROOM, ROOM * 0.75, 0, ROOM * 0.75, [58, 62, 70]),
  seg(0, ROOM * 0.75, 0, 0, [224, 130, 69]),
  // A couple of freestanding blocks, so there is depth to drive around.
  ...box(2.2, 1.6, 0.9, 0.9, [90, 96, 108]),
  ...box(5.4, 3.4, 1.1, 0.7, [90, 96, 108]),
  ...box(3.6, 4.6, 0.6, 0.6, [176, 84, 120]),
]

function seg(ax, ay, bx, by, colour) {
  return { ax, ay, bx, by, colour }
}

function box(x, y, w, h, colour) {
  return [
    seg(x, y, x + w, y, colour),
    seg(x + w, y, x + w, y + h, colour),
    seg(x + w, y + h, x, y + h, colour),
    seg(x, y + h, x, y, colour),
  ]
}

export function createWorld() {
  const pose = { x: 1.2, y: 1.2, th: 0.6 }

  // Integrate a differential drive. `left` and `right` are the same normalised
  // wheel demands the motion plane applies, so driving the simulator wrong in
  // the same way as the robot is at least consistent.
  function step(dt, left, right) {
    const v = ((left + right) / 2) * MAX_SPEED
    const omega = ((right - left) / TRACK) * MAX_SPEED

    pose.th += omega * dt
    const nx = pose.x + Math.cos(pose.th) * v * dt
    const ny = pose.y + Math.sin(pose.th) * v * dt

    // Refuse the translation rather than resolving it. Sliding along a wall
    // would be nicer to drive and would also quietly hide a console that is
    // sending the wrong thing.
    if (!blocked(nx, ny)) {
      pose.x = nx
      pose.y = ny
    }
  }

  function blocked(x, y) {
    for (const w of walls) {
      if (distanceToSegment(x, y, w) < RADIUS) return true
    }
    return false
  }

  return { pose, step }
}

// The camera frame: the same view drawn twice with a lateral offset, which is
// the side-by-side stereo pair the real camera produces.
//
// Drawn from a pose rather than owned by the simulation, because the two live
// on different threads: the physics runs in a worker (whose timers a hidden
// page does not throttle) while the drawing has to happen on the page's own
// thread (because that is the only canvas a captured MediaStream follows).
export function renderCamera(ctx, pose) {
  // Left and right eye, offset along the robot's lateral axis.
  const lx = -Math.sin(pose.th) * (EYE_SEP / 2)
  const ly = Math.cos(pose.th) * (EYE_SEP / 2)
  renderEye(ctx, pose, 0, pose.x - lx, pose.y - ly, 'L')
  renderEye(ctx, pose, EYE_W, pose.x + lx, pose.y + ly, 'R')

  // A timestamp burned into the frame: the one honest way to see, from the
  // console, that the picture is live and roughly how far behind it is.
  ctx.font = '600 15px ui-monospace, monospace'
  ctx.fillStyle = 'rgba(255,255,255,0.85)'
  ctx.textAlign = 'right'
  ctx.fillText(new Date().toISOString().slice(11, 23), FRAME_W - 10, FRAME_H - 10)
  ctx.textAlign = 'left'
}

function renderEye(ctx, pose, originX, camX, camY, label) {
  const dirX = Math.cos(pose.th)
  const dirY = Math.sin(pose.th)
  // Camera plane, perpendicular to the view direction. Leaving the ray
  // unnormalised is what makes the returned distance perpendicular to the
  // plane instead of radial, which is the difference between flat walls and
  // a fisheye.
  const planeLen = Math.tan(FOV / 2)
  const planeX = -dirY * planeLen
  const planeY = dirX * planeLen

  const horizon = FRAME_H / 2
  ctx.fillStyle = '#0e0f12'
  ctx.fillRect(originX, 0, EYE_W, horizon)
  ctx.fillStyle = '#191b1f'
  ctx.fillRect(originX, horizon, EYE_W, FRAME_H - horizon)

  for (let sx = 0; sx < EYE_W; sx++) {
    const cx = (2 * sx) / EYE_W - 1
    const rayX = dirX + planeX * cx
    const rayY = dirY + planeY * cx

    const hit = cast(camX, camY, rayX, rayY)
    if (!hit) continue

    const height = Math.min(FRAME_H * 3, FRAME_H / Math.max(hit.dist, 0.05))
    const top = horizon - height / 2
    // Fall off with distance so depth reads, and darken the faces that face
    // away so corners are visible.
    const shade = Math.max(0.12, Math.min(1, 1.9 / (1 + hit.dist))) * (hit.flip ? 0.72 : 1)
    const [r, g, b] = hit.colour
    ctx.fillStyle = `rgb(${(r * shade) | 0},${(g * shade) | 0},${(b * shade) | 0})`
    ctx.fillRect(originX + sx, top, 1, height)
  }

  ctx.font = '600 14px ui-monospace, monospace'
  ctx.fillStyle = 'rgba(255,255,255,0.5)'
  ctx.fillText(label, originX + 10, 22)
}

// Top-down view for the simulator's own page, drawn from a pose. Never part of
// the video, so it lives outside the simulation and can be drawn by whoever
// happens to be holding a pose.
export function drawMap(ctx, w, h, pose) {
  const scale = Math.min(w / ROOM, h / (ROOM * 0.75))
  ctx.clearRect(0, 0, w, h)
  ctx.save()
  ctx.translate(0, h)
  ctx.scale(scale, -scale)
  ctx.lineWidth = 1.5 / scale

  for (const seg of walls) {
    ctx.strokeStyle = `rgb(${seg.colour.join(',')})`
    ctx.beginPath()
    ctx.moveTo(seg.ax, seg.ay)
    ctx.lineTo(seg.bx, seg.by)
    ctx.stroke()
  }

  ctx.fillStyle = '#4dcaa8'
  ctx.beginPath()
  ctx.arc(pose.x, pose.y, RADIUS, 0, Math.PI * 2)
  ctx.fill()

  ctx.strokeStyle = '#e8e8e8'
  ctx.beginPath()
  ctx.moveTo(pose.x, pose.y)
  ctx.lineTo(pose.x + Math.cos(pose.th) * 0.45, pose.y + Math.sin(pose.th) * 0.45)
  ctx.stroke()
  ctx.restore()
}

// Nearest wall along the ray, or null. Standard segment intersection: the ray
// is p + t*r and the wall is a + u*s, so the cross products give both
// parameters directly.
function cast(px, py, rx, ry) {
  let best = null
  for (const w of walls) {
    const sx = w.bx - w.ax
    const sy = w.by - w.ay
    const denom = rx * sy - ry * sx
    if (denom === 0) continue // parallel
    const qx = w.ax - px
    const qy = w.ay - py
    const t = (qx * sy - qy * sx) / denom
    const u = (qx * ry - qy * rx) / denom
    if (t <= 0 || u < 0 || u > 1) continue
    if (!best || t < best.dist) {
      best = { dist: t, colour: w.colour, flip: denom > 0 }
    }
  }
  return best
}

function distanceToSegment(px, py, w) {
  const dx = w.bx - w.ax
  const dy = w.by - w.ay
  const len2 = dx * dx + dy * dy
  const t = len2 === 0 ? 0 : Math.max(0, Math.min(1, ((px - w.ax) * dx + (py - w.ay) * dy) / len2))
  const cx = w.ax + t * dx
  const cy = w.ay + t * dy
  return Math.hypot(px - cx, py - cy)
}
