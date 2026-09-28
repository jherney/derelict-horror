use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::{CursorGrabMode, PrimaryWindow};
use std::f32::consts::PI;
use std::sync::Arc;

// ------------------------------------------------------------- map definition
mod map {
    pub const SIZE: f32 = 3.5; // world units per cell
    pub const W: i32 = 15;
    pub const H: i32 = 13;
    pub const WALL_H: f32 = 3.0;

    pub const CELLS: &[&str] = &[
        "###############",
        "#S....#...C...#",
        "#.###.#.#####.#",
        "#...#.#...#...#",
        "###.#.###.#.###",
        "#...#.....#.#.#",
        "#.#######.#...#",
        "#C.#....#.#####",
        "#..#.A#.#...C.#",
        "##.#.##.###...#",
        "#..#....#..#..#",
        "#.#####.#..#.E#",
        "###############",
    ];

    pub fn cell(c: i32, r: i32) -> char {
        CELLS[r as usize].chars().nth(c as usize).unwrap_or('#')
    }

    pub fn is_wall(c: i32, r: i32) -> bool {
        if c < 0 || r < 0 || c >= W || r >= H {
            return true;
        }
        cell(c, r) == '#'
    }

    pub fn world_to_cell(x: f32, z: f32) -> (i32, i32) {
        (
            (x / SIZE + (W as f32 - 1.0) / 2.0).round() as i32,
            (z / SIZE + (H as f32 - 1.0) / 2.0).round() as i32,
        )
    }

    pub fn cell_to_world(c: i32, r: i32) -> (f32, f32) {
        (
            (c as f32 - (W as f32 - 1.0) / 2.0) * SIZE,
            (r as f32 - (H as f32 - 1.0) / 2.0) * SIZE,
        )
    }

    pub fn is_wall_at_world(x: f32, z: f32) -> bool {
        let (c, r) = world_to_cell(x, z);
        is_wall(c, r)
    }

    pub fn find_all(ch: char) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        for r in 0..H {
            for c in 0..W {
                if cell(c, r) == ch {
                    out.push((c, r));
                }
            }
        }
        out
    }
}

// ------------------------------------------------------------------- gameplay
const EYE_HEIGHT: f32 = 1.55;
const WALK_SPEED: f32 = 4.0;
const SPRINT_SPEED: f32 = 6.5;
const PLAYER_RADIUS: f32 = 0.45;
const MOUSE_SENS: f32 = 0.0022;

const CELL_PICKUP_DIST: f32 = 1.4;
const CELLS_TO_WIN: u32 = 3;
const POD_RADIUS: f32 = 2.2;

const ALIEN_SIGHT: f32 = 14.0;
const ALIEN_WANDER_SPEED: f32 = 1.4;
const ALIEN_HUNT_SPEED: f32 = 3.4;
const ALIEN_CATCH_DIST: f32 = 1.1;
const ALIEN_GIVE_UP: f32 = 18.0;

fn blocked(x: f32, z: f32) -> bool {
    for dx in [-PLAYER_RADIUS, 0.0, PLAYER_RADIUS] {
        for dz in [-PLAYER_RADIUS, 0.0, PLAYER_RADIUS] {
            if map::is_wall_at_world(x + dx, z + dz) {
                return true;
            }
        }
    }
    false
}

fn has_los(a: Vec3, b: Vec3) -> bool {
    let steps = (a.distance(b) / 0.5).max(1.0) as i32;
    for i in 1..steps {
        let p = a.lerp(b, i as f32 / steps as f32);
        if map::is_wall_at_world(p.x, p.z) {
            return false;
        }
    }
    true
}

// ------------------------------------------------------------------game state
#[derive(States, Default, Debug, Clone, PartialEq, Eq, Hash)]
enum GameState {
    #[default]
    Playing,
    Caught,
    Escaped,
}

// ------------------------------------------------------------------ components
#[derive(Component)]
struct PlayerRoot {
    yaw: f32,
    pitch: f32,
    bob: f32,
}

#[derive(Component)]
struct HeadCam;

#[derive(Component)]
struct Flashlight;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AlienMode {
    Wander,
    Hunt,
}

#[derive(Component)]
struct Alien {
    mode: AlienMode,
    target: (i32, i32),
}

#[derive(Component)]
struct CellPickup;

#[derive(Component)]
struct PodLight;

#[derive(Component)]
struct HudCells;

// Despawned & respawned when a round restarts.
#[derive(Component)]
struct Round;

