# DERELICT

A first-person 3D alien horror game in Rust + Bevy. You wake up on a derelict
ship: the lights are dying, something is stalking the corridors, and the only
way out is the escape pod.

Everything is generated in code. There are no binary assets in this repo — the
corridor geometry is built from primitives, the rusted-wall textures are
synthesized noise, and every sound (drone, heartbeat, skitters, screech) is
written out as raw PCM WAV at startup.

## Objective

Collect all 3 power cells scattered through the corridors (they glow cyan),
then reach the escape pod. The pod beacon turns green once the ship has
enough power. If the alien catches you, it's over.

The alien wanders the ship, but if it gets line of sight on you within ~14 m
it will hunt. Sprint if you have to. Listen: your heartbeat gets faster as it
closes in, and you'll hear it skitter when it's close.

## Controls

| Input | Action |
|---|---|
| Mouse | Look (click the window to capture the cursor) |
| W / A / S / D | Move |
| Shift | Sprint |
| Enter | Restart after caught / escaped |
| Esc | Release the cursor |

## Run it

```bash
cargo run
```

First build compiles the whole Bevy engine from source — expect a few
minutes. On Linux you may need the usual Bevy system deps first
(`libasound2-dev libudev-dev libx11-dev libwayland-dev xorg-dev` or the
Wayland equivalents; see <https://bevyengine.org/learn/quick-start/getting-started/setup/>).

## How it's built

- **World** — a hand-authored ASCII map (`map::CELLS` in `src/main.rs`) is
  turned into walls, a floor, a ceiling, red emergency lights, cell pickups,
  and an escape-pod doorframe.
- **Textures** — a xorshift-based noise function writes RGBA data straight
  into a Bevy `Image` with nearest sampling, so the walls read as grimy
  panels without any image files.
- **Audio** — samples are synthesized (sweeps, filtered noise, pulse
  envelopes), wrapped in a PCM WAV container, and decoded by Bevy's `wav`
  feature. The looping drone is tuned to a whole number of cycles so it
  loops seamlessly.
- **Alien AI** — two modes: wander (random neighbouring-cell targets) and
  hunt (direct pursuit when it has line of sight).

## Roadmap

- [ ] Multiple maps / procedurally generated layouts
- [ ] Alien limb animation and a proper model silhouette
- [ ] Hiding mechanics (lockers, vents)
- [ ] Stamina for sprinting
- [ ] Footstep sounds (yours and its), positional audio tuning
- [ ] WASM browser build
- [ ] Settings menu (sensitivity, volume, brightness)
- [ ] Jump-scare variants: false alarms, dead ends, vents opening
