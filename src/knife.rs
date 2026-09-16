//! The knife overlay: a transparent canvas stacked on top of the image.
//!
//! It draws
//! * three pixel-thin "difference" lines (image colours inverted, so they are
//!   visible on any background): one exactly on the cut boundary and one a
//!   physical pixel away on each side;
//! * a magnifier straddling the cut: the 6 image pixels along the line around
//!   the cursor, split into the 3 rows/columns that end up in the first piece and
//!   the 3 that end up in the second, with the cut running through the gap.
//!   It follows the mouse along the line.
//!
//! Everything is expressed in (`along`, `across`) coordinates: `along` runs
//! parallel to the cut line, `across` is perpendicular to it. For a horizontal
//! knife that is (x, y); for a vertical knife it is (y, x).

use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke, Text};
use iced::{event, Color, Point, Rectangle, Renderer, Size, Theme};
use image::{Rgba, RgbaImage};

use crate::ops::{self, Orientation};
use crate::Message;

/// Screen size (logical px) of one magnified image pixel.
const ZOOM: f32 = 12.0;
/// Magnifier footprint in image pixels: `SPAN` along the cut, `DEPTH` on each side of it.
const SPAN: i64 = 6;
const DEPTH: i64 = 3;
/// Gap between the two magnifier halves — the cut runs through it.
const GAP: f32 = 6.0;
/// Cap on the number of segments a difference line is split into (keeps
/// tessellation cheap on very wide displays).
const MAX_SEGMENTS: i64 = 1200;

pub struct Overlay<'a> {
    /// The image underneath (first frame), for sampling colours.
    pub pixels: &'a RgbaImage,
    pub orientation: Orientation,
    /// Normalised knife position across the cut axis, 0.0..=1.0.
    pub across: f32,
    /// Normalised cursor position along the cut line, 0.0..=1.0.
    pub along: f32,
    /// Physical pixels per logical pixel, for pixel-thin lines on HiDPI screens.
    pub scale_factor: f32,
}

/// Where an image of `image` size lands inside `bounds` under `ContentFit::Contain`,
/// centred — this mirrors the iced image widget so the overlay lines up with pixels.
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

fn point(o: Orientation, along: f32, across: f32) -> Point {
    match o {
        Orientation::Horizontal => Point::new(along, across),
        Orientation::Vertical => Point::new(across, along),
    }
}

fn size(o: Orientation, along: f32, across: f32) -> Size {
    match o {
        Orientation::Horizontal => Size::new(along, across),
        Orientation::Vertical => Size::new(across, along),
    }
}

/// Difference against white: the inverted colour. Transparent pixels show the
/// dark stage underneath, so white is the visible choice there.
fn difference(p: Rgba<u8>) -> Color {
    if p[3] == 0 {
        Color::WHITE
    } else {
        Color::from_rgb8(255 - p[0], 255 - p[1], 255 - p[2])
    }
}

/// Screen geometry of the fitted image in (along, across) terms.
struct Geo {
    /// Screen origin of the image along / across the cut.
    along0: f32,
    across0: f32,
    /// Screen length of the image along the cut.
    along_len: f32,
    /// Screen length of the canvas along the cut.
    bounds_along: f32,
    /// Screen px per image px (same on both axes).
    scale: f32,
    /// Physical px per logical px.
    dpi: f32,
}