// End-of-round UI.
#[derive(Component)]
struct Message;

// -------------------------------------------------------------------- resources
#[derive(Resource)]
struct Rng {
    state: u64,
}

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }
    fn roll(&mut self) -> f32 {
        (self.next() % 10_000) as f32 / 10_000.0
    }
}

#[derive(Resource, Clone)]
struct MeshBank {
    unit_box: Handle<Mesh>,
    sphere: Handle<Mesh>,
}

#[derive(Resource, Clone)]
struct MaterialBank {
    wall: Handle<StandardMaterial>,
    floor: Handle<StandardMaterial>,
    ceiling: Handle<StandardMaterial>,
    alien_skin: Handle<StandardMaterial>,
    alien_eye: Handle<StandardMaterial>,
    cell: Handle<StandardMaterial>,
    pod: Handle<StandardMaterial>,
}

#[derive(Resource)]
struct SoundBank {
    drone: Handle<AudioSource>,
    heartbeat: Handle<AudioSource>,
    skitter: Handle<AudioSource>,
    screech: Handle<AudioSource>,
    chime: Handle<AudioSource>,
    win: Handle<AudioSource>,
}

#[derive(Resource, Default)]
struct Collected(u32);

#[derive(Resource)]
struct Pulse(Timer);

// ------------------------------------------------------------------ textures
// Grimy metal panels: xorshift noise + seam lines, written straight into an
// Image. Same zero-asset trick as the sprite project.
fn noise_state(seed: u32) -> impl FnMut() -> f32 {
    let mut s = seed.max(1);
    move || {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        (s % 10_000) as f32 / 10_000.0
    }
}

fn panel_texture(size: u32, base: [f32; 3], seed: u32) -> Image {
    let mut rng = noise_state(seed);
    let mut data = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let n = rng();
            // darker blotches + panel seams every 16 px
            let seam = (x % 16 == 0) || (y % 16 == 0);
            let shade = (0.55 + n * 0.45) * if seam { 0.45 } else { 1.0 };
            let i = ((y * size + x) * 4) as usize;
            data[i] = (base[0] * shade) as u8;
            data[i + 1] = (base[1] * shade) as u8;
            data[i + 2] = (base[2] * shade) as u8;
            data[i + 3] = 255;
        }
    }
    let mut img = Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    img.sampler = bevy::image::ImageSampler::nearest();
    img
}

// ----------------------------------------------------------------- audio synth
const SAMPLE_RATE: u32 = 44100;

fn wav_bytes(samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

fn into_source(samples: Vec<i16>) -> AudioSource {
    AudioSource {
        bytes: Arc::from(wav_bytes(&samples).into_boxed_slice()),
    }
}

// 10-second low ship drone, tuned so every partial completes whole cycles
// inside the buffer -> loops seamlessly.
fn drone(vol: f32) -> Vec<i16> {
    let n = (SAMPLE_RATE * 10) as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / SAMPLE_RATE as f32;
        let lfo = 0.75 + 0.25 * (2.0 * PI * 0.2 * t).sin();
        let s = 0.55 * (2.0 * PI * 50.0 * t).sin()
            + 0.30 * (2.0 * PI * 75.0 * t).sin()
            + 0.15 * (2.0 * PI * 25.0 * t).sin();
        out.push((s * lfo * vol * 32767.0) as i16);
    }
    out
}

// lub-DUB over one second; played repeatedly with a gap scaled to distance.
fn heartbeat(vol: f32) -> Vec<i16> {
    let n = SAMPLE_RATE as usize;
    let mut out = Vec::with_capacity(n);
    let g = |t: f32, c: f32, w: f32| (-((t - c) / w).powi(2)).exp();
    for i in 0..n {
        let t = i as f32 / SAMPLE_RATE as f32;
        let thump = (2.0 * PI * 55.0 * t).sin() * g(t, 0.12, 0.035)
            + 0.65 * (2.0 * PI * 48.0 * t).sin() * g(t, 0.42, 0.045);
        out.push((thump * vol * 32767.0) as i16);
    }
    out
}

