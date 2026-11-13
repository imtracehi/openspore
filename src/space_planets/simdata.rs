//! The land albedo passes of the retail terrain (audit row 22, helper `space-shaders`).
//!
//! Retail colours the land in three full-quad GPU passes into the face's sim-data render
//! target (`cTerrainEditor` 0xF6A910, called by 0xF98370 with `Terrain::sTerrainSimDataRTTs_ID`;
//! the target is the `simData` of `TerrainAboveSynthPSMedium`: `simData.rgb * detail` is the
//! land albedo). The three materials are in the install's material table (group 0x40212000
//! instance 3, compiled states group 0x40212001) with the programs of the standard shader
//! table (group 0x40212004, `TerrainBrushTexture{Above,Beach,Cliff}Shader`); this module is
//! the arithmetic of their pixel programs (disassembled with `tools\d3d9dis.py`, dumps in
//! `space-shaders\dumps\`), written as plain functions with the render states of the passes:
//!
//! | pass | material | program | blend | samplers (all clamp) |
//! | ---- | -------- | ------- | ----- | -------------------- |
//! | 1 Above | 0xE8078E3B | 0x7000002C | none (alpha test > 0) | s0 map, s1 life table (linear, no mips) |
//! | 2 Beach | 0x100CB7AD | 0x7000002D | SRCALPHA / INVSRCALPHA, ADD | s0 height map (linear) |
//! | 3 Cliff | 0x19E45172 | 0x7000002E | SRCALPHA / INVSRCALPHA, ADD | s0 normal map, s1 height map, s2 detail (wrap) |
//!
//! The passes run in that order, so the result is a plain sequence of over blends
//! (`blend_over`); the destination alpha after pass 1 is `0.99 * value + 0.01`.
//!
//! Not part of this module (still open, see `notes\space-shaders-notes.md`): the content of
//! the 256 x 256 life table (the ramp rows are VERIFIED, `table_row_ramp_position`; the
//! columns 0, 0x7F, 0xFF written by 0xFB7D90 .. 0xFB84B0 were not read), what fills the
//! per-face 8-bit map that is sampler 0 of pass 1 (`value` below), and the normal map's
//! `w` channel that pass 3 reads.

/// Material ids of the three passes, the program id each compiled state names, and the
/// program's name (installed standard shader table; the ids of the earlier programs are the
/// same in `Spore_Graphics.package` and `PatchData.package`).
pub const PASSES: [(u32, u32, &str); 3] = [
    (0xE8078E3B, 0x7000002C, "TerrainBrushTextureAboveShader"),
    (0x100CB7AD, 0x7000002D, "TerrainBrushTextureBeachShader"),
    (0x19E45172, 0x7000002E, "TerrainBrushTextureCliffShader"),
];

/// Pass 1 column of the second table sample (`def c1.y` of the Above program: 0.75098, i.e.
/// texel 192 of the 256 columns; 0xFB8DA0 reads the table at column 191 with the same
/// meaning).
pub const ABOVE_BASE_COLUMN_U: f32 = 0.75098;
/// Pass 1 alpha: `value * 0.99 + 0.01` (`def c1.zw`).
pub const ABOVE_ALPHA_SCALE: f32 = 0.99;
pub const ABOVE_ALPHA_BIAS: f32 = 0.01;
/// The shader's literal altitude scale of passes 2 and 3 (`def c3.z` / `c4.x`).
pub const HEIGHT_SCALE: f32 = 100.0;
/// Pass 3 samples its detail texture at `uv * 20` (`def c3.x`).
pub const CLIFF_DETAIL_REPEAT: f32 = 20.0;

/// 0xFB7C40: the life table row `row` (0..255) is the planet colour ramp's image row
/// `63 - floor((255 - row) * 0.2431373)` blended with the row above it by the fraction.
/// Returns (the ramp row, the fraction toward the previous row): row 255 is ramp row 63,
/// row 0 is ramp row 1 (62 / 255 ramp rows per table row).
pub fn table_row_ramp_position(row: usize) -> (usize, f32) {
    // 0.24313726 = [0x149119C] = 62 / 255 (0x3E78F8F9).
    let i = (255 - row.min(255)) as f32;
    let scaled = i * 0.243_137_26;
    let floor = scaled.floor();
    (63 - floor as usize, scaled - floor)
}

