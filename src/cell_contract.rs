//! Shared Cell-stage contract. These plain types are the seams between the Cell
//! engine (rules, AI, world), cell animation, effects, the HUD and the Cell
//! Creator, so those pieces can be built and merged independently.
//!
//! Animation indices and ids come from the retail `CellAnimations` table
//! (Spore ModAPI `Simulator/Cell/CellAnimations.h`): the index is the engine's
//! request slot and the id is the animation resource id looked up for it.
//! Several slots share one id (for example three jet-swim slots), as in retail.

use bevy::prelude::*;

/// One of the retail Cell animation slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum CellAnimation {
    CellIdle = 0,
    CellIdleBlink1 = 1,
    CellIdleBlink2 = 2,
    CellIdleBlink3 = 3,
    CellMovFlg = 4,
    CellMovFlgBlink = 5,
    CellEatFilter = 6,
    CellDmgSolidSpike = 7,
    CellDmgSolidMand = 8,
    CellDeathAvatar = 9,
    CellDeathNpc = 10,
    CellDeathNpcIdle = 11,
    CellEatFilterNpc = 12,
    CellEatMand = 13,
    CellEatMandNpc = 14,
    CellEatProb = 15,
    CellTakeDmgElec = 16,
    CellTakeDmgPoison = 17,
    CellTakeDmgStab = 18,
    CellMoveJet = 19,
    CellMoveJetBlink = 20,
    CellMovFlg2 = 21,
    CellMoveNpcScared = 22,
    CellMoveNpcMad = 23,
    CellMoveNpcScared2 = 24,
    CellMoveNpcMad2 = 25,
    CellMoveJet2 = 26,
    CellMoveJet3 = 27,
    CellNpcScared = 28,
    CellNpcMad = 29,
    CellDeathNpcIdle2 = 30,
    CellEatFilterFail = 31,
    CellEatMandFail = 32,
    CellGlanceMad = 33,
    CellGlanceSpike = 34,
    CellPunctureSpike = 35,
    CellGrow = 36,
    CellDeathAvatarIdle = 37,
    CellElecRelease = 38,
    CellBubblePop = 39,
    CellEatProbStun = 40,
    CellNpcStartled = 41,
    CellPoked = 42,
    CellTakeDmgElecNpc = 43,
    CellTakeDmgPoisonNpc = 44,
    CellTakeDmgStabNpc = 45,
    CellGlanceSpikeOnSpike = 46,
    CellEatProbRetractTrgt = 47,
    CellEatProbRetract = 48,
    CellEatMandNpcBig = 49,
    CellIdleBig = 50,
    CellIdleBlink1Big = 51,
    CellIdleBlink2Big = 52,
    CellIdleBlink3Big = 53,
    CellMovFlgBig = 54,
    CellMovFlgBigBlink = 55,
    CellMovFlgBigMad = 56,
    CellMovCilBig = 57,
    CellMovCilBigBlink = 58,
    CellMovCilBigMad = 59,
    CellMovCilBig2 = 60,
    CellMovCilBigBlink2 = 61,
    CellMovCilBigMad2 = 62,
    CellNpcMadBig = 63,
    CellMovCil = 64,
    CellMovCilBlink = 65,
    CellMovPoisonRelease = 66,
    CellMovCilMad = 67,
    CellMovCilScared = 68,
    CellDmgSolidDeath = 69,
    CellPoisonRelease = 70,
    CellHatchLayEgg = 71,
    CellHatchEgg = 72,
    CellHatchIce = 73,
    CellGoToEditor = 74,
    CellDeathEaten = 75,
    CellDeathEatenNpc = 76,
    CellTakeDmgMand = 77,
    CellPartDiscover = 78,
    CellEatFilterChew = 79,
    CellEatMandChew = 80,
    CellEatProbChew = 81,
    CellDmgSolidMandNpc = 82,
    CellDmgSolidDeathNpc = 83,
    CellTakeDmgMandNpc = 84,
    CellMovToBeach = 85,
    CellGlanceMad2 = 86,
    CellMovFlgBigMadBlink = 87,
    CellGlanceProbOnProb = 88,
    CellMovCil180 = 89,
    CellMovCil180v2 = 90,
    CellMovCil180v3 = 91,
    CellMovCil180v4 = 92,
    CellMovCil180v5 = 93,
    CellMovFlgStart = 94,
    CellMovFlgStart2 = 95,
    CellMovCil2 = 96,
    CellMoveJet4 = 97,
    CellTakeDmgPoisonBig = 98,
    CellMovBankLt = 99,
    CellMovBankRt = 100,
    CellBackgrndGuy = 101,
    CellTakeDmgElecNpcBig = 102,
    EpicCellEyePokeReact = 103,
    EpicCellEyePokeClosedLF = 104,
    EpicCellEyePokeClosedRF = 105,
    EpicCellEyePokeClosedLC = 106,
    EpicCellEyePokeClosedRC = 107,
    EpicCellEyePokeClosedLB = 108,
    EpicCellEyePokeClosedRB = 109,
    EpicCellBiteRock = 110,
    EpicCellElectrocuted = 111,
    EpicCellChokes = 112,
    EpicCellProboscised = 113,
    EpicCellDeathTurnover = 114,
    EpicCellDeathIdle = 115,
    EpicCellEyePokeReactLt = 116,
    EpicCellEyePokeReactRt = 117,
    CellMate01 = 118,
    CellMate02 = 119,
    CellMatingCall = 120,
    CellEatProbRetractTrgtNpc = 121,
    CellEatProbRetractNpc = 122,
    CellEatProbStunNpc = 123,
    CellEatProbNpc = 124,
    CellTakeDmgStabNpcBig = 125,
    CellTakeDmgMandNpcBig = 126,
    EpicCellPoked = 127,
    EpicCellPoked2 = 128,
    CellBrainUpgrade = 129,
    CellHatchLayEggNpc = 130,
}

