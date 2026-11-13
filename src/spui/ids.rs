//! Class proxy ids, property ids and flag bits of the SPUI format that the
//! typed model interprets. Values come from the SporeModder-FX designer
//! definitions (see [`super::schema`], which a unit test cross-checks).
//!
//! Property ids are only unique per class: a button's `ButtonFlags` and an
//! animated icon's `Direction` share one id, so the constants are grouped by
//! the class that owns them.

/// Class proxy ids.
pub mod class {
    // Windows.
    pub const WINDOW: u32 = 0x4EC1_B8D8;
    pub const BUTTON: u32 = 0xEEEF_E2C3;
    pub const TEXT: u32 = 0x6F15_F51B;
    /// "Outlined Text" / `cSPUITextWin`: a text window with outline support.
    pub const OUTLINED_TEXT: u32 = 0x039A_721C;
    pub const COMBO_BOX: u32 = 0xAF55_2C4B;
    pub const SCROLLBAR: u32 = 0x8EF3_7D6D;
    pub const SLIDER: u32 = 0x2F00_BDB3;
    pub const SPINNER: u32 = 0x4F06_3BB3;
    pub const TEXT_EDIT: u32 = 0x6F42_B4F0;
    pub const DIALOG: u32 = 0x0F0B_8B73;
    pub const MESSAGE_BOX: u32 = 0x6F38_282B;
    pub const GRID: u32 = 0x6F1F_1AA6;
    pub const TREE_VIEW: u32 = 0xF02A_FF4B;
    pub const ANIMATED_ICON: u32 = 0x0106_F14B;
    pub const PROGRESS_BAR: u32 = 0x010E_DE03;
    pub const GRADIENT_PROGRESS_BAR: u32 = 0x015C_BBD6;
    pub const POPUP_MENU_ITEM: u32 = 0x04C0_4B4E;
    pub const POPUP_MENU: u32 = 0x04C0_4B4D;
    pub const WATCH_WINDOW: u32 = 0x6FB3_39C9;
    pub const WATCH_GRAPH: u32 = 0x0295_571A;
    pub const CONSOLE_WINDOW: u32 = 0xEFBC_56D8;
    pub const XHTML: u32 = 0x04D0_4553;

    // Layout styles.
    pub const SIMPLE_LAYOUT: u32 = 0x2F41_8D72;
    pub const PROPORTIONAL_LAYOUT: u32 = 0xAF3D_F411;
    pub const ZOOM_LAYOUT: u32 = 0x04EA_92A3;

    // Effects and other window procedures.
    pub const GLIDE: u32 = 0xCF2B_2AD4;
    pub const FADE: u32 = 0x6F2B_6D2C;
    pub const INFLATE: u32 = 0x4F3D_AF02;
    pub const MODULATE: u32 = 0x2F8B_BB4D;
    pub const CASCADE: u32 = 0x0F90_A5AE;
    pub const ROTATE: u32 = 0xCF2B_2AD5;
    pub const PERSPECTIVE: u32 = 0xCF2B_2AD6;
    pub const SPORE_ROTATE: u32 = 0x0208_6743;
    pub const SWARM_EFFECT: u32 = 0x0238_EA36;
    pub const MATERIAL_EFFECT: u32 = 0x02C2_00CE;
    pub const USE_MATERIAL: u32 = 0x02BC_FC0E;
    pub const LAYER_ID: u32 = 0x02BC_FC18;
    pub const SOUND_EFFECT: u32 = 0x085A_147E;
    pub const TOOLTIP: u32 = 0x0372_E920;
    pub const RELATIVE_NAVIGATOR: u32 = 0xAF87_72AF;
    pub const EXPLICIT_NAVIGATOR: u32 = 0x4F87_72AA;

    // Event system.
    pub const WINDOW_STATE_EVENT: u32 = 0x0256_11ED;
    pub const WINDOW_ANIMATION_ACTION: u32 = 0x0254_CB67;
    pub const WINDOW_STATE_ACTION: u32 = 0x033F_7AD6;
    pub const WINDOW_STATE_PREDICATE: u32 = 0x033C_9C5B;
    pub const WINDOW_COLOR: u32 = 0x0254_AA82;
    pub const WINDOW_SCALE: u32 = 0x0269_EFE5;
    pub const TIME_RAMP: u32 = 0x0253_ACD2;
    pub const TIME_SMOOTH_RAMP: u32 = 0x0253_ADD9;
    pub const TIME_OSCILLATOR: u32 = 0x0269_A661;

