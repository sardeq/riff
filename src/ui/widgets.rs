//! Small custom widgets shared by all views.

use super::theme::{self, BORDER, SURFACE, SURFACE_HI, TEXT, TEXT_DIM, TEXT_FAINT, alpha};
use egui::{Align2, Color32, CornerRadius, Margin, Rect, Response, Sense, Stroke, Ui, Vec2, pos2, vec2};
use std::sync::atomic::{AtomicUsize, Ordering};

static ACCENT: AtomicUsize = AtomicUsize::new(0);

pub fn set_accent(i: usize) {
    ACCENT.store(i, Ordering::Relaxed);
}

pub fn accent() -> Color32 {
    theme::color(ACCENT.load(Ordering::Relaxed))
}

pub fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(16))
        .inner_margin(Margin::same(18))
}

pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    card_frame()
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

pub fn section_title(ui: &mut Ui, icon: &str, title: &str, subtitle: &str) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
        ui.painter().rect_filled(r, 10.0, alpha(accent(), 38));
        ui.painter().text(r.center(), Align2::CENTER_CENTER, icon, theme::body(18.0), accent());
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(egui::RichText::new(title).font(theme::semibold(15.5)).color(TEXT));
            if !subtitle.is_empty() {
                ui.label(egui::RichText::new(subtitle).size(12.5).color(TEXT_DIM));
            }
        });
    });
    ui.add_space(6.0);
}

/// iOS-style switch.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let size = vec2(40.0, 22.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);
    let bg = theme::mix(SURFACE_HI, accent(), t);
    let bg = if resp.hovered() { theme::mix(bg, Color32::WHITE, 0.06) } else { bg };
    ui.painter().rect_filled(rect, 11.0, bg);
    let x = egui::lerp(rect.left() + 11.0..=rect.right() - 11.0, t);
    ui.painter().circle_filled(pos2(x, rect.center().y), 8.0, Color32::WHITE);
    resp
}

/// Label + description on the left, switch on the right.
pub fn toggle_row(ui: &mut Ui, label: &str, desc: &str, on: &mut bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.set_width((ui.available_width() - 56.0).max(80.0));
            ui.label(egui::RichText::new(label).color(TEXT));
            if !desc.is_empty() {
                ui.label(egui::RichText::new(desc).size(12.0).color(TEXT_FAINT));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            changed = toggle(ui, on).changed();
        });
    });
    changed
}

pub fn slider_row(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    fmt: impl Fn(f32) -> String,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).color(TEXT_DIM));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(fmt(*value)).font(theme::semibold(13.0)).color(TEXT));
        });
    });
    ui.scope(|ui| {
        ui.spacing_mut().slider_width = ui.available_width();
        ui.spacing_mut().interact_size.y = 18.0;
        changed |= ui.add(egui::Slider::new(value, range).show_value(false)).changed();
    });
    changed
}

fn button_core(ui: &mut Ui, icon: &str, text: &str, fill: Color32, fg: Color32, stroke: Stroke, h: f32) -> Response {
    let label = if text.is_empty() {
        icon.to_string()
    } else if icon.is_empty() {
        text.to_string()
    } else {
        format!("{icon}  {text}")
    };
    let galley = ui.painter().layout_no_wrap(label, theme::semibold(13.5), fg);
    let size = vec2(galley.size().x + if text.is_empty() { 0.0 } else { 28.0 }, h).max(vec2(h, h));
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hover = ui.ctx().animate_bool_responsive(resp.id.with("h"), resp.hovered());
    let press = resp.is_pointer_button_down_on();
    let mut f = theme::mix(fill, Color32::WHITE, 0.08 * hover);
    if press {
        f = theme::mix(f, Color32::BLACK, 0.12);
    }
    ui.painter().rect(rect, h / 2.0, f, stroke, egui::StrokeKind::Inside);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, fg);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn primary_button(ui: &mut Ui, icon: &str, text: &str) -> Response {
    button_core(ui, icon, text, accent(), Color32::WHITE, Stroke::NONE, 34.0)
}

pub fn ghost_button(ui: &mut Ui, icon: &str, text: &str) -> Response {
    button_core(ui, icon, text, SURFACE_HI, TEXT, Stroke::NONE, 34.0)
}