// Cluster of dry high-frequency clicks in dark corners.
fn skitter(vol: f32) -> Vec<i16> {
    let n = (SAMPLE_RATE as f32 * 0.45) as usize;
    let mut out = vec![0i16; n];
    let mut rng = noise_state(0xC3A5);
    let mut i = 0;
    while i < n {
        i += (rng() * n as f32 * 0.12) as usize;
        let freq = 900.0 + rng() * 1600.0;
        let len = (SAMPLE_RATE as f32 * 0.012) as usize;
        for k in 0..len.min(n.saturating_sub(i)) {
            let tt = k as f32 / SAMPLE_RATE as f32;
            let env = 1.0 - k as f32 / len as f32;
            out[i + k] = ((2.0 * PI * freq * tt).sin() * env * vol * 32767.0) as i16;
        }
        i += len;
    }
    out
}

// Harsh descending shriek on the kill.
fn screech(vol: f32) -> Vec<i16> {
    let n = (SAMPLE_RATE as f32 * 1.1) as usize;
    let mut out = Vec::with_capacity(n);
    let mut rng = noise_state(0x5EC1);
    let mut phase = 0.0f32;
    for i in 0..n {
        let frac = i as f32 / n as f32;
        let freq = 820.0 - 620.0 * frac;
        phase += 2.0 * PI * freq / SAMPLE_RATE as f32;
        let s = 0.7 * phase.sin() + 0.3 * (rng() * 2.0 - 1.0);
        let env = (1.0 - frac).powi(2);
        out.push((s * env * vol * 32767.0) as i16);
    }
    out
}

// Two soft rising notes (cell pickup).
fn chime(vol: f32) -> Vec<i16> {
    let per = (SAMPLE_RATE as f32 * 0.16) as usize;
    let mut out = Vec::with_capacity(per * 3);
    for (idx, &freq) in [660.0f32, 990.0].iter().enumerate() {
        let len = if idx == 1 { per * 2 } else { per };
        let mut phase = 0.0f32;
        for k in 0..len {
            phase += 2.0 * PI * freq / SAMPLE_RATE as f32;
            let env = (1.0 - k as f32 / len as f32).powi(2);
            out.push((phase.sin() * env * vol * 32767.0) as i16);
        }
    }
    out
}

// Rising arpeggio held at the top (escaped).
fn win_jingle(vol: f32) -> Vec<i16> {
    let per = (SAMPLE_RATE as f32 * 0.13) as usize;
    let notes = [329.63f32, 415.30, 554.37, 659.26];
    let mut out = Vec::with_capacity(per * notes.len() + per);
    let last = notes.len() - 1;
    for (idx, &freq) in notes.iter().enumerate() {
        let len = if idx == last { per * 2 } else { per };
        let mut phase = 0.0f32;
        for k in 0..len {
            phase += 2.0 * PI * freq / SAMPLE_RATE as f32;
            let env = if idx == last {
                (1.0 - k as f32 / len as f32).powi(2)
            } else {
                0.9
            };
            out.push((phase.sin() * env * vol * 32767.0) as i16);
        }
    }
    out
}