    // Drawables and images.
    pub const STD_DRAWABLE: u32 = 0xB03C_196F;
    pub const FRAME_DRAWABLE: u32 = 0x030D_54B9;
    pub const BUTTON_DRAWABLE_STANDARD: u32 = 0xAF3A_9E26;
    pub const BUTTON_DRAWABLE_RADIO: u32 = 0x2F3A_DC5D;
    pub const COMBO_BOX_DRAWABLE: u32 = 0x2F55_2CCE;
    pub const SCROLLBAR_DRAWABLE: u32 = 0x0F03_4637;
    pub const SLIDER_DRAWABLE: u32 = 0xEF03_4604;
    pub const SPINNER_DRAWABLE: u32 = 0xEF06_3C4A;
    pub const DIALOG_DRAWABLE: u32 = 0x6F0C_7AEA;
    pub const IMAGE_DRAWABLE: u32 = 0xCF3C_4EAE;
    pub const TREE_EXPANDER_DRAWABLE: u32 = 0xF02C_7C44;
    pub const SPORE_STD_DRAWABLE: u32 = 0x053E_B32A;
    pub const SPORE_STD_IMAGE_INFO: u32 = 0x0540_037E;
    pub const VARIABLE_WIDTH_DRAWABLE: u32 = 0x0109_C69D;
    pub const ATLAS_IMAGE: u32 = 0x01BE_6B15;
}

/// Window flag bits (`WindowFlags`, property `0xEEC1B000`).
pub mod flag {
    pub const VISIBLE: u32 = 0x1;
    pub const ENABLED: u32 = 0x2;
    pub const CLICK_TO_FRONT: u32 = 0x4;
    /// Buttons: draw the caption.
    pub const SHOW_TEXT: u32 = 0x8;
    pub const IGNORE_MOUSE: u32 = 0x10;
    pub const ALWAYS_IN_FRONT: u32 = 0x40;
    pub const CLIP: u32 = 0x400;
    pub const IGNORE_MOUSE_CHILDREN: u32 = 0x1000;
}

/// Window state bits (`GetState`).
pub mod state {
    pub const CLICKED: u32 = 2;
    pub const SELECTED: u32 = 4;
    pub const HOVER: u32 = 8;
}

/// Button flag bits (`ButtonFlags`).
pub mod button_flag {
    pub const TRIGGER_ON_DOWN: u32 = 1;
    pub const FIXED_WIDTH: u32 = 2;
    pub const FIXED_HEIGHT: u32 = 4;
}

/// Properties shared by every window class.
pub mod win {
    pub const FLAGS: u32 = 0xEEC1_B000;
    pub const CONTROL_ID: u32 = 0xEEC1_B001;
    pub const COMMAND_ID: u32 = 0xEEC1_B002;
    pub const STATE_FLAGS: u32 = 0xEEC1_B003;
    pub const SHADE_COLOR: u32 = 0xEEC1_B004;
    pub const AREA: u32 = 0xEEC1_B005;
    pub const FILL_COLOR: u32 = 0xEEC1_B006;
    /// The class specific drawable (fill, button, slider, ... drawable).
    pub const DRAWABLE: u32 = 0xEEC1_B007;
    pub const CAPTION: u32 = 0xEEC1_B00A;
    pub const CHILDREN: u32 = 0xEEC1_B00B;
    pub const PROCS: u32 = 0xEEC1_B00C;
    pub const TEXT_FONT: u32 = 0xEEC1_B00E;
}

pub mod button {
    pub const STATE_FLAGS: u32 = 0xEEC1_D001;
    pub const FLAGS: u32 = 0xEEC1_D002;
    pub const TYPE: u32 = 0xEEC1_D003;
    pub const ALIGNMENT: u32 = 0xEEC1_D004;
    pub const GROUP_ID: u32 = 0xEEC1_D005;
    pub const CAPTION_BORDER: u32 = 0xEEC1_D006;
    pub const CAPTION_OFFSET: u32 = 0xEEC1_D00A;
    pub const CAPTION_COLORS: u32 = 0xEEC1_D00B;
}

pub mod text {
    pub const BORDER: u32 = 0x6F43_FCCE;
    pub const COLOR: u32 = 0x6F43_FCCD;
}

/// `Borders` structure members.
pub mod borders {
    pub const LEFT: u32 = 1;
    pub const TOP: u32 = 2;
    pub const RIGHT: u32 = 3;
    pub const BOTTOM: u32 = 4;
}

pub mod anim_icon {
    pub const DURATION: u32 = 0xEEC1_D000;
    pub const END_MODE: u32 = 0xEEC1_D001;
    pub const DIRECTION: u32 = 0xEEC1_D002;
    pub const IMAGE: u32 = 0xEEC1_D003;
    pub const LOOP_COUNT: u32 = 0xEEC1_D004;
    pub const AUTO_PLAY: u32 = 0xEEC1_D005;
    pub const FRAME_WIDTH: u32 = 0xEEC1_D006;
    pub const FRAME_COUNT: u32 = 0xEEC1_D007;
    pub const ROWS: u32 = 0xEEC1_D008;
    pub const COLUMNS: u32 = 0xEEC1_D009;
}

