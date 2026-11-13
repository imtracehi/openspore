//! Cell-stage animation: plays the installed `cell_*` clips on Cell models.
//!
//! Native data path (details and evidence in `notes/cell-animation-notes.md`):
//! the engine writes a retail request code (`CellAnimState::request_code`);
//! [`select`] picks a row of the retail `cCellAnimDefinition` table ([`ROWS`],
//! dumped from the executable by cell-re) exactly as `0xE6CBD0` does; the row's
//! animation id is a TLSA group (`0xAAAA00xx`) naming one ANIM clip. The clip's
//! `Root`/`CrFr` channels move the whole body; its part channels (bound by
//! capability tag: `mtcl`, `mvcl`, `eycl`, `wpcl`, ...) only carry deform
//! tracks, and every part rig maps a deform id through its RW4 `0xFF0001`
//! table to its own keyframe clip, sampled at the track's normalized phase.
//! Playback follows `0xE6D340`/`0xE6D200`: one-shots fall back to the return
//! code, 0.2 s blends, `anim_dilation` time scaling by size ratio.
//!
//! Skinning is GPU skinning with shared immutable vertex data and a joint
//! palette per actor (the `locomotion_preview` pattern): every NPC GMDL
//! surface gets one shared skinned copy; each actor owns its joint entities.
//! The rig-to-model placement is the inverse of the GMDL binding's primary
//! transform and RW4 inverse binds are read in their row-vector layout.
//!
//! [`CellClipTimes`] answers what `PlayAnimation` (0xE6D200) answers a caller
//! that times a game event with the clip: the clip's length x the
//! `anim_dilation` factor of `realSize[cell] / realSize[player]` (0xE563E0),
//! 1.0 for a clip without length. The engine writes the retail request codes
//! (`CellAnimState::request_code`) for every cell, so the earlier interim
//! driver is gone.
//!
//! ADAPTED (marked inline): how several weighted deform tracks on one part
//! combine; root / creature-frame pivots; the eye left / right reading; the
//! player body's mesh-space mapping. The root deform 0x70E47545 is NOT a body
//! scale (it is an audio property, see [`ROOT_AUDIO_DEFORM`]).
use crate::{
    asset_catalog::{Capabilities, PartDefinition, ResourceIndex, CELL_PARTS, RW4},
    cell_contract::CellAnimState,
    cell_visuals::OriginalCellVisual,
    dbpf::ResourceKey,
    rw4,
    stages::StageOwned,
    Originals,
};
use bevy::{
    camera::visibility::DynamicSkinnedMeshBounds,
    math::Affine3A,
    mesh::{
        skinning::{SkinnedMesh, SkinnedMeshInverseBindposes},
        VertexAttributeValues,
    },
    prelude::*,
};
use std::{collections::HashMap, sync::Arc};

// ---------------------------------------------------------------------------
// Retail definition table and selection (original data + decoded logic)
// ---------------------------------------------------------------------------

/// One row of the retail `cCellAnimDefinition` table (.rdata 0x1483EB0,
/// 131 x 0x24 bytes, `notes/cell-re/cell_anim_definitions.txt`). The row
/// index is the ModAPI `CellAnimations` value (`CellAnimation` in the contract).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimRow {
    /// Request code this row answers (+0x00).
    pub code: i32,
    /// TLSA group id of the clip (+0x04).
    pub anim: u32,
    /// Weighted-random weight (+0x08).
    pub weight: f32,
    /// 0 player only, 1 NPC only, 2 both (+0x0C).
    pub selector: u8,
    /// 1 always; 0 fallback only; otherwise the UnlockType of a required part
    /// (with the jet > flagella > cilia precedence for 8/9/10) (+0x10).
    pub condition: u8,
    /// Playing this row makes its code the return code one-shots fall back to
    /// (byte +0x14, written to cell+0x1B4).
    pub sets_return: bool,
    /// Argument of the first size predicate `FUN_00E5C460` (+0x18).
    pub size_test: u8,
    /// Argument of the second predicate `FUN_00E5C410` (+0x1C).
    pub kind_test: u8,
    /// Duration and blends follow `anim_dilation` (byte +0x20).
    pub dilate: bool,
}

#[allow(clippy::too_many_arguments)]
const fn row(
    code: i32,
    anim: u32,
    weight: f32,
    selector: u8,
    condition: u8,
    p1: u8,
    p2: u8,
    kind: u8,
    f20: u8,
) -> AnimRow {
    AnimRow {
        code,
        anim,
        weight,
        selector,
        condition,
        sets_return: p1 != 0,
        size_test: p2,
        kind_test: kind,
        dilate: f20 != 0,
    }
}

/// The retail table, row for row (original data).
pub const ROWS: [AnimRow; 131] = [
    row(0, 0xAAAA0015, 5.0, 2, 1, 0, 1, 0, 1),
    row(0, 0xAAAA0016, 1.0, 2, 11, 1, 1, 0, 1),
    row(0, 0xAAAA0017, 1.0, 2, 11, 1, 1, 0, 1),
    row(0, 0xAAAA0018, 1.0, 2, 11, 1, 1, 0, 1),
    row(18, 0xAAAA0019, 5.0, 2, 9, 1, 1, 0, 1),
    row(18, 0xAAAA0020, 1.0, 2, 9, 1, 1, 0, 1),
    row(11, 0xAAAA0005, 1.0, 0, 3, 0, 0, 0, 1),
    row(4, 0xAAAA0000, 1.0, 2, 1, 0, 0, 0, 1),
    row(5, 0xAAAA0001, 1.0, 0, 1, 0, 0, 0, 1),
    row(7, 0xAAAA0002, 1.0, 0, 1, 0, 0, 0, 1),
    row(7, 0xAAAA0003, 1.0, 1, 1, 0, 0, 0, 1),
    row(10, 0xAAAA0004, 1.0, 1, 1, 1, 0, 0, 1),
    row(11, 0xAAAA0006, 1.0, 1, 3, 0, 0, 0, 1),
    row(12, 0xAAAA0007, 1.0, 0, 2, 0, 1, 0, 1),
    row(12, 0xAAAA0008, 1.0, 1, 2, 0, 1, 0, 1),
    row(13, 0xAAAA0009, 1.0, 0, 4, 0, 0, 0, 1),
    row(14, 0xAAAA0011, 1.0, 0, 1, 0, 0, 0, 1),
    row(15, 0xAAAA0012, 1.0, 0, 1, 0, 0, 0, 1),
    row(16, 0xAAAA0013, 1.0, 0, 1, 0, 0, 0, 1),
    row(18, 0xAAAA0021, 5.0, 2, 10, 1, 1, 0, 1),
    row(18, 0xAAAA0022, 1.0, 2, 10, 1, 1, 0, 1),
    row(18, 0xAAAA0019, 0.001, 2, 0, 1, 0, 0, 1),
    row(19, 0xAAAA0023, 1.0, 2, 0, 1, 0, 0, 1),
    row(20, 0xAAAA0024, 1.0, 2, 0, 1, 0, 0, 1),
    row(19, 0xAAAA0023, 1.0, 1, 9, 1, 0, 0, 1),
    row(20, 0xAAAA0024, 1.0, 1, 9, 1, 1, 0, 1),
    row(19, 0xAAAA0021, 1.0, 1, 10, 1, 0, 0, 1),
    row(20, 0xAAAA0021, 1.0, 1, 10, 1, 0, 0, 1),
    row(21, 0xAAAA0025, 1.0, 2, 1, 0, 0, 0, 1),
    row(22, 0xAAAA0026, 1.0, 2, 1, 0, 1, 0, 1),
    row(23, 0xAAAA0027, 1.0, 2, 1, 1, 0, 0, 1),
    row(24, 0xAAAA0028, 1.0, 2, 1, 0, 0, 0, 1),
    row(25, 0xAAAA0029, 1.0, 2, 1, 0, 0, 0, 1),
    row(26, 0xAAAA0030, 1.0, 2, 1, 0, 0, 0, 1),
    row(27, 0xAAAA0031, 1.0, 2, 1, 0, 0, 0, 1),
    row(28, 0xAAAA0032, 1.0, 2, 1, 0, 0, 0, 1),
    row(29, 0xAAAA0033, 1.0, 2, 1, 0, 0, 0, 1),
    row(10, 0xAAAA0036, 1.0, 0, 1, 1, 0, 0, 1),
    row(30, 0xAAAA0037, 1.0, 2, 1, 0, 0, 0, 1),
    row(31, 0xAAAA0038, 1.0, 2, 1, 0, 0, 0, 1),
    row(32, 0xAAAA0039, 1.0, 0, 1, 0, 0, 0, 1),
    row(35, 0xAAAA0040, 1.0, 2, 1, 0, 0, 0, 1),
    row(36, 0xAAAA0041, 1.0, 2, 1, 0, 0, 0, 1),
    row(14, 0xAAAA0042, 1.0, 1, 1, 0, 1, 0, 1),
    row(15, 0xAAAA0043, 1.0, 1, 1, 0, 1, 0, 1),
    row(16, 0xAAAA0044, 1.0, 1, 1, 0, 1, 0, 1),
    row(37, 0xAAAA0045, 1.0, 2, 1, 0, 0, 0, 1),
    row(33, 0xAAAA0046, 1.0, 0, 1, 0, 0, 0, 1),
    row(34, 0xAAAA0047, 1.0, 0, 1, 0, 0, 0, 1),
    row(12, 0xAAAA0048, 1.0, 1, 2, 0, 2, 0, 0),
    row(0, 0xAAAA0049, 5.0, 2, 1, 0, 2, 1, 1),
    row(0, 0xAAAA0050, 1.0, 2, 11, 1, 2, 1, 1),
    row(0, 0xAAAA0051, 1.0, 2, 11, 1, 2, 1, 1),
    row(0, 0xAAAA0052, 1.0, 2, 11, 1, 2, 1, 1),
    row(18, 0xAAAA0053, 5.0, 2, 9, 1, 2, 0, 1),
    row(18, 0xAAAA0054, 1.0, 2, 9, 1, 2, 0, 1),
    row(20, 0xAAAA0055, 3.0, 1, 9, 1, 2, 0, 1),
    row(18, 0xAAAA0056, 5.0, 2, 8, 1, 2, 0, 1),
    row(18, 0xAAAA0057, 1.0, 2, 8, 1, 2, 0, 1),
    row(20, 0xAAAA0058, 3.0, 1, 8, 1, 2, 0, 1),
    row(18, 0xAAAA0056, 5.0, 2, 10, 1, 2, 0, 1),
    row(18, 0xAAAA0057, 1.0, 2, 10, 1, 2, 0, 1),
    row(20, 0xAAAA0058, 3.0, 1, 10, 1, 2, 0, 1),
    row(22, 0xAAAA0059, 3.0, 2, 1, 1, 2, 0, 1),
    row(18, 0xAAAA0060, 5.0, 2, 8, 1, 1, 0, 1),
    row(18, 0xAAAA0061, 1.0, 2, 8, 1, 1, 0, 1),
    row(6, 0xAAAA0062, 1.0, 2, 1, 0, 0, 0, 1),
    row(20, 0xAAAA0063, 5.0, 1, 8, 1, 1, 0, 1),
    row(19, 0xAAAA0064, 1.0, 1, 8, 1, 1, 0, 1),
    row(8, 0xAAAA0065, 1.0, 0, 1, 0, 0, 0, 1),
    row(40, 0xAAAA0066, 1.0, 2, 1, 0, 0, 0, 1),
    row(41, 0xAAAA0067, 1.0, 2, 1, 0, 0, 0, 1),
    row(43, 0xAAAA0068, 1.0, 2, 1, 0, 0, 0, 1),
    row(44, 0xAAAA0069, 1.0, 2, 1, 0, 0, 0, 1),
    row(45, 0xAAAA0070, 1.0, 2, 1, 0, 0, 0, 1),
    row(9, 0xAAAA0071, 1.0, 0, 1, 1, 0, 0, 1),
    row(9, 0xAAAA0072, 1.0, 1, 1, 1, 0, 0, 1),
    row(17, 0xAAAA0073, 1.0, 0, 1, 0, 0, 0, 1),
    row(49, 0xAAAA0074, 1.0, 2, 1, 0, 0, 0, 1),
    row(1, 0xAAAA0075, 1.0, 2, 1, 0, 0, 0, 1),
    row(2, 0xAAAA0076, 1.0, 2, 1, 0, 0, 0, 1),
    row(3, 0xAAAA0077, 1.0, 2, 1, 0, 0, 0, 1),
    row(5, 0xAAAA0078, 1.0, 1, 1, 0, 0, 0, 1),
    row(8, 0xAAAA0079, 1.0, 1, 1, 0, 0, 0, 1),
    row(17, 0xAAAA0080, 1.0, 1, 1, 0, 1, 0, 1),
    row(46, 0xAAAA0081, 1.0, 2, 1, 0, 0, 0, 1),
    row(38, 0xAAAA0082, 1.0, 2, 1, 0, 0, 0, 1),
    row(20, 0xAAAA0083, 1.0, 1, 9, 1, 2, 0, 1),
    row(39, 0xAAAA0084, 1.0, 2, 1, 0, 0, 0, 1),
    row(47, 0xAAAA0085, 1.0, 2, 1, 0, 0, 0, 1),
    row(47, 0xAAAA0086, 1.0, 2, 1, 0, 0, 0, 1),
    row(47, 0xAAAA0087, 1.0, 2, 1, 0, 0, 0, 1),
    row(47, 0xAAAA0088, 1.0, 2, 1, 0, 0, 0, 1),
    row(47, 0xAAAA0089, 1.0, 2, 1, 0, 0, 0, 1),
    row(48, 0xAAAA0090, 5.0, 2, 9, 1, 0, 0, 1),
    row(48, 0xAAAA0090, 0.001, 2, 0, 1, 0, 0, 1),
    row(48, 0xAAAA0091, 5.0, 2, 8, 1, 0, 0, 1),
    row(48, 0xAAAA0092, 5.0, 2, 10, 1, 0, 0, 1),
    row(15, 0xAAAA0093, 1.0, 1, 1, 0, 2, 0, 1),
    row(50, 0xAAAA0095, 1.0, 2, 1, 0, 0, 0, 1),
    row(51, 0xAAAA0096, 1.0, 2, 1, 0, 0, 0, 1),
    row(0, 0xAAAA0097, 5.0, 2, 1, 0, 0, 2, 1),
    row(14, 0xAAAA0098, 1.0, 1, 1, 0, 2, 0, 1),
    row(52, 0xAAAA0099, 1.0, 1, 1, 0, 0, 0, 0),
    row(53, 0xAAAA0100, 1.0, 2, 1, 0, 0, 0, 0),
    row(54, 0xAAAA0101, 1.0, 2, 1, 0, 0, 0, 0),
    row(55, 0xAAAA0102, 1.0, 2, 1, 0, 0, 0, 0),
    row(56, 0xAAAA0103, 1.0, 2, 1, 0, 0, 0, 0),
    row(57, 0xAAAA0104, 1.0, 2, 1, 0, 0, 0, 0),
    row(58, 0xAAAA0105, 1.0, 2, 1, 0, 0, 0, 0),
    row(59, 0xAAAA0106, 1.0, 1, 1, 0, 0, 0, 0),
    row(60, 0xAAAA0107, 1.0, 1, 1, 0, 0, 0, 0),
    row(61, 0xAAAA0108, 1.0, 1, 1, 0, 0, 0, 0),
    row(62, 0xAAAA0109, 1.0, 1, 1, 0, 0, 0, 0),
    row(7, 0xAAAA0110, 100000.0, 1, 1, 0, 3, 0, 0),
    row(10, 0xAAAA0111, 100000.0, 1, 1, 1, 3, 0, 0),
    row(63, 0xAAAA0112, 1.0, 1, 1, 0, 0, 0, 0),
    row(64, 0xAAAA0113, 1.0, 1, 1, 0, 0, 0, 0),
    row(65, 0xAAAA0114, 1.0, 2, 1, 0, 0, 0, 1),
    row(66, 0xAAAA0115, 1.0, 2, 1, 0, 0, 0, 1),
    row(67, 0xAAAA0116, 1.0, 2, 1, 0, 0, 0, 1),
    row(33, 0xAAAA0117, 1.0, 1, 1, 0, 0, 0, 1),
    row(34, 0xAAAA0118, 1.0, 1, 1, 0, 0, 0, 1),
    row(32, 0xAAAA0119, 1.0, 1, 1, 0, 0, 0, 1),
    row(13, 0xAAAA0120, 1.0, 1, 4, 0, 0, 0, 1),
    row(16, 0xAAAA0121, 1.0, 1, 1, 0, 2, 0, 1),
    row(17, 0xAAAA0122, 1.0, 1, 1, 0, 2, 0, 1),
    row(16, 0xAAAA0123, 100000.0, 1, 1, 0, 3, 0, 0),
    row(17, 0xAAAA0123, 100000.0, 1, 1, 0, 3, 0, 0),
    row(68, 0xAAAA0124, 1.0, 2, 1, 0, 0, 0, 1),
    row(42, 0xAAAA0125, 1.0, 2, 1, 0, 0, 0, 1),
];

