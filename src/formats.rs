//! Decoding and encoding for the supported formats.
//!
//! PNG, JPEG and GIF go through the `image` crate; JPEG XL goes through
//! `jpegxl-rs` (libjxl) when the `jxl` feature is on.

use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::widget::image as iced_image;
use image::{AnimationDecoder, RgbaImage};

/// Extensions ExeyVue opens (lower-case, no dot).
pub const EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "jxl"];

/// One displayable frame. Still images have exactly one.
#[derive(Debug, Clone)]
pub struct Frame {
    pub handle: iced_image::Handle,
    pub delay: Duration,
}

/// A decoded image ready for viewing and editing.
#[derive(Debug, Clone)]
pub struct Picture {
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// Pixels the edit tools operate on (the first frame for animations).
    pub pixels: RgbaImage,
    pub frames: Vec<Frame>,
    /// File this image was loaded from, if any — Save defaults to the same
    /// file and format (and so the same compression) when this is set.
    pub source: Option<PathBuf>,
}

impl Picture {
    pub fn from_rgba(name: impl Into<String>, pixels: RgbaImage) -> Self {
        let (width, height) = pixels.dimensions();
        let frame = Frame {
            handle: handle_for(&pixels),
            delay: Duration::ZERO,
        };
        Self {
            name: name.into(),
            width,
            height,
            pixels,
            frames: vec![frame],
            source: None,
        }
    }
}

/// GPU-uploadable handle for an RGBA buffer.
pub fn handle_for(img: &RgbaImage) -> iced_image::Handle {
    iced_image::Handle::from_rgba(img.width(), img.height(), img.as_raw().clone())
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn is_supported(path: &Path) -> bool {
    let ext = extension(path);
    if ext == "jxl" {
        return cfg!(feature = "jxl");
    }
    EXTENSIONS.contains(&ext.as_str())
}

pub fn is_jxl(path: &Path) -> bool {
    extension(path) == "jxl"
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// All supported images in the same folder as `path`, sorted by name.
/// Always contains `path` itself so navigation has somewhere to start.
pub fn siblings(path: &Path) -> Vec<PathBuf> {
    let mut list: Vec<PathBuf> = path
        .parent()
        .and_then(|dir| std::fs::read_dir(dir).ok())
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file() && is_supported(p))
                .collect()
        })
        .unwrap_or_default();
    list.sort_by_key(|p| file_name(p).to_lowercase());
    if !list.iter().any(|p| p == path) {
        list.insert(0, path.to_path_buf());
    }
    list
}

pub fn load(path: &Path) -> Result<Picture, String> {
    let name = file_name(path);
    let mut picture = match extension(path).as_str() {
        "jxl" => load_jxl(path, name)?,
        "gif" => load_gif(path, name)?,
        _ => {
            let image = image::ImageReader::open(path)
                .map_err(|e| e.to_string())?
                .with_guessed_format()
                .map_err(|e| e.to_string())?
                .decode()
                .map_err(|e| e.to_string())?;
            Picture::from_rgba(name, image.into_rgba8())
        }
    };
    picture.source = Some(path.to_path_buf());
    Ok(picture)
}

fn load_gif(path: &Path, name: String) -> Result<Picture, String> {
    use image::codecs::gif::GifDecoder;

    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let decoder = GifDecoder::new(std::io::BufReader::new(file)).map_err(|e| e.to_string())?;
    let frames = decoder
        .into_frames()
        .collect_frames()
        .map_err(|e| e.to_string())?;

    let mut first: Option<RgbaImage> = None;
    let mut out = Vec::with_capacity(frames.len());
    for frame in frames {
        let (numer, denom) = frame.delay().numer_denom_ms();
        let ms = if denom == 0 { 100 } else { numer / denom };
        // Browsers treat "no delay" as 100 ms; follow that convention.
        let ms = if ms < 20 { 100 } else { ms };
        let buffer = frame.into_buffer();
        let handle = handle_for(&buffer);
        if first.is_none() {
            first = Some(buffer);
        }
        out.push(Frame {
            handle,
            delay: Duration::from_millis(u64::from(ms)),
        });
    }
    let pixels = first.ok_or_else(|| "GIF contains no frames".to_string())?;
    let (width, height) = pixels.dimensions();
    Ok(Picture {
        name,
        width,
        height,
        pixels,
        frames: out,
        source: None,
    })
}

/// Encoder settings for a JPEG XL save.
#[derive(Debug, Clone, Copy)]
pub struct JxlOptions {
    /// JPEG-style quality factor, 0..=100, higher is better. Ignored when
    /// `lossless` is set.
    pub quality: f32,
    /// True pixel-exact encoding.
    pub lossless: bool,
}

