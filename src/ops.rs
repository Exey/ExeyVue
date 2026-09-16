//! Pure image operations — no UI code here, so everything is unit-testable.
//!
//! * [`split`]  — the knife: one straight horizontal or vertical cut.
//! * [`merge`]  — combine any number of images horizontally, vertically or in a grid.

use std::borrow::Cow;
use std::fmt;

use image::imageops::{self, FilterType};
use image::{Rgba, RgbaImage};

/// Direction of the knife line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Orientation {
    /// A vertical line: the image is split into a left and a right piece.
    #[default]
    Vertical,
    /// A horizontal line: the image is split into a top and a bottom piece.
    Horizontal,
}

impl Orientation {
    pub const ALL: [Orientation; 2] = [Orientation::Vertical, Orientation::Horizontal];

    pub fn label(self) -> &'static str {
        match self {
            Orientation::Vertical => "Vertical",
            Orientation::Horizontal => "Horizontal",
        }
    }

    /// Short suffixes for the two pieces a cut produces.
    pub fn piece_labels(self) -> (&'static str, &'static str) {
        match self {
            Orientation::Vertical => ("L", "R"),
            Orientation::Horizontal => ("T", "B"),
        }
    }
}

/// How pieces are laid out when merging.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MergeDirection {
    Horizontal,
    #[default]
    Vertical,
    Grid,
}

impl MergeDirection {
    pub const ALL: [MergeDirection; 3] = [
        MergeDirection::Horizontal,
        MergeDirection::Vertical,
        MergeDirection::Grid,
    ];
}

impl fmt::Display for MergeDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            MergeDirection::Horizontal => "Horizontal",
            MergeDirection::Vertical => "Vertical",
            MergeDirection::Grid => "Grid",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MergeSettings {
    pub direction: MergeDirection,
    /// "Image Fill to biggest resolution": scale every piece up (aspect ratio kept)
    /// so it fills the largest width (vertical), height (horizontal) or cell (grid).
    /// When off, smaller pieces are centred on transparent padding instead.
    pub fill: bool,
    /// Grid columns; `0` means automatic (`ceil(sqrt(n))`).
    pub columns: u8,
}

impl Default for MergeSettings {
    fn default() -> Self {
        Self {
            direction: MergeDirection::Vertical,
            fill: true,
            columns: 0,
        }
    }
}

/// Pixel offset of a knife placed at normalised position `t` (0..=1) along an
/// edge of length `len`, clamped so that both resulting pieces are non-empty.
pub fn knife_pixel(len: u32, t: f32) -> Option<u32> {
    if len < 2 {
        return None;
    }
    let px = (t.clamp(0.0, 1.0) * len as f32).round() as u32;
    Some(px.clamp(1, len - 1))
}

/// Cut `img` once. Returns `(first, second)` = (left, right) or (top, bottom).
pub fn split(img: &RgbaImage, orientation: Orientation, t: f32) -> Option<(RgbaImage, RgbaImage)> {
    let (w, h) = img.dimensions();
    match orientation {
        Orientation::Vertical => {
            let x = knife_pixel(w, t)?;
            Some((
                imageops::crop_imm(img, 0, 0, x, h).to_image(),
                imageops::crop_imm(img, x, 0, w - x, h).to_image(),
            ))
        }
        Orientation::Horizontal => {
            let y = knife_pixel(h, t)?;
            Some((
                imageops::crop_imm(img, 0, 0, w, y).to_image(),
                imageops::crop_imm(img, 0, y, w, h - y).to_image(),
            ))
        }
    }
}

/// Number of grid columns chosen automatically for `n` pieces.
pub fn auto_columns(n: usize) -> usize {
    ((n as f64).sqrt().ceil() as usize).max(1)
}

/// Merge `images` (in order) according to `settings`. `None` if there is nothing to merge.
pub fn merge(images: &[&RgbaImage], settings: &MergeSettings) -> Option<RgbaImage> {
    if images.is_empty() {
        return None;
    }
    let max_w = images.iter().map(|i| i.width()).max()?;
    let max_h = images.iter().map(|i| i.height()).max()?;

    match settings.direction {
        MergeDirection::Vertical => {
            let prepared: Vec<Cow<'_, RgbaImage>> = images
                .iter()
                .map(|img| {
                    if settings.fill {
                        scale_to_width(img, max_w)
                    } else {
                        Cow::Borrowed(*img)
                    }
                })
                .collect();
            let total_h: u32 = prepared.iter().map(|i| i.height()).sum();
            let mut out = blank(max_w, total_h);
            let mut y = 0u32;
            for img in &prepared {
                let x = (max_w - img.width()) / 2;
                imageops::replace(&mut out, &**img, i64::from(x), i64::from(y));
                y += img.height();
            }
            Some(out)
        }
        MergeDirection::Horizontal => {
            let prepared: Vec<Cow<'_, RgbaImage>> = images
                .iter()
                .map(|img| {
                    if settings.fill {
                        scale_to_height(img, max_h)
                    } else {
                        Cow::Borrowed(*img)
                    }
                })
                .collect();
            let total_w: u32 = prepared.iter().map(|i| i.width()).sum();
            let mut out = blank(total_w, max_h);
            let mut x = 0u32;
            for img in &prepared {
                let y = (max_h - img.height()) / 2;
                imageops::replace(&mut out, &**img, i64::from(x), i64::from(y));
                x += img.width();
            }
            Some(out)
        }
        MergeDirection::Grid => {
            let n = images.len();
            let cols = if settings.columns == 0 {
                auto_columns(n)
            } else {
                usize::from(settings.columns)
            }
            .clamp(1, n);
            let rows = n.div_ceil(cols);
            let mut out = blank(max_w * cols as u32, max_h * rows as u32);
            for (i, img) in images.iter().enumerate() {
                let img = if settings.fill {
                    scale_to_fit(img, max_w, max_h)
                } else {
                    Cow::Borrowed(*img)
                };
                let col = (i % cols) as u32;
                let row = (i / cols) as u32;
                let x = col * max_w + (max_w - img.width()) / 2;
                let y = row * max_h + (max_h - img.height()) / 2;
                imageops::replace(&mut out, &*img, i64::from(x), i64::from(y));
            }
            Some(out)
        }
    }
}

