use crate::model::{CategoryTotal, MonthTotal};
use gtk::cairo::{Context, LineCap};
use gtk::prelude::*;
use std::f64::consts::PI;

fn rgb(ctx: &Context, color: (f64, f64, f64), alpha: f64) {
    ctx.set_source_rgba(color.0, color.1, color.2, alpha);
}
const GREEN: (f64, f64, f64) = (0.22, 0.76, 0.53);
const BLUE: (f64, f64, f64) = (0.64, 0.54, 0.94);
const PALETTE: [(f64, f64, f64); 8] = [
    (0.34, 0.58, 0.95),
    (0.66, 0.54, 0.95),
    (0.95, 0.54, 0.62),
    (0.98, 0.72, 0.39),
    (0.32, 0.77, 0.61),
    (0.53, 0.70, 0.85),
    (0.77, 0.67, 0.93),
    (0.67, 0.69, 0.74),
];

fn label(ctx: &Context, text: &str, x: f64, y: f64, size: f64) {
    rgb(ctx, (0.72, 0.75, 0.79), 1.0);
    ctx.select_font_face(
        "Sans",
        gtk::cairo::FontSlant::Normal,
        gtk::cairo::FontWeight::Normal,
    );
    ctx.set_font_size(size);
    ctx.move_to(x, y);
    let _ = ctx.show_text(text);
}

pub fn line_chart(data: Vec<MonthTotal>) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(640);
    area.set_content_height(250);
    area.set_hexpand(true);
    area.set_draw_func(move |_, ctx, w, h| {
        let w = w as f64;
        let h = h as f64;
        let left = 60.0;
        let right = (w - 20.0).max(left + 10.0);
        let top = 20.0;
        let bottom = (h - 42.0).max(top + 10.0);
        if data.iter().all(|m| m.income == 0 && m.expense == 0) {
            label(
                ctx,
                "No transactions in this period",
                left + 26.0,
                h / 2.0,
                14.0,
            );
            return;
        }
        // Stored amounts are integer minor units (cents); chart labels are major
        // currency units. Keep the units separate to avoid 25 MAD => 2,500 on axis.
        let peak = data
            .iter()
            .map(|x| x.income.max(x.expense))
            .max()
            .unwrap_or(0) as f64
            / 100.0;
        // Choose readable axis steps; small amounts should show 10, 20, 30, 40.
        let raw_step = (peak * 1.15 / 4.0).max(1.0);
        let scale = 10f64.powf(raw_step.log10().floor());
        let factor = raw_step / scale;
        let unit = if factor <= 1.0 {
            1.0
        } else if factor <= 2.0 {
            2.0
        } else if factor <= 2.5 {
            2.5
        } else if factor <= 5.0 {
            5.0
        } else {
            10.0
        };
        let max_major = (unit * scale * 4.0).max(4.0);
        let max_minor = max_major * 100.0;
        ctx.set_line_width(1.0);
        for step in 0..=4 {
            let y = top + (bottom - top) * (step as f64) / 4.0;
            rgb(ctx, (0.48, 0.50, 0.56), 0.20);
            ctx.move_to(left, y);
            ctx.line_to(right, y);
            let _ = ctx.stroke();
            let major = (4 - step) as f64 * max_major / 4.0;
            let axis = if major >= 1.0 {
                format!("{:.0}", major)
            } else {
                format!("{:.1}", major)
            };
            label(ctx, &axis, 8.0, y + 4.0, 11.0);
        }
        if data.is_empty() {
            label(
                ctx,
                "No data yet",
                (left + right) / 2.0 - 30.0,
                (top + bottom) / 2.0,
                13.0,
            );
            return;
        }
        let points = data.len();
        for (ix, category) in [true, false].iter().enumerate() {
            let color = if ix == 0 { GREEN } else { BLUE };
            rgb(ctx, color, 1.0);
            ctx.set_line_width(2.5);
            ctx.set_line_cap(LineCap::Round);
            for (i, m) in data.iter().enumerate() {
                let x = if points == 1 {
                    (left + right) / 2.0
                } else {
                    left + (right - left) * i as f64 / (points - 1) as f64
                };
                let value = if *category { m.income } else { m.expense };
                let y = bottom - (value as f64 / max_minor) * (bottom - top);
                if i == 0 {
                    ctx.move_to(x, y);
                } else {
                    ctx.line_to(x, y);
                }
            }
            let _ = ctx.stroke();
            for (i, m) in data.iter().enumerate() {
                let x = if points == 1 {
                    (left + right) / 2.0
                } else {
                    left + (right - left) * i as f64 / (points - 1) as f64
                };
                let value = if *category { m.income } else { m.expense };
                let y = bottom - (value as f64 / max_minor) * (bottom - top);
                rgb(ctx, color, 1.0);
                ctx.arc(x, y, 3.6, 0.0, 2.0 * PI);
                let _ = ctx.fill();
            }
        }
        for (i, m) in data.iter().enumerate() {
            let x = if points == 1 {
                (left + right) / 2.0
            } else {
                left + (right - left) * i as f64 / (points - 1) as f64
            };
            let numeric = m
                .month
                .get(5..7)
                .unwrap_or("")
                .parse::<usize>()
                .unwrap_or(0);
            let month = crate::i18n::month_name(numeric, true);
            label(ctx, month, x - 10.0, bottom + 24.0, 11.0);
        }
    });
    area
}

pub fn donut(categories: Vec<CategoryTotal>, currency: &str) -> gtk::DrawingArea {
    let currency = currency.to_string();
    let area = gtk::DrawingArea::new();
    area.set_content_width(225);
    area.set_content_height(225);
    area.set_hexpand(false);
    area.set_draw_func(move |_, ctx, w, h| {
        let w = w as f64;
        let h = h as f64;
        let radius = w.min(h) * 0.33;
        let sum: i64 = categories.iter().map(|x| x.amount_minor).sum();
        let cx = w / 2.0;
        let cy = h / 2.0;
        ctx.set_line_width(25.0);
        rgb(ctx, (0.47, 0.49, 0.54), 0.20);
        ctx.arc(cx, cy, radius, -PI / 2.0, 3.0 * PI / 2.0);
        let _ = ctx.stroke();
        if sum == 0 {
            label(ctx, "No data", cx - 27.0, cy + 5.0, 12.0);
            return;
        }
        let mut angle = -PI / 2.0;
        for (i, c) in categories.iter().enumerate() {
            let next = angle + 2.0 * PI * (c.amount_minor as f64 / sum as f64);
            let color = PALETTE[i % PALETTE.len()];
            rgb(ctx, color, 1.0);
            ctx.arc(cx, cy, radius, angle + 0.007, next - 0.007);
            let _ = ctx.stroke();
            angle = next;
        }
        let total = format!("{:.2}", sum as f64 / 100.0);
        label(ctx, &total, cx - (total.len() as f64 * 4.4), cy - 5.0, 14.0);
        label(
            ctx,
            &currency,
            cx - (currency.len() as f64 * 3.3),
            cy + 13.0,
            10.5,
        );
        label(ctx, "Total Spending", cx - 42.0, cy + 29.0, 9.5);
    });
    area
}

pub fn palette(index: usize) -> (f64, f64, f64) {
    PALETTE[index % PALETTE.len()]
}