/// Retail request codes recovered by cell-re (cell+0x18C values); the engine
/// writes them as `cell_engine::anim_code`, these name the table rows tests use.
#[allow(dead_code)]
pub mod code {
    pub const IDLE: i32 = 0;
    pub const DYING: i32 = 10;
    pub const MOVING: i32 = 18;
    pub const MOVE_SCARED: i32 = 19;
    pub const MOVE_MAD: i32 = 20;
    pub const SCARED: i32 = 21;
    pub const MAD: i32 = 22;
    pub const SWIM_START: i32 = 48;
    pub const MATING_CALL: i32 = 67;
}

/// UnlockType numbering of Cell parts (ModAPI `cCellCellResource::UnlockType`).
pub mod part_type {
    pub const MANDIBLE: u8 = 2;
    pub const FILTER: u8 = 3;
    pub const PROBOSCIS: u8 = 4;
    pub const POISON: u8 = 5;
    pub const ELECTRIC: u8 = 6;
    pub const SPIKE: u8 = 7;
    pub const CILIA: u8 = 8;
    pub const FLAGELLA: u8 = 9;
    pub const JET: u8 = 10;
    pub const EYE: u8 = 11;
}

/// What `Select` reads about the cell being animated.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SelectContext {
    /// The player's cell (row selector 0) rather than an NPC (selector 1).
    pub avatar: bool,
    /// Bit `t` set when the cell has a part of UnlockType `t`.
    pub parts: u32,
    /// SizeTest / LayerTest inputs (RE 13): size > 2.0 world units, ScaleDiff
    /// MuchLarger, background pool.
    pub big: bool,
    pub epic: bool,
    pub background: bool,
}

impl SelectContext {
    fn has(&self, part: u8) -> bool {
        self.parts & (1 << part) != 0
    }
}

/// Code and player/NPC selector (native first test).
fn row_answers(row: &AnimRow, code: i32, ctx: &SelectContext) -> bool {
    row.code == code && (row.selector == 2 || row.selector == u8::from(!ctx.avatar))
}

/// Condition test of `0xE6CBD0` with no "other" cell: any condition but 1
/// requires a part of that UnlockType; movement rows add jet > flagella > cilia.
fn condition_passes(row: &AnimRow, ctx: &SelectContext) -> bool {
    match row.condition {
        1 => true,
        c => {
            if !ctx.has(c) {
                return false;
            }
            match c {
                part_type::FLAGELLA => !ctx.has(part_type::JET),
                part_type::CILIA => !ctx.has(part_type::JET) && !ctx.has(part_type::FLAGELLA),
                _ => true,
            }
        }
    }
}

/// The two predicates every row carries (`notes\cell-re-notes.md` 13, VERIFIED):
/// `SizeTest 0xE5C460(cell, size_test)`: 0 pass, 1 pass iff the cell's
/// transform scale is at most 2.0 world units, 2 pass iff it is above, 3 pass
/// iff `ScaleDiff` is MuchLarger; `LayerTest 0xE5C410(cell, kind_test)`: 0
/// pass, 1 foreground pool, 2 background pool.
fn row_passes_size_tests(row: &AnimRow, ctx: &SelectContext) -> bool {
    let size = match row.size_test {
        0 => true,
        1 => !ctx.big,
        2 => ctx.big,
        3 => ctx.epic,
        _ => false,
    };
    let kind = match row.kind_test {
        0 => true,
        1 => !ctx.background,
        2 => ctx.background,
        _ => false,
    };
    size && kind
}

/// Rows `Select` would pick from, in table order (fallback rows only when no
/// conditional row qualifies, without the size tests, as natively).
pub fn candidates(request: i32, ctx: &SelectContext) -> Vec<usize> {
    let mut out: Vec<usize> = (0..ROWS.len())
        .filter(|&i| {
            let r = &ROWS[i];
            row_answers(r, request, ctx)
                && r.condition != 0
                && condition_passes(r, ctx)
                && row_passes_size_tests(r, ctx)
        })
        .collect();
    if out.iter().map(|&i| ROWS[i].weight).sum::<f32>() == 0.0 {
        out = (0..ROWS.len())
            .filter(|&i| row_answers(&ROWS[i], request, ctx) && ROWS[i].condition == 0)
            .collect();
    }
    out
}

/// Native weighted pick: `r = random * total`, subtract weights in table order
/// until `r <= 0`. `random` is in `[0, 1)`.
pub fn select(request: i32, ctx: &SelectContext, random: f32) -> Option<usize> {
    let rows = candidates(request, ctx);
    let total: f32 = rows.iter().map(|&i| ROWS[i].weight).sum();
    let mut r = (random.clamp(0.0, 1.0) * total).max(0.0);
    for &i in &rows {
        r -= ROWS[i].weight;
        if r <= 0.0 {
            return Some(i);
        }
    }
    rows.last().copied()
}

/// `anim_dilation` graph of `cell_properties.prop` (PatchData 0!C951EB63,
/// dumped by cell-re): x = realSize ratio cell/player, y = time factor.
pub const ANIM_DILATION: [(f32, f32); 5] =
    [(0.5, 0.5), (1.0, 0.75), (2.0, 1.0), (4.0, 2.0), (8.0, 4.0)];

/// Native piecewise-linear graph lookup (0xE837C0): clamped at both ends.
pub fn graph(points: &[(f32, f32)], x: f32) -> f32 {
    let Some(first) = points.first() else {
        return 0.0;
    };
    if !x.is_finite() || x <= first.0 {
        return first.1;
    }
    for pair in points.windows(2) {
        let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
        if x <= x1 {
            let t = if x1 > x0 { (x - x0) / (x1 - x0) } else { 1.0 };
            return y0 + (y1 - y0) * t;
        }
    }
    points.last().unwrap().1
}

/// Blend-in and blend-out of `PlayAnimation` (0xE6D200) before dilation.
pub fn blend_times(leaving: Option<i32>, entering: i32) -> (f32, f32) {
    let blend_in = if entering == 47 { 0.0 } else { 0.2 };
    let blend_out = match leaving {
        Some(50) | Some(51) => 0.7,
        Some(43) | Some(44) | Some(47) => 0.0,
        _ => 0.2,
    };
    (blend_in, blend_out)
}

// ---------------------------------------------------------------------------
// Playback state machine (0xE6D340 per frame, 0xE6D200 PlayAnimation)
// ---------------------------------------------------------------------------

/// One clip instance being played.
#[derive(Clone, Debug, PartialEq)]
pub struct Playing {
    pub row: usize,
    /// ANIM instance id.
    pub clip: u32,
    /// Position in clip seconds.
    pub time: f32,
    /// Clip seconds per real second (1 / dilation).
    pub rate: f32,
    pub length: f32,
    pub cyclic: bool,
}

impl Playing {
    /// Clip-local time to sample: cyclic clips wrap, others hold the end.
    pub fn sample_time(&self) -> f32 {
        if self.length <= 0.0 {
            0.0
        } else if self.cyclic {
            self.time.rem_euclid(self.length)
        } else {
            self.time.clamp(0.0, self.length)
        }
    }
}

/// What a clip contributes before blending: (instance, seconds, cyclic).
pub type ClipFacts = (u32, f32, bool);

/// Per-cell animation playback, public so gameplay can time events with the
/// clip that is playing (retail callers use PlayAnimation's duration).
#[derive(Component, Clone, Debug, Default)]
pub struct CellAnimPlayback {
    /// Code currently playing (cell+0x1B0); `None` before the first play.
    pub code: Option<i32>,
    /// Return code for finished one-shots (cell+0x1B4).
    pub return_code: i32,
    /// Real seconds left of the current play (cell+0x1B8).
    pub remaining: f32,
    pub current: Option<Playing>,
    pub previous: Option<Playing>,
    pub blend_in: f32,
    pub blend_out: f32,
    pub blend_clock: f32,
    /// The requested one-shot has run its course (mirrors `CellAnimState::finished`).
    pub finished: bool,
    /// Deterministic per-actor random state for the weighted pick.
    pub seed: u32,
}

impl CellAnimPlayback {
    pub fn with_seed(seed: u32) -> Self {
        Self {
            seed: seed | 1,
            ..default()
        }
    }

    fn random(&mut self) -> f32 {
        // xorshift32: reproducible captures; the native pick uses sMainRNG.
        let mut x = self.seed.max(1);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    /// `PlayAnimation(code)`. `facts` resolves a TLSA id to clip facts and
    /// `dilation` is G(anim_dilation, size ratio). `forced_row` bypasses Select.
    pub fn play(
        &mut self,
        code: i32,
        ctx: &SelectContext,
        dilation: f32,
        forced_row: Option<usize>,
        facts: &dyn Fn(u32) -> Option<ClipFacts>,
    ) {
        let random = self.random();
        let row = forced_row.or_else(|| select(code, ctx, random));
        let Some((row, (clip, length, cyclic))) =
            row.and_then(|r| facts(ROWS[r].anim).map(|f| (r, f)))
        else {
            // No row/clip for this cell: `Select` answers id 0, nothing plays
            // and `0xE4F020` returns 1.0 for a clip without length, so the
            // request runs for 1 s and then falls back to the return code (the
            // poses keep showing what played last).
            self.code = Some(code);
            self.remaining = 1.0;
            return;
        };
        let def = &ROWS[row];
        let dilation = if def.dilate && dilation > 0.0 {
            dilation
        } else {
            1.0
        };
        let (blend_in, blend_out) = blend_times(self.code, code);
        self.blend_in = blend_in * dilation;
        self.blend_out = blend_out * dilation;
        self.blend_clock = 0.0;
        self.previous = self.current.take();
        self.current = Some(Playing {
            row,
            clip,
            time: 0.0,
            rate: 1.0 / dilation,
            length,
            cyclic,
        });
        if def.sets_return {
            self.return_code = code;
        }
        self.code = Some(code);
        self.remaining = length * dilation;
    }

    /// One frame of `0xE6D340`, then advance the playing clips by `dt * speed`.
    pub fn step(
        &mut self,
        request: i32,
        dt: f32,
        speed: f32,
        ctx: &SelectContext,
        dilation: f32,
        forced_row: Option<usize>,
        facts: &dyn Fn(u32) -> Option<ClipFacts>,
    ) {
        let dt = dt.max(0.0);
        let speed = if speed.is_finite() && speed > 0.0 { speed } else { 1.0 };
        if self.code != Some(request) {
            self.finished = false;
            self.play(request, ctx, dilation, forced_row, facts);
        } else {
            self.remaining -= dt * speed;
            if self.remaining <= 0.0 {
                // The request ran its course unless it is its own return code
                // (loops such as idle/swim re-select and keep playing).
                if self.return_code != request {
                    self.finished = true;
                }
                let next = self.return_code;
                self.play(next, ctx, dilation, forced_row, facts);
            }
        }
        for playing in [&mut self.current, &mut self.previous].into_iter().flatten() {
            playing.time += dt * speed * playing.rate;
        }
        self.blend_clock += dt * speed;
        if self.previous.is_some() && self.weights().1 <= 0.0 {
            self.previous = None;
        }
    }

    /// Normalized (current, previous) blend weights.
    pub fn weights(&self) -> (f32, f32) {
        let rising = if self.blend_in <= 0.0 {
            1.0
        } else {
            (self.blend_clock / self.blend_in).clamp(0.0, 1.0)
        };
        let falling = match (&self.previous, self.blend_out <= 0.0) {
            (None, _) | (_, true) => 0.0,
            _ => 1.0 - (self.blend_clock / self.blend_out).clamp(0.0, 1.0),
        };
        let total = rising + falling;
        if total <= 0.0 {
            (1.0, 0.0)
        } else {
            (rising / total, falling / total)
        }
    }
}

// ---------------------------------------------------------------------------
// Clips: ANIM decoding, channel classes, sampling (native curves)
// ---------------------------------------------------------------------------

/// A part channel of a generalized clip.
#[derive(Clone, Debug)]
pub struct PartChannel {
    pub channel: usize,
    /// Capability tag (`mtcl`, `mvcl`, `eycl`, `wpcl`, `wpch`, `wpel`, `wpps`...).
    pub capability: [u8; 4],
    /// Primary-context flags: eye column/row selector bits.
    pub selector: u32,
    pub deform_ids: Vec<u32>,
}

/// A decoded Cell clip with its channels classified.
pub struct CellClip {
    pub anim: crate::animation::Animation,
    pub root: Option<usize>,
    pub frame: Option<usize>,
    pub parts: Vec<PartChannel>,
    pub seconds: f32,
    pub cyclic: bool,
}

/// Root deform 0x70E47545 of the clips (values 0.9 .. 1.11: idle 1.0 -> 1.077,
/// a wobble when hit). VERIFIED (disassembly, cell-anim3): it is an AUDIO
/// property, not a body scale. The animation system clears the rig-deform flag
/// 0x10 for exactly this id (0x9A868E) and turns its channel into a separate
/// output record (0x9A0F6D .. 0x9A10F5, flag 5, channel mask) instead of a
/// deform of the part rig; the only consumer of the value is the Cell GFX
/// update (0xE96DDA: `0xC9ED90(model)` -> `Audio::SetProperty(voice handle,
/// 0x70E47545, value)`, 0xE96DED), and the engine's own audio message
/// builders (0xA37340 / 0xA372F0) send the same id as a property name. No
/// code scales a model with it (the four code sites that name the id are
/// those two plus 0x9A0F6D / 0x9A868E), so the body is no longer scaled.
/// The value would drive the property of the cell's own voice; the property
/// id is a hash with no installed patch input, so it is not forwarded.
pub const ROOT_AUDIO_DEFORM: u32 = 0x70E4_7545;
/// Game data channels of the root (RE 7): Data1 hit knockback impulse,
/// Data2 move-speed factor, Data3 cilia / jet thrust, Data4 death camera.
pub const DATA1_KNOCKBACK: u32 = 0x98C9_42B6;
/// Data2: the move-speed factor outside the swim states (0xE56510); Data3: the
/// cilia (x 20, 0xE56300) / jet (x 100, 0xE56200) thrust strength.
pub const DATA2_SPEED: u32 = 0x98C9_42B5;
pub const DATA3_THRUST: u32 = 0x98C9_42B4;
/// Data4 (VERIFIED, 0xE60AD4 / 0xE60C20 / 0xE60D6D read by the camera update
/// 0xE600A0): the camera pull-in weight of the avatar's death (codes 7 / 8 /
/// 9), hatch (43 / 44), go-to-editor (41) and move-to-beach (46) clips; Data5
/// (0x98C942B2, code 41 only) is the weight that slides the look point to the
/// camera focus object.
pub const DATA4_CAMERA_PULL: u32 = 0x98C9_42B3;
pub const DATA5_CAMERA_FOCUS: u32 = 0x98C9_42B2;

/// Value of a root game channel for the cell's blended clips (0xE83380:
/// the sum of value x weight over the clips that key it, divided by the sum of
/// their weights; 0 when no playing clip keys it).
pub fn game_channel(assets: &CellAnimAssets, playback: &CellAnimPlayback, id: u32) -> f32 {
    game_channel_keyed(assets, playback, id).unwrap_or(0.0)
}

/// As [`game_channel`], `None` when no playing clip keys the channel (0xE83380
/// answers false: the callers then take their fallback constants).
pub fn game_channel_keyed(assets: &CellAnimAssets, playback: &CellAnimPlayback, id: u32) -> Option<f32> {
    let (wc, wp) = playback.weights();
    let mut sum = 0.0;
    let mut weight = 0.0;
    for (playing, blend) in [(&playback.current, wc), (&playback.previous, wp)] {
        let Some(p) = playing else {
            continue;
        };
        if blend <= 0.0 {
            continue;
        }
        let Some(Some(clip)) = assets.clips.get(&p.clip) else {
            continue;
        };
        let Some(s) = clip.root.and_then(|c| clip.sample(c, p.sample_time())) else {
            continue;
        };
        if let Some(&(_, v, w)) = s.deforms.iter().find(|d| d.0 == id) {
            if v.is_finite() && w > 0.0 {
                sum += v * w * blend;
                weight += w * blend;
            }
        }
    }
    (weight > 0.0).then(|| sum / weight)
}

fn tag(capability: &str) -> [u8; 4] {
    let mut out = [0u8; 4];
    for (o, b) in out.iter_mut().zip(capability.bytes()) {
        *o = b;
    }
    out
}

impl CellClip {
    pub fn from_animation(anim: crate::animation::Animation) -> Self {
        let mut root = None;
        let mut frame = None;
        let mut parts = vec![];
        for (i, ch) in anim.channels.iter().enumerate() {
            let flags = ch.primary.flags;
            let cap = ch.primary.capability.trim_end_matches('\0');
            if flags & 0x0010_0000 != 0 && cap.is_empty() {
                frame.get_or_insert(i);
            } else if flags & 0x0010_0003 == 1 || (flags == 0 && cap == "root") {
                root.get_or_insert(i);
            } else if !cap.is_empty() {
                parts.push(PartChannel {
                    channel: i,
                    capability: tag(cap),
                    selector: flags,
                    deform_ids: ch
                        .components
                        .iter()
                        .filter(|c| c.flags & 0xF == 3 && c.id != 0)
                        .map(|c| c.id)
                        .collect(),
                });
            }
        }
        let seconds = anim.duration_seconds();
        let cyclic = anim.flags & 8 != 0;
        Self {
            anim,
            root,
            frame,
            parts,
            seconds,
            cyclic,
        }
    }

