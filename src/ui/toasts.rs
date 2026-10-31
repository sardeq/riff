use super::theme::{self, BORDER, TEXT};
use crate::core::ToastKind;
use egui::{Align2, Stroke, pos2, vec2};
use egui_phosphor::regular as ic;
use std::time::{Duration, Instant};

struct Toast {
    kind: ToastKind,
    text: String,
    born: Instant,
}

#[derive(Default)]
pub struct Toasts {
    items: Vec<Toast>,
}

const LIFE: Duration = Duration::from_millis(3800);

impl Toasts {
    pub fn push(&mut self, kind: ToastKind, text: String) {
        self.items.retain(|t| t.text != text);
        self.items.push(Toast { kind, text, born: Instant::now() });
        if self.items.len() > 4 {
            self.items.remove(0);
        }
    }

    pub fn animating(&self) -> bool {
        !self.items.is_empty()
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        self.items.retain(|t| t.born.elapsed() < LIFE);
        let screen = ctx.content_rect();
        let mut y = screen.bottom() - 96.0;
        for (i, t) in self.items.iter().enumerate().rev() {
            let age = t.born.elapsed().as_secs_f32();
            let fade_in = (age / 0.18).min(1.0);
            let fade_out = ((LIFE.as_secs_f32() - age) / 0.3).min(1.0);
            let a = fade_in.min(fade_out);
            let ease = 1.0 - (1.0 - fade_in).powi(3);
            let (icon, col) = match t.kind {
                ToastKind::Info => (ic::INFO, theme::TEXT_DIM),
                ToastKind::Success => (ic::CHECK_CIRCLE, theme::OK),
                ToastKind::Error => (ic::WARNING_CIRCLE, theme::DANGER),
            };
            let resp = egui::Area::new(egui::Id::new(("toast", i, &t.text)))
                .order(egui::Order::Tooltip)
                .pivot(Align2::RIGHT_BOTTOM)
                .fixed_pos(pos2(screen.right() - 20.0 + (1.0 - ease) * 40.0, y))
                .interactable(false)
                .show(ctx, |ui| {
                    ui.set_opacity(a);
                    egui::Frame::new()
                        .fill(theme::SURFACE_HI)
                        .stroke(Stroke::new(1.0, BORDER))
                        .corner_radius(14)
                        .shadow(egui::Shadow {
                            offset: [0, 10],
                            blur: 24,
                            spread: 0,
                            color: egui::Color32::from_black_alpha(120),
                        })
                        .inner_margin(egui::Margin::symmetric(14, 10))
                        .show(ui, |ui| {
                            ui.set_max_width(380.0);
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(icon).size(17.0).color(col));
                                ui.add(egui::Label::new(egui::RichText::new(&t.text).color(TEXT)).wrap());
                            });
                        });
                });
            y -= resp.response.rect.height() + 8.0;
        }
        let _ = vec2(0.0, 0.0);
    }
}
