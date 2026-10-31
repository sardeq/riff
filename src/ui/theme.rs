//! Riff's look: dark, soft-cornered, with a single bright accent.

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Visuals};
use std::sync::Arc;

pub const BG: Color32 = Color32::from_rgb(0x10, 0x11, 0x16);
pub const SIDEBAR: Color32 = Color32::from_rgb(0x0B, 0x0C, 0x10);
pub const SURFACE: Color32 = Color32::from_rgb(0x17, 0x19, 0x21);
pub const SURFACE_HI: Color32 = Color32::from_rgb(0x1E, 0x21, 0x2B);
pub const SURFACE_LO: Color32 = Color32::from_rgb(0x13, 0x14, 0x1B);
pub const BORDER: Color32 = Color32::from_rgb(0x25, 0x28, 0x33);
pub const TEXT: Color32 = Color32::from_rgb(0xEE, 0xEF, 0xF4);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x9A, 0x9E, 0xAE);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x5E, 0x63, 0x73);
pub const DANGER: Color32 = Color32::from_rgb(0xF4, 0x3F, 0x5E);
pub const OK: Color32 = Color32::from_rgb(0x34, 0xD3, 0x99);
pub const WARN: Color32 = Color32::from_rgb(0xF5, 0x9E, 0x0B);

pub const PALETTE: &[(Color32, &str)] = &[
    (Color32::from_rgb(0x8B, 0x5C, 0xF6), "Violet"),
    (Color32::from_rgb(0xF4, 0x3F, 0x5E), "Rose"),
    (Color32::from_rgb(0x22, 0xD3, 0xEE), "Cyan"),
    (Color32::from_rgb(0xF5, 0x9E, 0x0B), "Amber"),
    (Color32::from_rgb(0x10, 0xB9, 0x81), "Emerald"),
    (Color32::from_rgb(0x3B, 0x82, 0xF6), "Blue"),
    (Color32::from_rgb(0xEC, 0x48, 0x99), "Pink"),
    (Color32::from_rgb(0x84, 0xCC, 0x16), "Lime"),
    (Color32::from_rgb(0xF9, 0x73, 0x16), "Orange"),
    (Color32::from_rgb(0x94, 0xA3, 0xB8), "Slate"),
];

pub fn color(i: usize) -> Color32 {
    PALETTE[i % PALETTE.len()].0
}

pub fn alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

pub fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("display".into()))
}

