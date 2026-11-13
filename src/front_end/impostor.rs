//! Planet impostors of the galaxy screen: every saved game's star shows its
//! homeworld, rendered from the game's own terrain (the planet engine runs the
//! homeworld's terrain script, `planet_gen`), as the retail galaxy screen shows
//! the saved planets.
//!
//! Retail (VERIFIED, `GalaxyGameEntry` star UI 0xDF6FF9 / 0xE21020, the notes
//! of helper `fe-planet3d`): the galaxy screen does not draw a flat picture.
//! The star's state code asks the planet impostor manager (0xB3D460) for the
//! impostor of the star's first planet record (`cStarRecord::GetPlanetRecord`
//! 0xBBABD0 -> `GetGeneratedTerrainKey` 0xB8DA30 -> 0xB7D640 / 0xB7D590) and
//! keeps the pair of models it returns at the entry + 0x20 (0xE21240): the
//! SAME two models the Space stage draws, "planetImpostor" and
//! "planetAtmoImpostor" (`space_planets::impostor`, programs planetImpostorVS /
//! PS and planetAtmoImpostorVS / PerPixelLightingPS, 0xFA8800). Each model is
//! moved by `ImpostorOffset` (gge 0x38980D2B = (150, 120, -120), 0xE20D40),
//! scaled by `GalaxyPlanetSmallImpostorScale` (2.6, 0xE20D10; the selected
//! game's large one `GalaxyPlanetLargeImpostorScale` is 2.6 too) and turned about
//! its axis once per `GalaxyPlanetImpostorRotationRate` (gge 0xFEE91D1C =
//! 12000 ms; the angle is `2 pi * ms / rate`, 0xE21020, which also points the
//! impostor viewer along the offset, 0xE20EF0). The layer is lit by the
//! `ImpostorLighting` set (gge 0xA4FF3982 -> 0x40200100!B18EFCC2, "lighting for
//! basic solar system"): one point light at (0, 0, 0) of colour (1, 0.9, 0.7),
//! strength 1, radius 1000 (`pointLightPos` / `Color` / `Strength` / `Radius`),
//! the layer's camera standing at the same origin.
//!
//! OpenSpore generates each planet on a worker thread at a small cube
//! resolution (stamps keep their world size, so the continents are the stage
//! planet's) and bakes it into a small equirectangular colour map with the
//! homeworld look of the Space stage (`space_planets::look_for` +
//! `Look::surface`: the land theme's ramp, beach, cliffs, sea depth, plus a
//! water mask for the specular channel). The frames of the turning planet are
//! then drawn from that map on the CPU (`Geometry::render`) a few times a
//! second, only for the planets on screen, into the one image of the planet,
//! with the maths of the retail programs: planetImpostorPS (terminator
//! cutoff 0.2, ambient 0.2, the blue night tint, emissive x 1.3, the water
//! highlight pow(cam . R, 50) in (1, 0.6, 0.06), rim fog 0.5) lit by the
//! point light of the lighting set, and the 1.2 x atmosphere shell
//! (alpha = sat(smoothstep(0.1, 1, N . -eye) x sat(L . N + 1)) x 2, back faces)
//! as `space_planets/impostor.rs` ports them. The map is cached as a small PNG
//! beside the saves (`<stem>-impostors\`).
//!
//! The layer's camera (VERIFIED, fe-rows2, 0xE20EF0 / 0xE21020 / 0xE2127x): the viewer
//! orientation is a transform whose position is the vector at 0x16AC208 (never
//! written: the origin, where the point light is) and whose rotation starts as the
//! identity (0x16AC214, static initializer 0x13705D0), then `LookAt(forward =
//! normalize(ImpostorOffset), up = (0, 0, 1))` (0x6BAC40); the models turn about their
//! own Z axis (0xE21020 calls 0x4099B0, the rotation of the first two basis rows,
//! with `2 pi ms / rate`). So the camera stands at the light, the planet's pole is the
//! world's up axis, and the camera looks down on it by the elevation of the offset
//! above its horizontal part (the sub-viewer point lies at +32 degrees latitude: the
//! north pole side shows), which is `view_tilt` below.
//!
//! ADAPTED: the image is a CPU rendering of the two impostor models (a lit
//! sphere and its shell) from a baked colour map, not the GPU models with the
//! baked diffuse / spec / normal / scatter cubes (the front end has no 3D
//! layer behind the SPUI; the sphere's shading is the program's with the sphere
//! normal in place of the normal cube); the scatter colour is the Space
//! stage's scatter bake (0xF9D960 at `BAKE_P`, `planet_sky::scatter`) read in
//! the one direction the camera sees (the eye vector equals the light vector,
//! so the lookup is the cube frame's -Z), with the Space stage's cube frame
//! (ADAPTED, see `space_planets::impostor`); the viewer's lens is not traced
//! (the planet is drawn orthographically at the shell's size).

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex, OnceLock},
};