impl CellAnimation {
    pub const ALL: [CellAnimation; 131] = [
        CellAnimation::CellIdle,
        CellAnimation::CellIdleBlink1,
        CellAnimation::CellIdleBlink2,
        CellAnimation::CellIdleBlink3,
        CellAnimation::CellMovFlg,
        CellAnimation::CellMovFlgBlink,
        CellAnimation::CellEatFilter,
        CellAnimation::CellDmgSolidSpike,
        CellAnimation::CellDmgSolidMand,
        CellAnimation::CellDeathAvatar,
        CellAnimation::CellDeathNpc,
        CellAnimation::CellDeathNpcIdle,
        CellAnimation::CellEatFilterNpc,
        CellAnimation::CellEatMand,
        CellAnimation::CellEatMandNpc,
        CellAnimation::CellEatProb,
        CellAnimation::CellTakeDmgElec,
        CellAnimation::CellTakeDmgPoison,
        CellAnimation::CellTakeDmgStab,
        CellAnimation::CellMoveJet,
        CellAnimation::CellMoveJetBlink,
        CellAnimation::CellMovFlg2,
        CellAnimation::CellMoveNpcScared,
        CellAnimation::CellMoveNpcMad,
        CellAnimation::CellMoveNpcScared2,
        CellAnimation::CellMoveNpcMad2,
        CellAnimation::CellMoveJet2,
        CellAnimation::CellMoveJet3,
        CellAnimation::CellNpcScared,
        CellAnimation::CellNpcMad,
        CellAnimation::CellDeathNpcIdle2,
        CellAnimation::CellEatFilterFail,
        CellAnimation::CellEatMandFail,
        CellAnimation::CellGlanceMad,
        CellAnimation::CellGlanceSpike,
        CellAnimation::CellPunctureSpike,
        CellAnimation::CellGrow,
        CellAnimation::CellDeathAvatarIdle,
        CellAnimation::CellElecRelease,
        CellAnimation::CellBubblePop,
        CellAnimation::CellEatProbStun,
        CellAnimation::CellNpcStartled,
        CellAnimation::CellPoked,
        CellAnimation::CellTakeDmgElecNpc,
        CellAnimation::CellTakeDmgPoisonNpc,
        CellAnimation::CellTakeDmgStabNpc,
        CellAnimation::CellGlanceSpikeOnSpike,
        CellAnimation::CellEatProbRetractTrgt,
        CellAnimation::CellEatProbRetract,
        CellAnimation::CellEatMandNpcBig,
        CellAnimation::CellIdleBig,
        CellAnimation::CellIdleBlink1Big,
        CellAnimation::CellIdleBlink2Big,
        CellAnimation::CellIdleBlink3Big,
        CellAnimation::CellMovFlgBig,
        CellAnimation::CellMovFlgBigBlink,
        CellAnimation::CellMovFlgBigMad,
        CellAnimation::CellMovCilBig,
        CellAnimation::CellMovCilBigBlink,
        CellAnimation::CellMovCilBigMad,
        CellAnimation::CellMovCilBig2,
        CellAnimation::CellMovCilBigBlink2,
        CellAnimation::CellMovCilBigMad2,
        CellAnimation::CellNpcMadBig,
        CellAnimation::CellMovCil,
        CellAnimation::CellMovCilBlink,
        CellAnimation::CellMovPoisonRelease,
        CellAnimation::CellMovCilMad,
        CellAnimation::CellMovCilScared,
        CellAnimation::CellDmgSolidDeath,
        CellAnimation::CellPoisonRelease,
        CellAnimation::CellHatchLayEgg,
        CellAnimation::CellHatchEgg,
        CellAnimation::CellHatchIce,
        CellAnimation::CellGoToEditor,
        CellAnimation::CellDeathEaten,
        CellAnimation::CellDeathEatenNpc,
        CellAnimation::CellTakeDmgMand,
        CellAnimation::CellPartDiscover,
        CellAnimation::CellEatFilterChew,
        CellAnimation::CellEatMandChew,
        CellAnimation::CellEatProbChew,
        CellAnimation::CellDmgSolidMandNpc,
        CellAnimation::CellDmgSolidDeathNpc,
        CellAnimation::CellTakeDmgMandNpc,
        CellAnimation::CellMovToBeach,
        CellAnimation::CellGlanceMad2,
        CellAnimation::CellMovFlgBigMadBlink,
        CellAnimation::CellGlanceProbOnProb,
        CellAnimation::CellMovCil180,
        CellAnimation::CellMovCil180v2,
        CellAnimation::CellMovCil180v3,
        CellAnimation::CellMovCil180v4,
        CellAnimation::CellMovCil180v5,
        CellAnimation::CellMovFlgStart,
        CellAnimation::CellMovFlgStart2,
        CellAnimation::CellMovCil2,
        CellAnimation::CellMoveJet4,
        CellAnimation::CellTakeDmgPoisonBig,
        CellAnimation::CellMovBankLt,
        CellAnimation::CellMovBankRt,
        CellAnimation::CellBackgrndGuy,
        CellAnimation::CellTakeDmgElecNpcBig,
        CellAnimation::EpicCellEyePokeReact,
        CellAnimation::EpicCellEyePokeClosedLF,
        CellAnimation::EpicCellEyePokeClosedRF,
        CellAnimation::EpicCellEyePokeClosedLC,
        CellAnimation::EpicCellEyePokeClosedRC,
        CellAnimation::EpicCellEyePokeClosedLB,
        CellAnimation::EpicCellEyePokeClosedRB,
        CellAnimation::EpicCellBiteRock,
        CellAnimation::EpicCellElectrocuted,
        CellAnimation::EpicCellChokes,
        CellAnimation::EpicCellProboscised,
        CellAnimation::EpicCellDeathTurnover,
        CellAnimation::EpicCellDeathIdle,
        CellAnimation::EpicCellEyePokeReactLt,
        CellAnimation::EpicCellEyePokeReactRt,
        CellAnimation::CellMate01,
        CellAnimation::CellMate02,
        CellAnimation::CellMatingCall,
        CellAnimation::CellEatProbRetractTrgtNpc,
        CellAnimation::CellEatProbRetractNpc,
        CellAnimation::CellEatProbStunNpc,
        CellAnimation::CellEatProbNpc,
        CellAnimation::CellTakeDmgStabNpcBig,
        CellAnimation::CellTakeDmgMandNpcBig,
        CellAnimation::EpicCellPoked,
        CellAnimation::EpicCellPoked2,
        CellAnimation::CellBrainUpgrade,
        CellAnimation::CellHatchLayEggNpc,
    ];