pub fn danger_button(ui: &mut Ui, icon: &str, text: &str) -> Response {
    button_core(ui, icon, text, alpha(theme::DANGER, 36), theme::DANGER, Stroke::NONE, 34.0)
}

pub fn icon_button(ui: &mut Ui, icon: &str, tip: &str) -> Response {
    let r = button_core(ui, icon, "", SURFACE_HI, TEXT, Stroke::NONE, 34.0);
    if tip.is_empty() { r } else { r.on_hover_text(tip) }
}

pub fn small_icon_button(ui: &mut Ui, icon: &str, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
    let col = if resp.hovered() { TEXT } else { TEXT_DIM };
    if resp.hovered() {
        ui.painter().rect_filled(rect, 8.0, SURFACE_HI);
    }
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, icon, theme::body(15.0), col);
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if tip.is_empty() { resp } else { resp.on_hover_text(tip) }
}

pub fn chip(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), theme::semibold(11.0), color);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + vec2(14.0, 6.0), Sense::hover());
    ui.painter().rect_filled(rect, rect.height() / 2.0, alpha(color, 30));
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
    resp
}

/// Segmented control. Returns true when the selection changed.
/// Painted manually so it lays out the same inside left- and right-aligned rows.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let font = theme::semibold(13.0);
    let galleys: Vec<_> = options
        .iter()
        .map(|(v, label)| {
            ui.painter().layout_no_wrap(label.to_string(), font.clone(), if *value == *v { TEXT } else { TEXT_DIM })
        })
        .collect();
    let pad = vec2(22.0, 12.0);
    let h = galleys.iter().map(|g| g.size().y).fold(0.0, f32::max) + pad.y;
    let total: f32 = galleys.iter().map(|g| g.size().x + pad.x).sum::<f32>() + 2.0 * (options.len() as f32 - 1.0) + 6.0;
    let (outer, _) = ui.allocate_exact_size(vec2(total, h + 6.0), Sense::hover());
    ui.painter().rect_filled(outer, 10.0, theme::SURFACE_LO);
    let mut x = outer.left() + 3.0;
    let mut changed = false;
    for (i, ((v, _), g)) in options.iter().zip(galleys).enumerate() {
        let r = Rect::from_min_size(pos2(x, outer.top() + 3.0), vec2(g.size().x + pad.x, h));
        x += r.width() + 2.0;
        let resp = ui.interact(r, ui.id().with(("seg", i, outer.left() as i32, outer.top() as i32)), Sense::click());
        let sel = *value == *v;
        if sel {
            ui.painter().rect_filled(r, 8.0, SURFACE_HI);
            ui.painter().rect_stroke(r, 8.0, Stroke::new(1.0, alpha(accent(), 90)), egui::StrokeKind::Inside);
        } else if resp.hovered() {
            ui.painter().rect_filled(r, 8.0, alpha(SURFACE_HI, 120));
        }
        ui.painter().galley(r.center() - g.size() / 2.0, g, TEXT);
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !sel {
            *value = *v;
            changed = true;
        }
    }
    changed
}

/// Mirrored bar waveform; bars before `progress` use `on`, the rest `off`.
pub fn paint_waveform(
    painter: &egui::Painter,
    rect: Rect,
    peaks: &[f32],
    progress: Option<f32>,
    on: Color32,
    off: Color32,
) {
    if peaks.is_empty() || rect.width() < 4.0 {
        return;
    }
    let bar_w = 2.0;
    let gap = 1.5;
    let n = ((rect.width() + gap) / (bar_w + gap)).floor().max(1.0) as usize;
    let mid = rect.center().y;
    let half = rect.height() / 2.0;
    for i in 0..n {
        let a = i * peaks.len() / n;
        let b = ((i + 1) * peaks.len() / n).max(a + 1).min(peaks.len());
        let p = peaks[a..b].iter().cloned().fold(0.0f32, f32::max);
        let h = (p * half).max(1.0);
        let x = rect.left() + i as f32 * (bar_w + gap);
        let frac = (i as f32 + 0.5) / n as f32;
        let c = match progress {
            Some(pr) if frac <= pr => on,
            Some(_) => off,
            None => on,
        };
        painter.rect_filled(Rect::from_min_max(pos2(x, mid - h), pos2(x + bar_w, mid + h)), 1.0, c);
    }
}