use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use crate::{asset_catalog::ResourceIndex, dbpf::ResourceKey};

/// Impostor image size (pixels).
pub const SIZE: u32 = 96;
/// Height-cube resolution of an impostor planet (retail stage planets: 512).
const CUBE_RES: usize = 64;
/// The atmosphere shell's scale over the planet ("planetAtmoImpostor", 0xFA8800).
const ATMOSPHERE_SCALE: f32 = 1.2;
/// Planet disc radius in the image (the shell fills the image).
const DISC: f32 = 1.0 / ATMOSPHERE_SCALE;
/// Group of the runtime textures registered with the SPUI painter.
const IMAGE_GROUP: u32 = 0x0F2E_1A00;
const PNG_TYPE: u32 = 0x2F7D_0004;
/// The baked planet map (longitude x latitude).
const MAP_W: usize = 256;
const MAP_H: usize = 128;
/// `GalaxyPlanetImpostorRotationRate` (gge 0xFEE91D1C): milliseconds per turn.
pub const ROTATION_PERIOD_MS: u32 = 12_000;
/// The frames of the turning planets are drawn at most this often.
const FRAME_INTERVAL_MS: u32 = 50;

/// The elevation of the impostor camera above the planet's equator. VERIFIED
/// from `ImpostorOffset` (gge 0x38980D2B = (150, 120, -120)), which 0xE20EF0
/// turns into the impostor viewer's orientation (look along the offset, up =
/// +Z, the planet's spin axis): the angle of that vector below the horizontal
/// plane, so the camera looks down on the planet from above its equator.
fn view_tilt() -> f32 {
    120.0f32.atan2(150.0f32.hypot(120.0))
}

/// Milliseconds since the first call (the planets' clock).
fn clock_ms() -> u32 {
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_millis() as u32
}

/// The turn of a planet at `ms` (0xE21020: 2 pi * ms / rate).
fn spin_at(ms: u32) -> f32 {
    (ms % ROTATION_PERIOD_MS) as f32 / ROTATION_PERIOD_MS as f32 * std::f32::consts::TAU
}

/// A planet baked for the impostor: surface colours on a longitude x latitude
/// grid (alpha = emissive, lava) and the atmosphere of its look.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanetMap {
    /// `MAP_W x MAP_H` RGBA8, row 0 = north pole, column 0 = longitude -pi.
    texels: Vec<u8>,
    /// `MAP_W x MAP_H`: 255 where the texel lies under the sea (the specular
    /// channel of the impostor's Sampler1).
    water: Vec<u8>,
    /// The scatter colour (rgb) and its additive part (a), 8-bit.
    scatter: [u8; 4],
    /// Whether the world has an atmosphere shell (PS 125's `hackOff`: a black
    /// scatter cube draws none).
    shell: bool,
}

