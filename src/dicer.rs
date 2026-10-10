//! The dicer overlay: two movable, resizable rectangles stacked on the image.
//!
//! Rectangles live in normalised image coordinates ([`NormRect`]), so they stay
//! on the same pixels when the window is resized. Dragging a rectangle's body
//! moves it, dragging an edge or corner resizes it. The overlay only reports the
//! new rectangles; the app owns them and does the actual cutting.

use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke, Text};
use iced::{event, Color, Point, Rectangle, Renderer, Size, Theme};

use crate::knife::fitted_rect;
use crate::ops::NormRect;
use crate::Message;

/// How far (screen px) outside an edge a grab still counts.
const GRAB: f32 = 8.0;
/// Smallest rectangle side on screen.
const MIN_SCREEN: f32 = 14.0;
/// Edge length of the square resize handles.
const HANDLE: f32 = 8.0;

/// Outline colour of rectangle 1 and 2.
pub const COLORS: [Color; 2] = [
    Color {
        r: 0.42,
        g: 0.72,
        b: 1.0,
        a: 1.0,
    },
    Color {
        r: 1.0,
        g: 0.64,
        b: 0.26,
        a: 1.0,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Grip {
    Move,
    Resize {
        left: bool,
        right: bool,
        top: bool,
        bottom: bool,
    },
}

#[derive(Debug, Clone, Copy)]
struct Drag {
    index: usize,
    grip: Grip,
    /// Cursor position when the drag started, in normalised image coordinates.
    start: Point,
    /// The rectangle as it was when the drag started.
    rect: NormRect,
}

#[derive(Debug, Default)]
pub struct State {
    drag: Option<Drag>,
    hover: Option<Grip>,
}

pub struct Overlay {
    /// Size of the image underneath, in pixels.
    pub image: (u32, u32),
    pub rects: [NormRect; 2],
    /// The rectangle drawn on top and grabbed first where they overlap.
    pub active: usize,
}

fn screen_rect(r: NormRect, fit: Rectangle) -> Rectangle {
    Rectangle {
        x: fit.x + r.x * fit.width,
        y: fit.y + r.y * fit.height,
        width: r.w * fit.width,
        height: r.h * fit.height,
    }
}

/// `v` limited to `lo..=hi`; unlike `f32::clamp` it tolerates `hi < lo`.
fn clamp_to(v: f32, lo: f32, hi: f32) -> f32 {
    v.max(lo).min(hi)
}

/// What a press at `p` would grab on screen rectangle `r`.
fn grip_at(r: Rectangle, p: Point) -> Option<Grip> {
    if p.x < r.x - GRAB
        || p.x > r.x + r.width + GRAB
        || p.y < r.y - GRAB
        || p.y > r.y + r.height + GRAB
    {
        return None;
    }
    // Keep some interior on small rectangles so they can still be moved.
    let near = |p: f32, lo: f32, hi: f32, tolerance: f32| {
        let (to_lo, to_hi) = ((p - lo).abs(), (p - hi).abs());
        (
            to_lo <= tolerance && to_lo <= to_hi,
            to_hi <= tolerance && to_hi < to_lo,
        )
    };
    let (left, right) = near(p.x, r.x, r.x + r.width, GRAB.min(r.width / 3.0));
    let (top, bottom) = near(p.y, r.y, r.y + r.height, GRAB.min(r.height / 3.0));
    if left || right || top || bottom {
        Some(Grip::Resize {
            left,
            right,
            top,
            bottom,
        })
    } else {
        Some(Grip::Move)
    }
}

/// The rectangle after dragging `drag` to cursor position `now`, keeping at
/// least `min` (normalised) per side and staying inside the image.
fn dragged(drag: &Drag, now: Point, min: (f32, f32)) -> NormRect {
    let (dx, dy) = (now.x - drag.start.x, now.y - drag.start.y);
    let r = drag.rect;
    match drag.grip {
        Grip::Move => NormRect {
            x: clamp_to(r.x + dx, 0.0, 1.0 - r.w),
            y: clamp_to(r.y + dy, 0.0, 1.0 - r.h),
            ..r
        },
        Grip::Resize {
            left,
            right,
            top,
            bottom,
        } => {
            let (mut x0, mut x1) = (r.x, r.x + r.w);
            let (mut y0, mut y1) = (r.y, r.y + r.h);
            if left {
                x0 = clamp_to(x0 + dx, 0.0, x1 - min.0);
            }
            if right {
                x1 = clamp_to(x1 + dx, x0 + min.0, 1.0);
            }
            if top {
                y0 = clamp_to(y0 + dy, 0.0, y1 - min.1);
            }
            if bottom {
                y1 = clamp_to(y1 + dy, y0 + min.1, 1.0);
            }
            NormRect {
                x: x0,
                y: y0,
                w: x1 - x0,
                h: y1 - y0,
            }
        }
    }
}

impl Overlay {
    fn image_size(&self) -> Size {
        Size::new(self.image.0 as f32, self.image.1 as f32)
    }

    /// Smallest rectangle (normalised) the user can shrink to.
    fn min_size(&self, fit: Rectangle) -> (f32, f32) {
        let side = |screen: f32, pixels: u32| {
            (MIN_SCREEN / screen.max(1.0))
                .max(1.0 / pixels.max(1) as f32)
                .min(0.5)
        };
        (side(fit.width, self.image.0), side(fit.height, self.image.1))
    }

    /// Topmost rectangle and grip under `p` (canvas coordinates).
    fn hit(&self, p: Point, fit: Rectangle) -> Option<(usize, Grip)> {
        [self.active, 1 - self.active]
            .into_iter()
            .find_map(|i| grip_at(screen_rect(self.rects[i], fit), p).map(|g| (i, g)))
    }

    fn label(frame: &mut Frame, at: Point, content: String) {
        frame.fill_text(Text {
            content: content.clone(),
            position: Point::new(at.x + 1.0, at.y + 1.0),
            color: Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.8,
            },
            size: 12.0.into(),
            ..Text::default()
        });
        frame.fill_text(Text {
            content,
            position: at,
            color: Color::WHITE,
            size: 12.0.into(),
            ..Text::default()
        });
    }
}

impl canvas::Program<Message> for Overlay {
    type State = State;

    fn update(
        &self,
        state: &mut Self::State,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (event::Status, Option<Message>) {
        // A release outside the window never arrives; don't keep dragging.
        if let canvas::Event::Mouse(mouse::Event::CursorLeft) = event {
            state.drag = None;
            state.hover = None;
            return (event::Status::Ignored, None);
        }
        // Not `position_in`: a drag keeps going when the cursor leaves the canvas.
        let Some(local) = cursor.position_from(bounds.position()) else {
            return (event::Status::Ignored, None);
        };
        let fit = fitted_rect(self.image_size(), bounds.size());
        if fit.width <= 0.0 || fit.height <= 0.0 {
            return (event::Status::Ignored, None);
        }
        let norm = Point::new((local.x - fit.x) / fit.width, (local.y - fit.y) / fit.height);

        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                match self.hit(local, fit) {
                    Some((index, grip)) => {
                        state.drag = Some(Drag {
                            index,
                            grip,
                            start: norm,
                            rect: self.rects[index],
                        });
                        (
                            event::Status::Captured,
                            Some(Message::DicerChanged {
                                rects: self.rects,
                                active: index,
                            }),
                        )
                    }
                    None => (event::Status::Ignored, None),
                }
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => match state.drag {
                Some(drag) => {
                    let mut rects = self.rects;
                    rects[drag.index] = dragged(&drag, norm, self.min_size(fit));
                    (
                        event::Status::Captured,
                        Some(Message::DicerChanged {
                            rects,
                            active: drag.index,
                        }),
                    )
                }
                None => {
                    state.hover = self.hit(local, fit).map(|(_, grip)| grip);
                    (event::Status::Ignored, None)
                }
            },
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.drag.take().is_some() {
                    (event::Status::Captured, None)
                } else {
                    (event::Status::Ignored, None)
                }
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
        let fit = fitted_rect(self.image_size(), bounds.size());

        // The active rectangle is drawn last, i.e. on top.
        for index in [1 - self.active, self.active] {
            let r = screen_rect(self.rects[index], fit);
            let color = COLORS[index];

            frame.fill_rectangle(r.position(), r.size(), Color { a: 0.14, ..color });
            frame.stroke(
                &Path::rectangle(r.position(), r.size()),
                Stroke::default().with_color(color).with_width(2.0),
            );

            let (right, bottom) = (r.x + r.width, r.y + r.height);
            let (mid_x, mid_y) = (r.x + r.width / 2.0, r.y + r.height / 2.0);
            let mut handles = vec![
                Point::new(r.x, r.y),
                Point::new(right, r.y),
                Point::new(r.x, bottom),
                Point::new(right, bottom),
            ];
            if r.width >= HANDLE * 3.0 {
                handles.push(Point::new(mid_x, r.y));
                handles.push(Point::new(mid_x, bottom));
            }
            if r.height >= HANDLE * 3.0 {
                handles.push(Point::new(r.x, mid_y));
                handles.push(Point::new(right, mid_y));
            }
            for p in handles {
                let origin = Point::new(p.x - HANDLE / 2.0, p.y - HANDLE / 2.0);
                let size = Size::new(HANDLE, HANDLE);
                frame.fill_rectangle(origin, size, Color::WHITE);
                frame.stroke(
                    &Path::rectangle(origin, size),
                    Stroke::default().with_color(color).with_width(1.5),
                );
            }

            let (_, _, w, h) = self.rects[index].to_pixels(self.image.0, self.image.1);
            Self::label(
                &mut frame,
                Point::new(r.x + 8.0, r.y + 8.0),
                format!("{} · {w}×{h}", index + 1),
            );
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        let dragging = state.drag.is_some();
        if !dragging && !cursor.is_over(bounds) {
            return mouse::Interaction::default();
        }
        match state.drag.map(|d| d.grip).or(state.hover) {
            Some(Grip::Move) if dragging => mouse::Interaction::Grabbing,
            Some(Grip::Move) => mouse::Interaction::Grab,
            Some(Grip::Resize {
                left,
                right,
                top,
                bottom,
            }) => {
                let horizontal = left || right;
                let vertical = top || bottom;
                match (horizontal, vertical) {
                    (true, false) => mouse::Interaction::ResizingHorizontally,
                    (false, true) => mouse::Interaction::ResizingVertically,
                    _ if (left && top) || (right && bottom) => {
                        mouse::Interaction::ResizingDiagonallyDown
                    }
                    _ => mouse::Interaction::ResizingDiagonallyUp,
                }
            }
            None => mouse::Interaction::default(),
        }
    }
}