    /// The retail animation slot index.
    pub fn index(self) -> usize {
        self as usize
    }

    /// The animation resource id the retail engine requests for this slot.
    pub fn animation_id(self) -> u32 {
        match self {
            CellAnimation::CellIdle => 0xAAAA0015,
            CellAnimation::CellIdleBlink1 => 0xAAAA0016,
            CellAnimation::CellIdleBlink2 => 0xAAAA0017,
            CellAnimation::CellIdleBlink3 => 0xAAAA0018,
            CellAnimation::CellMovFlg => 0xAAAA0019,
            CellAnimation::CellMovFlgBlink => 0xAAAA0020,
            CellAnimation::CellEatFilter => 0xAAAA0005,
            CellAnimation::CellDmgSolidSpike => 0xAAAA0000,
            CellAnimation::CellDmgSolidMand => 0xAAAA0001,
            CellAnimation::CellDeathAvatar => 0xAAAA0002,
            CellAnimation::CellDeathNpc => 0xAAAA0003,
            CellAnimation::CellDeathNpcIdle => 0xAAAA0004,
            CellAnimation::CellEatFilterNpc => 0xAAAA0006,
            CellAnimation::CellEatMand => 0xAAAA0007,
            CellAnimation::CellEatMandNpc => 0xAAAA0008,
            CellAnimation::CellEatProb => 0xAAAA0009,
            CellAnimation::CellTakeDmgElec => 0xAAAA0011,
            CellAnimation::CellTakeDmgPoison => 0xAAAA0012,
            CellAnimation::CellTakeDmgStab => 0xAAAA0013,
            CellAnimation::CellMoveJet => 0xAAAA0021,
            CellAnimation::CellMoveJetBlink => 0xAAAA0022,
            CellAnimation::CellMovFlg2 => 0xAAAA0019,
            CellAnimation::CellMoveNpcScared => 0xAAAA0023,
            CellAnimation::CellMoveNpcMad => 0xAAAA0024,
            CellAnimation::CellMoveNpcScared2 => 0xAAAA0023,
            CellAnimation::CellMoveNpcMad2 => 0xAAAA0024,
            CellAnimation::CellMoveJet2 => 0xAAAA0021,
            CellAnimation::CellMoveJet3 => 0xAAAA0021,
            CellAnimation::CellNpcScared => 0xAAAA0025,
            CellAnimation::CellNpcMad => 0xAAAA0026,
            CellAnimation::CellDeathNpcIdle2 => 0xAAAA0027,
            CellAnimation::CellEatFilterFail => 0xAAAA0028,
            CellAnimation::CellEatMandFail => 0xAAAA0029,
            CellAnimation::CellGlanceMad => 0xAAAA0030,
            CellAnimation::CellGlanceSpike => 0xAAAA0031,
            CellAnimation::CellPunctureSpike => 0xAAAA0032,
            CellAnimation::CellGrow => 0xAAAA0033,
            CellAnimation::CellDeathAvatarIdle => 0xAAAA0036,
            CellAnimation::CellElecRelease => 0xAAAA0037,
            CellAnimation::CellBubblePop => 0xAAAA0038,
            CellAnimation::CellEatProbStun => 0xAAAA0039,
            CellAnimation::CellNpcStartled => 0xAAAA0040,
            CellAnimation::CellPoked => 0xAAAA0041,
            CellAnimation::CellTakeDmgElecNpc => 0xAAAA0042,
            CellAnimation::CellTakeDmgPoisonNpc => 0xAAAA0043,
            CellAnimation::CellTakeDmgStabNpc => 0xAAAA0044,
            CellAnimation::CellGlanceSpikeOnSpike => 0xAAAA0045,
            CellAnimation::CellEatProbRetractTrgt => 0xAAAA0046,
            CellAnimation::CellEatProbRetract => 0xAAAA0047,
            CellAnimation::CellEatMandNpcBig => 0xAAAA0048,
            CellAnimation::CellIdleBig => 0xAAAA0049,
            CellAnimation::CellIdleBlink1Big => 0xAAAA0050,
            CellAnimation::CellIdleBlink2Big => 0xAAAA0051,
            CellAnimation::CellIdleBlink3Big => 0xAAAA0052,
            CellAnimation::CellMovFlgBig => 0xAAAA0053,
            CellAnimation::CellMovFlgBigBlink => 0xAAAA0054,
            CellAnimation::CellMovFlgBigMad => 0xAAAA0055,
            CellAnimation::CellMovCilBig => 0xAAAA0056,
            CellAnimation::CellMovCilBigBlink => 0xAAAA0057,
            CellAnimation::CellMovCilBigMad => 0xAAAA0058,
            CellAnimation::CellMovCilBig2 => 0xAAAA0056,
            CellAnimation::CellMovCilBigBlink2 => 0xAAAA0057,
            CellAnimation::CellMovCilBigMad2 => 0xAAAA0058,
            CellAnimation::CellNpcMadBig => 0xAAAA0059,
            CellAnimation::CellMovCil => 0xAAAA0060,
            CellAnimation::CellMovCilBlink => 0xAAAA0061,
            CellAnimation::CellMovPoisonRelease => 0xAAAA0062,
            CellAnimation::CellMovCilMad => 0xAAAA0063,
            CellAnimation::CellMovCilScared => 0xAAAA0064,
            CellAnimation::CellDmgSolidDeath => 0xAAAA0065,
            CellAnimation::CellPoisonRelease => 0xAAAA0066,
            CellAnimation::CellHatchLayEgg => 0xAAAA0067,
            CellAnimation::CellHatchEgg => 0xAAAA0068,
            CellAnimation::CellHatchIce => 0xAAAA0069,
            CellAnimation::CellGoToEditor => 0xAAAA0070,
            CellAnimation::CellDeathEaten => 0xAAAA0071,
            CellAnimation::CellDeathEatenNpc => 0xAAAA0072,
            CellAnimation::CellTakeDmgMand => 0xAAAA0073,
            CellAnimation::CellPartDiscover => 0xAAAA0074,
            CellAnimation::CellEatFilterChew => 0xAAAA0075,
            CellAnimation::CellEatMandChew => 0xAAAA0076,
            CellAnimation::CellEatProbChew => 0xAAAA0077,
            CellAnimation::CellDmgSolidMandNpc => 0xAAAA0078,
            CellAnimation::CellDmgSolidDeathNpc => 0xAAAA0079,
            CellAnimation::CellTakeDmgMandNpc => 0xAAAA0080,
            CellAnimation::CellMovToBeach => 0xAAAA0081,
            CellAnimation::CellGlanceMad2 => 0xAAAA0082,
            CellAnimation::CellMovFlgBigMadBlink => 0xAAAA0083,
            CellAnimation::CellGlanceProbOnProb => 0xAAAA0084,
            CellAnimation::CellMovCil180 => 0xAAAA0085,
            CellAnimation::CellMovCil180v2 => 0xAAAA0086,
            CellAnimation::CellMovCil180v3 => 0xAAAA0087,
            CellAnimation::CellMovCil180v4 => 0xAAAA0088,
            CellAnimation::CellMovCil180v5 => 0xAAAA0089,
            CellAnimation::CellMovFlgStart => 0xAAAA0090,
            CellAnimation::CellMovFlgStart2 => 0xAAAA0090,
            CellAnimation::CellMovCil2 => 0xAAAA0091,
            CellAnimation::CellMoveJet4 => 0xAAAA0092,
            CellAnimation::CellTakeDmgPoisonBig => 0xAAAA0093,
            CellAnimation::CellMovBankLt => 0xAAAA0095,
            CellAnimation::CellMovBankRt => 0xAAAA0096,
            CellAnimation::CellBackgrndGuy => 0xAAAA0097,
            CellAnimation::CellTakeDmgElecNpcBig => 0xAAAA0098,
            CellAnimation::EpicCellEyePokeReact => 0xAAAA0099,
            CellAnimation::EpicCellEyePokeClosedLF => 0xAAAA0100,
            CellAnimation::EpicCellEyePokeClosedRF => 0xAAAA0101,
            CellAnimation::EpicCellEyePokeClosedLC => 0xAAAA0102,
            CellAnimation::EpicCellEyePokeClosedRC => 0xAAAA0103,
            CellAnimation::EpicCellEyePokeClosedLB => 0xAAAA0104,
            CellAnimation::EpicCellEyePokeClosedRB => 0xAAAA0105,
            CellAnimation::EpicCellBiteRock => 0xAAAA0106,
            CellAnimation::EpicCellElectrocuted => 0xAAAA0107,
            CellAnimation::EpicCellChokes => 0xAAAA0108,
            CellAnimation::EpicCellProboscised => 0xAAAA0109,
            CellAnimation::EpicCellDeathTurnover => 0xAAAA0110,
            CellAnimation::EpicCellDeathIdle => 0xAAAA0111,
            CellAnimation::EpicCellEyePokeReactLt => 0xAAAA0112,
            CellAnimation::EpicCellEyePokeReactRt => 0xAAAA0113,
            CellAnimation::CellMate01 => 0xAAAA0114,
            CellAnimation::CellMate02 => 0xAAAA0115,
            CellAnimation::CellMatingCall => 0xAAAA0116,
            CellAnimation::CellEatProbRetractTrgtNpc => 0xAAAA0117,
            CellAnimation::CellEatProbRetractNpc => 0xAAAA0118,
            CellAnimation::CellEatProbStunNpc => 0xAAAA0119,
            CellAnimation::CellEatProbNpc => 0xAAAA0120,
            CellAnimation::CellTakeDmgStabNpcBig => 0xAAAA0121,
            CellAnimation::CellTakeDmgMandNpcBig => 0xAAAA0122,
            CellAnimation::EpicCellPoked => 0xAAAA0123,
            CellAnimation::EpicCellPoked2 => 0xAAAA0123,
            CellAnimation::CellBrainUpgrade => 0xAAAA0124,
            CellAnimation::CellHatchLayEggNpc => 0xAAAA0125,
        }
    }
}