impl PlanetMap {
    /// Bilinear lookup at texel coordinates (`u` wraps, `v` clamps).
    fn sample(&self, u: f32, v: f32) -> ([f32; 3], f32) {
        let (x0, y0) = (u.floor(), v.floor());
        let (fx, fy) = (u - x0, v - y0);
        let at = |x: i32, y: i32| -> [f32; 4] {
            let xi = x.rem_euclid(MAP_W as i32) as usize;
            let yi = y.clamp(0, MAP_H as i32 - 1) as usize;
            let i = (yi * MAP_W + xi) * 4;
            std::array::from_fn(|k| self.texels[i + k] as f32 / 255.0)
        };
        let (x0, y0) = (x0 as i32, y0 as i32);
        let (a, b, c, d) = (at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1));
        let mix = |k: usize| (a[k] * (1.0 - fx) + b[k] * fx) * (1.0 - fy) + (c[k] * (1.0 - fx) + d[k] * fx) * fy;
        ([mix(0), mix(1), mix(2)], mix(3))
    }

    /// Whether the texel nearest to (`u`, `v`) is water.
    fn is_water(&self, u: f32, v: f32) -> bool {
        let xi = (u.round() as i32).rem_euclid(MAP_W as i32) as usize;
        let yi = (v.round() as i32).clamp(0, MAP_H as i32 - 1) as usize;
        self.water[yi * MAP_W + xi] > 127
    }

    /// The scatter colour of the shell and of the planet's rim (rgb, a).
    fn scatter(&self) -> [f32; 4] {
        self.scatter.map(|v| v as f32 / 255.0)
    }
}

/// Bake the surface of `planet` with `look` into a map.
fn bake_map(planet: &crate::planet_query::Planet, look: &crate::space_planets::Look, sky: Option<([f32; 4], bool)>) -> PlanetMap {
    let mut texels = vec![0u8; MAP_W * MAP_H * 4];
    let mut water = vec![0u8; MAP_W * MAP_H];
    for ty in 0..MAP_H {
        let lat = std::f32::consts::FRAC_PI_2 - (ty as f32 + 0.5) / MAP_H as f32 * std::f32::consts::PI;
        for tx in 0..MAP_W {
            let lon = (tx as f32 + 0.5) / MAP_W as f32 * std::f32::consts::TAU - std::f32::consts::PI;
            let dir = Vec3::new(lat.cos() * lon.cos(), lat.sin(), lat.cos() * lon.sin());
            let (c, glow) = look.surface(planet, dir);
            let i = (ty * MAP_W + tx) * 4;
            for k in 0..3 {
                texels[i + k] = (c[k].clamp(0.0, 1.0) * 255.0).round() as u8;
            }
            texels[i + 3] = (glow.clamp(0.0, 1.0) * 255.0).round() as u8;
            water[ty * MAP_W + tx] = if planet.h_at(dir) < look.water_level { 255 } else { 0 };
        }
    }
    // Without the sky tables: the Space stage's stand-in (`impostor::scatter_for`).
    let (scatter, shell) = sky.unwrap_or_else(|| {
        let s = crate::space_planets::impostor::scatter_for(look);
        ([s.x, s.y, s.z, s.w], look.thickness > 0.0)
    });
    PlanetMap { texels, water, scatter: scatter.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8), shell }
}

/// What one pixel of the planet's disc shows, independent of the planet: where
/// its sphere point lies on the map and how the program lights it (the light is
/// fixed to the view, so none of it turns with the planet).
struct DiscPx {
    /// Map row of the point (texel units) and its longitude before the turn.
    v: f32,
    lon: f32,
    /// planetImpostorPS: `dot_x * terminator + ambient level`.
    day: f32,
    /// `dot_y`: the night-side weight of the tinted ambient.
    night: f32,
    terminator: f32,
    /// The rim fog `0.5 (1 - N . eye)`.
    fog: f32,
    /// `pow(cam . R, 50) * terminator`.
    glint: f32,
    alpha: f32,
}

/// The atmosphere shell's back face behind this pixel.
struct HaloPx {
    /// `sat(L . N + 1)`: multiplies the scatter colour.
    shadow: f32,
    /// `sat(smoothstep(0.1, 1, N . -eye) x shadow) x 2`.
    alpha: f32,
}

struct Geometry {
    px: Vec<(Option<DiscPx>, Option<HaloPx>)>,
}