    fn frame_at(&self, seconds: f32) -> f32 {
        let a = &self.anim;
        let frame = seconds / a.frame_length.max(1e-6);
        let last = (a.length_frames - 1.0).max(0.0);
        if self.cyclic && last > 0.0 {
            frame.rem_euclid(last)
        } else {
            frame.clamp(0.0, last)
        }
    }

    /// Sample one channel with the recovered native evaluator.
    pub fn sample(&self, channel: usize, seconds: f32) -> Option<crate::animation::Sample> {
        let ch = self.anim.channels.get(channel)?;
        crate::animation_sampling::sample(
            ch,
            self.frame_at(seconds),
            self.anim.length_frames,
            self.cyclic,
            crate::animation_sampling::RotationCurve::Spherical,
        )
        .ok()
    }
}

/// Whole-body motion sampled from a clip, in Spore model space (Z up).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyPose {
    pub root_offset: Vec3,
    pub root_rotation: Quat,
    pub frame_offset: Vec3,
    pub frame_rotation: Quat,
}

impl Default for BodyPose {
    fn default() -> Self {
        Self {
            root_offset: Vec3::ZERO,
            root_rotation: Quat::IDENTITY,
            frame_offset: Vec3::ZERO,
            frame_rotation: Quat::IDENTITY,
        }
    }
}

fn quat_xyzw(q: [f32; 4]) -> Quat {
    let q = Quat::from_xyzw(q[0], q[1], q[2], q[3]);
    if q.length_squared() > 1e-12 {
        q.normalize()
    } else {
        Quat::IDENTITY
    }
}

impl BodyPose {
    pub fn sample(clip: &CellClip, seconds: f32) -> Self {
        let mut pose = Self::default();
        if let Some(s) = clip.root.and_then(|c| clip.sample(c, seconds)) {
            if let Some((p, w, _)) = s.position {
                pose.root_offset = Vec3::from_array(p) * w;
            }
            if let Some((q, w, _)) = s.rotation {
                pose.root_rotation = Quat::IDENTITY.slerp(quat_xyzw(q), w.clamp(0.0, 1.0));
            }
            // The root deform 0x70E47545 is an audio property (see
            // `ROOT_AUDIO_DEFORM`): it does not scale the body.
        }
        if let Some(s) = clip.frame.and_then(|c| clip.sample(c, seconds)) {
            if let Some((p, w, _)) = s.position {
                pose.frame_offset = Vec3::from_array(p) * w;
            }
            if let Some((q, w, _)) = s.rotation {
                pose.frame_rotation = Quat::IDENTITY.slerp(quat_xyzw(q), w.clamp(0.0, 1.0));
            }
        }
        pose
    }

    pub fn blend(&self, other: &Self, t: f32) -> Self {
        Self {
            root_offset: self.root_offset.lerp(other.root_offset, t),
            root_rotation: self.root_rotation.slerp(other.root_rotation, t),
            frame_offset: self.frame_offset.lerp(other.frame_offset, t),
            frame_rotation: self.frame_rotation.slerp(other.frame_rotation, t),
        }
    }

    /// Model-space body matrix: creature frame about the model origin, then
    /// the root about `pivot` (ADAPTED pivots: the root rigblock / origin).
    pub fn matrix(&self, pivot: Vec3) -> Affine3A {
        let root = Affine3A::from_translation(pivot + self.root_offset)
            * Affine3A::from_quat(self.root_rotation)
            * Affine3A::from_translation(-pivot);
        Affine3A::from_rotation_translation(self.frame_rotation, self.frame_offset) * root
    }
}

/// One deform track sampled on a part channel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Track {
    pub id: u32,
    pub value: f32,
    pub weight: f32,
    pub cyclic: bool,
}