// ----------------------------------------------------------------------- setup
fn main() {
    App::new()
        .insert_resource(ClearColor(Color::BLACK))
        .insert_resource(AmbientLight {
            brightness: 6.0,
            ..default()
        })
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "DERELICT".into(),
                resolution: (1280., 720.).into(),
                ..default()
            }),
            ..default()
        }))
        .init_state::<GameState>()
        .init_resource::<Collected>()
        .insert_resource(Rng {
            state: 0x853C_49E6_748F_EA28,
        })
        .insert_resource(Pulse(Timer::from_seconds(1.2, TimerMode::Repeating)))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                player_look,
                player_move,
                flashlight_flicker,
                alien_ai,
                rotate_cells,
                pickups,
                pod_check,
                ambience,
                hud_update,
            )
                .run_if(in_state(GameState::Playing)),
        )
        .add_systems(Update, grab_cursor_on_click)
        .add_systems(Update, restart_game.run_if(not(in_state(GameState::Playing))))
        .add_systems(OnEnter(GameState::Caught), show_caught)
        .add_systems(OnEnter(GameState::Escaped), show_escaped)
        .run();
}

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut audio: ResMut<Assets<AudioSource>>,
) {
    // --- materials ----------------------------------------------------------
    let wall_tex = images.add(panel_texture(64, [150.0, 140.0, 132.0], 0x1FEA));
    let floor_tex = images.add(panel_texture(64, [120.0, 118.0, 118.0], 0xF100));
    let ceil_tex = images.add(panel_texture(64, [90.0, 88.0, 92.0], 0xC311));

    let mats = MaterialBank {
        wall: materials.add(StandardMaterial {
            base_color_texture: Some(wall_tex),
            perceptual_roughness: 0.85,
            ..default()
        }),
        floor: materials.add(StandardMaterial {
            base_color_texture: Some(floor_tex),
            perceptual_roughness: 0.95,
            ..default()
        }),
        ceiling: materials.add(StandardMaterial {
            base_color_texture: Some(ceil_tex),
            perceptual_roughness: 0.9,
            ..default()
        }),
        alien_skin: materials.add(StandardMaterial {
            base_color: Color::srgb(0.015, 0.015, 0.02),
            perceptual_roughness: 0.6,
            ..default()
        }),
        alien_eye: materials.add(StandardMaterial {
            base_color: Color::BLACK,
            emissive: LinearRgba::new(12.0, 0.15, 0.1, 1.0),
            ..default()
        }),
        cell: materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.9, 0.95),
            emissive: LinearRgba::new(0.5, 6.0, 7.0, 1.0),
            ..default()
        }),
        pod: materials.add(StandardMaterial {
            base_color: Color::srgb(0.35, 0.4, 0.45),
            metallic: 0.7,
            perceptual_roughness: 0.4,
            ..default()
        }),
    };

    let mesh_bank = MeshBank {
        unit_box: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        sphere: meshes.add(Sphere::new(0.5).mesh().uv(16, 12)),
    };

    let sounds = SoundBank {
        drone: audio.add(into_source(drone(0.35))),
        heartbeat: audio.add(into_source(heartbeat(0.5))),
        skitter: audio.add(into_source(skitter(0.35))),
        screech: audio.add(into_source(screech(0.7))),
        chime: audio.add(into_source(chime(0.45))),
        win: audio.add(into_source(win_jingle(0.5))),
    };

    // --- static environment (never respawned) -------------------------------
    build_environment(&mut commands, &mesh_bank, &mats);

    // --- round entities -----------------------------------------------------
    spawn_player(&mut commands);
    spawn_alien(&mut commands, &mesh_bank, &mats);
    spawn_cells(&mut commands, &mesh_bank, &mats);

    // --- HUD ----------------------------------------------------------------
    spawn_hud(&mut commands);

    // --- ambience loop -------------------------------------------------------
    commands.spawn((
        AudioPlayer::new(sounds.drone.clone()),
        PlaybackSettings::LOOP,
    ));

    commands.insert_resource(sounds);
    commands.insert_resource(mesh_bank);
    commands.insert_resource(mats);
}

fn build_environment(commands: &mut Commands, mesh_bank: &MeshBank, mats: &MaterialBank) {
    // floor + ceiling slabs
    let w = map::W as f32 * map::SIZE;
    let h = map::H as f32 * map::SIZE;
    for (y, mat) in [
        (-0.15, mats.floor.clone()),
        (map::WALL_H + 0.15, mats.ceiling.clone()),
    ] {
        commands.spawn((
            Mesh3d(mesh_bank.unit_box.clone()),
            MeshMaterial3d(mat),
            Transform::from_xyz(0.0, y, 0.0).with_scale(Vec3::new(w, 0.3, h)),
        ));
    }

    // walls
    for r in 0..map::H {
        for c in 0..map::W {
            if map::is_wall(c, r) {
                let (x, z) = map::cell_to_world(c, r);
                commands.spawn((
                    Mesh3d(mesh_bank.unit_box.clone()),
                    MeshMaterial3d(mats.wall.clone()),
                    Transform::from_xyz(x, map::WALL_H / 2.0, z)
                        .with_scale(Vec3::splat(map::SIZE).with_y(map::WALL_H)),
                ));
            }
        }
    }

    // sparse red emergency lights
    let mut placed = 0;
    for r in 0..map::H {
        for c in 0..map::W {
            if !map::is_wall(c, r) && (c * 7 + r * 13) % 9 == 0 && placed < 10 {
                let (x, z) = map::cell_to_world(c, r);
                commands.spawn((
                    PointLight {
                        color: Color::srgb(1.0, 0.08, 0.05),
                        intensity: 12_000.0,
                        range: 9.0,
                        ..default()
                    },
                    Transform::from_xyz(x, map::WALL_H - 0.35, z),
                ));
                placed += 1;
            }
        }
    }

    // escape pod: doorframe + beacon light
    if let Some(&(c, r)) = map::find_all('E').first() {
        let (x, z) = map::cell_to_world(c, r);
        for offset in [-1.1, 1.1] {
            commands.spawn((
                Mesh3d(mesh_bank.unit_box.clone()),
                MeshMaterial3d(mats.pod.clone()),
                Transform::from_xyz(x + offset, 1.5, z).with_scale(Vec3::new(0.3, 3.0, 0.3)),
            ));
        }
        commands.spawn((
            Mesh3d(mesh_bank.unit_box.clone()),
            MeshMaterial3d(mats.pod.clone()),
            Transform::from_xyz(x, 2.9, z).with_scale(Vec3::new(2.5, 0.3, 0.5)),
        ));
        commands.spawn((
            PointLight {
                color: Color::srgb(1.0, 0.15, 0.1),
                intensity: 25_000.0,
                range: 14.0,
                ..default()
            },
            Transform::from_xyz(x, 2.2, z),
            PodLight,
        ));
    }
}