/// What a cell object is currently animating. The engine writes `requested`
/// whenever its rules change a cell's state (as retail `PlayAnimation` does);
/// the animation system plays it on the cell's rig and reports completion.
#[derive(Component, Clone, Copy, Debug)]
pub struct CellAnimState {
    pub requested: CellAnimation,
    /// Seconds since `requested` last changed.
    pub elapsed: f32,
    /// Playback speed multiplier (swim animations follow swim speed).
    pub speed: f32,
    /// Set by the animation system when a non-looping clip has finished.
    pub finished: bool,
    /// Retail request code 0..=68 written by the engine exactly as the
    /// original does; the animation system maps it to a table row and plays it.
    pub request_code: i32,
}

impl Default for CellAnimState {
    fn default() -> Self {
        Self {
            requested: CellAnimation::CellIdle,
            elapsed: 0.0,
            speed: 1.0,
            finished: false,
            request_code: 0,
        }
    }
}

impl CellAnimState {
    pub fn request(&mut self, animation: CellAnimation) {
        if self.requested != animation {
            self.requested = animation;
            self.elapsed = 0.0;
            self.finished = false;
        }
    }
}

/// Request to play an installed visual effect (a Swarm effect resource id) at a
/// place in the Cell world, optionally following an entity.
#[derive(Message, Clone, Debug)]
pub struct CellEffectRequest {
    /// Effect resource instance id (retail `effectID`).
    pub effect: u32,
    /// World position when `attach_to` is `None` (and the local offset otherwise).
    pub position: Vec3,
    /// Entity whose transform the effect follows, if any.
    pub attach_to: Option<Entity>,
    /// Uniform scale (cell radius, usually).
    pub scale: f32,
}