/// Deform tracks of one part channel at `seconds`.
pub fn part_tracks(clip: &CellClip, channel: usize, seconds: f32) -> Vec<Track> {
    let Some(sample) = clip.sample(channel, seconds) else {
        return vec![];
    };
    let flags: HashMap<u32, u32> = clip.anim.channels[channel]
        .components
        .iter()
        .filter(|c| c.flags & 0xF == 3)
        .map(|c| (c.id, c.flags))
        .collect();
    sample
        .deforms
        .into_iter()
        .filter(|d| d.0 != 0)
        .map(|(id, value, weight)| Track {
            id,
            value,
            weight,
            cyclic: flags.get(&id).is_some_and(|f| f & 0x40 != 0),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Part rigs: RW4 skeleton, row-layout inverse binds, deform clip table
// ---------------------------------------------------------------------------

/// A bone transform (rotation, translation, scale) in its parent's space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bone3 {
    pub rotation: Quat,
    pub translation: Vec3,
    pub scale: Vec3,
}

impl Bone3 {
    pub const IDENTITY: Self = Self {
        rotation: Quat::IDENTITY,
        translation: Vec3::ZERO,
        scale: Vec3::ONE,
    };
    fn affine(&self) -> Affine3A {
        Affine3A::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

/// One deform clip of a part rig with its bone -> channel map.
#[derive(Clone, Debug)]
pub struct RigClip {
    pub anim: rw4::KeyAnim,
    pub channel_for_bone: Vec<Option<usize>>,
}

/// A part's own rig (RW4 rig-only or full MODEL record).
#[derive(Clone, Debug)]
pub struct PartRig {
    pub names: Vec<u32>,
    pub parents: Vec<Option<usize>>,
    pub bind_local: Vec<Bone3>,
    /// Rig-space inverse binds (column-vector form).
    pub inverse_bind: Vec<Affine3A>,
    /// Deform id -> clip (RW4 0xFF0001 table order).
    pub clips: Vec<(u32, RigClip)>,
}

/// RW4 AnimSkin poses are D3D row-vector matrices: rows 0..2 hold the 3x3 and
/// row 3 the translation, so the column-vector inverse bind is `[R^T | t]`.
pub fn inverse_bind_from_row_pose(pose: &rw4::BonePose) -> Affine3A {
    let r = pose.rotation;
    // Mat3::from_cols(row i) builds the transpose of the stored row-major R.
    let m = Mat3::from_cols(
        Vec3::from_array(r[0]),
        Vec3::from_array(r[1]),
        Vec3::from_array(r[2]),
    );
    Affine3A::from_mat3_translation(m, Vec3::from_array(pose.inv_translation))
}

fn decompose(m: Affine3A) -> Bone3 {
    let (scale, rotation, translation) = m.to_scale_rotation_translation();
    Bone3 {
        rotation: rotation.normalize(),
        translation,
        scale,
    }
}

/// Parse the RW4 `0xFF0001` animations table: (deform id, section index).
pub fn animation_table(blob: &[u8]) -> Result<Vec<(u32, usize)>, String> {
    let sections = rw4::sections(blob)?;
    let Some((_, data)) = sections.iter().find(|(kind, _)| *kind == 0x00FF_0001) else {
        return Ok(vec![]);
    };
    let word = |at: usize| -> Option<u32> {
        data.get(at..at + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    };
    let count = word(4).ok_or("short animations table")? as usize;
    if count > 256 {
        return Err("implausible animations table".into());
    }
    (0..count)
        .map(|i| {
            let id = word(8 + i * 8).ok_or("short animations table")?;
            let reference = word(12 + i * 8).ok_or("short animations table")?;
            if reference >> 22 != 0 {
                return Err(format!("unsupported animation reference {reference:08X}"));
            }
            let index = reference as usize;
            if sections.get(index).map(|s| s.0) != Some(rw4::T_KEYANIM) {
                return Err(format!("animation {id:08X} does not reference a KeyAnim"));
            }
            Ok((id, index))
        })
        .collect()
}

impl PartRig {
    /// Build from one RW4 record (its single SkinsInK skeleton/skin).
    pub fn from_rw4(blob: &[u8]) -> Result<Self, String> {
        let mut rigs = rw4::extract_linked_skin_rigs(blob)?;
        if rigs.len() != 1 {
            return Err(format!("{} linked skins (need exactly one)", rigs.len()));
        }
        let rig = rigs.pop().unwrap();
        let names: Vec<u32> = rig.bones.iter().map(|b| b.name).collect();
        let parents: Vec<Option<usize>> = rig.bones.iter().map(|b| b.parent).collect();
        if parents.iter().enumerate().any(|(i, p)| p.is_some_and(|p| p >= i)) {
            return Err("part rig parents must precede children".into());
        }
        let inverse_bind: Vec<Affine3A> = rig.poses.iter().map(inverse_bind_from_row_pose).collect();
        let worlds: Vec<Affine3A> = inverse_bind.iter().map(|m| m.inverse()).collect();
        let bind_local = parents
            .iter()
            .enumerate()
            .map(|(i, p)| decompose(p.map_or(worlds[i], |p| worlds[p].inverse() * worlds[i])))
            .collect();
        // KeyAnim objects come back in section order; index them by section.
        let mut stats = rw4::MeshStats::default();
        let anims = rw4::extract_anims(blob, &mut stats);
        let sections = rw4::sections(blob)?;
        let keyanim_sections: Vec<usize> = sections
            .iter()
            .enumerate()
            .filter(|(_, s)| s.0 == rw4::T_KEYANIM)
            .map(|(i, _)| i)
            .collect();
        if keyanim_sections.len() != anims.len() {
            return Err("KeyAnim sections did not all decode".into());
        }
        let mut clips = vec![];
        for (id, section) in animation_table(blob)? {
            let Some(ordinal) = keyanim_sections.iter().position(|&s| s == section) else {
                continue;
            };
            let anim = anims[ordinal].clone();
            let channel_for_bone = names
                .iter()
                .map(|name| anim.channels.iter().position(|c| c.id == *name))
                .collect();
            clips.push((
                id,
                RigClip {
                    anim,
                    channel_for_bone,
                },
            ));
        }
        Ok(Self {
            names,
            parents,
            bind_local,
            inverse_bind,
            clips,
        })
    }

    pub fn bone_count(&self) -> usize {
        self.parents.len()
    }

    pub fn has_deform(&self, id: u32) -> bool {
        self.clips.iter().any(|(d, _)| *d == id)
    }

    /// Local pose for weighted deform tracks: each weighted track samples its
    /// clip at `phase * length`; tracks combine as a weighted average
    /// (ADAPTED rule), the bind pose when no track has weight.
    pub fn locals(&self, tracks: &[Track], out: &mut Vec<Bone3>) {
        out.clear();
        out.extend_from_slice(&self.bind_local);
        let mut total = vec![0.0f32; self.bone_count()];
        let mut acc: Vec<(Vec4, Vec3, Vec3)> = vec![(Vec4::ZERO, Vec3::ZERO, Vec3::ZERO); self.bone_count()];
        for track in tracks {
            if !(track.weight > 1e-4) || !track.value.is_finite() {
                continue;
            }
            let Some((_, clip)) = self.clips.iter().find(|(id, _)| *id == track.id) else {
                continue;
            };
            let phase = if track.cyclic {
                track.value.rem_euclid(1.0)
            } else {
                track.value.clamp(0.0, 1.0)
            };
            let t = phase * clip.anim.length;
            for (bone, channel) in clip.channel_for_bone.iter().enumerate() {
                let (rotation, translation, scale) = match channel
                    .and_then(|c| clip.anim.sample_frame_clamped(c, t))
                {
                    Some(k) => (quat_xyzw(k.rotation), Vec3::from_array(k.location), Vec3::from_array(k.scale)),
                    None => {
                        let b = self.bind_local[bone];
                        (b.rotation, b.translation, b.scale)
                    }
                };
                let w = track.weight;
                let mut q = Vec4::from(rotation);
                if acc[bone].0.dot(q) < 0.0 {
                    q = -q;
                }
                acc[bone].0 += q * w;
                acc[bone].1 += translation * w;
                acc[bone].2 += scale * w;
                total[bone] += w;
            }
        }
        for bone in 0..self.bone_count() {
            if total[bone] > 1e-4 {
                let (q, t, s) = acc[bone];
                let q = Quat::from_vec4(q / total[bone]);
                out[bone] = Bone3 {
                    rotation: if q.length_squared() > 1e-12 { q.normalize() } else { Quat::IDENTITY },
                    translation: t / total[bone],
                    scale: s / total[bone],
                };
            }
        }
    }

    /// Rig-space skin deltas (`world * inverse_bind`) for a local pose.
    pub fn skin(&self, locals: &[Bone3], out: &mut Vec<Affine3A>) {
        out.clear();
        let mut worlds: Vec<Affine3A> = Vec::with_capacity(self.bone_count());
        for (bone, parent) in self.parents.iter().enumerate() {
            let local = locals.get(bone).copied().unwrap_or(self.bind_local[bone]).affine();
            let world = parent.map_or(local, |p| worlds[p] * local);
            worlds.push(world);
            out.push(world * self.inverse_bind[bone]);
        }
    }
}

/// Blend two local poses (`t` = weight of `b`).
pub fn blend_locals(a: &mut [Bone3], b: &[Bone3], t: f32) {
    for (x, y) in a.iter_mut().zip(b) {
        x.rotation = x.rotation.slerp(y.rotation, t);
        x.translation = x.translation.lerp(y.translation, t);
        x.scale = x.scale.lerp(y.scale, t);
    }
}

// ---------------------------------------------------------------------------
// Channel binding: which clip channel drives a part
// ---------------------------------------------------------------------------

/// Eye-region class of a part on its body (selector bits of eye channels):
/// column 0x04 left / 0x08 right / 0x0C centre, row 0x10 front / 0x20 back /
/// 0x30 middle. Creatures face -Y with Z up, so left is +X (ADAPTED reading
/// of the channel names Eye_L_Fr, Eye_R_Ba, Eye_C...).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Region {
    pub column: u32,
    pub row: u32,
}

impl Region {
    /// Classify a model-space attachment point against the body extent.
    pub fn classify(point: Vec3, body_min: Vec3, body_max: Vec3) -> Self {
        let width = (body_max.x - body_min.x).max(1e-4);
        let length = (body_max.y - body_min.y).max(1e-4);
        let center = (body_min + body_max) * 0.5;
        let column = if (point.x - center.x).abs() < width * 0.08 {
            0x0C
        } else if point.x > center.x {
            0x04
        } else {
            0x08
        };
        let along = (point.y - center.y) / length;
        let row = if along < -1.0 / 6.0 {
            0x10
        } else if along > 1.0 / 6.0 {
            0x20
        } else {
            0x30
        };
        Self { column, row }
    }

    fn matches(&self, selector: u32) -> Option<u32> {
        let column = selector & 0x0C;
        let row = selector & 0x30;
        if (column != 0 && column != self.column) || (row != 0 && row != self.row) {
            return None;
        }
        Some(u32::from(column != 0) + u32::from(row != 0))
    }
}

/// The channel of `clip` that drives a part with these capabilities: matching
/// capability, the most specific eye region, then the most deform ids the
/// part's rig knows (spikes take `Weapon`, electric parts `WeaponChrg`).
pub fn bind_channel(clip: &CellClip, caps: &[[u8; 4]], region: Region, rig: &PartRig) -> Option<usize> {
    clip.parts
        .iter()
        .enumerate()
        .filter(|(_, p)| caps.contains(&p.capability))
        .filter_map(|(i, p)| {
            let specific = region.matches(p.selector)?;
            let known = p.deform_ids.iter().filter(|id| rig.has_deform(**id)).count() as u32;
            Some((i, specific, known))
        })
        .max_by_key(|&(i, specific, known)| (known.min(1), specific, known, usize::MAX - i))
        .map(|(i, _, _)| clip.parts[i].channel)
}

/// Capability tags and UnlockType of a Cell part from its catalog abilities.
pub fn part_caps(abilities: &Capabilities) -> (Vec<[u8; 4]>, Option<u8>) {
    let a = abilities;
    let (caps, kind): (&[&[u8; 4]], Option<u8>) = if a.cell_jet > 0 {
        (&[b"mvcl"], Some(part_type::JET))
    } else if a.cell_cilia > 0 {
        (&[b"mvcl"], Some(part_type::CILIA))
    } else if a.cell_flagella > 0 {
        (&[b"mvcl"], Some(part_type::FLAGELLA))
    } else if a.cell_electric > 0 {
        (&[b"wpcl", b"wpch", b"wpel"], Some(part_type::ELECTRIC))
    } else if a.cell_poison > 0 {
        (&[b"wpcl", b"wpch", b"wpps"], Some(part_type::POISON))
    } else if a.cell_spike > 0 {
        (&[b"wpcl"], Some(part_type::SPIKE))
    } else if a.eye > 0 {
        (&[b"eycl"], Some(part_type::EYE))
    } else if a.herbivore > 0 && a.carnivore > 0 {
        (&[b"mtcl"], Some(part_type::PROBOSCIS))
    } else if a.carnivore > 0 {
        (&[b"mtcl"], Some(part_type::MANDIBLE))
    } else if a.herbivore > 0 || a.mouth > 0 {
        (&[b"mtcl"], Some(part_type::FILTER))
    } else {
        (&[], None)
    };
    (caps.iter().map(|c| **c).collect(), kind)
}

/// Fallback when no catalog entry: infer tags from the rig's deform table.
fn caps_from_rig(rig: &PartRig) -> Vec<[u8; 4]> {
    if rig.has_deform(0xB37B_55B2) || rig.has_deform(0x6FB7_60FF) {
        vec![*b"mvcl"]
    } else if rig.has_deform(0x114B_B90C) {
        vec![*b"mtcl"]
    } else if rig.has_deform(0x9891_EEC7) {
        vec![*b"eycl"]
    } else if rig.has_deform(0x0EFD_249C) {
        vec![*b"wpcl", *b"wpch", *b"wpel", *b"wpps"]
    } else {
        vec![*b"wpcl"]
    }
}

// ---------------------------------------------------------------------------
// Assembled Cell models (NPC GMDLs)
// ---------------------------------------------------------------------------

/// GMDL Z-up model space -> the Bevy basis of `baked_model::mesh` ((-x, z, y)).
pub fn gmdl_basis() -> Affine3A {
    Affine3A::from_mat3(Mat3::from_cols(Vec3::NEG_X, Vec3::Z, Vec3::Y))
}

/// Column-vector form of a native row-vector Transform (v' = s·(v·R) + o).
pub fn native_transform(raw: [u8; 56]) -> Option<Affine3A> {
    let t = crate::gmdl_bindings::NativeTransform::decode(raw);
    if !t.all_values_finite || t.scale == 0.0 {
        return None;
    }
    // from_cols_array reads column-major, so the row-major R arrives as R^T.
    let m = Mat3::from_cols_array(&t.rotation_values) * t.scale;
    Some(Affine3A::from_mat3_translation(m, Vec3::from_array(t.offset)))
}

/// A rigged part of an assembled model.
#[derive(Clone, Debug)]
pub struct ModelPart {
    pub first_joint: usize,
    pub rig: Arc<PartRig>,
    /// Rig space -> model space (inverse of the binding's primary transform).
    pub to_model: Affine3A,
    pub from_model: Affine3A,
    pub caps: Vec<[u8; 4]>,
    pub region: Region,
}

/// A creation rigblock's anchor on the animated NPC model: what retail's
/// part pose entry (0xE84150: the rigblock's pose in the animated model's
/// pose array) answers for the weapon code, in the model space of the GMDL.
/// The base is the rigblock position T (the pose entry starts at the block's
/// placement); the poison emit point adds the rigblock's +0x60 offset rotated
/// by the block's orientation (0xE68000, scale 1 in the 2.5x part-list frame,
/// so 1/2.5 in creation units).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartAnchor {
    /// Index in the creation's rigblock list (the part-list index of 0xE83E30).
    pub block: usize,
    /// Part type (`UnlockType`) of the block, `None` for a block that is no
    /// catalogued part.
    pub kind: u8,
    /// Rigblock position (model space).
    pub rest: Vec3,
    /// Poison emit point: `rest + R * offset / 2.5` (model space).
    pub emit: Vec3,
    /// The part's outward axis, local -Y turned by the block orientation
    /// (0xE67D30), as a model-space direction.
    pub axis: Vec3,
    /// Part-list radius of the block in creation units (0xE83E30).
    pub radius: f32,
    /// First joint of the part's group: the animated root bone of the part.
    pub joint: Option<usize>,
}

/// Joint layout and part rigs of one Cell GMDL.
#[derive(Clone, Debug)]
pub struct ModelRig {
    pub key: ResourceKey,
    pub joint_count: usize,
    pub body_joints: usize,
    pub parts: Vec<ModelPart>,
    /// Root rigblock position (model space), the ADAPTED root pivot.
    pub pivot: Vec3,
    /// Half the body's horizontal extent in model units.
    pub model_radius: f32,
    pub parts_present: u32,
    /// Per GMDL piece: joint indices and normalized weights per vertex.
    pub skins: Vec<(Vec<[u16; 4]>, Vec<[f32; 4]>)>,
    /// Anchors of the creation's catalogued parts (weapons need their pose).
    pub anchors: Vec<PartAnchor>,
}

/// Bevy's GPU skinning palette limit.
pub const MAX_JOINTS: usize = 256;
const CELL_ANATOMY_GROUP: u32 = 0x4061_6200;

impl ModelRig {
    pub fn build(
        index: &ResourceIndex,
        catalog: &crate::asset_catalog::PartCatalog,
        key: ResourceKey,
        rigs: &mut HashMap<ResourceKey, Option<Arc<PartRig>>>,
    ) -> Result<Self, String> {
        let model = crate::baked_model::parse(&index.read(key)?)?;
        let layout = crate::animation_layout::resolve(index, &model)?;
        if !layout.all_drawn_slots_covered || layout.bone_count != layout.declared_range_end {
            return Err("GMDL palette does not cover its drawn vertices".into());
        }
        if layout.bone_count > MAX_JOINTS {
            return Err(format!("{} joints exceed the GPU palette", layout.bone_count));
        }
        let anatomy = index
            .read(ResourceKey {
                group_id: CELL_ANATOMY_GROUP,
                instance_id: key.instance_id,
                type_id: crate::creature_data::TYPE,
            })
            .and_then(|b| crate::creature_data::parse(&b))
            .ok();
        // Bindings after the body group follow the non-vertebra rigblocks.
        let part_blocks: Vec<&crate::creature_data::RigBlock> = anatomy
            .as_ref()
            .map(|a| a.rigblocks.iter().filter(|b| b.block_type() != 5).collect())
            .unwrap_or_default();
        let blocks_match = part_blocks.len() + 1 == model.anim_data.len();
        let body_joints = layout
            .groups
            .first()
            .filter(|g| g.reserved)
            .map_or(0, |g| g.bone_count);
        let mut parts = vec![];
        let mut parts_present = 0u32;
        // Part-block index (among the non-vertebra blocks) -> first joint.
        let mut group_joints: HashMap<usize, usize> = HashMap::new();
        for group in layout.groups.iter().skip(1) {
            if blocks_match && group.bone_count > 0 && group.first_bone < layout.bone_count {
                group_joints.insert(group.binding - 1, group.first_bone);
            }
            let binding = &model.anim_data[group.binding];
            let block = blocks_match.then(|| part_blocks[group.binding - 1]);
            let abilities = block.and_then(|b| {
                catalog
                    .by_model_in(CELL_PARTS, b.instance_id())
                    .map(|d| d.abilities)
            });
            let kind = abilities.as_ref().and_then(|a| part_caps(a).1);
            if let Some(kind) = kind {
                parts_present |= 1 << kind;
            }
            if group.reserved {
                continue;
            }
            let rig = rigs
                .entry(binding.key)
                .or_insert_with(|| {
                    index
                        .read(binding.key)
                        .and_then(|b| PartRig::from_rw4(&b))
                        .map(Arc::new)
                        .map_err(|e| warn!("cell part rig {:08X}: {e}", binding.key.instance_id))
                        .ok()
                })
                .clone();
            let Some(rig) = rig.filter(|r| r.bone_count() == group.bone_count) else {
                continue;
            };
            let Some(primary) = native_transform(binding.transform) else {
                continue;
            };
            let to_model = primary.inverse();
            let caps = match &abilities {
                Some(a) if !part_caps(a).0.is_empty() => part_caps(a).0,
                _ => caps_from_rig(&rig),
            };
            parts.push(ModelPart {
                first_joint: group.first_bone,
                rig,
                to_model,
                from_model: primary,
                caps,
                region: Region::default(),
            });
        }
        // Body extent for eye regions and the fallback pivot.
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        let mut skins = vec![];
        for (piece, range) in model.pieces.iter().zip(&model.bone_ranges) {
            let drawn = crate::animation_layout::drawn_vertices(piece)?;
            let mut joints = Vec::with_capacity(piece.positions.len());
            let mut weights = Vec::with_capacity(piece.positions.len());
            for v in 0..piece.positions.len() {
                let (mut j, mut w) = ([0u16; 4], [0f32; 4]);
                if drawn[v] {
                    let mut total = 0.0;
                    for k in 0..4 {
                        let weight = piece.weights[v][k];
                        if weight > 0.0 {
                            let global = range[0] as usize + piece.bones[v][k] as usize / 3;
                            j[k] = global.min(layout.bone_count.saturating_sub(1)) as u16;
                            w[k] = weight;
                            total += weight;
                            if global < body_joints {
                                let p = Vec3::from_array(piece.positions[v]);
                                min = min.min(p);
                                max = max.max(p);
                            }
                        }
                    }
                    if total > 0.0 {
                        w.iter_mut().for_each(|x| *x /= total);
                    } else {
                        w[0] = 1.0;
                    }
                } else {
                    w[0] = 1.0;
                }
                joints.push(j);
                weights.push(w);
            }
            skins.push((joints, weights));
        }
        if !min.is_finite() {
            min = Vec3::splat(-0.5);
            max = Vec3::splat(0.5);
        }
        for part in &mut parts {
            part.region = Region::classify(part.to_model.translation.into(), min, max);
        }
        let pivot = anatomy
            .as_ref()
            .and_then(|a| a.rigblocks.first())
            .map(|b| {
                let bits = b.position_bits();
                Vec3::new(
                    f32::from_bits(bits[0]),
                    f32::from_bits(bits[1]),
                    f32::from_bits(bits[2]),
                )
            })
            .filter(|p| p.is_finite())
            .unwrap_or((min + max) * 0.5);
        // Part anchors (0xE83E30 entries): every catalogued part block of the
        // creation, with the joint of its group when the model binds it.
        let mut anchors = vec![];
        if let Some(a) = anatomy.as_ref() {
            let mut part_index = 0usize;
            for (list_index, block) in a.rigblocks.iter().enumerate() {
                let this_part = (block.block_type() != 5).then(|| {
                    part_index += 1;
                    part_index - 1
                });
                if block.group_id() != CELL_PARTS {
                    continue;
                }
                let Some(kind) = catalog
                    .by_model_in(CELL_PARTS, block.instance_id())
                    .and_then(|d| crate::cell_motion::part_type(&d.abilities))
                else {
                    continue;
                };
                let f = f32::from_bits;
                let t = block.position_bits().map(f);
                let r = block.orientation_bits().map(|row| row.map(f));
                // Rigblock +0x60..+0x6C: the poison emit offset (0xE83E30 copies it to
                // entry +0x3C; only the poison gland 0x64028FE0 has one, (0, -0.257, 0)).
                let off: [f32; 3] = std::array::from_fn(|i| {
                    block
                        .raw_bytes
                        .get(0x60 + i * 4..0x64 + i * 4)
                        .map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap()))
                });
                let bb = block.bounding_box_bits().map(f);
                let s = f(block.scale_bits());
                let rest = Vec3::from_array(t);
                // Row-vector convention (v' = v R): local v maps to sum_j v_j R[j][i].
                let turn = |v: Vec3| {
                    Vec3::new(
                        v.x * r[0][0] + v.y * r[1][0] + v.z * r[2][0],
                        v.x * r[0][1] + v.y * r[1][1] + v.z * r[2][1],
                        v.x * r[0][2] + v.y * r[1][2] + v.z * r[2][2],
                    )
                };
                let emit = rest + turn(Vec3::from_array(off)) / 2.5;
                let axis = turn(Vec3::NEG_Y);
                let mut radius = (bb[3] - bb[0]).abs().max((bb[5] - bb[2]).abs()) * s * 0.5;
                if kind == 2 && block.instance_id() != 0xD900_7204 {
                    radius *= 0.5;
                }
                if !(rest.is_finite() && emit.is_finite() && axis.is_finite()) {
                    continue;
                }
                anchors.push(PartAnchor {
                    block: list_index,
                    kind: kind as u8,
                    rest,
                    emit,
                    axis,
                    radius,
                    joint: this_part.and_then(|p| group_joints.get(&p).copied()),
                });
            }
        }
        Ok(Self {
            key,
            joint_count: layout.bone_count,
            body_joints,
            parts,
            pivot,
            model_radius: ((max - min).truncate().max_element() * 0.5).max(1e-3),
            parts_present,
            skins,
            anchors,
        })
    }

    /// Model-space joint palette for a body matrix and per-part rig skins.
    pub fn palette(&self, body: Affine3A, part_skins: &[Vec<Affine3A>], out: &mut Vec<Affine3A>) {
        out.clear();
        out.resize(self.joint_count, body);
        for (part, skins) in self.parts.iter().zip(part_skins) {
            for (b, skin) in skins.iter().enumerate() {
                if let Some(slot) = out.get_mut(part.first_joint + b) {
                    *slot = body * part.to_model * *skin * part.from_model;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Bevy: assets, components, systems
// ---------------------------------------------------------------------------

/// Decoded clips, rigs and shared skinned meshes (built once, reused).
#[derive(Resource, Default)]
pub struct CellAnimAssets {
    /// TLSA group id -> (ANIM instance, end mode).
    pub groups: HashMap<u32, (u32, u32)>,
    pub groups_loaded: bool,
    pub clips: HashMap<u32, Option<Arc<CellClip>>>,
    pub rigs: HashMap<ResourceKey, Option<Arc<PartRig>>>,
    pub models: HashMap<ResourceKey, Option<Arc<ModelRig>>>,
    /// (model key, surface) -> shared skinned mesh.
    pub skinned: HashMap<(ResourceKey, usize), Handle<Mesh>>,
    pub part_meshes: HashMap<AssetId<Mesh>, Option<(Handle<Mesh>, ResourceKey, Affine3A)>>,
    pub bindposes: HashMap<usize, Handle<SkinnedMeshInverseBindposes>>,
}

/// Capture/debug override: hold one request code or one table row on every
/// cell (`--anim-code N` / `--anim-row N` in quick capture).
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct CellAnimOverride {
    pub code: Option<i32>,
    pub row: Option<usize>,
}

/// One weapon / mouth part of an animated NPC in the cell's fitted frame
/// (the radius-1 model frame of `CellArt::spheres`: x, z of the cell's local
/// plane, scaled by the cell radius and turned by its heading).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartPose {
    /// Index in the creation's rigblock list.
    pub block: usize,
    /// Part type (`UnlockType`).
    pub kind: u8,
    /// The part's animated pose position (0xE84150).
    pub point: Vec2,
    /// Poison emit point: the pose position plus the rigblock offset
    /// (0xE68000).
    pub emit: Vec2,
    /// Outward axis of the part (0xE67D30), unit length in the plane (zero
    /// when the axis points out of the plane).
    pub axis: Vec2,
}

/// The animated part poses of an NPC (retail 0xE84150 on the animated model).
/// Written every frame by [`animate`]; absent until the cell is rigged.
#[derive(Component, Clone, Debug, Default)]
pub struct CellPartPoses {
    pub parts: Vec<PartPose>,
}

impl CellPartPoses {
    /// Poses of one part type, in rigblock order.
    pub fn of_kind(&self, kind: u8) -> impl Iterator<Item = &PartPose> {
        self.parts.iter().filter(move |p| p.kind == kind)
    }
}

/// Anchors of a rigged model in the fitted frame, from the model-space body
/// matrix and joint palette of this frame. `mesh_from_model` maps model space
/// to the holder's mesh space and `fit` is the holder's local transform (the
/// mesh -> fitted-frame fit of `centered_fit`).
pub fn part_poses(rig: &ModelRig, body: Affine3A, palette: &[Affine3A], mesh_from_model: Affine3A, fit: Transform) -> CellPartPoses {
    let fit = Affine3A::from_scale_rotation_translation(fit.scale, fit.rotation, fit.translation);
    let into_fit = |m: Affine3A, p: Vec3| fit.transform_point3(mesh_from_model.transform_point3(m.transform_point3(p)));
    let parts = rig
        .anchors
        .iter()
        .map(|a| {
            let m = a.joint.and_then(|j| palette.get(j)).copied().unwrap_or(body);
            let point = into_fit(m, a.rest);
            let emit = into_fit(m, a.emit);
            let axis = fit.transform_vector3(mesh_from_model.transform_vector3(m.transform_vector3(a.axis)));
            PartPose {
                block: a.block,
                kind: a.kind,
                point: Vec2::new(point.x, point.z),
                emit: Vec2::new(emit.x, emit.z),
                axis: Vec2::new(axis.x, axis.z).normalize_or_zero(),
            }
        })
        .collect();
    CellPartPoses { parts }
}

/// Joint entities of an animated cell visual.
#[derive(Component, Clone, Debug)]
pub struct CellAnimSkeleton {
    pub model: ResourceKey,
    pub joints: Vec<Entity>,
    pub holder: Entity,
    /// Extra surfaces spawned for multi-piece models.
    pub surfaces: Vec<Entity>,
    /// Model space -> holder mesh space.
    pub mesh_from_model: Affine3A,
    pub player: bool,
}

/// A player part skinned on its own rig.
#[derive(Component, Clone, Debug)]
pub struct CellPartSkeleton {
    pub rig: ResourceKey,
    pub joints: Vec<Entity>,
    /// Rig space -> part mesh space.
    pub mesh_from_rig: Affine3A,
    pub caps: Vec<[u8; 4]>,
    pub region: Region,
}

/// The visual could not be rigged; it keeps the clip's body motion only.
#[derive(Component)]
pub struct CellAnimBodyOnly;

/// Resources of the Cell animation system. Its three systems are registered
/// in `main.rs` (`load_clip_times`, `attach_visuals`, `animate`, chained, in the
/// Presentation set during the Cell stage, paused with the pause menu).
pub struct CellAnimPlugin;

impl Plugin for CellAnimPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CellAnimAssets>()
            .init_resource::<CellAnimOverride>()
            .init_resource::<CellClipTimes>();
    }
}

/// The action lists of every TLSA record of the installed packages.
fn index_lists(index: &ResourceIndex) -> Vec<(u32, crate::tlsa::Tlsa)> {
    let mut keys: Vec<ResourceKey> = index
        .records
        .keys()
        .filter(|k| k.type_id == crate::tlsa::TYPE_ID)
        .copied()
        .collect();
    keys.sort_by_key(|k| (k.group_id, k.instance_id));
    keys.into_iter()
        .filter_map(|key| index.read(key).and_then(|b| crate::tlsa::parse(&b)).ok())
        .map(|list| (list.version, list))
        .collect()
}

/// TLSA group id (`0xAAAA00xx`) -> (ANIM instance, end mode) of the Cell clips.
/// The newest list wins (PatchData v10 over Spore_Game v7; same clips).
fn groups_of(mut lists: Vec<(u32, crate::tlsa::Tlsa)>) -> HashMap<u32, (u32, u32)> {
    lists.sort_by_key(|(v, _)| *v);
    let mut groups = HashMap::new();
    for (_, list) in lists {
        for group in list.groups.iter().filter(|g| g.id >> 16 == 0xAAAA) {
            if let Some(anim) = group.choices.first().and_then(|c| c.animations.first()) {
                groups.insert(group.id, (anim.id, group.end_mode));
            }
        }
    }
    groups
}

fn read_groups(originals: &Originals) -> HashMap<u32, (u32, u32)> {
    let mut lists: Vec<(u32, crate::tlsa::Tlsa)> = originals
        .action_lists
        .lists
        .iter()
        .map(|(_, l)| (l.version, l.clone()))
        .collect();
    if lists.is_empty() {
        // Startup may not have cataloged action lists (headless tests).
        lists = index_lists(&originals.resource_index);
    }
    groups_of(lists)
}

fn load_groups(assets: &mut CellAnimAssets, originals: &Originals) {
    if assets.groups_loaded {
        return;
    }
    assets.groups_loaded = true;
    assets.groups = read_groups(originals);
    info!("cell animation: {} TLSA groups", assets.groups.len());
}

fn clip(assets: &mut CellAnimAssets, originals: &Originals, instance: u32) -> Option<Arc<CellClip>> {
    assets
        .clips
        .entry(instance)
        .or_insert_with(|| {
            let key = ResourceKey {
                group_id: 0,
                instance_id: instance,
                type_id: crate::decode::TYPE_ANIM,
            };
            originals
                .resource_index
                .read(key)
                .and_then(|b| crate::animation::parse(&b))
                .map(|a| Arc::new(CellClip::from_animation(a)))
                .map_err(|e| warn!("cell clip {instance:08X}: {e}"))
                .ok()
        })
        .clone()
}

fn facts_for(assets: &mut CellAnimAssets, originals: &Originals, anim_id: u32) -> Option<ClipFacts> {
    let (instance, _) = *assets.groups.get(&anim_id)?;
    let c = clip(assets, originals, instance)?;
    Some((instance, c.seconds, c.cyclic))
}

// ---------------------------------------------------------------------------
// Clip lengths and event times of the retail rows (what PlayAnimation returns)
// ---------------------------------------------------------------------------

/// Name of the eventControl marker that releases a timed event's callback.
pub const CONTACT_EVENT: &str = "contact";

/// One clip event with its time (the Info keys of the clip, `0x9A6D70`'s
/// dispatcher input): `kind` is `flags & 0x20007` (0 sound, 1 effect, 2
/// event control, 4 stop, 6 parameter, 7 footstep, 0x20001 message).
#[derive(Clone, Debug, PartialEq)]
pub struct ClipMark {
    pub time: f32,
    pub kind: u32,
    pub name: String,
}

/// The clip of one table row.
#[derive(Clone, Debug, PartialEq)]
pub struct RowClip {
    /// ANIM instance id.
    pub clip: u32,
    pub seconds: f32,
    pub cyclic: bool,
    pub events: Vec<ClipMark>,
}

/// Every row of [`ROWS`] resolved to its clip once (TLSA group -> ANIM), so the
/// rules can time a game event the way retail does: `PlayAnimation` (0xE6D200)
/// returns the clip length (`GetAnimationLength`, 0xE4F020: 1.0 when the clip
/// has none) times G(anim_dilation, realSize[cell] / realSize[player]) for rows
/// that ask for dilation, and the event is created with that time
/// (0xE51EC0(type, time, ...)). Filled by [`load_clip_times`] on the first
/// frame of the stage; every lookup answers `None` until then (the callers keep
/// their fallback constants, e.g. without installed clips).
#[derive(Resource, Default)]
pub struct CellClipTimes {
    rows: Vec<Option<RowClip>>,
}

/// The events of a decoded clip with their times.
pub fn clip_marks(anim: &crate::animation::Animation) -> Vec<ClipMark> {
    let mut out = vec![];
    for channel in &anim.channels {
        for component in &channel.components {
            let crate::animation::Keys::Info(keys) = &component.keys else {
                continue;
            };
            for k in keys {
                for event in anim.events.iter().skip(k.event_start as usize).take(k.event_count as usize) {
                    out.push(ClipMark {
                        time: k.frame as f32 * anim.frame_length,
                        kind: event.flags & 0x2_0007,
                        name: event.name.clone(),
                    });
                }
            }
        }
    }
    out.sort_by(|a, b| a.time.total_cmp(&b.time));
    out
}

impl CellClipTimes {
    pub fn is_loaded(&self) -> bool {
        !self.rows.is_empty()
    }

    /// Resolve every row's clip from the installed packages.
    pub fn load(&mut self, originals: &Originals) {
        self.load_rows(&read_groups(originals), &originals.resource_index);
    }

    fn load_rows(&mut self, groups: &HashMap<u32, (u32, u32)>, index: &ResourceIndex) {
        let mut clips: HashMap<u32, Option<RowClip>> = HashMap::new();
        self.rows = ROWS
            .iter()
            .map(|row| {
                let (instance, _) = *groups.get(&row.anim)?;
                clips
                    .entry(instance)
                    .or_insert_with(|| {
                        let key = ResourceKey {
                            group_id: 0,
                            instance_id: instance,
                            type_id: crate::decode::TYPE_ANIM,
                        };
                        index
                            .read(key)
                            .and_then(|b| crate::animation::parse(&b))
                            .ok()
                            .map(|a| RowClip {
                                clip: instance,
                                seconds: a.duration_seconds(),
                                cyclic: a.flags & 8 != 0,
                                events: clip_marks(&a),
                            })
                    })
                    .clone()
            })
            .collect();
    }

    /// A table with synthetic clips for tests: (row, seconds, events as
    /// (time, kind, name)).
    #[cfg(test)]
    pub fn for_test(rows: &[(usize, f32, &[(f32, u32, &str)])]) -> Self {
        let mut table = vec![None; ROWS.len()];
        for (row, seconds, events) in rows {
            table[*row] = Some(RowClip {
                clip: 1,
                seconds: *seconds,
                cyclic: false,
                events: events
                    .iter()
                    .map(|(time, kind, name)| ClipMark { time: *time, kind: *kind, name: name.to_string() })
                    .collect(),
            });
        }
        Self { rows: table }
    }

    /// The clip of table row `row`.
    pub fn row_clip(&self, row: usize) -> Option<&RowClip> {
        self.rows.get(row)?.as_ref()
    }

    /// Row and clip `Select` picks for a request (the first candidate: the
    /// request codes events are timed with answer one clip per context, see the
    /// `event_requests_have_one_length_per_context` census).
    pub fn pick(&self, code: i32, ctx: &SelectContext) -> Option<(usize, &RowClip)> {
        let row = select(code, ctx, 0.0)?;
        Some((row, self.row_clip(row)?))
    }

    /// When the playing clip posts its `contact` event (an eventControl marker
    /// with id FNV("contact") = 0xCA880D53) times the dilation: a timed game
    /// event's callback (the bite landing 0xE7B280, the swallow taking its
    /// victim 0xE7DB80) runs the first frame the cell's animation has posted it
    /// (0xE59C10 reads the cell's event inbox with 0xE50600), else at the end of
    /// the event's timer. `None` for a clip without a `contact` marker.
    pub fn contact_seconds(&self, code: i32, ctx: &SelectContext, dilation: f32) -> Option<f32> {
        let (row, clip) = self.pick(code, ctx)?;
        let mark = clip.events.iter().find(|e| e.kind == 2 && e.name == CONTACT_EVENT)?;
        Some(if ROWS[row].dilate && dilation > 0.0 { mark.time * dilation } else { mark.time })
    }

    /// What `PlayAnimation(cell, code)` returns for a modelled cell:
    /// the clip length (1.0 for none) times `dilation` (see [`dilation_factor`]) when
    /// the row asks for it. `None` before the table is loaded or when no row
    /// answers the request.
    pub fn play_seconds(&self, code: i32, ctx: &SelectContext, dilation: f32) -> Option<f32> {
        let (row, clip) = self.pick(code, ctx)?;
        let length = if clip.seconds > 0.0 { clip.seconds } else { 1.0 };
        Some(if ROWS[row].dilate && dilation > 0.0 { length * dilation } else { length })
    }
}

/// The dilation factor of `PlayAnimation` (0xE563E0): G(anim_dilation,
/// realSize[cell.mScale] / realSize[player]); `None` is the avatar (mScale -1),
/// whose ratio is 1 (G = 0.75).
pub fn dilation_factor(cell: Option<crate::cell_data::CellScale>, player: crate::cell_data::CellScale) -> f32 {
    let ratio = cell
        .filter(|c| *c != crate::cell_data::CellScale::NONE)
        .map_or(1.0, |c| crate::cell_engine::real_size(c) / crate::cell_engine::real_size(player));
    graph(&ANIM_DILATION, ratio)
}

/// Fill [`CellClipTimes`] once the installed packages are known.
pub fn load_clip_times(mut times: ResMut<CellClipTimes>, originals: Option<Res<Originals>>) {
    if times.is_loaded() {
        return;
    }
    let Some(originals) = originals else { return };
    times.load(&originals);
    let found = times.rows.iter().flatten().count();
    info!("cell animation: {found} of {} table rows resolve to a clip", ROWS.len());
}

fn bindposes(
    assets: &mut CellAnimAssets,
    world: &mut World,
    count: usize,
) -> Handle<SkinnedMeshInverseBindposes> {
    assets
        .bindposes
        .entry(count)
        .or_insert_with(|| {
            world
                .resource_mut::<Assets<SkinnedMeshInverseBindposes>>()
                .add(vec![Mat4::IDENTITY; count])
        })
        .clone()
}

fn skinned_copy(source: &Mesh, joints: Vec<[u16; 4]>, weights: Vec<[f32; 4]>) -> Option<Mesh> {
    let count = match source.attribute(Mesh::ATTRIBUTE_POSITION)? {
        VertexAttributeValues::Float32x3(v) => v.len(),
        _ => return None,
    };
    if count != joints.len() || count != weights.len() {
        return None;
    }
    let mut mesh = source.clone();
    mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(joints));
    mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, weights);
    mesh.generate_skinned_mesh_bounds().ok()?;
    Some(mesh)
}

fn spawn_joints(world: &mut World, holder: Entity, count: usize) -> Vec<Entity> {
    (0..count)
        .map(|_| {
            world
                .spawn((
                    StageOwned,
                    Name::new("cell joint"),
                    Transform::IDENTITY,
                    ChildOf(holder),
                ))
                .id()
        })
        .collect()
}

/// The player's body mesh space from GMDL model space (ADAPTED: the engine
/// bakes its own fit into the stripped body mesh, so the mapping is rebuilt
/// from the two bounding boxes; the fit's half-turn about Y, if any, is read
/// from where the vertex centroid sits inside each box).
fn player_mesh_from_model(rig: &ModelRig, model: &crate::baked_model::Model, mesh: &Mesh) -> Option<Affine3A> {
    let VertexAttributeValues::Float32x3(positions) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)? else {
        return None;
    };
    let (mut mn, mut mx, mut msum) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY), Vec3::ZERO);
    for p in positions {
        mn = mn.min(Vec3::from_array(*p));
        mx = mx.max(Vec3::from_array(*p));
        msum += Vec3::from_array(*p);
    }
    let basis = gmdl_basis();
    let (mut gn, mut gx, mut gsum, mut gcount) =
        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY), Vec3::ZERO, 0usize);
    for (piece, (skin, _)) in model.pieces.iter().zip(&rig.skins) {
        let drawn = crate::animation_layout::drawn_vertices(piece).ok()?;
        for (v, joints) in skin.iter().enumerate() {
            if drawn[v] && joints.iter().all(|&j| (j as usize) < rig.body_joints) {
                let p = basis.transform_point3(Vec3::from_array(piece.positions[v]));
                gn = gn.min(p);
                gx = gx.max(p);
                gsum += p;
                gcount += 1;
            }
        }
    }
    let span = (gx - gn).max_element();
    if !mn.is_finite() || !gn.is_finite() || span <= 0.0 || positions.is_empty() || gcount == 0 {
        return None;
    }
    let scale = (mx - mn).max_element() / span;
    let center_g = (gn + gx) * 0.5;
    let center_m = (mn + mx) * 0.5;
    let lean_m = msum / positions.len() as f32 - center_m;
    let lean_g = (gsum / gcount as f32 - center_g) * scale;
    let turned = Quat::from_rotation_y(std::f32::consts::PI);
    let rotation = if (turned * lean_g).distance(lean_m) <= lean_g.distance(lean_m) {
        turned
    } else {
        Quat::IDENTITY
    };
    Some(
        Affine3A::from_translation(center_m)
            * Affine3A::from_quat(rotation)
            * Affine3A::from_scale(Vec3::splat(scale))
            * Affine3A::from_translation(-center_g)
            * basis,
    )
}