fn geometry() -> &'static Geometry {
    static GEOMETRY: OnceLock<Geometry> = OnceLock::new();
    GEOMETRY.get_or_init(Geometry::build)
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Geometry {
    fn build() -> Self {
        let n = SIZE as usize;
        // The sub-viewer point lies at the camera's elevation north of the equator (VERIFIED, `view_tilt`).
        let turn = Quat::from_rotation_x(-view_tilt());
        // The light is the lighting set's point light at the origin, where the
        // camera stands: the direction to the light is the direction to the viewer.
        let light = Vec3::Z;
        let eye = Vec3::Z;
        // planetImpostorPS constants (`shaders/space_impostor.wesl`).
        let (cutoff, ambient_dot_offset, ambient_level, spec_size) = (0.2f32, 0.65f32, 0.2f32, 50.0f32);
        let mut px = Vec::with_capacity(n * n);
        for y in 0..n {
            for x in 0..n {
                let (fx, fy) = (((x as f32 + 0.5) / n as f32) * 2.0 - 1.0, 1.0 - ((y as f32 + 0.5) / n as f32) * 2.0);
                let r = (fx * fx + fy * fy).sqrt();
                let mut disc = None;
                let mut halo = None;
                if r <= DISC + 1.5 / n as f32 {
                    let (sx, sy) = (fx / DISC, fy / DISC);
                    let z = (1.0 - (sx * sx + sy * sy).min(1.0)).sqrt();
                    let view = Vec3::new(sx, sy, z).normalize_or(Vec3::Z);
                    let dir = turn * view;
                    let n_dot_l = view.dot(light);
                    let terminator = ((n_dot_l + cutoff) / (2.0 * cutoff)).clamp(0.0, 1.0);
                    let dot_x = n_dot_l.clamp(0.0, 1.0);
                    let dot_y = ((-n_dot_l + ambient_dot_offset) / (1.0 + ambient_dot_offset)).clamp(0.0, 1.0);
                    let reflected = (light - 2.0 * n_dot_l * view).normalize_or(Vec3::Z);
                    disc = Some(DiscPx {
                        v: (0.5 - dir.y.clamp(-1.0, 1.0).asin() / std::f32::consts::PI) * MAP_H as f32 - 0.5,
                        lon: dir.z.atan2(dir.x),
                        day: dot_x * terminator + ambient_level,
                        night: dot_y,
                        terminator,
                        fog: 0.5 * (1.0 - view.dot(eye).clamp(0.0, 1.0)),
                        glint: (-eye).dot(reflected).clamp(0.0, 1.0).powf(spec_size) * terminator,
                        // Anti-aliased edge.
                        alpha: ((DISC - r) * n as f32 * 0.5 + 0.5).clamp(0.0, 1.0),
                    });
                }
                // planetAtmoImpostor: the shell's back faces (front faces culled)
                // out to its scale; the planet hides them where it is in front.
                let rho = r / DISC;
                if rho > 1.0 - 1.0 / n as f32 && rho <= ATMOSPHERE_SCALE {
                    let (px_, py_) = (fx / DISC, fy / DISC);
                    let depth = (ATMOSPHERE_SCALE * ATMOSPHERE_SCALE - px_ * px_ - py_ * py_).max(0.0).sqrt();
                    let normal = Vec3::new(px_, py_, -depth) / ATMOSPHERE_SCALE;
                    let dp_view = smoothstep(0.1, 1.0, normal.dot(-eye));
                    let shadow = (light.dot(normal) + 1.0).clamp(0.0, 1.0);
                    halo = Some(HaloPx { shadow, alpha: (dp_view * shadow).clamp(0.0, 1.0) * 2.0 });
                }
                px.push((disc, halo));
            }
        }
        Self { px }
    }

    /// The planet turned by `spin` radians (features move towards the right),
    /// RGBA8 sRGB.
    fn render(&self, map: &PlanetMap, spin: f32) -> Vec<u8> {
        let scatter = map.scatter();
        // PS 125's `hackOff`: an airless world's black scatter cube draws no shell.
        let shell = map.shell;
        let mut out = vec![0u8; self.px.len() * 4];
        for (i, (disc, halo)) in self.px.iter().enumerate() {
            let (mut rgb, mut alpha) = ([0.0f32; 3], 0.0f32);
            if let Some(d) = disc {
                let u = ((d.lon + spin + std::f32::consts::PI) / std::f32::consts::TAU).rem_euclid(1.0) * MAP_W as f32 - 0.5;
                let (c, glow) = map.sample(u, d.v);
                // specEmiss = sat(Sampler1 + nonWaterSpec 0.05): r the water
                // specular, b the emissive (the lava glow).
                let spec = if map.is_water(u, d.v) { 1.0 } else { 0.05 };
                let emissive = (glow + 0.05).clamp(0.0, 1.0);
                let luma = c[0] * 0.3 + c[1] * 0.59 + c[2] * 0.11;
                let night_lum = d.night * luma;
                let tint = [0.7, 0.3, 2.3];
                for k in 0..3 {
                    // ambient_tint = night luminance x (0.7, 0.3, 2.3) x 0.5 x ambient 0.9.
                    let mut col = night_lum * tint[k] * 0.5 * 0.9 + c[k] * d.day;
                    col += (c[k] - col) * emissive * 1.3;
                    col += d.glint * spec * [1.0, 0.6, 0.06][k];
                    // `s = scatter x terminator`; the rim fog toward it, plus its additive part.
                    let s = scatter[k] * d.terminator;
                    rgb[k] = col + (s - col) * d.fog + s * scatter[3] * d.terminator;
                }
                alpha = d.alpha;
            }
            if let (Some(h), true) = (halo, shell) {
                let a = h.alpha;
                let over = alpha;
                for k in 0..3 {
                    // PS 125: scatter x shadow + scatter x scatter.a.
                    rgb[k] = rgb[k] * over + scatter[k] * (h.shadow + scatter[3]) * (1.0 - over);
                }
                alpha = over + a * (1.0 - over);
            }
            for k in 0..3 {
                out[i * 4 + k] = (rgb[k].clamp(0.0, 1.0) * 255.0).round() as u8;
            }
            out[i * 4 + 3] = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        out
    }
}

enum Entry {
    Pending,
    Ready(Ready),
    Failed,
}

struct Ready {
    key: ResourceKey,
    handle: Handle<Image>,
    map: Arc<PlanetMap>,
}

struct Job {
    seed: u64,
    cache: Option<PathBuf>,
}

struct Done {
    seed: u64,
    map: Result<PlanetMap, String>,
    millis: u128,
    cached: bool,
}

/// The impostor cache and its worker.
#[derive(Resource, Default)]
pub struct Impostors {
    jobs: Option<mpsc::Sender<Job>>,
    done: Option<Mutex<mpsc::Receiver<Done>>>,
    entries: HashMap<u64, Entry>,
    /// Seeds whose planet is on screen this frame.
    touched: Vec<u64>,
    last_frame: u32,
}

/// The SPUI image key of a seed's impostor.
pub fn image_key(seed: u64) -> ResourceKey {
    ResourceKey { group_id: IMAGE_GROUP, instance_id: (seed ^ (seed >> 32)) as u32, type_id: PNG_TYPE }
}

/// Where the impostors of the saves beside `save` are cached.
pub fn cache_dir(save: &Path) -> PathBuf {
    let stem = save.file_stem().and_then(|s| s.to_str()).unwrap_or("openspore_save");
    save.with_file_name(format!("{stem}-impostors"))
}

fn cache_file(dir: &Path, seed: u64) -> PathBuf {
    dir.join(format!("{seed:016x}-map2.png"))
}

impl Impostors {
    /// The finished impostor of `seed`; asks the worker for it when new.
    pub fn request(&mut self, seed: u64, cache: Option<&Path>, index: &ResourceIndex) -> Option<ResourceKey> {
        match self.entries.get(&seed) {
            Some(Entry::Ready(ready)) => return Some(ready.key),
            Some(_) => return None,
            None => {}
        }
        if self.jobs.is_none() {
            if index.records.is_empty() {
                return None;
            }
            let (job_tx, job_rx) = mpsc::channel::<Job>();
            let (done_tx, done_rx) = mpsc::channel::<Done>();
            let index = Arc::new(index.clone());
            let spawned = std::thread::Builder::new()
                .name("gge-impostors".into())
                .spawn(move || worker(index, job_rx, done_tx));
            if spawned.is_err() {
                return None;
            }
            self.jobs = Some(job_tx);
            self.done = Some(Mutex::new(done_rx));
        }
        let job = Job { seed, cache: cache.map(Path::to_path_buf) };
        if self.jobs.as_ref().is_some_and(|tx| tx.send(job).is_ok()) {
            self.entries.insert(seed, Entry::Pending);
        }
        None
    }

    /// Finished bakes become textures of the SPUI painter.
    pub fn poll(&mut self, images: &mut Assets<Image>, assets: &mut crate::spui::view::SpuiAssets) {
        let Some(rx) = self.done.as_ref() else { return };
        let finished: Vec<Done> = match rx.lock() {
            Ok(rx) => rx.try_iter().collect(),
            Err(_) => return,
        };
        for done in finished {
            match done.map {
                Ok(map) => {
                    let map = Arc::new(map);
                    let image = Image::new(
                        Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
                        TextureDimension::D2,
                        geometry().render(&map, spin_at(clock_ms())),
                        // The SPUI painter samples its textures unconverted (`spui::view::import_texture`).
                        TextureFormat::Rgba8Unorm,
                        RenderAssetUsages::default(),
                    );
                    let key = image_key(done.seed);
                    let handle = images.add(image);
                    assets.insert_texture(key, crate::spui::view::Tex { handle: handle.clone(), size: [SIZE, SIZE] });
                    info!(
                        "front end: planet impostor {:016x} {} in {} ms",
                        done.seed,
                        if done.cached { "read from the cache" } else { "rendered" },
                        done.millis
                    );
                    self.entries.insert(done.seed, Entry::Ready(Ready { key, handle, map }));
                }
                Err(error) => {
                    warn!("front end: planet impostor {:016x}: {error}", done.seed);
                    self.entries.insert(done.seed, Entry::Failed);
                }
            }
        }
    }

    /// The planet of `seed` is on screen this frame.
    pub fn touch(&mut self, seed: u64) {
        self.touched.push(seed);
    }

    /// Turn the planets on screen: redraw their images from the baked maps
    /// (the retail impostors turn once per `ROTATION_PERIOD_MS`).
    pub fn animate(&mut self, images: &mut Assets<Image>) {
        let touched = std::mem::take(&mut self.touched);
        let now = clock_ms();
        if touched.is_empty() || now.wrapping_sub(self.last_frame) < FRAME_INTERVAL_MS {
            return;
        }
        self.last_frame = now;
        let spin = spin_at(now);
        let mut done: Vec<u64> = Vec::with_capacity(touched.len());
        for seed in touched {
            if done.contains(&seed) {
                continue;
            }
            done.push(seed);
            if let Some(Entry::Ready(ready)) = self.entries.get(&seed) {
                if let Some(mut image) = images.get_mut(&ready.handle) {
                    image.data = Some(geometry().render(&ready.map, spin));
                }
            }
        }
    }
}

fn worker(index: Arc<ResourceIndex>, jobs: mpsc::Receiver<Job>, done: mpsc::Sender<Done>) {
    let mut library: Option<crate::planet_data::BrushLibrary> = None;
    let mut theme: Option<Option<crate::world_catalog::PlanetPalette>> = None;
    for job in jobs {
        let start = std::time::Instant::now();
        let file = job.cache.as_ref().map(|d| cache_file(d, job.seed));
        if let Some(map) = file.as_ref().and_then(|f| read_map(f)) {
            let _ = done.send(Done { seed: job.seed, map: Ok(map), millis: start.elapsed().as_millis(), cached: true });
            continue;
        }
        let theme = theme
            .get_or_insert_with(|| crate::world_presentation::LandTheme::load(&index).ok().map(|t| t.palette))
            .clone();
        let map = render_seed(&index, &mut library, theme.as_ref(), job.seed);
        if let (Ok(map), Some(file)) = (&map, &file) {
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Err(error) = write_map(file, map) {
                warn!("front end: impostor cache {}: {error}", file.display());
            }
        }
        let _ = done.send(Done { seed: job.seed, map, millis: start.elapsed().as_millis(), cached: false });
    }
}

/// Generate the homeworld of a save seed (the script `terrain::generate_planet_cache`
/// picks for it) at the impostor resolution and bake it.
fn render_seed(
    index: &ResourceIndex,
    library: &mut Option<crate::planet_data::BrushLibrary>,
    theme: Option<&crate::world_catalog::PlanetPalette>,
    seed: u64,
) -> Result<PlanetMap, String> {
    use crate::planet_data as pd;
    let scripts = pd::first_group(index);
    let script = pd::choose_homeworld(&scripts, seed).cloned().ok_or("no FirstGroup planet scripts")?;
    let descriptor = pd::load_descriptor(index, script.terrain_script)?;
    let lib = match library.take() {
        Some(lib) => lib,
        None => pd::BrushLibrary::load(index)?,
    };
    let source = pd::PlanetSource::from_parts(index, script, descriptor, lib);
    let (cube, _) = crate::planet_gen::generate_res(&source, CUBE_RES);
    let d = &source.descriptor;
    let planet = crate::planet_query::Planet::new(cube, d.radius(), d.water_level());
    let climate = crate::space_planets::Climate {
        info: [d.planet_info[0], d.planet_info[1]],
        base_water: d.wta[0],
        now: (d.wta[1], d.wta[2]),
        t_score: 3,
    };
    let look = crate::space_planets::look_for(true, theme, None, climate);
    // The scatter bake of the Space stage (0xF9D960 at `BAKE_P`, the planet
    // record path 0xF35450 starts the atmosphere / water types at 11) read in
    // the cube frame's -Z, the direction a camera at the light sees.
    let inputs = {
        use crate::planet_sky::SkyInputs;
        let sky = SkyInputs::from_descriptor_or(d, 11);
        SkyInputs { temperature: look.climate.0.clamp(0.0, 1.0), atmosphere: look.climate.1.clamp(0.0, 1.0), ..sky }
    };
    let sky = crate::planet_sky::SkyTables::load(index, inputs.atmosphere_type, inputs.water_type).map(|tables| {
        let lobes = tables.lobes(&inputs, crate::space_planets::impostor::BAKE_P, Some(look.air));
        let cube = crate::planet_sky::scatter_cube(&lobes);
        (crate::planet_sky::scatter(&lobes, Vec3::NEG_Z), !crate::space_planets::impostor::cube_is_black(&cube))
    });
    let map = bake_map(&planet, &look, sky);
    *library = Some(source.library);
    Ok(map)
}

/// The map as a PNG: `MAP_H` rows of texels, `MAP_H` rows of the water mask
/// (red) and one more row whose first texel is the scatter colour (rgba) and
/// whose second has the shell flag in red.
fn write_map(path: &Path, map: &PlanetMap) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), MAP_W as u32, MAP_H as u32 * 2 + 1);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    let mut data = map.texels.clone();
    data.extend(map.water.iter().flat_map(|w| [*w, 0, 0, 255]));
    data.extend_from_slice(&map.scatter);
    data.extend_from_slice(&[if map.shell { 255 } else { 0 }, 0, 0, 255]);
    data.resize(MAP_W * (MAP_H * 2 + 1) * 4, 0);
    writer.write_image_data(&data).map_err(|e| e.to_string())
}