/// Marks the stand-in mesh the engine gives a cell whose retail art is only an
/// effect attachment (rocks); presentation hides it once the effect draws.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct CellFallbackVisual;

/// Where the camera and the water currently are, for presentation systems
/// (backgrounds, effects, animation). Filled by the engine every frame.
#[derive(Resource, Clone, Debug, Default)]
pub struct CellWorldState {
    /// Retail `CellStageScale` index of the player's current size level.
    pub scale: i32,
    /// Camera height above the swimming plane, world units.
    pub camera_height: f32,
    /// Player position on the swimming plane (x, z).
    pub player: Vec2,
    /// Water flow of the current world/level (retail advect entry).
    pub flow_strength: f32,
    pub flow_variance: f32,
    pub flow_period: f32,
    /// Seconds of play since the stage started (for deterministic effects).
    pub time: f32,
    /// Global world scale (retail `cCellGame+0x514C`): the player keeps a
    /// constant world size and the world is rescaled by 1/r when this grows
    /// by r. Smoothed toward the player's `cCellScaleInfo` size (1 .. 52000).
    pub world_scale: f32,
    /// `mFoodProgression` (0..1000; 1000 completes the stage).
    pub food_progression: i32,
    /// Biome pair 0..=4: ice, rock, plant, water, beach.
    pub biome: usize,
}