/// Pass 1 (`TerrainBrushTextureAboveShader` 0x7000002C): the land colour and the alpha from
/// the map value `value` (0..1, sampler 0) and the table column `column` (`terrainSynthParams`
/// c0.x = cTerrainStateMgr +0x44C = 0xFB88A0's column and fraction). `table(u, v)` is the
/// life table sampled bilinearly (sampler 1) at normalised coordinates, rgb.
pub fn above_pass(table: impl Fn(f32, f32) -> [f32; 3], value: f32, column: f32) -> ([f32; 3], f32) {
    let frac = column - column.floor();
    let base = column - frac;
    let a = table(base * 0.003_921_57, value);
    let b = table(ABOVE_BASE_COLUMN_U, value);
    let rgb = [0, 1, 2].map(|k| b[k] + frac * (a[k] - b[k]));
    (rgb, value * ABOVE_ALPHA_SCALE + ABOVE_ALPHA_BIAS)
}

// ---------------------------------------------------------------------------
// The life table (cTerrainStateMgr +0x328 `AboveColorRamp`, created 256 x 256 BGRA by 0xFBDAE0
// and refilled by 0xFBBBD0 -> 0xFB8760 whenever the ramp column changes: 0xFBD850 stores
// mgr +8 / +0xC = (terrainScriptPlanetInfo.x - 0.5) / 64 and rebuilds it).
// ---------------------------------------------------------------------------

/// Texels per side of the life table (columns = climate position, rows = the map value).
pub const LIFE_TABLE_SIZE: usize = 256;
/// Side of the planet colour ramp images (`planet_color_ramps_*.32bitImage`, BGRA).
pub const RAMP_IMAGE_SIZE: usize = 64;
/// Bytes of one ramp image.
pub const RAMP_IMAGE_BYTES: usize = RAMP_IMAGE_SIZE * RAMP_IMAGE_SIZE * 4;
/// Rows of the band colour image (`0xF30D0A76.32bitImage`, 64 x 10, BGRA).
pub const BAND_IMAGE_ROWS: usize = 10;
/// Column of the table that holds the ice ramp (0xFB7D90 with tcol 0), the living ramp (0xFB7ED0,
/// tcol 0x7F) and the lava ramp (0xFB7D90, tcol 0xFF), with the dead ramp in between.
pub const LIFE_COLUMN_ICE: usize = 0;
pub const LIFE_COLUMN_LIVING: usize = 0x7F;
pub const LIFE_COLUMN_LAVA: usize = 0xFF;

/// 62 / 255 as the f32 of `[0x149119C]` (0x3E78F8F9): ramp rows per table row.
const ROWS_PER_TABLE_ROW: f32 = f32::from_bits(0x3E78_F8F9);

/// `cvtss2si` (round to nearest even) then the `ucomiss / cmovb` adjustment the code uses
/// everywhere it needs `floor` (0xFB7C40 .. 0xFB7ED0, 0xFB9E40, 0xFBBC99).
fn floor_like(x: f32) -> i32 {
    let e = x.round_ties_even() as i32;
    if x < e as f32 {
        e - 1
    } else {
        e
    }
}

/// The 256 x 256 life table, bytes as the texture stores them (B, G, R, A per texel, row
/// major, row 0 first). Row `r` is the map value `r / 255`. Columns: 0 = ice ramp, 0x7F =
/// living ramp, 0xFF = lava ramp, each skewed into its neighbours, the dead ramp elsewhere.
#[derive(Clone)]
pub struct LifeTable {
    texels: Vec<u8>,
}

impl std::fmt::Debug for LifeTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LifeTable({} bytes)", self.texels.len())
    }
}

impl LifeTable {
    fn at(&self, row: usize, col: usize) -> [u8; 4] {
        let i = (row * LIFE_TABLE_SIZE + col) * 4;
        [self.texels[i], self.texels[i + 1], self.texels[i + 2], self.texels[i + 3]]
    }

    fn put(&mut self, row: usize, col: usize, v: [u8; 4]) {
        let i = (row * LIFE_TABLE_SIZE + col) * 4;
        self.texels[i..i + 4].copy_from_slice(&v);
    }

    /// The table bytes (B, G, R, A).
    pub fn bytes(&self) -> &[u8] {
        &self.texels
    }