pub mod slider {
    pub const VALUE: u32 = 0xEEC1_E001;
    pub const MIN: u32 = 0xEEC1_E002;
    pub const MAX: u32 = 0xEEC1_E003;
    pub const ORIENTATION: u32 = 0xEEC1_E004;
}

pub mod scrollbar {
    pub const VALUE: u32 = 0xEEC1_C001;
    pub const MIN: u32 = 0xEEC1_C002;
    pub const MAX: u32 = 0xEEC1_C003;
    pub const VISIBLE_RANGE: u32 = 0xEEC1_C004;
    pub const ORIENTATION: u32 = 0xEEC1_C007;
}

pub mod layout {
    /// `SimpleLayout.Anchor` and `ProportionalLayout.Proportions` share an id.
    pub const PARAM: u32 = 0x0F3D_0000;
    pub const ANCHOR_TOP: u32 = 1;
    pub const ANCHOR_BOTTOM: u32 = 2;
    pub const ANCHOR_LEFT: u32 = 4;
    pub const ANCHOR_RIGHT: u32 = 8;
}

/// Window effects (`BiStateEffect` family).
pub mod effect {
    pub const TIME: u32 = 0x4F2B_0000;
    pub const TRIGGER: u32 = 0x4F2B_0001;
    pub const INTERPOLATION: u32 = 0x4F2B_0002;
    pub const EASE_IN: u32 = 0x4F2B_0003;
    pub const EASE_OUT: u32 = 0x4F2B_0004;
    pub const DAMPING: u32 = 0x4F2B_0005;
    /// Glide offset / inflate scale / modulate first colour.
    pub const PARAM0: u32 = 0x4F2C_0000;
    pub const PARAM1: u32 = 0x4F2C_0001;
    pub const ROT_X: u32 = 0x4F2C_0100;
    pub const ROT_Y: u32 = 0x4F2C_0101;
    pub const ROT_Z: u32 = 0x4F2C_0102;
    pub const ROT_ANGLE: u32 = 0x4F2C_0103;
    pub const NEAR_PLANE: u32 = 0x4F2C_0110;
    pub const CASCADE_DELAY: u32 = 0x4F2B_0000;
    pub const CASCADE_INTERVAL: u32 = 0x4F2B_0001;
    pub const FROM_MATERIAL: u32 = 0x02C1_F2C7;
    pub const TO_MATERIAL: u32 = 0x02C1_F2CA;
    pub const MATERIAL_RECURSIVE: u32 = 0x02C1_F2D4;
    pub const SWARM_NAME: u32 = 0x0238_EC88;
}

pub mod tooltip {
    pub const LAYOUT: u32 = 0x0375_4D73;
    pub const CONTROL: u32 = 0x0375_4D74;
    pub const DETAIL_CONTROL: u32 = 0x04EB_D284;
    pub const STRING: u32 = 0x0375_4D75;
    pub const DETAIL_STRING: u32 = 0x04EB_AA53;
    pub const OFFSET: u32 = 0x0375_4D76;
    pub const BEHAVIOR: u32 = 0x04DD_1A0D;
}

/// Event system properties.
pub mod event {
    pub const MESSAGE: u32 = 0x0255_EAF8;
    pub const BROADCAST: u32 = 0x0255_EAF9;
    pub const WINDOW_ID: u32 = 0x0255_EAFA;
    pub const TRIGGER: u32 = 0x0333_9952;
    pub const ACTIONS: [u32; 4] = [0x0333_5C41, 0x0333_5C42, 0x0333_5C43, 0x0333_5C44];
    pub const PREDICATES: [u32; 4] = [0x0333_5C12, 0x0333_5C13, 0x0333_5C14, 0x0333_5C15];
    pub const TIME_FUNCTION: u32 = 0x0254_CDFB;
    pub const ANIMATED_VALUE: u32 = 0x0254_CDFC;
    pub const STATE_ACTION: u32 = 0x033F_7F81;
    pub const STATE_PARAMETER: u32 = 0x033F_7F8C;
    pub const STATE_WINDOW_ID: u32 = 0x033F_7F89;
    pub const STATE_WINDOW_IS: u32 = 0x0341_F297;
    pub const NEGATE: u32 = 0x0252_1AFB;
    pub const COMBINE: u32 = 0x0252_1AFA;
    pub const QUERY: u32 = 0x033C_DC1B;
    pub const QUERY_PARAMETER: u32 = 0x033C_DCF4;
    pub const QUERY_WINDOW_ID: u32 = 0x033C_DC22;
    pub const QUERY_WINDOW_IS: u32 = 0x0341_EF60;
    pub const FROM_WINDOW: u32 = 0x0254_AB8E;
    pub const FROM_VALUE: u32 = 0x0254_B153;
    pub const TO_VALUE: u32 = 0x0254_B152;
    pub const TIME_NEGATED: u32 = 0x0253_AD19;
    pub const TIME_DURATION: u32 = 0x0253_ACED;
    pub const TIME_PRE_DELAY: u32 = 0x0269_A849;
    pub const TIME_POST_DELAY: u32 = 0x0269_A84B;
    pub const SMOOTH_IN: u32 = 0x0253_AD8C;
    pub const SMOOTH_OUT: u32 = 0x0253_AD8D;
    pub const OSC_TYPE: u32 = 0x0269_B186;
    pub const OSC_FREQUENCY: u32 = 0x0269_A919;
    pub const OSC_DAMPING: u32 = 0x0269_A91B;
    pub const OSC_CEILING: u32 = 0x026A_02AB;