/// Horizontal segmented level meter.
pub fn meter(ui: &mut Ui, level: f32, width: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 6.0), Sense::hover());
    let segs = 16;
    let w = (rect.width() - (segs - 1) as f32 * 2.0) / segs as f32;
    let db = if level > 1e-5 { 20.0 * level.log10() } else { -60.0 };
    let lit = ((db + 48.0) / 48.0 * segs as f32).clamp(0.0, segs as f32) as usize;
    for i in 0..segs {
        let x = rect.left() + i as f32 * (w + 2.0);
        let r = Rect::from_min_size(pos2(x, rect.top()), vec2(w, rect.height()));
        let col = if i >= lit {
            SURFACE_HI
        } else if i >= segs - 2 {
            theme::DANGER
        } else if i >= segs - 5 {
            theme::WARN
        } else {
            theme::OK
        };
        ui.painter().rect_filled(r, 2.0, col);
    }
}

pub fn fmt_time(secs: f32) -> String {
    let s = secs.max(0.0);
    if s < 10.0 { format!("{s:.1}s") } else { format!("{}:{:02}", (s / 60.0) as u32, (s % 60.0) as u32) }
}

pub fn dim(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text.into()).color(TEXT_DIM)
}

pub fn faint(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text.into()).color(TEXT_FAINT).size(12.0)
}

pub fn label_caps(ui: &mut Ui, text: &str) {
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .font(theme::semibold(10.5))
            .color(TEXT_FAINT)
            .extra_letter_spacing(1.2),
    );
}

/// Text input styled as a rounded search field with a leading icon.
pub fn search_field(ui: &mut Ui, text: &mut String, hint: &str, width: f32, id: egui::Id) -> Response {
    egui::Frame::new()
        .fill(theme::SURFACE_LO)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(17)
        .inner_margin(Margin { left: 12, right: 10, top: 3, bottom: 3 })
        .show(ui, |ui| {
            ui.allocate_ui_with_layout(
                vec2(width + 56.0, 28.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.label(egui::RichText::new(egui_phosphor::regular::MAGNIFYING_GLASS).color(TEXT_FAINT));
                    let r = ui.add(
                        egui::TextEdit::singleline(text)
                            .id(id)
                            .hint_text(hint)
                            .frame(egui::Frame::NONE)
                            .desired_width(width),
                    );
                    if !text.is_empty() && small_icon_button(ui, egui_phosphor::regular::X, "").clicked() {
                        text.clear();
                    }
                    r
                },
            )
            .inner
        })
        .inner
}

/// Rounded rectangle filled with a diagonal two-color gradient.
pub fn gradient_rounded_rect(painter: &egui::Painter, rect: Rect, radius: f32, c1: Color32, c2: Color32) {
    let r = radius.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let corners = [
        (pos2(rect.right() - r, rect.top() + r), -90.0f32),
        (pos2(rect.right() - r, rect.bottom() - r), 0.0),
        (pos2(rect.left() + r, rect.bottom() - r), 90.0),
        (pos2(rect.left() + r, rect.top() + r), 180.0),
    ];
    let col = |p: egui::Pos2| {
        let t = ((p.x - rect.left()) / rect.width() * 0.5 + (p.y - rect.top()) / rect.height() * 0.5).clamp(0.0, 1.0);
        theme::mix(c1, c2, t)
    };
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.center(), col(rect.center()));
    for (c, start) in corners {
        for k in 0..=6 {
            let a = (start + 90.0 * k as f32 / 6.0).to_radians();
            let p = c + Vec2::angled(a) * r;
            mesh.colored_vertex(p, col(p));
        }
    }
    let n = mesh.vertices.len() as u32 - 1;
    for i in 1..=n {
        let j = if i == n { 1 } else { i + 1 };
        mesh.add_triangle(0, i, j);
    }
    painter.add(egui::Shape::mesh(mesh));
}