/// Build skinned visuals for cells (NPC GMDLs, the player body) and the
/// player's parts. Unsupported models keep their meshes (body motion only).
pub fn attach_visuals(world: &mut World) {
    if !world.contains_resource::<Originals>() || !world.contains_resource::<Assets<SkinnedMeshInverseBindposes>>() {
        return;
    }
    let pending: Vec<(Entity, ResourceKey, bool)> = world
        .query_filtered::<(Entity, &OriginalCellVisual, Has<crate::game::Player>), (Without<CellAnimSkeleton>, Without<CellAnimBodyOnly>)>()
        .iter(world)
        .map(|(e, v, p)| (e, v.model, p))
        .collect();
    let parts: Vec<(Entity, Handle<Mesh>)> = world
        .query_filtered::<(Entity, &Mesh3d), (With<crate::editor::CellPart>, Without<CellPartSkeleton>)>()
        .iter(world)
        .map(|(e, m)| (e, m.0.clone()))
        .collect();
    if pending.is_empty() && parts.is_empty() {
        return;
    }
    world.resource_scope(|world, mut assets: Mut<CellAnimAssets>| {
        world.resource_scope(|world, originals: Mut<Originals>| {
            load_groups(&mut assets, &originals);
            for (entity, key, player) in pending {
                if !attach_cell(world, &mut assets, &originals, entity, key, player) {
                    world.entity_mut(entity).insert(CellAnimBodyOnly);
                }
            }
            for (entity, mesh) in parts {
                attach_part(world, &mut assets, &originals, entity, mesh);
            }
        });
    });
}

