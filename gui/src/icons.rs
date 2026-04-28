//! Phosphor icon re-exports.
//!
//! Re-exports from egui_phosphor::regular for use throughout the app.
//! Reference: <https://phosphoricons.com/>

pub use egui_phosphor::regular::{
    ARROW_COUNTER_CLOCKWISE,
    ARROW_DOWN,
    ARROW_LEFT,
    ARROW_RIGHT,
    ARROW_SQUARE_OUT,
    // Navigation
    ARROW_UP,
    ARROWS_CLOCKWISE,

    ARROWS_COUNTER_CLOCKWISE,
    // UI Actions
    BOOK,
    BOOK_OPEN,
    BUG,
    CARET_DOWN,

    CARET_UP,
    CHECK,
    // Status icons
    CHECK_CIRCLE,
    CLOCK,
    CODE,
    COPY,
    CUBE,
    DOWNLOAD,
    EYE,
    EYE_SLASH,
    // Files
    FILE,
    FLOPPY_DISK,

    FOLDER,
    FOLDER_OPEN,

    // Misc
    GEAR,
    HOURGLASS,
    // Media/Content
    IMAGE,
    INFO,
    LIGHTNING,
    MAGIC_WAND,

    MAGNIFYING_GLASS,
    MAP_PIN,
    PAINT_BRUSH,
    PALETTE,
    PAUSE,
    // Actions
    PLAY,
    PROHIBIT,
    SPINNER,
    STAR,
    STOP,
    TERMINAL,
    TRASH,
    TRAY_ARROW_DOWN,
    WARNING,
    X,

    X_CIRCLE,
};

// Aliases for compatibility with existing code
pub const CIRCLE_CHECK: &str = CHECK_CIRCLE;
pub const CIRCLE_XMARK: &str = X_CIRCLE;
pub const XMARK: &str = X;
pub const ARROWS_ROTATE: &str = ARROWS_CLOCKWISE;
pub const ROTATE: &str = ARROWS_CLOCKWISE;
pub const ROTATE_LEFT: &str = ARROW_COUNTER_CLOCKWISE;
pub const COG: &str = GEAR;
pub const GEARS: &str = GEAR;
pub const BOLT: &str = LIGHTNING;
pub const WAND_MAGIC: &str = MAGIC_WAND;
pub const LOCATION_DOT: &str = MAP_PIN;
pub const EXTERNAL_LINK: &str = ARROW_SQUARE_OUT;
pub const CHEVRON_UP: &str = CARET_UP;
pub const CHEVRON_DOWN: &str = CARET_DOWN;