/// Everything the Cell HUD displays, filled by the engine each frame so the
/// HUD never reads simulation internals.
#[derive(Resource, Clone, Debug, Default)]
pub struct CellHudState {
    /// Spendable DNA ("evolution points") shown beside the progress bar.
    pub dna_points: i32,
    /// DNA earned since the last creator visit (retail `0xE51410(food)` minus
    /// the cached `serializable.field_30`, set on the editor return): the mate
    /// reminder bubble needs more than 14 of it (RE 8).
    pub dna_since_creator: i32,
    /// Progress through the current size level, `0.0..=1.0`.
    pub level_progress: f32,
    /// Index of the current size level and the number of levels in the stage.
    pub level: usize,
    pub level_count: usize,
    /// Player health points out of `max_health` (retail maximum is 6).
    pub health: i32,
    pub max_health: i32,
    /// The Mating Call button is shown once a first part has been collected.
    pub show_mate_button: bool,
    /// Mission/goal cards in display order: (title key or text, body text).
    pub goals: Vec<(String, String)>,
    /// Rollover for the cell under the cursor: name, health, part icon flag.
    pub rollover: Option<CellRollover>,
    /// Every cell the rollover pool should know about this frame: the cell
    /// under the cursor (`CellRollover::hovered`), the cells whose health just
    /// changed (`CellRollover::trigger`, the `ShowHealthRollover` call sites
    /// `0xE7A7C0` / `0xE7D070`) and the ones watched for a few seconds after
    /// that so their records follow them.
    pub rollovers: Vec<CellRollover>,
    /// Current zoom factor of the camera (HUD zoom slider position).
    pub zoom: f32,
    pub paused: bool,
    /// Progress through the whole stage, `0.0..=1.0` (food / 1000).
    pub stage_progress: f32,
    /// Biome pair 0..=4 (ice, rock, plant, water, beach); the bar's knobs sit
    /// between biomes (`cCellScaleInfo` +0x18 = 0, .25, .5, .75, 1).
    pub biome: usize,
    pub food_progression: i32,
    /// Structured goal cards (the `goals` pairs are derived from these).
    pub goal_cards: Vec<CellGoalCard>,
    /// Callouts to show, front first (retail callout names such as
    /// `CAllout_CLG_Opening`, `CAllout_CLG_ProgressBar`, `Callout_CLG_Ending`).
    /// The game is paused while any is shown; Enter (or a click on it)
    /// dismisses the front one.
    pub callouts: Vec<String>,
    /// Active first-minute hint (`clg-move`, `clg-carnivore`, `clg-herbivore`).
    pub hint: Option<String>,
    /// The end-of-stage summary window (HUD hidden, paused); Enter or its
    /// button closes it.
    pub summary: Option<CellSummary>,
    /// Cinematic letterbox bars (leaving the stage).
    pub letterbox: bool,
    /// Hide the whole HUD (cinematics, summary).
    pub hud_hidden: bool,
    /// The Advance button (0x06455C5E) and its glow (0x0652A628) are shown and
    /// History (0x044827B8) hidden; clicking Advance presses F4.
    pub advance_visible: bool,
}