fn spawn_player(commands: &mut Commands) {
    let (c, r) = map::find_all('S').first().copied().unwrap_or((1, 1));
    let (x, z) = map::cell_to_world(c, r);

    commands
        .spawn((
            PlayerRoot {
                yaw: 0.0,
                pitch: 0.0,
                bob: 0.0,
            },
            Transform::from_xyz(x, 0.0, z),
            GlobalTransform::default(),
            Round,
        ))
        .with_children(|root| {
            root.spawn((
                Camera3d::default(),
                Transform::from_xyz(0.0, EYE_HEIGHT, 0.0),
                HeadCam,
            ))
            .with_children(|cam| {
                cam.spawn((
                    SpotLight {
                        color: Color::srgb(1.0, 0.95, 0.85),
                        intensity: 420_000.0,
                        range: 26.0,
                        inner_angle: 0.35,
                        outer_angle: 0.55,
                        shadows_enabled: true,
                        ..default()
                    },
                    Transform::from_xyz(0.22, -0.18, 0.0)
                        .with_rotation(Quat::from_rotation_x(-0.05)),
                    Flashlight,
                ));
            });
        });
}

fn spawn_alien(commands: &mut Commands, mesh_bank: &MeshBank, mats: &MaterialBank) {
    let (c, r) = map::find_all('A').first().copied().unwrap_or((5, 5));
    let (x, z) = map::cell_to_world(c, r);

    commands
        .spawn((
            Alien {
                mode: AlienMode::Wander,
                target: (c, r),
            },
            Transform::from_xyz(x, 0.0, z),
            GlobalTransform::default(),
            Round,
        ))
        .with_children(|root| {
            // torso
            root.spawn((
                Mesh3d(mesh_bank.unit_box.clone()),
                MeshMaterial3d(mats.alien_skin.clone()),
                Transform::from_xyz(0.0, 1.05, 0.0).with_scale(Vec3::new(0.7, 1.7, 0.45)),
            ));
            // head
            root.spawn((
                Mesh3d(mesh_bank.unit_box.clone()),
                MeshMaterial3d(mats.alien_skin.clone()),
                Transform::from_xyz(0.0, 2.05, 0.05).with_scale(Vec3::new(0.45, 0.42, 0.35)),
            ));
            // eyes
            for off in [-0.12, 0.12] {
                root.spawn((
                    Mesh3d(mesh_bank.sphere.clone()),
                    MeshMaterial3d(mats.alien_eye.clone()),
                    Transform::from_xyz(off, 2.1, 0.24).with_scale(Vec3::splat(0.09)),
                ));
            }
            // faint red glow so you can glimpse it in the dark
            root.spawn((
                PointLight {
                    color: Color::srgb(0.9, 0.05, 0.05),
                    intensity: 2_500.0,
                    range: 5.0,
                    ..default()
                },
                Transform::from_xyz(0.0, 1.6, 0.0),
            ));
        });
}

fn spawn_cells(commands: &mut Commands, mesh_bank: &MeshBank, mats: &MaterialBank) {
    for (c, r) in map::find_all('C') {
        let (x, z) = map::cell_to_world(c, r);
        commands
            .spawn((
                Mesh3d(mesh_bank.unit_box.clone()),
                MeshMaterial3d(mats.cell.clone()),
                Transform::from_xyz(x, 0.9, z).with_scale(Vec3::splat(0.28)),
                CellPickup,
                Round,
            ))
            .with_children(|p| {
                p.spawn((
                    PointLight {
                        color: Color::srgb(0.3, 1.0, 1.0),
                        intensity: 7_000.0,
                        range: 7.0,
                        ..default()
                    },
                    Transform::from_xyz(0.0, 2.5, 0.0),
                ));
            });
    }
}

