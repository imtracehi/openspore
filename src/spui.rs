//! SPUI: Spore's data-driven user interface (layout files of resource type
//! `0x250FE9A2`), ported from the SporeModder-FX sources and checked against
//! the retail frames.
//!
//! An SPUI layout is a tree of windows plus the drawables (image sets), window
//! procedures (layout styles, effects, events, tooltips) and image atlases they
//! reference. Everything about how a retail screen looks lives in these files;
//! the game code only fills text, toggles visibility and reacts to clicks. This
//! module therefore has two halves:
//!
//! * pure data (no Bevy): [`format`] reads the binary container into a
//!   property bag, [`model`] turns it into a typed [`model::Layout`],
//!   [`solve`] resolves window rectangles for a given screen size exactly like
//!   the retail layout styles do, and [`style`] reads the text style sheet;
//! * presentation (Bevy UI): [`view`] spawns one UI node per window, keeps it in
//!   sync with a runtime state object and handles hover/press input.
//!
//! No retail data is embedded: layouts, atlases, fonts, the style sheet and
//! localized strings are read from the installed game at runtime. The class and
//! property id tables in [`schema`] describe the *file format* (they come from
//! the SporeModder-FX UI designer definitions), not any game asset.

pub mod caption;
pub mod effects;
pub mod fonts;
pub mod format;
pub mod ids;
pub mod model;
pub mod outline;
pub mod paint;
pub mod schema;
pub mod solve;
pub mod style;
pub mod surface;
pub mod tooltip;
pub mod view;