fn model_rig(assets: &mut CellAnimAssets, originals: &Originals, key: ResourceKey) -> Option<Arc<ModelRig>> {
    if let Some(rig) = assets.models.get(&key) {
        return rig.clone();
    }
    let rig = ModelRig::build(&originals.resource_index, &originals.part_catalog, key, &mut assets.rigs)
        .map(Arc::new)
        .map_err(|e| warn!("cell model {:08X} not rigged: {e}", key.instance_id))
        .ok();
    if let Some(r) = &rig {
        info!(
            "cell model {:08X}: {} joints, {} rigged parts",
            key.instance_id,
            r.joint_count,
            r.parts.len()
        );
    }
    assets.models.insert(key, rig.clone());
    rig
}

fn attach_cell(
    world: &mut World,
    assets: &mut CellAnimAssets,
    originals: &Originals,
    entity: Entity,
    key: ResourceKey,
    player: bool,
) -> bool {
    let Some(rig) = model_rig(assets, originals, key) else {
        return false;
    };
    let Some(source) = originals.baked_models.get(&key).cloned() else {
        return false;
    };
    if player {
        // The engine draws the stripped starter body on the player entity.
        let Some(mesh_handle) = world.get::<Mesh3d>(entity).map(|m| m.0.clone()) else {
            return false;
        };
        let Some(mesh) = world.resource::<Assets<Mesh>>().get(&mesh_handle).cloned() else {
            return false;
        };
        let Ok(model) = originals
            .resource_index
            .read(key)
            .and_then(|b| crate::baked_model::parse(&b))
        else {
            return false;
        };
        let Some(mesh_from_model) = player_mesh_from_model(&rig, &model, &mesh) else {
            return false;
        };
        let count = match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(VertexAttributeValues::Float32x3(v)) => v.len(),
            _ => return false,
        };
        let Some(skinned) = skinned_copy(&mesh, vec![[0; 4]; count], vec![[1.0, 0.0, 0.0, 0.0]; count]) else {
            return false;
        };
        let handle = world.resource_mut::<Assets<Mesh>>().add(skinned);
        let inverse = bindposes(assets, world, 1);
        let joints = spawn_joints(world, entity, 1);
        world.entity_mut(entity).insert((
            Mesh3d(handle),
            SkinnedMesh {
                inverse_bindposes: inverse,
                joints: joints.clone(),
            },
            DynamicSkinnedMeshBounds,
            CellAnimSkeleton {
                model: key,
                joints,
                holder: entity,
                surfaces: vec![],
                mesh_from_model,
                player: true,
            },
        ));
        return true;
    }
    // NPC: the engine's visual child draws surface 0 of the baked model.
    let surface0 = source.draw_mesh();
    let holder = world.get::<Children>(entity).and_then(|children| {
        children.iter().find(|c| {
            world
                .get::<Mesh3d>(*c)
                .is_some_and(|m| m.0 == surface0 || source.surfaces.iter().any(|s| s.handle == m.0))
        })
    });
    let Some(holder) = holder else {
        return false;
    };
    let local = world.get::<Transform>(holder).copied().unwrap_or_default();
    let mut handles = vec![];
    for (i, surface) in source.surfaces.iter().enumerate() {
        let handle = match assets.skinned.get(&(key, i)) {
            Some(h) => h.clone(),
            None => {
                let Some((joints, weights)) = rig.skins.get(surface.source_section).cloned() else {
                    return false;
                };
                let Some(mesh) = world.resource::<Assets<Mesh>>().get(&surface.handle).cloned() else {
                    return false;
                };
                let Some(skinned) = skinned_copy(&mesh, joints, weights) else {
                    return false;
                };
                let h = world.resource_mut::<Assets<Mesh>>().add(skinned);
                assets.skinned.insert((key, i), h.clone());
                h
            }
        };
        handles.push((handle, surface.material.clone()));
    }
    let inverse = bindposes(assets, world, rig.joint_count);
    let joints = spawn_joints(world, holder, rig.joint_count);
    let skinned = SkinnedMesh {
        inverse_bindposes: inverse,
        joints: joints.clone(),
    };
    let mut surfaces = vec![];
    for (i, (mesh, material)) in handles.into_iter().enumerate() {
        if i == 0 {
            world.entity_mut(holder).insert((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                skinned.clone(),
                DynamicSkinnedMeshBounds,
            ));
        } else {
            // Multi-piece models: the engine draws only the first surface.
            let extra = world
                .spawn((
                    StageOwned,
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    local,
                    skinned.clone(),
                    DynamicSkinnedMeshBounds,
                    ChildOf(entity),
                ))
                .id();
            surfaces.push(extra);
        }
    }
    world.entity_mut(entity).insert(CellAnimSkeleton {
        model: key,
        joints,
        holder,
        surfaces,
        mesh_from_model: gmdl_basis(),
        player: false,
    });
    true
}

/// Skin one player part (an editor part model) on its own rig.
fn attach_part(
    world: &mut World,
    assets: &mut CellAnimAssets,
    originals: &Originals,
    entity: Entity,
    mesh: Handle<Mesh>,
) {
    let entry = assets.part_meshes.get(&mesh.id()).cloned();
    let entry = match entry {
        Some(e) => e,
        None => {
            let built = build_part_mesh(world, assets, originals, &mesh);
            let value = built.map(|(m, rig, c)| (world.resource_mut::<Assets<Mesh>>().add(m), rig, c));
            assets.part_meshes.insert(mesh.id(), value.clone());
            value
        }
    };
    let definition = originals
        .models
        .iter()
        .find(|m| m.surfaces.first().is_some_and(|s| s.handle == mesh))
        .and_then(|m| originals.part_catalog.by_model_in(CELL_PARTS, m.instance));
    // Electric parts carry the avatar's charged glow (cell_fx::charged_glow).
    if definition.is_some_and(|d| d.abilities.cell_electric > 0) {
        world.entity_mut(entity).insert(crate::cell_fx::CellElectricPart);
    }
    let Some((skinned, rig_key, mesh_from_rig)) = entry else {
        // No usable rig: the part follows the body motion as a rigid piece.
        world.entity_mut(entity).insert(CellPartSkeleton {
            rig: ResourceKey { group_id: 0, instance_id: 0, type_id: 0 },
            joints: vec![],
            mesh_from_rig: Affine3A::IDENTITY,
            caps: vec![],
            region: Region::default(),
        });
        return;
    };
    let Some(rig) = assets
        .rigs
        .get(&rig_key)
        .cloned()
        .flatten()
    else {
        return;
    };
    let (caps, _) = definition.map(|d| part_caps(&d.abilities)).unwrap_or_default();
    let caps = if caps.is_empty() { caps_from_rig(&rig) } else { caps };
    // Region from the slot on the player's body (Bevy body space: front -Z,
    // left -X) mapped to the Spore convention used by the channels.
    let slot = world.get::<Transform>(entity).map(|t| t.translation).unwrap_or_default();
    let region = Region::classify(Vec3::new(-slot.x, slot.z, slot.y), Vec3::new(-0.9, -0.9, -0.9), Vec3::new(0.9, 0.9, 0.9));
    let inverse = bindposes(assets, world, rig.bone_count());
    let joints = spawn_joints(world, entity, rig.bone_count());
    world.entity_mut(entity).insert((
        Mesh3d(skinned),
        SkinnedMesh {
            inverse_bindposes: inverse,
            joints: joints.clone(),
        },
        DynamicSkinnedMeshBounds,
        CellPartSkeleton {
            rig: rig_key,
            joints,
            mesh_from_rig,
            caps,
            region,
        },
    ));
}

fn part_rig_cached(rigs: &mut HashMap<ResourceKey, Option<Arc<PartRig>>>, index: &ResourceIndex, key: ResourceKey) -> Option<Arc<PartRig>> {
    rigs.entry(key)
        .or_insert_with(|| index.read(key).and_then(|b| PartRig::from_rw4(&b)).map(Arc::new).ok())
        .clone()
}

/// Skinned copy of an editor part's first surface plus its rig key and the
/// rig -> mesh mapping of `editor::assembled_mesh` (Rx(-90°)·R_def·(v + offset)).
fn build_part_mesh(
    world: &mut World,
    assets: &mut CellAnimAssets,
    originals: &Originals,
    mesh: &Handle<Mesh>,
) -> Option<(Mesh, ResourceKey, Affine3A)> {
    let model = originals
        .models
        .iter()
        .find(|m| m.surfaces.first().is_some_and(|s| s.handle == *mesh))?;
    let definition: &PartDefinition = originals.part_catalog.by_model_in(CELL_PARTS, model.instance)?;
    let key = definition.model;
    if key.type_id != RW4 {
        return None;
    }
    let blob = originals.resource_index.read(key).ok()?;
    let mut stats = rw4::MeshStats::default();
    let imported = rw4::extract_meshes(&blob, &mut stats);
    let first = imported.first()?;
    let n = first.positions.len();
    let (joints, weights): (Vec<[u16; 4]>, Vec<[f32; 4]>) = if first.bone_indices.len() == n && first.bone_weights.len() == n {
        first
            .bone_indices
            .iter()
            .zip(&first.bone_weights)
            .map(|(b, w)| {
                let total: f32 = w.iter().sum();
                let w = if total > 0.0 { w.map(|x| x / total) } else { [1.0, 0.0, 0.0, 0.0] };
                (b.map(|x| x as u16 / 3), w)
            })
            .unzip()
    } else if first.skin_idx.len() == n && first.skin_w.len() == n {
        first
            .skin_idx
            .iter()
            .zip(&first.skin_w)
            .map(|(b, w)| {
                let total = w[0] + w[1];
                let w = if total > 0.0 { [w[0] / total, w[1] / total, 0.0, 0.0] } else { [1.0, 0.0, 0.0, 0.0] };
                ([b[0] / 3, b[1] / 3, 0, 0], w)
            })
            .unzip()
    } else {
        return None;
    };
    let rig = part_rig_cached(&mut assets.rigs, &originals.resource_index, key)?;
    if joints.iter().flatten().any(|&j| j as usize >= rig.bone_count()) {
        return None;
    }
    let source = world.resource::<Assets<Mesh>>().get(mesh)?.clone();
    let skinned = skinned_copy(&source, joints, weights)?;
    let rotation = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
        * Quat::from_euler(
            EulerRot::XYZ,
            definition.model_rotation[0].to_radians(),
            definition.model_rotation[1].to_radians(),
            definition.model_rotation[2].to_radians(),
        );
    let mesh_from_rig = Affine3A::from_quat(rotation) * Affine3A::from_translation(Vec3::from_array(definition.model_offset));
    Some((skinned, key, mesh_from_rig))
}

struct ActorWork {
    entity: Entity,
    skeleton: CellAnimSkeleton,
    request: i32,
    speed: f32,
    /// `CellObject::scale` of an NPC (`None` for the avatar, mScale -1).
    scale: Option<crate::cell_data::CellScale>,
    /// Size test inputs (RE 13): the cell's size in retail world units,
    /// ScaleDiff == MuchLarger, background pool.
    size_world: f32,
    epic: bool,
    background: bool,
}

/// Step playback for every animated cell and pose its joints.
pub fn animate(world: &mut World) {
    if !world.contains_resource::<Originals>() {
        return;
    }
    let dt = world.resource::<Time>().delta_secs().clamp(0.0, 0.1);
    let forced = *world.resource::<CellAnimOverride>();
    let mut actors = vec![];
    // The player's size level: PlayAnimation's dilation uses
    // realSize[cell.scale] / realSize[player] (0xE563E0).
    let player_scale = world
        .get_resource::<crate::game::RunProgress>()
        .map(|r| crate::cell_engine::player_scale(r.cell.food_progression));
    {
        let mut query = world.query::<(Entity, &CellAnimSkeleton, &CellAnimState)>();
        let mut objects = world.query::<&crate::cell_engine::CellObject>();
        let found: Vec<_> = query
            .iter(world)
            .map(|(e, s, st)| (e, s.clone(), forced.code.unwrap_or(st.request_code), st.speed))
            .collect();
        for (entity, skeleton, request, speed) in found {
            let (size_world, epic, background, scale) = objects
                .get(world, entity)
                .map(|o| {
                    (
                        o.radius / crate::cell_engine::UNIT,
                        player_scale.is_some_and(|p| crate::cell_engine::scale_diff(o.scale, p) == 4),
                        o.background,
                        (!skeleton.player).then_some(o.scale),
                    )
                })
                .unwrap_or((0.0, false, false, None));
            actors.push(ActorWork {
                entity,
                skeleton,
                request,
                speed,
                scale,
                size_world,
                epic,
                background,
            });
        }
    }
    let player_parts: Vec<(Entity, CellPartSkeleton, Transform)> = world
        .query_filtered::<(Entity, &CellPartSkeleton, &Transform), With<crate::editor::CellPart>>()
        .iter(world)
        .map(|(e, s, t)| (e, s.clone(), *t))
        .collect();
    world.resource_scope(|world, mut assets: Mut<CellAnimAssets>| {
        world.resource_scope(|world, originals: Mut<Originals>| {
            for actor in actors {
                let Some(rig) = assets.models.get(&actor.skeleton.model).cloned().flatten() else {
                    continue;
                };
                let ctx = SelectContext {
                    avatar: actor.skeleton.player,
                    parts: if actor.skeleton.player {
                        player_part_mask(world, &originals)
                    } else {
                        rig.parts_present
                    },
                    // RE 13: SizeTest "big" = transform scale > 2.0 world
                    // units; 3 = MuchLarger; LayerTest = background pool.
                    big: actor.size_world > 2.0,
                    epic: actor.epic,
                    background: actor.background,
                };
                let dilation = player_scale.map_or(graph(&ANIM_DILATION, 1.0), |p| dilation_factor(actor.scale, p));
                // Playback step (selection + timing).
                let mut playback = world
                    .get::<CellAnimPlayback>(actor.entity)
                    .cloned()
                    .unwrap_or_else(|| {
                        CellAnimPlayback::with_seed((actor.entity.to_bits() as u32).wrapping_mul(2_654_435_761))
                    });
                {
                    let assets_ref = &mut *assets;
                    let originals_ref = &*originals;
                    let facts_cache = std::cell::RefCell::new(assets_ref);
                    let facts = |id: u32| facts_for(&mut facts_cache.borrow_mut(), originals_ref, id);
                    playback.step(actor.request, dt, actor.speed, &ctx, dilation, forced.row, &facts);
                }
                let finished = playback.finished;
                if let Some(mut state) = world.get_mut::<CellAnimState>(actor.entity) {
                    if state.finished != finished {
                        state.finished = finished;
                    }
                }
                // Skip posing for visuals the camera did not see last frame.
                let visible = world
                    .get::<ViewVisibility>(actor.skeleton.holder)
                    .is_none_or(|v| v.get());
                // Weapon parts need their pose for the glow, the poison emit
                // points and the zap sources even when off screen.
                let weapons = rig.anchors.iter().any(|a| a.kind == part_type::POISON || a.kind == part_type::ELECTRIC);
                let pose = if visible || actor.skeleton.player || weapons {
                    evaluate(&mut assets, &originals, &rig, &playback)
                } else {
                    None
                };
                world.entity_mut(actor.entity).insert(playback);
                let Some((body, palette)) = pose else {
                    continue;
                };
                let mesh_from_model = actor.skeleton.mesh_from_model;
                let model_from_mesh = mesh_from_model.inverse();
                if !actor.skeleton.player && !rig.anchors.is_empty() {
                    let fit = world.get::<Transform>(actor.skeleton.holder).copied().unwrap_or_default();
                    let poses = part_poses(&rig, body, &palette, mesh_from_model, fit);
                    world.entity_mut(actor.entity).insert(poses);
                }
                for (joint, matrix) in actor.skeleton.joints.iter().zip(&palette) {
                    let m = mesh_from_model * *matrix * model_from_mesh;
                    if let Some(mut t) = world.get_mut::<Transform>(*joint) {
                        *t = Transform::from_matrix(Mat4::from(m));
                    }
                }
                if actor.skeleton.player {
                    let body_mesh = mesh_from_model * body * model_from_mesh;
                    pose_player_parts(world, &mut assets, &originals, &player_parts, body_mesh, actor.entity);
                }
            }
        });
    });
}

fn player_part_mask(world: &mut World, originals: &Originals) -> u32 {
    let run = world.resource::<crate::game::RunProgress>();
    let mut mask = 0;
    for part in &run.cell_parts {
        if let Some(d) = originals.part_catalog.by_part_in(CELL_PARTS, part) {
            if let Some(kind) = part_caps(&d.abilities).1 {
                mask |= 1 << kind;
            }
        }
    }
    mask
}

