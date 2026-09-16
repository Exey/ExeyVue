//! The knife overlay: a transparent canvas stacked on top of the image that
//! draws the cut line where the cursor is and emits a message on click.

use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke, Text};
use iced::{event, Color, Point, Rectangle, Renderer, Size, Theme};

use crate::ops::{self, Orientation};
use crate::Message;

pub struct Overlay {
    /// Native size of the image underneath, in pixels.
    pub image: Size,
    pub orientation: Orientation,
    /// Normalised knife position along the cut axis, 0.0..=1.0.
    pub position: f32,
}

/// Where an image of `image` size lands inside `bounds` under `ContentFit::Contain`,
/// centred — this mirrors the iced image widget so the line lines up with pixels.
pub fn fitted_rect(image: Size, bounds: Size) -> Rectangle {
    if image.width <= 0.0 || image.height <= 0.0 {
        return Rectangle::new(Point::ORIGIN, bounds);
    }
    let scale = (bounds.width / image.width).min(bounds.height / image.height);
    let size = Size::new(image.width * scale, image.height * scale);
    Rectangle::new(
        Point::new(
            (bounds.width - size.width) / 2.0,
            (bounds.height - size.height) / 2.0,
        ),
        size,
    )
}

impl Overlay {
    fn pixel(&self) -> Option<u32> {
        match self.orientation {
            Orientation::Vertical => ops::knife_pixel(self.image.width as u32, self.position),
            Orientation::Horizontal => ops::knife_pixel(self.image.height as u32, self.position),
        }
    }
}

impl canvas::Program<Message> for Overlay {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (event::Status, Option<Message>) {
        let Some(cursor) = cursor.position_in(bounds) else {
            return (event::Status::Ignored, None);
        };
        let fit = fitted_rect(self.image, bounds.size());

        match event {
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let t = match self.orientation {
                    Orientation::Vertical => (cursor.x - fit.x) / fit.width,
                    Orientation::Horizontal => (cursor.y - fit.y) / fit.height,
                };
                (
                    event::Status::Captured,
                    Some(Message::KnifeHover(t.clamp(0.0, 1.0))),
                )
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if fit.contains(cursor) =>
            {
                (event::Status::Captured, Some(Message::KnifeCut))
            }
            _ => (event::Status::Ignored, None),
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let fit = fitted_rect(self.image, bounds.size());

        let (from, to, label_at) = match self.orientation {
            Orientation::Vertical => {
                let x = fit.x + fit.width * self.position;
                (
                    Point::new(x, fit.y),
                    Point::new(x, fit.y + fit.height),
                    Point::new(x + 8.0, fit.y + 8.0),
                )
            }
            Orientation::Horizontal => {
                let y = fit.y + fit.height * self.position;
                (
                    Point::new(fit.x, y),
                    Point::new(fit.x + fit.width, y),
                    Point::new(fit.x + 8.0, y + 8.0),
                )
            }
        };

        let line = Path::line(from, to);
        // Soft glow underneath, crisp white line on top.
        frame.stroke(
            &line,
            Stroke::default()
                .with_color(Color {
                    r: 0.42,
                    g: 0.72,
                    b: 1.0,
                    a: 0.35,
                })
                .with_width(7.0),
        );
        frame.stroke(
            &line,
            Stroke::default().with_color(Color::WHITE).with_width(1.5),
        );

        if let Some(px) = self.pixel() {
            frame.fill_text(Text {
                content: format!("{px} px"),
                position: label_at,
                color: Color::WHITE,
                size: 12.0.into(),
                ..Text::default()
            });
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }
}
