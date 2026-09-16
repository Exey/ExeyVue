//! The liquid-glass look: translucent panels, soft borders, one accent colour.
//!
//! iced has no backdrop blur, so "glass" here is layered translucency over a
//! transparent window. On a compositing desktop the wallpaper shows through.

use iced::widget::{button, container};
use iced::{theme, Background, Border, Color, Shadow, Theme, Vector};

pub const ACCENT: Color = Color {
    r: 0.42,
    g: 0.72,
    b: 1.0,
    a: 1.0,
};

pub const MUTED: Color = Color {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.62,
};

fn white(alpha: f32) -> Color {
    Color {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: alpha,
    }
}

/// Dark palette so built-in widgets (pick list menus, sliders) match the glass.
pub fn theme() -> Theme {
    Theme::custom(
        "ExeyVue".to_owned(),
        theme::Palette {
            background: Color {
                r: 0.07,
                g: 0.07,
                b: 0.10,
                a: 1.0,
            },
            text: Color::WHITE,
            primary: ACCENT,
            ..theme::Palette::DARK
        },
    )
}

/// Full-window tint. `alpha` 0.0 = see straight through, 1.0 = opaque.
pub fn backdrop(alpha: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(Color {
            r: 0.05,
            g: 0.05,
            b: 0.08,
            a: alpha,
        })),
        ..container::Style::default()
    }
}

/// A frosted panel.
pub fn glass(alpha: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(white(alpha))),
        border: Border {
            color: white(0.14),
            width: 1.0,
            radius: 16.0.into(),
        },
        shadow: Shadow {
            color: Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.28,
            },
            offset: Vector::new(0.0, 8.0),
            blur_radius: 24.0,
        },
        text_color: None,
    }
}

fn button_style(fill: Color, hover: Color, press: Color, status: button::Status) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Active => (fill, Color::WHITE),
        button::Status::Hovered => (hover, Color::WHITE),
        button::Status::Pressed => (press, Color::WHITE),
        button::Status::Disabled => (white(0.03), white(0.35)),
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color,
        border: Border {
            color: white(0.12),
            width: 1.0,
            radius: 10.0.into(),
        },
        shadow: Shadow::default(),
    }
}

/// Default translucent button.
pub fn glass_button(_theme: &Theme, status: button::Status) -> button::Style {
    button_style(white(0.09), white(0.16), white(0.24), status)
}

/// Highlighted button (active tab, primary action).
pub fn accent_button(_theme: &Theme, status: button::Status) -> button::Style {
    button_style(
        Color { a: 0.85, ..ACCENT },
        Color { a: 1.0, ..ACCENT },
        Color { a: 0.70, ..ACCENT },
        status,
    )
}

/// Invisible until hovered — used around tray thumbnails.
pub fn flat_button(_theme: &Theme, status: button::Status) -> button::Style {
    button_style(Color::TRANSPARENT, white(0.08), white(0.14), status)
}