/// Sample the playing clip(s) and build the model-space palette.
fn evaluate(
    assets: &mut CellAnimAssets,
    originals: &Originals,
    rig: &ModelRig,
    playback: &CellAnimPlayback,
) -> Option<(Affine3A, Vec<Affine3A>)> {
    let (w_current, w_previous) = playback.weights();
    let mut layers = vec![];
    for (playing, weight) in [(&playback.current, w_current), (&playback.previous, w_previous)] {
        let Some(playing) = playing else { continue };
        if weight <= 0.0 {
            continue;
        }
        let Some(c) = clip(assets, originals, playing.clip) else {
            continue;
        };
        layers.push((c, playing.sample_time(), weight));
    }
    if layers.is_empty() {
        return None;
    }
    let mut body = BodyPose::default();
    let mut part_locals: Vec<Vec<Bone3>> = vec![];
    let mut accumulated = 0.0;
    for (c, t, weight) in &layers {
        let pose = BodyPose::sample(c, *t);
        let blend = if accumulated <= 0.0 { 1.0 } else { weight / (accumulated + weight) };
        body = if accumulated <= 0.0 { pose } else { body.blend(&pose, blend) };
        for (i, part) in rig.parts.iter().enumerate() {
            let tracks = bind_channel(c, &part.caps, part.region, &part.rig)
                .map(|ch| part_tracks(c, ch, *t))
                .unwrap_or_default();
            let mut locals = vec![];
            part.rig.locals(&tracks, &mut locals);
            if accumulated <= 0.0 {
                part_locals.push(locals);
            } else if let Some(existing) = part_locals.get_mut(i) {
                blend_locals(existing, &locals, blend);
            }
        }
        accumulated += weight;
    }
    let body_matrix = body.matrix(rig.pivot);
    let skins: Vec<Vec<Affine3A>> = rig
        .parts
        .iter()
        .zip(&part_locals)
        .map(|(part, locals)| {
            let mut out = vec![];
            part.rig.skin(locals, &mut out);
            out
        })
        .collect();
    let mut palette = vec![];
    rig.palette(body_matrix, &skins, &mut palette);
    Some((body_matrix, palette))
}

