//! Debug-only layout overlay: draws panel rectangles and pointer hit-testing
//! on top of the UI when compiled with `debug_assertions`.

use super::*;

#[cfg(debug_assertions)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct DebugLayoutRects {
    pub(crate) root: egui::Rect,
    pub(crate) top: egui::Rect,
    pub(crate) left: egui::Rect,
    pub(crate) preview: egui::Rect,
    pub(crate) bottom: egui::Rect,
}

#[cfg(debug_assertions)]
pub(crate) fn draw_debug_layout_overlay(ctx: &egui::Context, rects: DebugLayoutRects) {
    let painter = ctx.debug_painter();
    debug_layout_rect(
        &painter,
        "root",
        rects.root,
        egui::Color32::from_rgb(220, 220, 220),
    );
    debug_layout_rect(
        &painter,
        "top",
        rects.top,
        egui::Color32::from_rgb(225, 176, 84),
    );
    debug_layout_rect(
        &painter,
        "left",
        rects.left,
        egui::Color32::from_rgb(104, 166, 200),
    );
    debug_layout_rect(
        &painter,
        "preview",
        rects.preview,
        egui::Color32::from_rgb(94, 176, 132),
    );
    debug_layout_rect(
        &painter,
        "bottom",
        rects.bottom,
        egui::Color32::from_rgb(190, 142, 72),
    );

    if let Some(pointer) = ctx.pointer_hover_pos() {
        painter.circle_filled(pointer, 4.0, egui::Color32::from_rgb(240, 96, 96));
        let mut hits = Vec::new();
        for (name, rect) in [
            ("root", rects.root),
            ("top", rects.top),
            ("left", rects.left),
            ("preview", rects.preview),
            ("bottom", rects.bottom),
        ] {
            if rect.contains(pointer) {
                hits.push(name);
            }
        }
        let hit_text = if hits.is_empty() {
            "none".to_owned()
        } else {
            hits.join(", ")
        };
        painter.text(
            rects.root.left_top() + egui::vec2(10.0, 10.0),
            egui::Align2::LEFT_TOP,
            format!("pointer {:.1},{:.1} in: {}", pointer.x, pointer.y, hit_text),
            egui::FontId::monospace(13.0),
            egui::Color32::WHITE,
        );
    }
}

#[cfg(debug_assertions)]
fn debug_layout_rect(painter: &egui::Painter, name: &str, rect: egui::Rect, color: egui::Color32) {
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(2.0, color),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.left_top() + egui::vec2(6.0, 6.0),
        egui::Align2::LEFT_TOP,
        format!(
            "{} x {:.1}-{:.1} y {:.1}-{:.1} {:.1}x{:.1}",
            name,
            rect.left(),
            rect.right(),
            rect.top(),
            rect.bottom(),
            rect.width(),
            rect.height()
        ),
        egui::FontId::monospace(12.0),
        color,
    );
}