    /// 0xFB7C40 / 0xFB7D90 / 0xFB7ED0: the colour of the table row written for `edi`
    /// (memory row `255 - edi`) from a ramp image column, blending two ramp rows by the
    /// fraction of `edi * 62 / 255`. `flip`: the dead ramp reads rows `63 - n` and `62 - n`
    /// (0xFB7C40), the others rows `n` and `n + 1`.
    fn ramp_row(ramp: &[u8], ramp_col: usize, edi: usize, flip: bool, channels: usize) -> [u8; 4] {
        let x = edi as f32 * ROWS_PER_TABLE_ROW;
        let n = floor_like(x);
        let frac = x - n as f32;
        let (cur, other) = if flip {
            let row = (63 - n) as usize;
            (row * RAMP_IMAGE_SIZE + ramp_col, (row - 1) * RAMP_IMAGE_SIZE + ramp_col)
        } else {
            let row = n as usize;
            (row * RAMP_IMAGE_SIZE + ramp_col, (row + 1) * RAMP_IMAGE_SIZE + ramp_col)
        };
        let mut out = [0u8; 4];
        for k in 0..channels {
            let a = ramp[other * 4 + k] as f32 * frac;
            let b = ramp[cur * 4 + k] as f32 * (1.0 - frac);
            out[k] = (a + b) as i32 as u8;
        }
        out
    }

    /// 0xFB8760 (called by 0xFBBBD0 with the four ramp images and the ramp columns): fills the
    /// whole table. `col_a` is the dead ramp's column (mgr +8, or +0xC in the scenario game
    /// mode), `col_b` the living / ice / lava columns (mgr +8). Checked byte for byte against
    /// the retail routines (executed from the code image under an emulator, see
    /// `the_life_table_matches_the_retail_routines`).
    pub fn build(dead: &[u8], living: &[u8], ice: &[u8], lava: &[u8], col_a: usize, col_b: usize) -> Self {
        let mut t = LifeTable { texels: vec![0; LIFE_TABLE_SIZE * LIFE_TABLE_SIZE * 4] };
        // 0xFB7C40: every column the dead ramp, alpha 0.
        for edi in 0..256 {
            let c = Self::ramp_row(dead, col_a, edi, true, 3);
            for col in 0..LIFE_TABLE_SIZE {
                t.put(255 - edi, col, c);
            }
        }
        t.write_column(ice, col_b, LIFE_COLUMN_ICE, false);
        t.skew_right(LIFE_COLUMN_ICE);
        t.smooth(LIFE_COLUMN_ICE, true);
        t.write_column(living, col_b, LIFE_COLUMN_LIVING, true);
        t.skew_left(LIFE_COLUMN_LIVING);
        t.smooth(LIFE_COLUMN_LIVING, false);
        t.skew_right(LIFE_COLUMN_LIVING);
        t.smooth(LIFE_COLUMN_LIVING, true);
        t.write_column(lava, col_b, LIFE_COLUMN_LAVA, false);
        t.skew_left(LIFE_COLUMN_LAVA);
        t.smooth(LIFE_COLUMN_LAVA, false);
        t
    }

    /// 0xFB7D90 (alpha 0, rgb of the ramp) and 0xFB7ED0 (`with_alpha`: all four channels).
    fn write_column(&mut self, ramp: &[u8], ramp_col: usize, tcol: usize, with_alpha: bool) {
        for edi in 0..256 {
            let c = Self::ramp_row(ramp, ramp_col, edi, false, if with_alpha { 4 } else { 3 });
            self.put(255 - edi, tcol, c);
        }
    }

    /// 0xFB8030: the columns to the right of `tcol` take its colours shifted `4 + 4 j` rows
    /// (`dst(r, tcol + 1 + j) = src(r - 4 - 4 j, tcol)` for the rows `r >= 4 + 4 j`).
    fn skew_right(&mut self, tcol: usize) {
        for j in 0..63usize {
            for k in 0..(252 - 4 * j) {
                let v = self.at(251 - 4 * j - k, tcol);
                self.put(255 - k, tcol + 1 + j, v);
            }
        }
    }

    /// 0xFB83E0: the columns to the left of `tcol`, shifted the other way
    /// (`dst(r, tcol - 1 - n) = src(r + 4 + 4 n, tcol)` for `r <= 251 - 4 n`).
    fn skew_left(&mut self, tcol: usize) {
        for n in 0..63usize {
            for k in 0..(252 - 4 * n) {
                let v = self.at(255 - k, tcol);
                self.put(251 - 4 * n - k, tcol - 1 - n, v);
            }
        }
    }

    /// 0xFB80D0 (`right`) / 0xFB84B0: blends the skewed edge, in groups of four rows, with its
    /// left neighbour (weights `4 - s` and `s` of 4 for row `255 - 4 i - s`; alpha untouched).
    fn smooth(&mut self, tcol: usize, right: bool) {
        for i in 0..64usize {
            for s in 0..4usize {
                let row = 255 - 4 * i - s;
                let col = if right { tcol + 64 - i } else { tcol - i };
                let a = self.at(row, col - 1);
                let mut d = self.at(row, col);
                for k in 0..3 {
                    d[k] = ((a[k] as usize * (4 - s) + d[k] as usize * s) >> 2) as u8;
                }
                self.put(row, col, d);
            }
        }
    }