fn spawn_hud(commands: &mut Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(16.0),
            top: Val::Px(14.0),
            ..default()
        },
        Text::new("CELLS 0/3"),
        TextFont {
            font_size: 22.0,
            ..default()
        },
        TextColor(Color::srgb(0.7, 0.85, 0.9)),
        HudCells,
    ));
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(16.0),
            bottom: Val::Px(14.0),
            ..default()
        },
        Text::new("WASD move · SHIFT sprint · find 3 cells, reach the pod"),
        TextFont {
            font_size: 14.0,
            ..default()
        },
        TextColor(Color::srgba(0.7, 0.75, 0.8, 0.6)),
    ));
}

// ---------------------------------------------------------------------- cursor
fn grab_cursor_on_click(
    mouse: Res<ButtonInput<MouseButton>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if mouse.just_pressed(MouseButton::Left) {
        if let Ok(mut w) = windows.single_mut() {
            w.cursor_options.grab_mode = CursorGrabMode::Confined;
            w.cursor_options.visible = false;
        }
    }
}

// ---------------------------------------------------------------------- player
fn player_look(
    motion: Res<AccumulatedMouseMotion>,
    mut root_q: Query<(&mut PlayerRoot, &mut Transform), Without<Camera3d>>,
    mut cam_q: Query<&mut Transform, (With<Camera3d>, Without<PlayerRoot>)>,
) {
    let Ok((mut player, mut root_t)) = root_q.single_mut() else {
        return;
    };
    let d = motion.delta;
    player.yaw -= d.x * MOUSE_SENS;
    player.pitch = (player.pitch - d.y * MOUSE_SENS).clamp(-1.45, 1.45);
    root_t.rotation = Quat::from_rotation_y(player.yaw);
    if let Ok(mut cam_t) = cam_q.single_mut() {
        cam_t.rotation = Quat::from_rotation_x(player.pitch);
    }
}

fn player_move(
    kb: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut root_q: Query<(&mut PlayerRoot, &mut Transform), Without<Camera3d>>,
    mut cam_q: Query<&mut Transform, (With<Camera3d>, Without<PlayerRoot>)>,
) {
    let Ok((mut player, mut root_t)) = root_q.single_mut() else {
        return;
    };
    let mut wish = Vec2::ZERO;
    if kb.pressed(KeyCode::KeyW) {
        wish.y += 1.0;
    }
    if kb.pressed(KeyCode::KeyS) {
        wish.y -= 1.0;
    }
    if kb.pressed(KeyCode::KeyA) {
        wish.x -= 1.0;
    }
    if kb.pressed(KeyCode::KeyD) {
        wish.x += 1.0;
    }

    let mut moving = false;
    if wish.length_squared() > 0.0 {
        moving = true;
        wish = wish.normalize();
        let speed = if kb.pressed(KeyCode::ShiftLeft) {
            SPRINT_SPEED
        } else {
            WALK_SPEED
        };
        let yaw = player.yaw;
        let fwd = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
        let right = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
        let step = (fwd * wish.y + right * wish.x) * speed * time.delta_secs();

        let p = root_t.translation;
        if !blocked(p.x + step.x, p.z) {
            root_t.translation.x += step.x;
        }
        if !blocked(root_t.translation.x, p.z + step.z) {
            root_t.translation.z += step.z;
        }
        player.bob += speed * time.delta_secs();
    }

    // subtle head-bob while moving
    if let Ok(mut cam_t) = cam_q.single_mut() {
        let lift = if moving { (player.bob * 7.5).sin() * 0.035 } else { 0.0 };
        cam_t.translation.y = EYE_HEIGHT + lift;
    }
}

fn flashlight_flicker(mut rng: ResMut<Rng>, mut q: Query<&mut SpotLight, With<Flashlight>>) {
    for mut light in &mut q {
        let roll = rng.roll();
        light.intensity = if roll < 0.04 {
            45_000.0 // brown-out dip
        } else if roll < 0.06 {
            620_000.0 // overvoltage strobe
        } else {
            420_000.0
        };
    }
}