    /// Window state message ids (`WindowStateMessage`).
    pub const MSG_VISIBLE: u32 = 0x025A_5F84;
    pub const MSG_MOUSE_FOCUS: u32 = 0x0249_F699;
    pub const MSG_KEYBOARD_FOCUS: u32 = 0x025A_5F5A;
    pub const MSG_ENABLED: u32 = 0x025C_8CB3;
    pub const MSG_BUTTON_HIGHLIGHTED: u32 = 0x025A_5F8E;
    pub const MSG_BUTTON_DEPRESSED: u32 = 0x025A_5F95;
    pub const MSG_BUTTON_SELECTED: u32 = 0x025A_5F9A;
    pub const MSG_BUTTON_CLICKED: u32 = 0x025A_61C5;
}

/// Drawable properties.
pub mod drawable {
    // StdDrawable / SporeStdDrawable.
    pub const IMAGES: u32 = 0x0F3A_C75E;
    pub const HIT_MASK: u32 = 0x0F3C_000D;
    pub const SCALE_TYPE: u32 = 0xEF3C_000A;
    pub const SCALE_FACTOR: u32 = 0xEF3C_000B;
    pub const SCALE_AREA: u32 = 0xEF3C_000C;
    pub const SPORE_STATES: [u32; 8] = [
        0x0540_10A0,
        0x0540_10A1,
        0x0540_10A2,
        0x0540_10A3,
        0x0540_10A4,
        0x0540_10A5,
        0x0540_10A6,
        0x0540_10A7,
    ];
    // SporeStdDrawableImageInfo.
    pub const INFO_BACKGROUND: u32 = 1;
    pub const INFO_ICON: u32 = 2;
    pub const INFO_BACKGROUND_COLOR: u32 = 3;
    pub const INFO_ICON_COLOR: u32 = 4;
    pub const INFO_ICON_MODE: u32 = 5;
    pub const INFO_ICON_SCALE: u32 = 6;
    pub const INFO_ICON_OFFSET: u32 = 7;
    pub const INFO_STROKE_MODE: u32 = 8;
    pub const INFO_HALO_MODE: u32 = 9;
    pub const INFO_STROKE_SHADOW: u32 = 0xA;
    pub const INFO_HALO_SHADOW: u32 = 0xB;
    pub const INFO_BACKGROUND_SCALE: u32 = 0xC;
    pub const INFO_BACKGROUND_OFFSET: u32 = 0xD;
    // ImageDrawable.
    pub const IMAGE: u32 = 0x4F3C_4A26;
    pub const IMAGE_FLAGS: u32 = 0x4F3C_4A27;
    pub const ALIGN_V: u32 = 0x4F3C_4A28;
    pub const ALIGN_H: u32 = 0x4F3C_4A29;
    pub const TILING: u32 = 0x4F3C_4A2A;
    pub const SCALE: u32 = 0x4F3C_4A2B;
    // Single-image button drawables.
    pub const BUTTON_IMAGE: u32 = 0x0F3A_C75E;
    pub const BUTTON_TILEABLE: u32 = 0xEF3C_000A;
    // Multi-image control drawables.
    pub const COMBO_IMAGES: u32 = 0x2F59_1E33;
    pub const SCROLLBAR_IMAGES: u32 = 0xAF42_D91C;
    pub const SLIDER_IMAGES: u32 = 0x6F42_D5CD;
    pub const SPINNER_IMAGES: u32 = 0x4F41_34A8;
    pub const DIALOG_IMAGES: u32 = 0x2F42_E1A1;
    // Variable width drawable.
    pub const VARWIDTH_IMAGE: u32 = 0xEEC1_D000;
    pub const VARWIDTH_FILL: u32 = 0xEEC1_D001;
    // Atlas image.
    pub const ATLAS_FILE: u32 = 0x01BE_0001;
    pub const ATLAS_UV: u32 = 0x01BE_0002;
    pub const ATLAS_DIMENSIONS: u32 = 0x01BE_0003;
}