fn read_map(path: &Path) -> Option<PlanetMap> {
    let file = std::fs::File::open(path).ok()?;
    let decoder = png::Decoder::new(std::io::BufReader::new(file));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let ok = info.width as usize == MAP_W
        && info.height as usize == MAP_H * 2 + 1
        && info.color_type == png::ColorType::Rgba
        && info.bit_depth == png::BitDepth::Eight;
    ok.then(|| {
        buf.truncate(info.buffer_size());
        let split = MAP_W * MAP_H * 4;
        let water = buf[split..split * 2].chunks_exact(4).map(|p| p[0]).collect();
        let meta = split * 2;
        PlanetMap {
            texels: buf[..split].to_vec(),
            water,
            scatter: [buf[meta], buf[meta + 1], buf[meta + 2], buf[meta + 3]],
            shell: buf[meta + 4] > 127,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planet_query::{h_to_raw, texel_dir, HeightCube, Planet};

    /// Half land (+x hemisphere), half sea.
    fn half_planet() -> PlanetMap {
        let res = 16;
        let mut cube = HeightCube::filled(res, -0.3);
        for face in 0..6 {
            for row in 0..res {
                for col in 0..res {
                    if texel_dir(face, row, col, res).x > 0.0 {
                        let i = cube.index(face, row, col);
                        cube.raw_mut()[i] = h_to_raw(0.2);
                    }
                }
            }
        }
        let planet = Planet::new(cube, 500.0, 0.0);
        let climate = crate::space_planets::Climate { info: [32.0; 2], base_water: 0.5, now: (0.5, 0.5), t_score: 3 };
        let look = crate::space_planets::look_for(true, None, None, climate);
        bake_map(&planet, &look, Some(([0.2, 0.3, 0.5, 0.0], true)))
    }

    #[test]
    fn impostor_shows_land_sea_and_a_transparent_corner() {
        let map = half_planet();
        let rgba = geometry().render(&map, 0.0);
        assert_eq!(rgba.len(), (SIZE * SIZE * 4) as usize);
        assert_eq!(rgba[3], 0, "corner outside the rim is transparent");
        let centre = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
        assert_eq!(rgba[centre + 3], 255, "the disc is opaque");
        // Different colours across the disc (land and sea).
        let px = |rgba: &[u8], x: u32| {
            let i = ((SIZE / 2 * SIZE + x) * 4) as usize;
            [rgba[i], rgba[i + 1], rgba[i + 2]]
        };
        assert_ne!(px(&rgba, SIZE / 4), px(&rgba, SIZE * 3 / 4));
        // PNG round trip through the cache format.
        let dir = std::env::temp_dir().join(format!("openspore-fe-impostor-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("x.png");
        write_map(&file, &map).unwrap();
        assert_eq!(read_map(&file).as_ref(), Some(&map));
        std::fs::remove_dir_all(dir).unwrap();
        assert_eq!(cache_dir(Path::new("C:/x/qc_save.json")), Path::new("C:/x/qc_save-impostors"));
        // Longitude 0 (+x) is the middle column of the map; the -x side the edge.
        assert!(!map.is_water(MAP_W as f32 * 0.5, MAP_H as f32 * 0.5), "the +x hemisphere is land");
        assert!(map.is_water(MAP_W as f32 * 0.05, MAP_H as f32 * 0.5), "the -x hemisphere is sea");
    }

    #[test]
    fn the_light_is_the_view_and_the_shell_fades_with_the_programs_formula() {
        let map = half_planet();
        let g = geometry();
        let n = SIZE as usize;
        let at = |x: usize, y: usize| &g.px[y * n + x];
        // The disc centre faces the light: terminator 1, a water glint, no rim fog.
        let (disc, halo) = at(n / 2, n / 2);
        let d = disc.as_ref().expect("disc");
        assert!(d.terminator > 0.99 && d.fog < 0.01 && d.glint > 0.9, "{} {} {}", d.terminator, d.fog, d.glint);
        assert!(halo.is_none(), "the planet hides the shell's back face at the centre");
        // Just outside the limb the shell shows: shadow 0.45, alpha about 0.45 (PS 125).
        let x = ((DISC * 1.05 + 1.0) * 0.5 * n as f32) as usize;
        let (_, halo) = at(x.min(n - 1), n / 2);
        let h = halo.as_ref().expect("shell");
        assert!(h.shadow > 0.3 && h.shadow < 0.6 && h.alpha > 0.2 && h.alpha < 0.8, "{} {}", h.shadow, h.alpha);
        // The rendered shell is only for a world with an atmosphere.
        let air = PlanetMap { scatter: [100, 150, 255, 0], shell: true, ..map.clone() };
        let rgba = g.render(&air, 0.0);
        let px = |rgba: &[u8], x: usize| rgba[(n / 2 * n + x) * 4 + 3];
        assert!(px(&rgba, x.min(n - 1)) > 0, "air: the shell is drawn");
        let airless = PlanetMap { scatter: [0; 4], shell: false, ..map.clone() };
        assert_eq!(px(&g.render(&airless, 0.0), x.min(n - 1)), 0, "airless: no shell");
    }

    #[test]
    fn the_planet_turns_once_per_rotation_period() {
        let map = half_planet();
        let g = geometry();
        let at = |ms: u32| g.render(&map, spin_at(ms));
        assert_eq!(at(0), at(ROTATION_PERIOD_MS), "a full period is a full turn");
        assert_ne!(at(0), at(ROTATION_PERIOD_MS / 2), "half a period shows the other hemisphere");
        assert!((spin_at(ROTATION_PERIOD_MS / 4) - std::f32::consts::FRAC_PI_2).abs() < 1.0e-5);
        // The camera looks down on the planet by the angle of `ImpostorOffset`.
        assert!((view_tilt().to_degrees() - 32.0).abs() < 0.1);
    }
}
