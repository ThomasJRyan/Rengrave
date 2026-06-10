pub mod batch;
pub mod bitmap;
pub mod cleanup;
pub mod dxf;
pub mod external;
pub mod font;
pub mod gcode;
pub mod geometry;
pub mod layout;
pub mod project;
pub mod settings;
pub mod vcarve;

pub const FENGRAVE_VERSION: &str = "1.78";
pub const RENGRAVE_VERSION: &str = env!("CARGO_PKG_VERSION");