impl Overlay<'_> {
    fn image_size(&self) -> Size {
        Size::new(self.pixels.width() as f32, self.pixels.height() as f32)
    }

    /// Image extent (along, across) in pixels.
    fn extents(&self) -> (u32, u32) {
        let (w, h) = self.pixels.dimensions();
        match self.orientation {
            Orientation::Horizontal => (w, h),
            Orientation::Vertical => (h, w),
        }
    }

    /// Image-space boundary the cut falls on.
    fn cut(&self) -> Option<u32> {
        ops::knife_pixel(self.extents().1, self.across)
    }

    fn sample(&self, along: i64, across: i64) -> Option<Rgba<u8>> {
        let (x, y) = match self.orientation {
            Orientation::Horizontal => (along, across),
            Orientation::Vertical => (across, along),
        };
        let (w, h) = self.pixels.dimensions();
        if x < 0 || y < 0 || x >= i64::from(w) || y >= i64::from(h) {
            None
        } else {
            Some(*self.pixels.get_pixel(x as u32, y as u32))
        }
    }

    fn geo(&self, bounds: Rectangle) -> Geo {
        let image = self.image_size();
        let fit = fitted_rect(image, bounds.size());
        let (along0, across0, along_len, bounds_along) = match self.orientation {
            Orientation::Horizontal => (fit.x, fit.y, fit.width, bounds.width),
            Orientation::Vertical => (fit.y, fit.x, fit.height, bounds.height),
        };
        let dpi = if self.scale_factor.is_finite() && self.scale_factor > 0.0 {
            self.scale_factor
        } else {
            1.0
        };
        Geo {
            along0,
            across0,
            along_len,
            bounds_along,
            scale: fit.width / image.width,
            dpi,
        }
    }

    /// One physical-pixel-thin line at physical row/column `across_phys`, coloured
    /// with the inverse of the image underneath, drawn as runs of equal colour.
    fn difference_line(&self, frame: &mut Frame, g: &Geo, across_phys: i64) {
        let o = self.orientation;
        let px = 1.0 / g.dpi;
        let across = across_phys as f32 * px;
        let across_img = ((across + px * 0.5 - g.across0) / g.scale).floor() as i64;

        let first = (g.along0 * g.dpi).round() as i64;
        let last = ((g.along0 + g.along_len) * g.dpi).round() as i64;
        if last <= first {
            return;
        }
        let step = ((last - first) + MAX_SEGMENTS - 1) / MAX_SEGMENTS;

        let mut run_start = first;
        let mut run: Option<Rgba<u8>> = None;
        let mut p = first;
        while p < last {
            let seg_end = (p + step).min(last);
            let centre = (p + seg_end) as f32 * 0.5 * px;
            let along_img = ((centre - g.along0) / g.scale).floor() as i64;
            let sample = self.sample(along_img, across_img);
            if sample != run {
                if let Some(pixel) = run {
                    frame.fill_rectangle(
                        point(o, run_start as f32 * px, across),
                        size(o, (p - run_start) as f32 * px, px),
                        difference(pixel),
                    );
                }
                run_start = p;
                run = sample;
            }
            p = seg_end;
        }
        if let Some(pixel) = run {
            frame.fill_rectangle(
                point(o, run_start as f32 * px, across),
                size(o, (last - run_start) as f32 * px, px),
                difference(pixel),
            );
        }
    }

    /// One magnifier half: `SPAN × DEPTH` image pixels starting at
    /// (`first_along`, `first_across`), drawn at (`along`, `across`) on screen.
    fn block(
        &self,
        frame: &mut Frame,
        along: f32,
        across: f32,
        first_along: i64,
        first_across: i64,
    ) {
        let o = self.orientation;
        let size_along = SPAN as f32 * ZOOM;
        let size_across = DEPTH as f32 * ZOOM;
        let origin = point(o, along, across);
        let extent = size(o, size_along, size_across);

        frame.fill_rectangle(
            origin,
            extent,
            Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.75,
            },
        );

        for i in 0..SPAN {
            for j in 0..DEPTH {
                if let Some(p) = self.sample(first_along + i, first_across + j) {
                    frame.fill_rectangle(
                        point(o, along + i as f32 * ZOOM, across + j as f32 * ZOOM),
                        Size::new(ZOOM, ZOOM),
                        Color::from_rgba8(p[0], p[1], p[2], f32::from(p[3]) / 255.0),
                    );
                }
            }
        }

        // Hairlines between the cells make single pixels easy to count.
        let grid = Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.35,
        };
        for i in 1..SPAN {
            frame.fill_rectangle(
                point(o, along + i as f32 * ZOOM - 0.5, across),
                size(o, 1.0, size_across),
                grid,
            );
        }
        for j in 1..DEPTH {
            frame.fill_rectangle(
                point(o, along, across + j as f32 * ZOOM - 0.5),
                size(o, size_along, 1.0),
                grid,
            );
        }

        frame.stroke(
            &Path::rectangle(origin, extent),
            Stroke::default()
                .with_color(Color {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.85,
                })
                .with_width(1.0),
        );
    }

    fn label(&self, frame: &mut Frame, at: Point, content: String) {
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

impl canvas::Program<Message> for Overlay<'_> {
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
        let fit = fitted_rect(self.image_size(), bounds.size());

        match event {
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let (along, across) = match self.orientation {
                    Orientation::Horizontal => (
                        (cursor.x - fit.x) / fit.width,
                        (cursor.y - fit.y) / fit.height,
                    ),
                    Orientation::Vertical => (
                        (cursor.y - fit.y) / fit.height,
                        (cursor.x - fit.x) / fit.width,
                    ),
                };
                (
                    event::Status::Captured,
                    Some(Message::KnifeHover {
                        across: across.clamp(0.0, 1.0),
                        along: along.clamp(0.0, 1.0),
                    }),
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
        let Some(cut) = self.cut() else {
            return vec![frame.into_geometry()];
        };
        let g = self.geo(bounds);
        let o = self.orientation;

        // The cut boundary on screen, snapped to the physical pixel grid.
        let boundary = g.across0 + cut as f32 * g.scale;
        let boundary_phys = (boundary * g.dpi).round() as i64;

        // 1. Three pixel-thin difference lines: on the boundary, and one pixel
        //    away on either side (so the image shows through the 1 px gaps).
        for offset in [-2, 0, 2] {
            self.difference_line(&mut frame, &g, boundary_phys + offset);
        }

        // 2. The magnifier, centred on the cursor's position along the line.
        let (along_px, _) = self.extents();
        let a_idx = ((self.along.clamp(0.0, 1.0) * along_px as f32).floor() as i64)
            .clamp(0, i64::from(along_px) - 1);
        let half = SPAN as f32 * ZOOM / 2.0;
        let mut centre = g.along0 + (a_idx as f32 + 0.5) * g.scale;
        if g.bounds_along > 2.0 * half + 8.0 {
            centre = centre.clamp(half + 4.0, g.bounds_along - half - 4.0);
        }
        let start_along = centre - half;
        let first_cell = a_idx - SPAN / 2;
        let cut = i64::from(cut);

        // First piece: the last DEPTH rows/columns before the cut.
        self.block(
            &mut frame,
            start_along,
            boundary - GAP / 2.0 - DEPTH as f32 * ZOOM,
            first_cell,
            cut - DEPTH,
        );
        // Second piece: the first DEPTH rows/columns after it.
        self.block(&mut frame, start_along, boundary + GAP / 2.0, first_cell, cut);

        // 3. Pixel readout beside the magnifier.
        let axis = match o {
            Orientation::Horizontal => "y",
            Orientation::Vertical => "x",
        };
        self.label(
            &mut frame,
            point(o, start_along + SPAN as f32 * ZOOM + 8.0, boundary - 7.0),
            format!("{axis} = {cut} px"),
        );

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