/// Pose the player's parts on their own rigs, following the body motion.
fn pose_player_parts(
    world: &mut World,
    assets: &mut CellAnimAssets,
    originals: &Originals,
    parts: &[(Entity, CellPartSkeleton, Transform)],
    body_mesh: Affine3A,
    player: Entity,
) {
    let Some(playback) = world.get::<CellAnimPlayback>(player).cloned() else {
        return;
    };
    let (w_current, w_previous) = playback.weights();
    for (_, skeleton, transform) in parts {
        let placement = Affine3A::from_scale_rotation_translation(transform.scale, transform.rotation, transform.translation);
        let follow = placement.inverse() * body_mesh * placement;
        let rig = (skeleton.rig.type_id != 0)
            .then(|| assets.rigs.get(&skeleton.rig).cloned().flatten())
            .flatten();
        let Some(rig) = rig else {
            // Rigid part: no joints, nothing to pose beyond its own transform.
            let _ = follow;
            continue;
        };
        let mut locals: Option<Vec<Bone3>> = None;
        let mut accumulated = 0.0;
        for (playing, weight) in [(&playback.current, w_current), (&playback.previous, w_previous)] {
            let Some(playing) = playing else { continue };
            if weight <= 0.0 {
                continue;
            }
            let Some(c) = clip(assets, originals, playing.clip) else {
                continue;
            };
            let t = playing.sample_time();
            let tracks = bind_channel(&c, &skeleton.caps, skeleton.region, &rig)
                .map(|ch| part_tracks(&c, ch, t))
                .unwrap_or_default();
            let mut layer = vec![];
            rig.locals(&tracks, &mut layer);
            match &mut locals {
                None => locals = Some(layer),
                Some(existing) => blend_locals(existing, &layer, weight / (accumulated + weight)),
            }
            accumulated += weight;
        }
        let locals = locals.unwrap_or_else(|| rig.bind_local.clone());
        let mut skins = vec![];
        rig.skin(&locals, &mut skins);
        let rig_from_mesh = skeleton.mesh_from_rig.inverse();
        if std::env::var_os("OPENSPORE_DEBUG_CELL_ANIM").is_some() {
            let (s, r, t) = follow.to_scale_rotation_translation();
            let (bs, br, bt) = body_mesh.to_scale_rotation_translation();
            info!(
                "cell part {:08X}: placement t={} r={} s={} follow s={s} angle={:.3} t={t} body s={bs} angle={:.3} t={bt} joints={}",
                skeleton.rig.instance_id,
                transform.translation,
                transform.rotation,
                transform.scale,
                r.to_axis_angle().1,
                br.to_axis_angle().1,
                skeleton.joints.len()
            );
        }
        for (joint, skin) in skeleton.joints.iter().zip(&skins) {
            let m = follow * skeleton.mesh_from_rig * *skin * rig_from_mesh;
            if let Some(mut t) = world.get_mut::<Transform>(*joint) {
                *t = Transform::from_matrix(Mat4::from(m));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod pose_tests {
    use super::*;

    fn rig(anchors: Vec<PartAnchor>) -> ModelRig {
        ModelRig {
            key: ResourceKey { group_id: 0, instance_id: 0, type_id: 0 },
            joint_count: 2,
            body_joints: 1,
            parts: vec![],
            pivot: Vec3::ZERO,
            model_radius: 1.0,
            parts_present: 0,
            skins: vec![],
            anchors,
        }
    }

    #[test]
    fn part_poses_follow_the_joint_and_the_fit() {
        // Model (x, y, z) is mesh (-x, z, y); the fit scales by 2 and moves by (1, 0, 3).
        let anchor = PartAnchor {
            block: 3,
            kind: part_type::ELECTRIC,
            rest: Vec3::new(0.5, -1.0, 0.25),
            emit: Vec3::new(0.5, -1.5, 0.25),
            axis: Vec3::NEG_Y,
            radius: 0.1,
            joint: Some(1),
        };
        let rig = rig(vec![anchor]);
        let fit = Transform::from_translation(Vec3::new(1.0, 0.0, 3.0)).with_scale(Vec3::splat(2.0));
        // At rest (identity joints) the pose is the fitted rigblock position.
        let rest = part_poses(&rig, Affine3A::IDENTITY, &[Affine3A::IDENTITY; 2], gmdl_basis(), fit);
        let p = rest.parts[0];
        assert_eq!((p.block, p.kind), (3, part_type::ELECTRIC));
        assert!((p.point - Vec2::new(-0.5 * 2.0 + 1.0, -1.0 * 2.0 + 3.0)).length() < 1e-5);
        assert!((p.emit - Vec2::new(-1.0 + 1.0, -3.0 + 3.0)).length() < 1e-5);
        // The model's -Y axis (outward) is mesh -Z, plane (0, -1).
        assert!((p.axis - Vec2::new(0.0, -1.0)).length() < 1e-5);
        // The part's joint moves it (a model-space shift of +1 on x), the body joint does not.
        let moved = Affine3A::from_translation(Vec3::X);
        let bent = part_poses(&rig, Affine3A::IDENTITY, &[Affine3A::IDENTITY, moved], gmdl_basis(), fit);
        assert!((bent.parts[0].point - Vec2::new(-1.5 * 2.0 + 1.0, -1.0 * 2.0 + 3.0)).length() < 1e-5);
        // A part without a joint follows the body matrix.
        let mut free = rig.clone();
        free.anchors[0].joint = None;
        let carried = part_poses(&free, moved, &[Affine3A::IDENTITY; 2], gmdl_basis(), fit);
        assert!((carried.parts[0].point - bent.parts[0].point).length() < 1e-5);
        assert_eq!(carried.of_kind(part_type::POISON).count(), 0);
    }

    #[test]
    fn the_root_deform_does_not_scale_the_body() {
        // The root 0x70E47545 is an audio property: the body matrix carries no scale.
        let pose = BodyPose::default();
        let m = pose.matrix(Vec3::new(1.0, 2.0, 3.0));
        assert!((m.transform_vector3(Vec3::X).length() - 1.0).abs() < 1e-6);
        assert!(m.translation.length() < 1e-6);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell_contract::CellAnimation;

    #[test]
    fn rows_match_the_contract_table() {
        for (i, animation) in CellAnimation::ALL.iter().enumerate() {
            assert_eq!(ROWS[i].anim, animation.animation_id(), "row {i}");
        }
    }

    fn ctx(parts: &[u8]) -> SelectContext {
        SelectContext {
            avatar: true,
            parts: parts.iter().fold(0, |m, p| m | (1 << p)),
            ..default()
        }
    }

    #[test]
    fn idle_picks_blinks_only_with_eyes_and_weights_five_to_one() {
        assert_eq!(candidates(code::IDLE, &ctx(&[])), vec![0]);
        assert_eq!(candidates(code::IDLE, &ctx(&[part_type::EYE])), vec![0, 1, 2, 3]);
        let c = ctx(&[part_type::EYE]);
        assert_eq!(select(code::IDLE, &c, 0.0), Some(0));
        assert_eq!(select(code::IDLE, &c, 0.62), Some(0)); // 5/8 = 0.625
        assert_eq!(select(code::IDLE, &c, 0.70), Some(1));
        assert_eq!(select(code::IDLE, &c, 0.99), Some(3));
    }

    #[test]
    fn movement_follows_jet_flagella_cilia_precedence_and_fallback() {
        let rows = |parts: &[u8]| candidates(code::MOVING, &ctx(parts));
        assert_eq!(rows(&[part_type::FLAGELLA]), vec![4, 5]);
        assert_eq!(rows(&[part_type::FLAGELLA, part_type::JET]), vec![19, 20]);
        assert_eq!(rows(&[part_type::CILIA]), vec![64, 65]);
        assert_eq!(rows(&[part_type::CILIA, part_type::FLAGELLA]), vec![4, 5]);
        // No movement part: only the condition-0 fallback row (0.001 weight).
        assert_eq!(rows(&[]), vec![21]);
        // NPC-only rows stay out for the avatar.
        let npc = SelectContext { avatar: false, ..ctx(&[part_type::CILIA]) };
        assert!(candidates(code::MOVE_MAD, &npc).contains(&67));
        assert!(!candidates(code::MOVE_MAD, &ctx(&[part_type::CILIA])).contains(&67));
    }

    fn timed(rows: &[(usize, f32)]) -> CellClipTimes {
        let rows: Vec<(usize, f32, &[(f32, u32, &str)])> = rows.iter().map(|(r, s)| (*r, *s, &[][..])).collect();
        CellClipTimes::for_test(&rows)
    }

    #[test]
    fn play_seconds_is_clip_length_times_dilation_or_one() {
        use crate::cell_data::CellScale;
        let avatar = SelectContext { avatar: true, ..default() };
        // Row 72 = request 43 (HatchEgg): 3.333 s, dilated; the player's ratio is 1 (G = 0.75).
        let times = timed(&[(72, 3.333)]);
        let g = dilation_factor(None, CellScale(2));
        assert_eq!(g, 0.75);
        assert!((times.play_seconds(43, &avatar, g).unwrap() - 2.49975).abs() < 1e-4);
        // An undilated row (row 49: the big NPC's mandible eat, f20 = 0) keeps its length.
        let big = SelectContext { avatar: false, big: true, parts: 1 << part_type::MANDIBLE, ..default() };
        let times = timed(&[(49, 4.933)]);
        assert_eq!(times.play_seconds(12, &big, 4.0), Some(4.933));
        // A clip without length answers 1.0 (0xE4F020); an unloaded table answers nothing.
        assert_eq!(timed(&[(72, 0.0)]).play_seconds(43, &avatar, 0.75), Some(0.75));
        assert_eq!(CellClipTimes::default().play_seconds(43, &avatar, g), None);
        assert!(!CellClipTimes::default().is_loaded() && times.is_loaded());
        // realSize ratio: an NPC one level up is 3x the player (G = 1.5), two levels 10x (clamped to 4).
        assert_eq!(dilation_factor(Some(CellScale(2)), CellScale(2)), 0.75);
        assert!((dilation_factor(Some(CellScale(3)), CellScale(2)) - 1.5).abs() < 1e-5);
        assert_eq!(dilation_factor(Some(CellScale(4)), CellScale(2)), 4.0);
        assert_eq!(dilation_factor(Some(CellScale::NONE), CellScale(2)), 0.75);
    }

    #[test]
    fn contact_is_the_clips_eventcontrol_marker_times_the_dilation() {
        let avatar = SelectContext { avatar: true, ..default() };
        // Row 8 = request 5 (bite, avatar): 1.1 s clip, `contact` at 0.13 s.
        let times = CellClipTimes::for_test(&[(8, 1.1, &[(0.0, 1, "cell_eatMandible_eventBite"), (0.13, 2, CONTACT_EVENT)])]);
        assert!((times.contact_seconds(5, &avatar, 0.75).unwrap() - 0.0975).abs() < 1e-5);
        assert!((times.play_seconds(5, &avatar, 0.75).unwrap() - 0.825).abs() < 1e-5);
        // An effect with the same name is not the marker; no marker, no contact.
        let times = CellClipTimes::for_test(&[(8, 1.1, &[(0.13, 1, CONTACT_EVENT)])]);
        assert_eq!(times.contact_seconds(5, &avatar, 0.75), None);
    }

    #[test]
    fn a_request_with_no_row_runs_one_second_then_falls_back() {
        let c = ctx(&[]);
        let nothing = |_: u32| None;
        let mut p = CellAnimPlayback::with_seed(5);
        p.step(code::IDLE, 0.0, 1.0, &c, 1.0, None, &facts);
        p.step(29, 0.0, 1.0, &c, 1.0, Some(0), &nothing);
        assert_eq!((p.code, p.remaining), (Some(29), 1.0));
        let mut finished = false;
        for _ in 0..70 {
            p.step(29, 0.016, 1.0, &c, 1.0, Some(0), &nothing);
            finished |= p.finished;
        }
        assert!(finished, "the request ends after 1 s and falls back to the return code");
    }

    #[test]
    fn dilation_graph_is_clamped_piecewise_linear() {
        assert_eq!(graph(&ANIM_DILATION, 1.0), 0.75);
        assert_eq!(graph(&ANIM_DILATION, 0.1), 0.5);
        assert_eq!(graph(&ANIM_DILATION, 100.0), 4.0);
        assert!((graph(&ANIM_DILATION, 3.0) - 1.5).abs() < 1e-6);
        assert_eq!(blend_times(Some(50), 0), (0.2, 0.7));
        assert_eq!(blend_times(Some(47), 47), (0.0, 0.0));
    }

    fn facts(id: u32) -> Option<ClipFacts> {
        // Synthetic: every clip lasts 1 s; idle/moving loops are cyclic.
        Some((id, 1.0, matches!(id, 0xAAAA0015..=0xAAAA0020)))
    }

    #[test]
    fn one_shots_fall_back_to_the_return_code_and_report_finished() {
        let c = ctx(&[part_type::MANDIBLE]);
        let mut p = CellAnimPlayback::with_seed(7);
        p.step(code::MOVING, 0.0, 1.0, &c, 1.0, None, &facts);
        assert_eq!(p.code, Some(code::MOVING));
        assert_eq!(p.current.as_ref().unwrap().row, 21);
        assert_eq!(p.return_code, code::MOVING);
        // Swallow (mandible) is a one-shot: it plays, then returns to moving.
        p.step(12, 0.016, 1.0, &c, 1.0, None, &facts);
        assert_eq!(p.current.as_ref().unwrap().row, 13);
        assert!(!p.finished);
        let mut steps = 0;
        while !p.finished && steps < 100 {
            p.step(12, 0.016, 1.0, &c, 1.0, None, &facts);
            steps += 1;
        }
        assert!(p.finished && (60..=64).contains(&steps), "{steps}");
        assert_eq!(p.code, Some(code::MOVING), "fell back to the return code");
        // Natively the state machine replays a request that stays set.
        p.step(12, 0.016, 1.0, &c, 1.0, None, &facts);
        assert_eq!(p.current.as_ref().unwrap().row, 13);
        assert!(!p.finished);
        // A looping request never reports finished and re-selects per cycle.
        let mut q = CellAnimPlayback::with_seed(3);
        for _ in 0..200 {
            q.step(code::IDLE, 0.016, 1.0, &ctx(&[part_type::EYE]), 0.75, None, &facts);
        }
        assert!(!q.finished);
        assert!(q.remaining <= 0.75);
    }

    #[test]
    fn blends_rise_and_fall_with_native_times() {
        let mut p = CellAnimPlayback::with_seed(1);
        let c = ctx(&[]);
        p.step(code::IDLE, 0.0, 1.0, &c, 1.0, None, &facts);
        p.step(code::MOVING, 0.1, 1.0, &c, 1.0, None, &facts);
        let (a, b) = p.weights();
        assert!((a - 0.5).abs() < 1e-5 && (b - 0.5).abs() < 1e-5, "{a} {b}");
        p.step(code::MOVING, 0.2, 1.0, &c, 1.0, None, &facts);
        assert_eq!(p.weights(), (1.0, 0.0));
        assert!(p.previous.is_none());
    }

    #[test]
    fn row_layout_inverse_binds_invert_the_column_world() {
        // A bone at (1, 2, 3) turned 90 degrees about Z, stored the D3D way.
        let world = Affine3A::from_rotation_translation(Quat::from_rotation_z(1.0), Vec3::new(1., 2., 3.));
        let inverse = world.inverse();
        let m = inverse.matrix3;
        let pose = rw4::BonePose {
            // Row-vector storage = transpose of the column matrix.
            rotation: [m.col(0).to_array(), m.col(1).to_array(), m.col(2).to_array()],
            inv_translation: inverse.translation.to_array(),
        };
        let back = inverse_bind_from_row_pose(&pose);
        assert!(back.abs_diff_eq(inverse, 1e-5));
        assert!((back * world).abs_diff_eq(Affine3A::IDENTITY, 1e-5));
    }

    fn two_bone_rig() -> PartRig {
        let key = |t: f32, z: f32| rw4::Keyframe {
            time: t,
            rotation: Quat::from_rotation_z(z).to_array(),
            location: [1.0, 0.0, 0.0],
            scale: [1.0; 3],
            factor: 0.0,
            blend: false,
        };
        let anim = rw4::KeyAnim {
            skeleton_id: 1,
            length: 1.0,
            channels: vec![
                rw4::AnimChannel { id: 10, keys: vec![key(0.0, 0.0), key(1.0, 0.0)] },
                rw4::AnimChannel { id: 11, keys: vec![key(0.0, 0.0), key(1.0, 1.0)] },
            ],
        };
        let bind = |x: f32| Affine3A::from_translation(Vec3::new(x, 0.0, 0.0)).inverse();
        PartRig {
            names: vec![10, 11],
            parents: vec![None, Some(0)],
            bind_local: vec![
                Bone3 { translation: Vec3::X, ..Bone3::IDENTITY },
                Bone3 { translation: Vec3::X, ..Bone3::IDENTITY },
            ],
            inverse_bind: vec![bind(1.0), bind(2.0)],
            clips: vec![(0xB37B55B2, RigClip { anim, channel_for_bone: vec![Some(0), Some(1)] })],
        }
    }

    #[test]
    fn deform_phase_samples_the_part_clip_and_skins_relative_to_bind() {
        let rig = two_bone_rig();
        let mut locals = vec![];
        rig.locals(&[], &mut locals);
        let mut skin = vec![];
        rig.skin(&locals, &mut skin);
        assert!(skin.iter().all(|m| m.abs_diff_eq(Affine3A::IDENTITY, 1e-5)));
        let track = Track { id: 0xB37B55B2, value: 0.5, weight: 1.0, cyclic: false };
        rig.locals(&[track], &mut locals);
        assert!((locals[1].rotation.angle_between(Quat::from_rotation_z(0.5))).abs() < 1e-4);
        rig.skin(&locals, &mut skin);
        // The second bone's tip turns about the joint at x = 2.
        let tip = skin[1].transform_point3(Vec3::new(3.0, 0.0, 0.0));
        assert!(tip.abs_diff_eq(Vec3::new(2.0 + 0.5f32.cos(), 0.5f32.sin(), 0.0), 1e-4), "{tip}");
        // Cyclic phases wrap; zero weight keeps the bind pose.
        let wrapped = Track { value: 1.5, cyclic: true, ..track };
        let mut a = vec![];
        rig.locals(&[wrapped], &mut a);
        assert!(a[1].rotation.abs_diff_eq(locals[1].rotation, 1e-5));
        rig.locals(&[Track { weight: 0.0, ..track }], &mut a);
        assert_eq!(a, rig.bind_local);
    }

    #[test]
    fn eye_regions_bind_the_most_specific_channel() {
        let left_front = Region::classify(Vec3::new(0.3, -0.4, 0.0), Vec3::splat(-0.5), Vec3::splat(0.5));
        assert_eq!(left_front, Region { column: 0x04, row: 0x10 });
        assert_eq!(left_front.matches(0x14), Some(2));
        assert_eq!(left_front.matches(0x24), None);
        assert_eq!(left_front.matches(0), Some(0));
        let centre = Region::classify(Vec3::ZERO, Vec3::splat(-0.5), Vec3::splat(0.5));
        assert_eq!(centre, Region { column: 0x0C, row: 0x30 });
        assert_eq!(centre.matches(0x0C), Some(1));
    }

    #[test]
    fn primary_transform_inverse_places_the_rig_on_the_model() {
        // simnu's flagellum binding: 180 degrees about Z, s = 0.7731.
        let mut raw = [0u8; 56];
        let values = [0.0002f32, 0.315, 0.0773, 0.7731, -1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 1.0];
        for (i, v) in values.iter().enumerate() {
            raw[4 + i * 4..8 + i * 4].copy_from_slice(&v.to_le_bytes());
        }
        let to_model = native_transform(raw).unwrap().inverse();
        let origin = to_model.transform_point3(Vec3::ZERO);
        assert!(origin.abs_diff_eq(Vec3::new(0.00026, 0.4075, -0.1), 1e-3), "{origin}");
        // The rig's -Y chain points to +Y on the model (towards the tail).
        let tip = to_model.transform_point3(Vec3::new(0.0, -0.3, 0.0));
        assert!(tip.y > origin.y);
    }

    #[test]
    #[ignore = "reads the installed Spore packages"]
    fn installed_hit_clips_key_the_knockback_channel() {
        // The Cell clips (TLSA groups 0xAAAA....) that key Data1: the hit
        // reactions push the cell away from the attacker (positive values with
        // `cell_motion::hit_impulse`'s sign convention).
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let mut groups = std::collections::BTreeMap::new();
        for key in index.records.keys().filter(|k| k.type_id == crate::tlsa::TYPE_ID) {
            if let Ok(list) = index.read(*key).and_then(|b| crate::tlsa::parse(&b)) {
                for g in list.groups.iter().filter(|g| g.id >> 16 == 0xAAAA) {
                    if let Some(a) = g.choices.first().and_then(|c| c.animations.first()) {
                        groups.insert(g.id, a.id);
                    }
                }
            }
        }
        let mut keyed = 0;
        let mut positive = 0;
        for (group, instance) in &groups {
            let key = ResourceKey { group_id: 0, instance_id: *instance, type_id: crate::decode::TYPE_ANIM };
            let Ok(anim) = index.read(key).and_then(|b| crate::animation::parse(&b)) else {
                continue;
            };
            let clip = CellClip::from_animation(anim);
            let Some(root) = clip.root else {
                continue;
            };
            let (mut lo, mut hi) = (f32::MAX, f32::MIN);
            for k in 0..=40 {
                let t = clip.seconds * k as f32 / 40.0;
                if let Some(s) = clip.sample(root, t) {
                    if let Some(&(_, v, w)) = s.deforms.iter().find(|d| d.0 == DATA1_KNOCKBACK) {
                        if w > 0.0 {
                            lo = lo.min(v);
                            hi = hi.max(v);
                        }
                    }
                }
            }
            if lo <= hi {
                keyed += 1;
                if hi > 0.0 {
                    positive += 1;
                }
                println!("data1 {group:08X} clip {instance:08X} {:.2}s: {lo:.4} .. {hi:.4}", clip.seconds);
            }
        }
        println!("{keyed} cell clips key Data1, {positive} with positive values");
        assert!(keyed > 0, "the hit reactions key the knockback channel");
    }

    /// Data2 / Data3 of the installed Cell clips (the values `cell_motion` reads for
    /// the thrust and the speed factor): printed so a change in the data shows.
    #[test]
    #[ignore = "reads the installed Spore packages"]
    fn installed_move_clips_key_the_thrust_channel() {
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let mut groups = std::collections::BTreeMap::new();
        for key in index.records.keys().filter(|k| k.type_id == crate::tlsa::TYPE_ID) {
            if let Ok(list) = index.read(*key).and_then(|b| crate::tlsa::parse(&b)) {
                for g in list.groups.iter().filter(|g| g.id >> 16 == 0xAAAA) {
                    if let Some(a) = g.choices.first().and_then(|c| c.animations.first()) {
                        groups.insert(g.id, a.id);
                    }
                }
            }
        }
        let mut thrust = 0;
        for (group, instance) in &groups {
            let key = ResourceKey { group_id: 0, instance_id: *instance, type_id: crate::decode::TYPE_ANIM };
            let Ok(anim) = index.read(key).and_then(|b| crate::animation::parse(&b)) else {
                continue;
            };
            let clip = CellClip::from_animation(anim);
            let Some(root) = clip.root else {
                continue;
            };
            for (name, id) in [("data2", DATA2_SPEED), ("data3", DATA3_THRUST)] {
                let (mut lo, mut hi, mut sum, mut n) = (f32::MAX, f32::MIN, 0.0, 0);
                for k in 0..=40 {
                    let t = clip.seconds * k as f32 / 40.0;
                    if let Some(s) = clip.sample(root, t) {
                        if let Some(&(_, v, w)) = s.deforms.iter().find(|d| d.0 == id) {
                            if w > 0.0 {
                                lo = lo.min(v);
                                hi = hi.max(v);
                                sum += v;
                                n += 1;
                            }
                        }
                    }
                }
                if n > 0 {
                    if id == DATA3_THRUST {
                        thrust += 1;
                    }
                    println!("{name} {group:08X} clip {instance:08X} {:.2}s: {lo:.4} .. {hi:.4} mean {:.4}", clip.seconds, sum / n as f32);
                }
            }
        }
        println!("{thrust} cell clips key Data3");
    }

    /// Data4 / Data5 (the camera pull-in weights of 0xE60A12) of the installed
    /// Cell clips: which clips key them and over what range (printed so a change
    /// in the data shows).
    #[test]
    #[ignore = "reads the installed Spore packages"]
    fn installed_clips_key_the_camera_channels() {
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let mut groups = std::collections::BTreeMap::new();
        for key in index.records.keys().filter(|k| k.type_id == crate::tlsa::TYPE_ID) {
            if let Ok(list) = index.read(*key).and_then(|b| crate::tlsa::parse(&b)) {
                for g in list.groups.iter().filter(|g| g.id >> 16 == 0xAAAA) {
                    if let Some(a) = g.choices.first().and_then(|c| c.animations.first()) {
                        groups.insert(g.id, a.id);
                    }
                }
            }
        }
        let mut keyed = 0;
        for (group, instance) in &groups {
            let key = ResourceKey { group_id: 0, instance_id: *instance, type_id: crate::decode::TYPE_ANIM };
            let Ok(anim) = index.read(key).and_then(|b| crate::animation::parse(&b)) else {
                continue;
            };
            let clip = CellClip::from_animation(anim);
            let Some(root) = clip.root else {
                continue;
            };
            for (name, id) in [("data4", DATA4_CAMERA_PULL), ("data5", DATA5_CAMERA_FOCUS)] {
                let mut samples = vec![];
                for k in 0..=10 {
                    let t = clip.seconds * k as f32 / 10.0;
                    let v = clip
                        .sample(root, t)
                        .and_then(|s| s.deforms.iter().find(|d| d.0 == id).map(|&(_, v, w)| (v, w)));
                    samples.push(v);
                }
                if samples.iter().any(|s| s.is_some_and(|(_, w)| w > 0.0)) {
                    keyed += 1;
                    let curve: Vec<String> = samples
                        .iter()
                        .map(|s| s.map_or("-".to_string(), |(v, w)| format!("{v:.2}/{w:.1}")))
                        .collect();
                    println!("{name} {group:08X} clip {instance:08X} {:.2}s: {}", clip.seconds, curve.join(" "));
                }
            }
        }
        println!("{keyed} (clip, channel) pairs key the camera channels");
    }

    /// The retail lengths of the clips the Cell rules time events with, and a
    /// check that each such request answers one length per context (so the
    /// first `Select` candidate is the clip `PlayAnimation` plays).
    #[test]
    #[ignore = "reads the installed Spore packages"]
    fn event_requests_have_one_length_per_context() {
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let mut times = CellClipTimes::default();
        times.load_rows(&groups_of(index_lists(&index)), &index);
        assert!(times.is_loaded());
        let found = times.rows.iter().flatten().count();
        println!("{found} of {} rows resolve to a clip", ROWS.len());
        assert_eq!(found, ROWS.len(), "every row has a clip");
        // (code, what) of every request an event or a lock is timed with.
        let timed_requests: [(i32, &str); 24] = [
            (4, "stab"),
            (5, "bite"),
            (8, "break death"),
            (9, "death eaten"),
            (11, "eat filter"),
            (12, "eat mandible"),
            (13, "eat proboscis"),
            (14, "hit electric"),
            (15, "hit poison"),
            (16, "hit stab"),
            (17, "hit bite"),
            (29, "grow"),
            (41, "go to editor (egg)"),
            (42, "spawn"),
            (43, "hatch"),
            (44, "hatch ice"),
            (45, "go to editor"),
            (46, "leave stage"),
            (49, "part pickup"),
            (65, "mate 1"),
            (66, "mate 2"),
            (67, "mating call"),
            (68, "ending"),
            (31, "bubble pop"),
        ];
        for (request, what) in timed_requests {
            for avatar in [true, false] {
                for (big, epic) in [(false, false), (true, false), (true, true)] {
                    for parts in [0u32, 1 << part_type::FILTER, 1 << part_type::MANDIBLE, 1 << part_type::PROBOSCIS] {
                        let ctx = SelectContext { avatar, parts, big, epic, background: false };
                        let rows = candidates(request, &ctx);
                        let lengths: Vec<f32> = rows
                            .iter()
                            .filter_map(|&r| times.row_clip(r).map(|c| c.seconds))
                            .collect();
                        let first = lengths.first().copied();
                        if lengths.iter().any(|l| (l - first.unwrap()).abs() > 1e-3) {
                            println!("request {request} ({what}) avatar {avatar} big {big} epic {epic} parts {parts:#x}: {lengths:?}");
                        }
                    }
                }
            }
            let mouths = (1 << part_type::MANDIBLE) | (1 << part_type::FILTER) | (1 << part_type::PROBOSCIS);
            let avatar = SelectContext { avatar: true, parts: mouths, ..default() };
            let npc = SelectContext { avatar: false, parts: mouths, ..default() };
            let events = |ctx: &SelectContext| {
                times
                    .pick(request, ctx)
                    .map(|(_, c)| c.events.iter().filter(|e| e.kind != 0 && e.kind != 6).map(|e| format!("{:.2}s {}:{}", e.time, e.kind, e.name)).collect::<Vec<_>>())
                    .unwrap_or_default()
            };
            println!(
                "request {request:2} {what:20} avatar {:.3?} s  npc {:.3?} s  non-sound events avatar {:?} npc {:?}",
                times.pick(request, &avatar).map(|(_, c)| c.seconds),
                times.pick(request, &npc).map(|(_, c)| c.seconds),
                events(&avatar),
                events(&npc)
            );
        }
    }

    #[test]
    #[ignore = "reads the installed Spore packages"]
    fn installed_simnu_rig_binds_and_swims() {
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let catalog = crate::asset_catalog::PartCatalog::from_index(&index);
        let key = ResourceKey { group_id: 0x4061_7100, instance_id: 0x00F5_29D5, type_id: crate::decode::TYPE_GMDL };
        let mut rigs = HashMap::new();
        let rig = ModelRig::build(&index, &catalog, key, &mut rigs).unwrap();
        assert_eq!((rig.joint_count, rig.body_joints, rig.parts.len()), (32, 10, 4));
        assert!(rig.parts_present & (1 << part_type::FLAGELLA) != 0);
        let anim = crate::animation::parse(&index.read(ResourceKey { group_id: 0, instance_id: 0x70D7_A8BE, type_id: crate::decode::TYPE_ANIM }).unwrap()).unwrap();
        let clip = CellClip::from_animation(anim);
        let flagellum = rig.parts.iter().find(|p| p.caps.contains(b"mvcl")).unwrap();
        let channel = bind_channel(&clip, &flagellum.caps, flagellum.region, &flagellum.rig).unwrap();
        let mut a = vec![];
        let mut b = vec![];
        flagellum.rig.locals(&part_tracks(&clip, channel, 0.0), &mut a);
        flagellum.rig.locals(&part_tracks(&clip, channel, 0.4), &mut b);
        let moved = a.iter().zip(&b).any(|(x, y)| x.rotation.angle_between(y.rotation) > 0.05);
        assert!(moved, "the flagellum must lash during cell_mov_flg");
        let pose = BodyPose::sample(&clip, 0.2);
        assert!(pose.root_rotation.angle_between(Quat::IDENTITY) > 0.05, "swim sways the body");
    }
}