pub fn body(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "inter".into(),
        Arc::new(FontData::from_static(include_bytes!("../../assets/fonts/Inter-Regular.ttf"))),
    );
    fonts.font_data.insert(
        "inter-semibold".into(),
        Arc::new(FontData::from_static(include_bytes!("../../assets/fonts/Inter-SemiBold.ttf"))),
    );
    fonts.font_data.insert(
        "inter-display".into(),
        Arc::new(FontData::from_static(include_bytes!("../../assets/fonts/InterDisplay-Bold.ttf"))),
    );
    let fallbacks = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    fonts.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "inter".into());
    let with = |first: &str| {
        let mut v = vec![first.to_string()];
        v.extend(fallbacks.iter().cloned());
        v
    };
    fonts.families.insert(FontFamily::Name("semibold".into()), with("inter-semibold"));
    fonts.families.insert(FontFamily::Name("display".into()), with("inter-display"));
    // Monochrome Noto Emoji (newer than egui's bundled one) so every pad icon renders.
    fonts.font_data.insert(
        "noto-emoji".into(),
        Arc::new(FontData::from_static(include_bytes!("../../assets/fonts/NotoEmoji-Medium.ttf"))),
    );
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    // icons + emoji resolve in every text family: [text, phosphor, emoji, egui fallbacks…]
    if let Some(v) = fonts.families.get_mut(&FontFamily::Proportional) {
        v.insert(2.min(v.len()), "noto-emoji".into());
    }
    for fam in ["semibold", "display"] {
        if let Some(v) = fonts.families.get_mut(&FontFamily::Name(fam.into())) {
            v.insert(1, "phosphor".into());
            v.insert(2, "noto-emoji".into());
        }
    }
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context, accent: Color32) {
    let mut v = Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = SURFACE;
    v.extreme_bg_color = SURFACE_LO;
    v.faint_bg_color = SURFACE;
    v.code_bg_color = SURFACE_LO;
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.window_corner_radius = CornerRadius::same(16);
    v.menu_corner_radius = CornerRadius::same(12);
    v.window_shadow = egui::Shadow { offset: [0, 12], blur: 32, spread: 0, color: Color32::from_black_alpha(110) };
    v.popup_shadow = egui::Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(100) };
    v.selection.bg_fill = alpha(accent, 90);
    v.selection.stroke = Stroke::new(1.0, accent);
    v.hyperlink_color = accent;
    v.override_text_color = None;
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;

    let r = CornerRadius::same(10);
    v.widgets.noninteractive.bg_fill = SURFACE;
    v.widgets.noninteractive.weak_bg_fill = SURFACE;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_DIM);
    v.widgets.noninteractive.corner_radius = r;

    v.widgets.inactive.bg_fill = SURFACE_HI;
    v.widgets.inactive.weak_bg_fill = SURFACE_HI;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.inactive.corner_radius = r;

    v.widgets.hovered.bg_fill = mix(SURFACE_HI, accent, 0.18);
    v.widgets.hovered.weak_bg_fill = mix(SURFACE_HI, accent, 0.12);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, alpha(accent, 120));
    v.widgets.hovered.fg_stroke = Stroke::new(1.5, TEXT);
    v.widgets.hovered.corner_radius = r;
    v.widgets.hovered.expansion = 0.0;

    v.widgets.active.bg_fill = mix(SURFACE_HI, accent, 0.35);
    v.widgets.active.weak_bg_fill = mix(SURFACE_HI, accent, 0.3);
    v.widgets.active.bg_stroke = Stroke::new(1.0, accent);
    v.widgets.active.fg_stroke = Stroke::new(2.0, TEXT);
    v.widgets.active.corner_radius = r;
    v.widgets.active.expansion = 0.0;

    v.widgets.open = v.widgets.active;

    ctx.set_visuals_of(egui::Theme::Dark, v);
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.all_styles_mut(|s| {
        s.text_styles = [
            (TextStyle::Heading, display(22.0)),
            (TextStyle::Body, body(14.0)),
            (TextStyle::Button, body(14.0)),
            (TextStyle::Small, body(11.5)),
            (TextStyle::Monospace, FontId::monospace(12.5)),
        ]
        .into();
        s.spacing.item_spacing = egui::vec2(8.0, 8.0);
        s.spacing.button_padding = egui::vec2(12.0, 6.0);
        s.spacing.interact_size.y = 30.0;
        s.spacing.slider_rail_height = 5.0;
        s.spacing.slider_width = 160.0;
        s.spacing.scroll = egui::style::ScrollStyle::floating();
        s.spacing.combo_width = 180.0;
        s.interaction.selectable_labels = false;
        s.animation_time = 0.12;
    });
}

/// Window icon: gradient tile with three equaliser bars.
pub fn app_icon() -> egui::IconData {
    const N: usize = 128;
    let mut rgba = vec![0u8; N * N * 4];
    let a = PALETTE[0].0;
    let b = PALETTE[6].0;
    let bars = [(0.30, 0.42), (0.47, 0.70), (0.64, 0.52)];
    for y in 0..N {
        for x in 0..N {
            let (fx, fy) = (x as f32 / N as f32, y as f32 / N as f32);
            // rounded-square mask
            let r = 0.22;
            let dx = (fx - 0.5).abs() - (0.5 - r);
            let dy = (fy - 0.5).abs() - (0.5 - r);
            let d = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() - r;
            let cover = (0.5 - d * N as f32).clamp(0.0, 1.0);
            let mut c = mix(a, b, (fx + fy) * 0.5);
            for (cx, h) in bars {
                let w = 0.065;
                let top = 0.5 - h / 2.0;
                if (fx - cx).abs() < w && fy > top && fy < top + h {
                    c = Color32::WHITE;
                }
            }
            let i = (y * N + x) * 4;
            rgba[i] = c.r();
            rgba[i + 1] = c.g();
            rgba[i + 2] = c.b();
            rgba[i + 3] = (cover * 255.0) as u8;
        }
    }
    egui::IconData { rgba, width: N as u32, height: N as u32 }
}