impl Default for JxlOptions {
    fn default() -> Self {
        Self {
            quality: 90.0,
            lossless: false,
        }
    }
}

/// Write `img` to `path`, choosing the codec from the extension (PNG when there is none).
/// `jxl` only matters when the target is JPEG XL. Returns the path actually written.
pub fn save(img: &RgbaImage, path: &Path, jxl: JxlOptions) -> Result<PathBuf, String> {
    let mut path = path.to_path_buf();
    if extension(&path).is_empty() {
        path.set_extension("png");
    }
    match extension(&path).as_str() {
        "png" | "gif" => img.save(&path).map_err(|e| e.to_string())?,
        "jpg" | "jpeg" => flatten_on_white(img)
            .save(&path)
            .map_err(|e| e.to_string())?,
        "jxl" => save_jxl(img, &path, jxl)?,
        other => return Err(format!("unsupported output format .{other}")),
    }
    Ok(path)
}

/// JPEG has no alpha; composite onto white like most editors do.
fn flatten_on_white(img: &RgbaImage) -> image::RgbImage {
    let mut out = image::RgbImage::new(img.width(), img.height());
    for (dst, src) in out.pixels_mut().zip(img.pixels()) {
        let a = u32::from(src[3]);
        for c in 0..3 {
            dst[c] = ((u32::from(src[c]) * a + 255 * (255 - a)) / 255) as u8;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// JPEG XL
// ---------------------------------------------------------------------------

#[cfg(feature = "jxl")]
fn load_jxl(path: &Path, name: String) -> Result<Picture, String> {
    use jpegxl_rs::decode::Pixels;

    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let runner = jpegxl_rs::ThreadsRunner::default();
    #[allow(unused_mut)]
    let mut decoder = jpegxl_rs::decoder_builder()
        .parallel_runner(&runner)
        .build()
        .map_err(|e| e.to_string())?;
    let (meta, pixels) = decoder.decode(&data).map_err(|e| e.to_string())?;

    let bytes: Vec<u8> = match pixels {
        Pixels::Uint8(v) => v,
        Pixels::Uint16(v) => v.into_iter().map(|x| (x >> 8) as u8).collect(),
        Pixels::Float(v) => v.into_iter().map(unit_to_u8).collect(),
        Pixels::Float16(v) => v.into_iter().map(|x| unit_to_u8(f32::from(x))).collect(),
    };
    let rgba = to_rgba(bytes, meta.width, meta.height)?;
    Ok(Picture::from_rgba(name, rgba))
}

#[cfg(feature = "jxl")]
fn unit_to_u8(x: f32) -> u8 {
    (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// libjxl hands back 1–4 interleaved channels depending on the file; normalise to RGBA.
#[cfg(feature = "jxl")]
fn to_rgba(bytes: Vec<u8>, width: u32, height: u32) -> Result<RgbaImage, String> {
    let n = width as usize * height as usize;
    if n == 0 || bytes.len() % n != 0 {
        return Err("JPEG XL decoder returned an unexpected buffer size".into());
    }
    let rgba: Vec<u8> = match bytes.len() / n {
        4 => bytes,
        3 => bytes
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        2 => bytes
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        1 => bytes.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        c => return Err(format!("JPEG XL image with {c} channels is not supported")),
    };
    RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| "JPEG XL buffer does not match its dimensions".into())
}

#[cfg(feature = "jxl")]
fn save_jxl(img: &RgbaImage, path: &Path, opts: JxlOptions) -> Result<(), String> {
    use jpegxl_rs::encode::{EncoderFrame, EncoderResult};

    let runner = jpegxl_rs::ThreadsRunner::default();
    let mut builder = jpegxl_rs::encoder_builder();
    builder.parallel_runner(&runner).has_alpha(true);
    if opts.lossless {
        builder.lossless(true);
    } else {
        builder.jpeg_quality(opts.quality);
    }
    let mut encoder = builder.build().map_err(|e| e.to_string())?;
    let frame = EncoderFrame::new(img.as_raw().as_slice()).num_channels(4);
    let encoded: EncoderResult<u8> = encoder
        .encode_frame(&frame, img.width(), img.height())
        .map_err(|e| e.to_string())?;
    std::fs::write(path, &encoded.data).map_err(|e| e.to_string())
}

#[cfg(not(feature = "jxl"))]
fn load_jxl(_path: &Path, _name: String) -> Result<Picture, String> {
    Err("this build has no JPEG XL support (enable the `jxl` feature)".into())
}

#[cfg(not(feature = "jxl"))]
fn save_jxl(_img: &RgbaImage, _path: &Path, _opts: JxlOptions) -> Result<(), String> {
    Err("this build has no JPEG XL support (enable the `jxl` feature)".into())
}