fn blank(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_pixel(w.max(1), h.max(1), Rgba([0, 0, 0, 0]))
}

fn resized(img: &RgbaImage, w: u32, h: u32) -> Cow<'_, RgbaImage> {
    let (w, h) = (w.max(1), h.max(1));
    if img.width() == w && img.height() == h {
        Cow::Borrowed(img)
    } else {
        Cow::Owned(imageops::resize(img, w, h, FilterType::Lanczos3))
    }
}

fn scale_to_width(img: &RgbaImage, w: u32) -> Cow<'_, RgbaImage> {
    let h = (f64::from(img.height()) * f64::from(w) / f64::from(img.width())).round() as u32;
    resized(img, w, h)
}

fn scale_to_height(img: &RgbaImage, h: u32) -> Cow<'_, RgbaImage> {
    let w = (f64::from(img.width()) * f64::from(h) / f64::from(img.height())).round() as u32;
    resized(img, w, h)
}

/// Largest size that fits inside `cell_w × cell_h` while keeping the aspect ratio.
fn scale_to_fit(img: &RgbaImage, cell_w: u32, cell_h: u32) -> Cow<'_, RgbaImage> {
    let s = (f64::from(cell_w) / f64::from(img.width()))
        .min(f64::from(cell_h) / f64::from(img.height()));
    let w = (f64::from(img.width()) * s).round() as u32;
    let h = (f64::from(img.height()) * s).round() as u32;
    resized(img, w.min(cell_w), h.min(cell_h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, v: u8) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([v, v, v, 255]))
    }

    #[test]
    fn split_vertical_uses_rounded_pixel_position() {
        let img = solid(10, 4, 7);
        let (a, b) = split(&img, Orientation::Vertical, 0.3).unwrap();
        assert_eq!(a.dimensions(), (3, 4));
        assert_eq!(b.dimensions(), (7, 4));
    }

    #[test]
    fn split_clamps_to_non_empty_pieces() {
        let img = solid(10, 4, 0);
        let (a, b) = split(&img, Orientation::Horizontal, 0.0).unwrap();
        assert_eq!((a.height(), b.height()), (1, 3));
        let (a, b) = split(&img, Orientation::Horizontal, 1.0).unwrap();
        assert_eq!((a.height(), b.height()), (3, 1));
        assert!(split(&solid(1, 4, 0), Orientation::Vertical, 0.5).is_none());
    }

    #[test]
    fn split_then_merge_restores_dimensions() {
        let img = solid(64, 32, 9);
        let (a, b) = split(&img, Orientation::Vertical, 0.5).unwrap();
        let settings = MergeSettings {
            direction: MergeDirection::Horizontal,
            fill: false,
            columns: 0,
        };
        let merged = merge(&[&a, &b], &settings).unwrap();
        assert_eq!(merged.dimensions(), (64, 32));
        assert_eq!(merged.get_pixel(63, 31)[0], 9);
    }

    #[test]
    fn vertical_merge_fill_scales_to_widest() {
        let a = solid(100, 50, 1);
        let b = solid(50, 50, 2);
        let m = merge(&[&a, &b], &MergeSettings::default()).unwrap();
        assert_eq!(m.dimensions(), (100, 150));
    }

    #[test]
    fn vertical_merge_without_fill_pads_transparently() {
        let a = solid(100, 50, 1);
        let b = solid(50, 50, 2);
        let settings = MergeSettings {
            fill: false,
            ..MergeSettings::default()
        };
        let m = merge(&[&a, &b], &settings).unwrap();
        assert_eq!(m.dimensions(), (100, 100));
        assert_eq!(m.get_pixel(0, 75)[3], 0, "padding is transparent");
        assert_eq!(m.get_pixel(50, 75)[0], 2, "small piece is centred");
    }

    #[test]
    fn horizontal_merge_fill_scales_to_tallest() {
        let a = solid(50, 100, 1);
        let b = solid(50, 50, 2);
        let settings = MergeSettings {
            direction: MergeDirection::Horizontal,
            ..MergeSettings::default()
        };
        let m = merge(&[&a, &b], &settings).unwrap();
        assert_eq!(m.dimensions(), (150, 100));
    }

    #[test]
    fn grid_auto_columns_is_ceil_sqrt() {
        assert_eq!(auto_columns(1), 1);
        assert_eq!(auto_columns(4), 2);
        assert_eq!(auto_columns(5), 3);
        let imgs = vec![solid(10, 10, 1); 5];
        let refs: Vec<&RgbaImage> = imgs.iter().collect();
        let settings = MergeSettings {
            direction: MergeDirection::Grid,
            fill: true,
            columns: 0,
        };
        let m = merge(&refs, &settings).unwrap();
        assert_eq!(m.dimensions(), (30, 20));
    }

    #[test]
    fn grid_respects_explicit_columns() {
        let imgs = vec![solid(4, 4, 1); 6];
        let refs: Vec<&RgbaImage> = imgs.iter().collect();
        let settings = MergeSettings {
            direction: MergeDirection::Grid,
            fill: true,
            columns: 2,
        };
        let m = merge(&refs, &settings).unwrap();
        assert_eq!(m.dimensions(), (8, 12));
    }
}