// ----------------------------------------------------------------------- alien
fn alien_ai(
    time: Res<Time>,
    mut rng: ResMut<Rng>,
    mut commands: Commands,
    sounds: Res<SoundBank>,
    player_q: Query<&Transform, (With<PlayerRoot>, Without<Alien>)>,
    mut alien_q: Query<(&mut Alien, &mut Transform), Without<PlayerRoot>>,
    mut next: ResMut<NextState<GameState>>,
) {
    let (Ok(p_t), Ok((mut alien, mut a_t))) = (player_q.single(), alien_q.single_mut())
    else {
        return;
    };

    let dist = p_t.translation.distance(a_t.translation);
    let los = has_los(a_t.translation, p_t.translation);

    // mode switching
    match alien.mode {
        AlienMode::Wander => {
            if dist < ALIEN_SIGHT && los {
                alien.mode = AlienMode::Hunt;
            }
        }
        AlienMode::Hunt => {
            if dist > ALIEN_GIVE_UP || !los {
                alien.mode = AlienMode::Wander;
                let (ac, ar) = map::world_to_cell(a_t.translation.x, a_t.translation.z);
                alien.target = (ac, ar);
            }
        }
    }

    let dt = time.delta_secs();
    match alien.mode {
        AlienMode::Wander => {
            let (tx, tz) = map::cell_to_world(alien.target.0, alien.target.1);
            let to = Vec2::new(tx - a_t.translation.x, tz - a_t.translation.z);
            if to.length() < 0.3 {
                // pick a random open neighbour
                let (c, r) = alien.target;
                let mut options = Vec::new();
                for (dc, dr) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    if !map::is_wall(c + dc, r + dr) {
                        options.push((c + dc, r + dr));
                    }
                }
                if !options.is_empty() {
                    let pick = (rng.next() as usize) % options.len();
                    alien.target = options[pick];
                }
            } else {
                let dir = to.normalize();
                move_actor(&mut a_t, dir, ALIEN_WANDER_SPEED, dt);
                a_t.look_at(
                    Vec3::new(p_t.translation.x, 0.0, p_t.translation.z) * 0.0
                        + Vec3::new(a_t.translation.x + dir.x, 0.0, a_t.translation.z + dir.y),
                    Vec3::Y,
                );
            }
        }
        AlienMode::Hunt => {
            let to = Vec2::new(
                p_t.translation.x - a_t.translation.x,
                p_t.translation.z - a_t.translation.z,
            );
            if dist > 0.01 {
                let dir = to.normalize();
                move_actor(&mut a_t, dir, ALIEN_HUNT_SPEED, dt);
                a_t.look_at(
                    Vec3::new(p_t.translation.x, 0.0, p_t.translation.z),
                    Vec3::Y,
                );
            }
            if dist < ALIEN_CATCH_DIST {
                commands.spawn(AudioPlayer::new(sounds.screech.clone()));
                next.set(GameState::Caught);
            }
        }
    }
}

fn move_actor(t: &mut Transform, dir: Vec2, speed: f32, dt: f32) {
    let step = dir * speed * dt;
    if !blocked(t.translation.x + step.x, t.translation.z) {
        t.translation.x += step.x;
    }
    if !blocked(t.translation.x, t.translation.z + step.y) {
        t.translation.z += step.y;
    }
}

// -------------------------------------------------------------------- pickups
fn rotate_cells(time: Res<Time>, mut q: Query<&mut Transform, With<CellPickup>>) {
    for mut t in &mut q {
        t.rotate_y(1.2 * time.delta_secs());
        t.translation.y = 0.9 + (time.elapsed_secs() * 2.0 + t.translation.x).sin() * 0.1;
    }
}

fn pickups(
    mut commands: Commands,
    player_q: Query<&Transform, With<PlayerRoot>>,
    cells: Query<(Entity, &Transform), (With<CellPickup>, Without<PlayerRoot>)>,
    mut collected: ResMut<Collected>,
    sounds: Res<SoundBank>,
) {
    let Ok(p_t) = player_q.single() else {
        return;
    };
    for (e, t) in &cells {
        let d = Vec2::new(t.translation.x - p_t.translation.x, t.translation.z - p_t.translation.z)
            .length();
        if d < CELL_PICKUP_DIST {
            if let Ok(ec) = commands.get_entity(e) {
                ec.despawn();
            }
            collected.0 += 1;
            commands.spawn(AudioPlayer::new(sounds.chime.clone()));
        }
    }
}