    /// The table as the Above pass samples it (sampler 1: bilinear, clamp, no mips) at the
    /// normalised coordinates (`u` = column, `v` = the map value): rgb (0..1, R, G, B order)
    /// and alpha.
    pub fn sample(&self, u: f32, v: f32) -> ([f32; 3], f32) {
        let n = LIFE_TABLE_SIZE as f32;
        let x = (u * n - 0.5).clamp(0.0, n - 1.0);
        let y = (v * n - 0.5).clamp(0.0, n - 1.0);
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(LIFE_TABLE_SIZE - 1), (y0 + 1).min(LIFE_TABLE_SIZE - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let mut out = [0.0f32; 4];
        for (k, o) in out.iter_mut().enumerate() {
            let tex = |r: usize, c: usize| self.at(r, c)[k] as f32 / 255.0;
            let top = tex(y0, x0) + (tex(y0, x1) - tex(y0, x0)) * fx;
            let bottom = tex(y1, x0) + (tex(y1, x1) - tex(y1, x0)) * fx;
            *o = top + (bottom - top) * fy;
        }
        // B, G, R, A -> R, G, B
        ([out[2], out[1], out[0]], out[3])
    }

    /// Pass 1 at one map value for a table `column` (`above_pass` with this table).
    pub fn above(&self, value: f32, column: f32) -> ([f32; 3], f32) {
        above_pass(|u, v| self.sample(u, v).0, value, column)
    }
}

/// 0xFB88A0: which table column the planet's temperature and atmosphere select. Returns the
/// column and its fraction as `terrainSynthParams` c0.x = mgr +0x44C holds them
/// (`column + fraction`; the fraction is the weight of the climate column against the base
/// column 192 in the Above program). `temperature` = mMaterial.mTemperatureScore (mgr +0x30),
/// `atmosphere_reciprocal` = mAtmosphereReciprocalScore (mgr +0x2C, `1 - atmosphere`).
///
/// The code reads two globals (0x16D66F0 / 0x16D66F4) as the table ends of the lowest and
/// highest band; no instruction of the image stores to them (they lie in the zero-filled tail
/// of `.data`), so they are 0: the bands run 0 .. 63 for T < 0.2, 63 .. 191 for T in
/// 0.2 .. 0.8, 191 .. 255 above.
pub fn life_column(temperature: f32, atmosphere_reciprocal: f32) -> f32 {
    const A: f32 = 0.0; // [0x16D66F0]
    const B: f32 = 0.0; // [0x16D66F4]
    let x = temperature;
    let k255 = f32::from_bits(0x437F_0000);
    let lo = f32::from_bits(0x3E4C_CCCD); // 0.2 = [0x15B15C0]
    let hi = f32::from_bits(0x3F4C_CCCD); // 0.8 = [0x15B15B4]
    let (col, alpha): (f32, f32) = if x < lo {
        let t = (x - B) / (lo - B);
        (t * (f32::from_bits(0x3E7C_FCFD) - A) + A, f32::from_bits(0x3F7F_BE77))
    } else if x < hi {
        let c0 = f32::from_bits(0x3E7C_FCFD);
        let t = (x - lo) / (hi - lo);
        let col = t * (f32::from_bits(0x3F3F_BFC0) - c0) + c0;
        let two_y_minus_one = 2.0 * atmosphere_reciprocal as f64 - 1.0;
        let alpha = ((1.0 - two_y_minus_one.abs()) * f32::from_bits(0x3F7F_BE77) as f64) as f32;
        (col, alpha)
    } else {
        let c1 = f32::from_bits(0x3F3F_BFC0);
        let t = (x - hi) / (1.0 - hi);
        (t * (1.0 - c1) + c1, f32::from_bits(0x3F7F_BE77))
    };
    let n = (col * k255).round_ties_even() as i32;
    (n as f64 + alpha as f64) as f32
}

/// The ramp column of a world (0xFBBBD0: `floor(mgr +8 * 63 + 0.5)`; mgr +8 and +0xC are
/// `(terrainScriptPlanetInfo.x - 0.5) / 64`, 0xFBD850 as called by the planet settings parser
/// 0xF35548 .. 0xF356E4 and by 0xF9B3A0; the scenario game mode reads +0xC for the dead ramp
/// only, so the dead and the living / ice / lava columns are the same in the other stages).
pub fn ramp_column(planet_info_x: f32) -> usize {
    let m = (planet_info_x - 0.5) * 0.015_625;
    floor_like(m * 63.0 + 0.5).clamp(0, RAMP_IMAGE_SIZE as i32 - 1) as usize
}

/// 0xFB9AA0 / 0xFB9C70: the beach and cliff colours (`beachColor` c0 of pass 2 and
/// `cliffColor` c0 of pass 3, rgb 0..1) from the temperature score. The band colours are rows
/// 0 .. 7 of the band image (`0xF30D0A76.32bitImage`, 64 x 10, stored by 0xFBBBD0 at mgr
/// +0x50C + 16 row) at the ramp column; the temperature picks the row: beach rows / cliff
/// rows 2 / 6 for T <= 0.125, 0 / 4 for T <= 0.375, 1 / 5 for T <= 0.625, 0 / 4 for
/// T <= 0.875 and 3 / 7 above.
pub fn band_colours(bands: &[u8], col_a: usize, col_b: usize, temperature: f32) -> ([f32; 3], [f32; 3]) {
    let t = temperature;
    let (beach_row, cliff_row) = if t <= 0.125 {
        (2usize, 6usize)
    } else if t <= 0.375 || (t > 0.625 && t <= 0.875) {
        (0, 4)
    } else if t <= 0.625 {
        (1, 5)
    } else {
        (3, 7)
    };
    let pick = |row: usize| -> [f32; 3] {
        // rows 0 and 4 come from the dead column (mgr +8 / +0xC), the others from mgr +8.
        let col = if row == 0 || row == 4 { col_a } else { col_b };
        let i = (row * RAMP_IMAGE_SIZE + col) * 4;
        // B, G, R, A -> R, G, B (x 0.003921569 = [0x13EECB4])
        [bands[i + 2], bands[i + 1], bands[i]].map(|b| b as f32 * 0.003_921_569)
    };
    (pick(beach_row), pick(cliff_row))
}

/// The top of the beach band on the packed `100 * h` scale: `(mBeachLevel + mWaterLevel) *
/// mAltitudeRange` (`terrainSynthParams` second vector z = cTerrainStateMgr +0x444, from
/// cTerrainMapSet +0x44 / +0x3C / +0x38 in 0xFBF540).
pub fn beach_top(beach_level: f32, water_level: f32, altitude_range: f32) -> f32 {
    (beach_level + water_level) * altitude_range
}

/// Pass 2 (`TerrainBrushTextureBeachShader` 0x7000002D): the beach colour's alpha at a
/// texel of height `h` (the h scale, -1..1): full below `top`, linear to zero over one
/// altitude unit (`sat(1 - min(100 h - top, 1))`).
pub fn beach_alpha(h: f32, top: f32) -> f32 {
    (1.0 - (HEIGHT_SCALE * h - top).min(1.0)).clamp(0.0, 1.0)
}

/// Pass 3 (`TerrainBrushTextureCliffShader` 0x7000002E): the cliff alpha from the normal map's
/// `w` channel (`normal_w`, 0..1), the height `h` (h scale), the cliff gradients
/// (`min_gradient` = c1.z = cTerrainMapSet +0x48, `max_gradient` = c1.w = +0x4C, stored by 0xFC0E7F
/// from two planet-settings properties) and `water_cut` = the second vector's w
/// (cTerrainStateMgr +0x448, `(mapset +0x40 + water) * altitude`): no cliff colour below
/// `water_cut - 2` altitude units.
pub fn cliff_alpha(normal_w: f32, h: f32, min_gradient: f32, max_gradient: f32, water_cut: f32) -> f32 {
    let g = HEIGHT_SCALE * normal_w - min_gradient;
    let r = max_gradient - g;
    let ramp = (1.0 - r / max_gradient).max(0.0);
    let slope = if r >= 0.0 { ramp } else { 1.0 };
    let above_water = if -HEIGHT_SCALE * h + (water_cut - 2.0) >= 0.0 { 0.0 } else { 1.0 };
    slope * above_water
}

/// One alpha-blended pass (SRCALPHA / INVSRCALPHA, ADD): `dst + (src - dst) * a`.
pub fn blend_over(dst: [f32; 3], src: [f32; 3], a: f32) -> [f32; 3] {
    [0, 1, 2].map(|k| dst[k] + (src[k] - dst[k]) * a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_rows_map_onto_the_ramp_as_0xfb7c40_writes_them() {
        assert_eq!(table_row_ramp_position(255), (63, 0.0));
        assert_eq!(table_row_ramp_position(0), (1, 0.0));
        // one ramp row per 4.11 table rows
        let (r, f) = table_row_ramp_position(255 - 8);
        assert_eq!(r, 63 - 1);
        assert!((f - (8.0 * 0.243_137_26 - 1.0)).abs() < 1e-5);
        // monotone: lower rows reach lower ramp rows
        let mut last = 64.0;
        for row in (0..=255).rev() {
            let (r, f) = table_row_ramp_position(row);
            let pos = r as f32 - f;
            assert!(pos <= last + 1e-4);
            last = pos;
        }
    }

    #[test]
    fn the_above_pass_lerps_between_the_base_column_and_the_climate_column() {
        // A table whose colour is its column number / 255 in r, the row in g, 0 in b.
        let table = |u: f32, v: f32| [u, v, 0.0];
        // column 100.25 -> floor 100, fraction 0.25: lerp(table(0.75098), table(100 / 255), 0.25).
        let (rgb, a) = above_pass(table, 0.5, 100.25);
        let expect_r = 0.75098 + 0.25 * (100.0 * 0.003_921_57 - 0.75098);
        assert!((rgb[0] - expect_r).abs() < 1e-5, "{rgb:?}");
        assert!((rgb[1] - 0.5).abs() < 1e-6);
        assert!((a - (0.5 * 0.99 + 0.01)).abs() < 1e-6);
        // fraction 0 keeps the base column (texel 192), the 0.999 of 0xFB88A0 nearly all of the
        // climate column.
        let (rgb, _) = above_pass(table, 0.0, 63.0);
        assert!((rgb[0] - ABOVE_BASE_COLUMN_U).abs() < 1e-6, "{rgb:?}");
        let (rgb, _) = above_pass(table, 0.0, 63.999);
        assert!((rgb[0] - 63.0 * 0.003_921_57).abs() < 2e-3, "{rgb:?}");
        // the alpha never reaches zero (the pass is alpha tested against 0).
        assert!(above_pass(table, 0.0, 0.0).1 >= 0.01);
    }

    #[test]
    fn the_beach_band_is_full_to_the_top_and_one_unit_soft_above() {
        // retail planets: altitude range 100, beach level 0.025, water at h = -0.2.
        let top = beach_top(0.025, -0.2, 100.0);
        assert!((top - (-17.5)).abs() < 1e-4);
        // altitude above the water in world units = (h - water) * 100
        let at = |above: f32| beach_alpha(-0.2 + above / 100.0, top);
        assert_eq!(at(-5.0), 1.0);
        assert_eq!(at(0.0), 1.0);
        assert!((at(2.5) - 1.0).abs() < 1e-5);
        assert!((at(3.0) - 0.5).abs() < 1e-4);
        assert!(at(3.5).abs() < 1e-5);
        assert_eq!(at(10.0), 0.0);
    }

    #[test]
    fn the_cliff_alpha_ramps_with_the_gradient_and_stops_at_the_water() {
        let (min_g, max_g) = (32.0, 45.0);
        let water_cut = 100.0 * -0.2;
        let a = |nw: f32, h: f32| cliff_alpha(nw, h, min_g, max_g, water_cut);
        // below the minimum gradient: no cliff
        assert_eq!(a(0.20, 0.1), 0.0);
        // at the minimum the ramp starts, it grows with the gradient
        assert!(a(0.32, 0.1).abs() < 1e-6);
        let mid = a(0.50, 0.1);
        assert!(mid > 0.3 && mid < 0.6, "{mid}");
        // beyond min + max the colour is fully opaque
        assert_eq!(a(0.90, 0.1), 1.0);
        // under the water cut nothing is drawn
        assert_eq!(a(0.90, -0.5), 0.0);
    }

    #[test]
    fn the_passes_blend_in_sequence() {
        let land = [0.2, 0.4, 0.1];
        let beach = [0.8, 0.7, 0.5];
        let cliff = [0.4, 0.4, 0.4];
        let c = blend_over(blend_over(land, beach, 0.5), cliff, 0.25);
        let mid = [0.5, 0.55, 0.3];
        for k in 0..3 {
            assert!((c[k] - (mid[k] + (cliff[k] - mid[k]) * 0.25)).abs() < 1e-6);
        }
        assert_eq!(blend_over(land, beach, 0.0), land);
        assert_eq!(blend_over(land, beach, 1.0), beach);
    }

    #[test]
    fn the_pass_materials_name_the_installed_terrain_programs() {
        // The installed table (checked with `scripts\mats.py`): compiled state of material
        // 0xE8078E3B names program 0x7000002C, 0x100CB7AD 0x7000002D, 0x19E45172 0x7000002E.
        assert_eq!(PASSES[0].1 + 1, PASSES[1].1);
        assert_eq!(PASSES[1].1 + 1, PASSES[2].1);
        assert!(PASSES.iter().all(|p| p.2.starts_with("TerrainBrushTexture")));
    }

    /// The generator of the synthetic ramps used to check the table against the retail code
    /// (`scripts\emu.py synth_ramps`): an LCG byte stream.
    fn lcg_bytes(seed: u32, n: usize) -> Vec<u8> {
        let mut x = seed;
        (0..n)
            .map(|_| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (x >> 24) as u8
            })
            .collect()
    }

    fn fnv1a64(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0xCBF2_9CE4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01B3))
    }

    #[test]
    fn the_life_table_matches_the_retail_routines() {
        // Hashes of the 256 KB table that 0xFB8760 writes when it is executed from the code
        // image (unicorn, `space-livedead\scripts\emu.py`, `hashes.py`) on the same synthetic
        // ramps (lava, dead, living, ice order of one LCG stream, seed 1).
        let all = lcg_bytes(1, 4 * RAMP_IMAGE_BYTES);
        let (lava, dead, living, ice) = (
            &all[..RAMP_IMAGE_BYTES],
            &all[RAMP_IMAGE_BYTES..2 * RAMP_IMAGE_BYTES],
            &all[2 * RAMP_IMAGE_BYTES..3 * RAMP_IMAGE_BYTES],
            &all[3 * RAMP_IMAGE_BYTES..],
        );
        for (col_a, col_b, hash) in [
            (20usize, 30usize, 0xEAFE_8A0B_488D_6F98u64),
            (0, 0, 0x5168_2529_D1CC_DC48),
            (63, 63, 0xBD25_6E29_CB8A_1264),
            (17, 40, 0xE5F5_1540_0B1F_6D6D),
        ] {
            let table = LifeTable::build(dead, living, ice, lava, col_a, col_b);
            assert_eq!(fnv1a64(table.bytes()), hash, "columns {col_a} / {col_b}");
        }
    }

    #[test]
    fn the_life_column_follows_0xfb88a0() {
        // (temperature, 1 - atmosphere, bits of the column + fraction): 0xFB88A0 executed from
        // the code image (`scripts\vectors.py`).
        for (t, y, bits) in [
            (0.0f32, 0.5f32, 0x3F7FBE77u32),
            (0.05, 0.3, 0x4187FDF4),
            (0.1, 0.9, 0x4203FEFA),
            (0.19, 0.5, 0x4273FEFA),
            (0.2, 0.5, 0x427FFEFA),
            (0.25, 0.0, 0x42940000),
            (0.3, 0.25, 0x42A8FFBE),
            (0.45, 0.5, 0x42E9FF7D),
            (0.5, 0.75, 0x42FEFFBE),
            (0.6, 1.0, 0x43140000),
            (0.79, 0.4, 0x433DCC98),
            (0.8, 0.5, 0x433FFFBE),
            (0.9, 0.5, 0x435FFFBE),
            (1.0, 0.1, 0x437FFFBE),
        ] {
            let got = life_column(t, y);
            let want = f32::from_bits(bits);
            assert!((got - want).abs() <= 1.0e-4 * want.max(1.0), "T {t} y {y}: {got} vs {want}");
        }
        // The columns run ice (0) .. living (127) .. lava (255) with temperature, the atmosphere
        // only weights the middle band (zero at the ends of its range, full at 0.5).
        assert_eq!(life_column(0.0, 0.5).floor(), 0.0);
        assert_eq!(life_column(0.5, 0.5).floor(), 127.0);
        assert_eq!(life_column(1.0, 0.5).floor(), 255.0);
        assert_eq!(life_column(0.5, 0.0).fract(), 0.0);
        assert!(life_column(0.5, 0.5).fract() > 0.99);
    }

    #[test]
    fn the_ramp_column_follows_the_planet_info() {
        // floor(((x - 0.5) / 64) * 63 + 0.5): the default x = 32 selects column 31.
        assert_eq!(ramp_column(32.0), 31);
        assert_eq!(ramp_column(1.0), 0);
        assert_eq!(ramp_column(64.0), 63);
        assert_eq!(ramp_column(8.0), 7);
        assert_eq!(ramp_column(54.0), 53);
    }

    #[test]
    fn the_band_colours_follow_0xfb9aa0_and_0xfb9c70() {
        // Band colours of a synthetic image (`scripts\vectors.py`, run_bands): the beach / cliff
        // rows by temperature zone from the retail code.
        let bytes = lcg_bytes(7, 4 * RAMP_IMAGE_BYTES);
        let bands = &bytes[RAMP_IMAGE_BYTES..RAMP_IMAGE_BYTES + RAMP_IMAGE_SIZE * 10 * 4];
        let cases: [(f32, [f32; 3], [f32; 3]); 9] = [
            (0.0, [0.611_764_73, 0.639_215_7, 0.180_392_16], [0.321_568_64, 0.662_745_1, 0.447_058_86]),
            (0.125, [0.611_764_73, 0.639_215_7, 0.180_392_16], [0.321_568_64, 0.662_745_1, 0.447_058_86]),
            (0.2, [0.262_745_1, 0.839_215_76, 0.784_313_8], [0.043_137_256, 0.043_137_256, 0.384_313_76]),
            (0.375, [0.262_745_1, 0.839_215_76, 0.784_313_8], [0.043_137_256, 0.043_137_256, 0.384_313_76]),
            (0.5, [0.709_803_94, 0.549_019_63, 0.333_333_34], [0.756_862_8, 0.780_392_23, 0.223_529_42]),
            (0.625, [0.709_803_94, 0.549_019_63, 0.333_333_34], [0.756_862_8, 0.780_392_23, 0.223_529_42]),
            (0.7, [0.262_745_1, 0.839_215_76, 0.784_313_8], [0.043_137_256, 0.043_137_256, 0.384_313_76]),
            (0.875, [0.262_745_1, 0.839_215_76, 0.784_313_8], [0.043_137_256, 0.043_137_256, 0.384_313_76]),
            (0.95, [0.713_725_5, 0.333_333_34, 0.403_921_6], [0.337_254_9, 0.396_078_47, 0.298_039_23]),
        ];
        for (t, beach, cliff) in cases {
            let (b, c) = band_colours(bands, 5, 9, t);
            for k in 0..3 {
                assert!((b[k] - beach[k]).abs() < 1.0e-6, "beach T {t}: {b:?} vs {beach:?}");
                assert!((c[k] - cliff[k]).abs() < 1.0e-6, "cliff T {t}: {c:?} vs {cliff:?}");
            }
        }
    }

    #[test]
    fn the_table_carries_the_special_ramps_in_their_columns() {
        // Flat ramps: dead red, living green, ice blue, lava grey (B, G, R, A bytes).
        let flat = |bgra: [u8; 4]| -> Vec<u8> { bgra.iter().copied().cycle().take(RAMP_IMAGE_BYTES).collect() };
        let (dead, living, ice, lava) =
            (flat([0, 0, 200, 255]), flat([0, 200, 0, 250]), flat([200, 0, 0, 255]), flat([200, 200, 200, 255]));
        let t = LifeTable::build(&dead, &living, &ice, &lava, 3, 3);
        // the special columns at a mid row
        assert_eq!(t.at(128, LIFE_COLUMN_ICE)[..3], [200, 0, 0]);
        assert_eq!(t.at(128, LIFE_COLUMN_LIVING), [0, 200, 0, 250]);
        assert_eq!(t.at(128, LIFE_COLUMN_LAVA)[..3], [200, 200, 200]);
        // left of the living column the colours shift down, so the top rows keep the dead ramp
        // (and no alpha); 0xFB84B0 also gives the column's own top four rows back to it
        assert_eq!(t.at(200, 100)[..3], [0, 0, 200]);
        assert_eq!(t.at(200, 100)[3], 0);
        assert_eq!(t.at(100, 100), [0, 200, 0, 250]);
        assert_eq!(t.at(255, LIFE_COLUMN_LIVING)[..3], [0, 0, 200]);
        // right of it the low rows keep the dead ramp
        assert_eq!(t.at(5, LIFE_COLUMN_LIVING + 10)[..3], [0, 0, 200]);
        // the dead ramp between the special columns, past the feathers
        assert_eq!(t.at(5, LIFE_COLUMN_LIVING + 62)[..3], [0, 0, 200]);
        // the living ramp spreads to its neighbours (skewed to higher rows to the right)
        assert_eq!(t.at(200, LIFE_COLUMN_LIVING + 3)[3], 250);
        assert_eq!(t.at(2, LIFE_COLUMN_LIVING + 3)[3], 0, "the low rows keep the dead ramp");
        // bilinear sampling in the climate column reads the column's colour (R, G, B order)
        let (rgb, _) = t.sample(LIFE_COLUMN_LIVING as f32 / 255.0, 0.5);
        assert!(rgb[1] > 0.7 && rgb[0] < 0.1, "{rgb:?}");
    }
}
