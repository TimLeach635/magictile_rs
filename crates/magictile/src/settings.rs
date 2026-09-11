//! User settings, with the original program's defaults. (Persistence and a settings panel arrive
//! in phase 4.)

use eframe::egui::Color32;
use r3::models::{HyperbolicModel, SphericalModel};

#[derive(Clone, Debug)]
pub struct Settings {
    /// Twist animation rate, 0 to 1 (1 is instantaneous).
    pub rotation_rate: f64,
    /// How long the view keeps moving after a flick, 0 to 1.
    pub gliding: f64,
    pub show_only_fundamental: bool,
    pub highlight_twisting_circles: bool,
    pub enable_texture_mipmaps: bool,
    /// Debug: only draw the cells used in state calculations.
    pub show_state_calc_cells: bool,
    pub show_systolic_pants: bool,
    pub spherical_model: SphericalModel,
    pub hyperbolic_model: HyperbolicModel,
    pub color_twisting_circles: Color32,
    pub color_bg: Color32,
    pub color_tile_edges: Color32,
    pub color_off: Color32,
    /// Puzzle face colors.
    pub colors: Vec<Color32>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            rotation_rate: 0.5,
            gliding: 0.5,
            show_only_fundamental: false,
            highlight_twisting_circles: true,
            enable_texture_mipmaps: true,
            show_state_calc_cells: false,
            show_systolic_pants: true,
            spherical_model: SphericalModel::Sterographic,
            hyperbolic_model: HyperbolicModel::Poincare,
            color_twisting_circles: hex(0xFF4500), // OrangeRed
            color_bg: hex(0xA9A9A9),               // DarkGray
            color_tile_edges: Color32::BLACK,
            color_off: Color32::from_rgb(25, 25, 25),
            colors: DEFAULT_COLORS.iter().map(|&c| hex(c)).collect(),
        }
    }
}

impl Settings {
    /// The color for a sticker's color index (-1 is "off", for lights on puzzles).
    pub fn sticker_color(&self, index: i32) -> Color32 {
        match usize::try_from(index) {
            Ok(i) => self.colors.get(i).copied().unwrap_or(Color32::GRAY),
            Err(_) => self.color_off,
        }
    }
}

fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// The original's 56 default colors (.NET named colors where it used them).
const DEFAULT_COLORS: [u32; 56] = [
    0xFFFFFF, // White
    0x008000, // Green
    0x0000FF, // Blue
    0xFFFF00, // Yellow
    0xFF0000, // Red
    0xFF8000, // Orange
    0x00FFFF, // Cyan
    0x800080, // Purple
    0xC0C0C0, // Silver
    0x800000, // Maroon
    0xFF69B4, // HotPink
    0x6A5ACD, // SlateBlue
    0x808000, // Olive
    0xFF6347, // Tomato
    0x00BFFF, // DeepSkyBlue
    0x00FA9A, // MediumSpringGreen
    0x32CD32, // LimeGreen
    0xFFD700, // Gold
    0xC71585, // MediumVioletRed
    0x404040, // (64, 64, 64)
    0x000080, // Navy
    0xA0522D, // Sienna
    0x7FFF00, // Chartreuse
    0x008080, // Teal
    0x400040, // (64, 0, 64)
    0xFA8072, // Salmon
    0x00FFFF, // Aqua
    0x87CEFA, // LightSkyBlue
    0xFF4500, // OrangeRed
    0xC0C000, // (192, 192, 0)
    0xBC8F8F, // RosyBrown
    0xD2B48C, // Tan
    0xF5FFFA, // MintCream
    0x8A2BE2, // BlueViolet
    0x5F9EA0, // CadetBlue
    0xFF1493, // DeepPink
    0xD2691E, // Chocolate
    0xFF7F50, // Coral
    0x6495ED, // CornflowerBlue
    0xF0E68C, // Khaki
    0xDC143C, // Crimson
    0xB8860B, // DarkGoldenrod
    0xA9A9A9, // DarkGray
    0x006400, // DarkGreen
    0xBDB76B, // DarkKhaki
    0x8B008B, // DarkMagenta
    0x556B2F, // DarkOliveGreen
    0xFF8C00, // DarkOrange
    0x9932CC, // DarkOrchid
    0x8B0000, // DarkRed
    0xFFC0FF, // (255, 192, 255)
    0x8FBC8F, // DarkSeaGreen
    0x483D8B, // DarkSlateBlue
    0x2F4F4F, // DarkSlateGray
    0x00CED1, // DarkTurquoise
    0x9400D3, // DarkViolet
];