fn pod_check(
    player_q: Query<&Transform, With<PlayerRoot>>,
    mut pod: Query<(&Transform, &mut PointLight), (With<PodLight>, Without<PlayerRoot>)>,
    collected: Res<Collected>,
    mut commands: Commands,
    sounds: Res<SoundBank>,
    mut next: ResMut<NextState<GameState>>,
) {
    let (Ok(p_t), Ok((pod_t, mut light))) = (player_q.single(), pod.single_mut()) else {
        return;
    };

    let powered = collected.0 >= CELLS_TO_WIN;
    light.color = if powered {
        Color::srgb(0.2, 1.0, 0.35)
    } else {
        Color::srgb(1.0, 0.15, 0.1)
    };

    let d = p_t.translation.distance(pod_t.translation);
    if powered && d < POD_RADIUS {
        commands.spawn(AudioPlayer::new(sounds.win.clone()));
        next.set(GameState::Escaped);
    }
}

// -------------------------------------------------------------------- ambience
fn ambience(
    time: Res<Time>,
    mut pulse: ResMut<Pulse>,
    mut rng: ResMut<Rng>,
    mut commands: Commands,
    sounds: Res<SoundBank>,
    player_q: Query<&Transform, With<PlayerRoot>>,
    alien_q: Query<&Transform, (With<Alien>, Without<PlayerRoot>)>,
) {
    let (Ok(p_t), Ok(a_t)) = (player_q.single(), alien_q.single()) else {
        return;
    };
    let dist = p_t.translation.distance(a_t.translation);

    pulse.0.tick(time.delta());
    if pulse.0.just_finished() && dist < 12.0 {
        commands.spawn(AudioPlayer::new(sounds.heartbeat.clone()));
        // closer = faster heartbeat
        let gap = 0.45 + (dist / 12.0).clamp(0.0, 1.0) * 1.3;
        pulse.0.set_duration(std::time::Duration::from_secs_f32(gap));
        pulse.0.reset();
    }

    // occasional skitter when it's near or watching you
    if dist < 14.0 && has_los(a_t.translation, p_t.translation) && rng.roll() < 0.004 {
        commands.spawn(AudioPlayer::new(sounds.skitter.clone()));
    }
}

// ------------------------------------------------------------------------- HUD
fn hud_update(collected: Res<Collected>, mut q: Query<&mut Text, With<HudCells>>) {
    if collected.is_changed() {
        for mut text in &mut q {
            text.0 = format!("CELLS {}/{}", collected.0, CELLS_TO_WIN);
        }
    }
}

// ------------------------------------------------------------------ end screens
fn end_screen(commands: &mut Commands, lines: &[&str], color: Color, tint: Color) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(14.0),
                ..default()
            },
            BackgroundColor(tint),
            Message,
        ))
        .with_children(|parent| {
            for (i, line) in lines.iter().enumerate() {
                parent.spawn((
                    Text::new(*line),
                    TextFont {
                        font_size: if i == 0 { 52.0 } else { 20.0 },
                        ..default()
                    },
                    TextColor(if i == 0 {
                        color
                    } else {
                        Color::srgb(0.75, 0.75, 0.8)
                    }),
                ));
            }
        });
}

fn show_caught(mut commands: Commands) {
    end_screen(
        &mut commands,
        &["IT FOUND YOU", "The ship goes quiet. — ENTER to try again"],
        Color::srgb(1.0, 0.25, 0.2),
        Color::srgba(0.28, 0.0, 0.0, 0.55),
    );
}

fn show_escaped(mut commands: Commands) {
    end_screen(
        &mut commands,
        &["YOU ESCAPED", "The pod detaches into the black. — ENTER to play again"],
        Color::srgb(0.35, 1.0, 0.5),
        Color::srgba(0.0, 0.2, 0.08, 0.5),
    );
}

fn restart_game(
    mut commands: Commands,
    kb: Res<ButtonInput<KeyCode>>,
    round: Query<Entity, Or<(With<Round>, With<Message>)>>,
    mesh_bank: Res<MeshBank>,
    mats: Res<MaterialBank>,
    mut collected: ResMut<Collected>,
    mut next: ResMut<NextState<GameState>>,
) {
    if !kb.just_pressed(KeyCode::Enter) {
        return;
    }
    for e in &round {
        if let Ok(ec) = commands.get_entity(e) {
            ec.despawn();
        }
    }
    collected.0 = 0;
    spawn_player(&mut commands);
    spawn_alien(&mut commands, &mesh_bank, &mats);
    spawn_cells(&mut commands, &mesh_bank, &mats);
    next.set(GameState::Playing);
}