/// Stats shown by the end-of-stage summary window.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CellSummary {
    pub game_time_seconds: f32,
    pub kills: i32,
    pub deaths: i32,
    /// Diet class: `clg_plant`, `clg_meat` or `clg_mix`.
    pub diet: String,
    /// DNA handed to the Creature stage (earned - spent + 50).
    pub dna: i32,
}

/// One retail goal card ("Collect 1/6 — Unlock Cell Parts: ...").
#[derive(Clone, Debug, Default)]
pub struct CellGoalCard {
    /// Card type label ("Collect", "Mate").
    pub kind: String,
    /// Progress counter shown on the card (`1/6`), if any.
    pub progress: Option<(i32, i32)>,
    /// Title and body, read from the installed locale tables.
    pub title: String,
    pub body: String,
}

#[derive(Clone, Debug)]
pub struct CellRollover {
    pub entity: Entity,
    pub name: String,
    pub health: i32,
    pub max_health: i32,
    /// The cell carries a part the player can win (retail shows a part icon).
    pub carries_part: bool,
    /// Unlock slots (`mUnlockedParts` index) of the still locked parts the
    /// cell carries; the retail plate shows the first two as icons
    /// (`0xE57EC0`).
    pub part_slots: Vec<usize>,
    /// `GetScaleDifferenceWithPlayer == 4` (far larger than the player): the
    /// health bar is the silver one (`0xE57FD0`).
    pub giant: bool,
    /// Screen-space anchor in logical pixels.
    pub screen: Vec2,
    /// The cursor rests on this cell.
    pub hovered: bool,
    /// The cell's health just changed (damage the avatar dealt or took, a
    /// heal of the avatar): its health before the change, which is what
    /// `ShowHealthRollover(cell, initialHealth)` receives (`0xE62340`).
    pub trigger: Option<i32>,
    /// The avatar itself: its record shows no name (`0xE62200` skips the
    /// name for the avatar cell).
    pub avatar: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animation_table_matches_the_retail_slot_order() {
        assert_eq!(CellAnimation::ALL.len(), 131);
        for (slot, animation) in CellAnimation::ALL.iter().enumerate() {
            assert_eq!(animation.index(), slot, "slots are contiguous from zero");
        }
        assert_eq!(CellAnimation::CellIdle.animation_id(), 0xAAAA0015);
        assert_eq!(CellAnimation::CellMatingCall.animation_id(), 0xAAAA0116);
        assert_eq!(CellAnimation::CellHatchLayEggNpc.index(), 130);
    }

    #[test]
    fn requesting_the_same_animation_keeps_its_clock() {
        let mut state = CellAnimState::default();
        state.elapsed = 1.5;
        state.request(CellAnimation::CellIdle);
        assert_eq!(state.elapsed, 1.5);
        state.request(CellAnimation::CellMovFlg);
        assert_eq!(state.elapsed, 0.0);
    }
}
