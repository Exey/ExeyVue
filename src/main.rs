//! ExeyVue — fast small images viewer and editor.
//!
//! One iced application (Elm-style state → update → view).
//! * `formats` decodes and encodes files,
//! * `ops` holds the pure image operations (knife + merge),
//! * `knife` is the canvas overlay that draws the knife line,
//! * `style` is the liquid-glass look.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod formats;
mod knife;
mod ops;
mod style;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use iced::widget::{
    button, canvas, center, checkbox, column, container, horizontal_space, image as iced_image,
    pick_list, row, scrollable, slider, stack, text,
};
use iced::{
    event, keyboard, time, window, Alignment, Color, ContentFit, Element, Event, Length,
    Subscription, Task, Theme,
};
use image::RgbaImage;

use formats::Picture;
use ops::{MergeDirection, MergeSettings, Orientation};

const APP_NAME: &str = "ExeyVue";

fn main() -> iced::Result {
    iced::application(ExeyVue::title, ExeyVue::update, ExeyVue::view)
        .subscription(ExeyVue::subscription)
        .theme(ExeyVue::theme)
        .style(ExeyVue::style)
        .window(window_settings())
        .antialiasing(true)
        .run_with(ExeyVue::new)
}

fn window_settings() -> window::Settings {
    window::Settings {
        size: iced::Size::new(1120.0, 760.0),
        min_size: Some(iced::Size::new(720.0, 480.0)),
        transparent: true,
        // Hide the title bar so the glass runs edge to edge; the traffic lights stay.
        #[cfg(target_os = "macos")]
        platform_specific: window::settings::PlatformSpecific {
            title_hidden: true,
            titlebar_transparent: true,
            fullsize_content_view: true,
        },
        ..window::Settings::default()
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    View,
    Knife,
    Merge,
}

impl Mode {
    const ALL: [Mode; 3] = [Mode::View, Mode::Knife, Mode::Merge];

    fn label(self) -> &'static str {
        match self {
            Mode::View => "View",
            Mode::Knife => "Knife",
            Mode::Merge => "Merge",
        }
    }
}

/// An image waiting in the tray to be merged (a knife piece or an added file).
#[derive(Debug, Clone)]
struct Piece {
    label: String,
    pixels: RgbaImage,
    handle: iced_image::Handle,
}

impl Piece {
    fn new(label: impl Into<String>, pixels: RgbaImage) -> Self {
        let handle = formats::handle_for(&pixels);
        Self {
            label: label.into(),
            pixels,
            handle,
        }
    }
}

struct ExeyVue {
    theme: Theme,
    mode: Mode,

    // Viewer
    playlist: Vec<PathBuf>,
    index: usize,
    current: Option<Arc<Picture>>,
    frame: usize,
    history: Vec<Arc<Picture>>,

    // Knife
    orientation: Orientation,
    knife_pos: f32,

    // Merge
    tray: Vec<Piece>,
    merge: MergeSettings,

    // Chrome
    glass: f32,
    status: String,
    busy: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    OpenPressed,
    OpenChosen(Option<Vec<PathBuf>>),
    Loaded(Result<Arc<Picture>, String>),
    FileDropped(PathBuf),
    Prev,
    Next,
    SetMode(Mode),

    SetOrientation(Orientation),
    KnifeHover(f32),
    KnifeCut,

    AddFilesPressed,
    AddFilesChosen(Option<Vec<PathBuf>>),
    TrayLoaded(Result<Arc<Picture>, String>),
    AddCurrentToTray,
    TrayMoveLeft(usize),
    TrayMoveRight(usize),
    TrayRemove(usize),
    TrayOpen(usize),
    TrayClear,
    SetDirection(MergeDirection),
    SetFill(bool),
    SetColumns(u8),
    Merge,
    Merged(Option<Arc<Picture>>),
    Undo,

    SavePressed,
    SaveChosen(Option<PathBuf>),
    Saved(Result<PathBuf, String>),

    Tick(Instant),
    SetGlass(f32),
}

impl ExeyVue {
    fn new() -> (Self, Task<Message>) {
        let mut app = Self {
            theme: style::theme(),
            mode: Mode::View,
            playlist: Vec::new(),
            index: 0,
            current: None,
            frame: 0,
            history: Vec::new(),
            orientation: Orientation::Vertical,
            knife_pos: 0.5,
            tray: Vec::new(),
            merge: MergeSettings::default(),
            glass: 0.6,
            status: String::from("Open an image, or drop one onto the window"),
            busy: false,
        };
        let task = match std::env::args_os().nth(1) {
            Some(arg) => app.open_path(PathBuf::from(arg)),
            None => Task::none(),
        };
        (app, task)
    }

    fn title(&self) -> String {
        match &self.current {
            Some(p) => format!("{} — {APP_NAME}", p.name),
            None => APP_NAME.to_owned(),
        }
    }

    fn theme(&self) -> Theme {
        self.theme.clone()
    }

    /// Transparent window background; the glass panels supply their own tint.
    fn style(&self, _theme: &Theme) -> iced::application::Appearance {
        iced::application::Appearance {
            background_color: Color::TRANSPARENT,
            text_color: Color::WHITE,
        }
    }

    // -- helpers ------------------------------------------------------------

    fn open_path(&mut self, path: PathBuf) -> Task<Message> {
        self.playlist = formats::siblings(&path);
        self.index = self.playlist.iter().position(|p| *p == path).unwrap_or(0);
        self.load_current()
    }

    fn load_current(&mut self) -> Task<Message> {
        let Some(path) = self.playlist.get(self.index).cloned() else {
            return Task::none();
        };
        self.busy = true;
        self.status = format!("Opening {}…", formats::file_name(&path));
        Task::perform(
            async move { formats::load(&path).map(Arc::new) },
            Message::Loaded,
        )
    }

    fn step(&mut self, delta: isize) -> Task<Message> {
        if self.playlist.len() < 2 {
            return Task::none();
        }
        let n = self.playlist.len() as isize;
        self.index = (self.index as isize + delta).rem_euclid(n) as usize;
        self.load_current()
    }

    fn load_into_tray(&mut self, paths: Vec<PathBuf>) -> Task<Message> {
        let paths: Vec<PathBuf> = paths
            .into_iter()
            .filter(|p| formats::is_supported(p))
            .collect();
        if paths.is_empty() {
            self.status = "No supported images to add".to_owned();
            return Task::none();
        }
        self.status = format!("Adding {} file(s) to the tray…", paths.len());
        Task::batch(paths.into_iter().map(|path| {
            Task::perform(
                async move { formats::load(&path).map(Arc::new) },
                Message::TrayLoaded,
            )
        }))
    }

    /// Show `picture`, remembering the previous one for Undo.
    fn replace_current(&mut self, picture: Arc<Picture>) {
        if let Some(prev) = self.current.take() {
            self.history.push(prev);
        }
        self.current = Some(picture);
        self.frame = 0;
    }

    // -- update -------------------------------------------------------------

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenPressed => Task::perform(pick_images("Open image"), Message::OpenChosen),
            Message::OpenChosen(paths) => {
                let Some(paths) = paths else {
                    return Task::none();
                };
                let mut paths = paths.into_iter();
                let Some(first) = paths.next() else {
                    return Task::none();
                };
                let open = self.open_path(first);
                let rest: Vec<PathBuf> = paths.collect();
                if rest.is_empty() {
                    open
                } else {
                    // Extra selections go straight to the tray.
                    Task::batch([open, self.load_into_tray(rest)])
                }
            }
            Message::Loaded(Ok(picture)) => {
                self.busy = false;
                self.status = describe(&picture);
                self.current = Some(picture);
                self.frame = 0;
                self.history.clear();
                Task::none()
            }
            Message::Loaded(Err(e)) => {
                self.busy = false;
                self.status = format!("Could not open: {e}");
                Task::none()
            }
            Message::FileDropped(path) => {
                if !formats::is_supported(&path) {
                    self.status = format!("Unsupported file: {}", formats::file_name(&path));
                    return Task::none();
                }
                if self.mode == Mode::Merge {
                    self.load_into_tray(vec![path])
                } else {
                    self.open_path(path)
                }
            }
            Message::Prev => self.step(-1),
            Message::Next => self.step(1),
            Message::SetMode(mode) => {
                self.mode = mode;
                if mode != Mode::View {
                    // Edit tools work on the first frame; show it.
                    self.frame = 0;
                }
                self.status = hint(mode);
                Task::none()
            }

            Message::SetOrientation(orientation) => {
                self.orientation = orientation;
                Task::none()
            }
            Message::KnifeHover(t) => {
                self.knife_pos = t;
                Task::none()
            }
            Message::KnifeCut => {
                let Some(picture) = &self.current else {
                    return Task::none();
                };
                match ops::split(&picture.pixels, self.orientation, self.knife_pos) {
                    Some((first, second)) => {
                        let (a, b) = self.orientation.piece_labels();
                        let stem = stem_of(&picture.name).to_owned();
                        let px = match self.orientation {
                            Orientation::Vertical => {
                                ops::knife_pixel(picture.width, self.knife_pos)
                            }
                            Orientation::Horizontal => {
                                ops::knife_pixel(picture.height, self.knife_pos)
                            }
                        }
                        .unwrap_or(0);
                        self.tray.push(Piece::new(format!("{stem}·{a}"), first));
                        self.tray.push(Piece::new(format!("{stem}·{b}"), second));
                        self.status = format!(
                            "Cut at {px} px → two pieces added to the tray ({} total)",
                            self.tray.len()
                        );
                    }
                    None => self.status = "Image is too small to cut".to_owned(),
                }
                Task::none()
            }

            Message::AddFilesPressed => Task::perform(
                pick_images("Add images to the tray"),
                Message::AddFilesChosen,
            ),
            Message::AddFilesChosen(paths) => match paths {
                Some(paths) => self.load_into_tray(paths),
                None => Task::none(),
            },
            Message::TrayLoaded(Ok(picture)) => {
                self.tray
                    .push(Piece::new(picture.name.clone(), picture.pixels.clone()));
                self.status = format!("Added {} to the tray", picture.name);
                Task::none()
            }
            Message::TrayLoaded(Err(e)) => {
                self.status = format!("Could not add: {e}");
                Task::none()
            }
            Message::AddCurrentToTray => {
                if let Some(picture) = &self.current {
                    self.tray
                        .push(Piece::new(picture.name.clone(), picture.pixels.clone()));
                    self.status = format!("Added {} to the tray", picture.name);
                }
                Task::none()
            }
            Message::TrayMoveLeft(i) => {
                if i > 0 && i < self.tray.len() {
                    self.tray.swap(i, i - 1);
                }
                Task::none()
            }
            Message::TrayMoveRight(i) => {
                if i + 1 < self.tray.len() {
                    self.tray.swap(i, i + 1);
                }
                Task::none()
            }
            Message::TrayRemove(i) => {
                if i < self.tray.len() {
                    self.tray.remove(i);
                }
                Task::none()
            }
            Message::TrayOpen(i) => {
                let Some(piece) = self.tray.get(i) else {
                    return Task::none();
                };
                let picture = Arc::new(Picture::from_rgba(
                    piece.label.clone(),
                    piece.pixels.clone(),
                ));
                self.status = describe(&picture);
                self.replace_current(picture);
                Task::none()
            }
            Message::TrayClear => {
                self.tray.clear();
                Task::none()
            }
            Message::SetDirection(direction) => {
                self.merge.direction = direction;
                Task::none()
            }
            Message::SetFill(fill) => {
                self.merge.fill = fill;
                Task::none()
            }
            Message::SetColumns(columns) => {
                self.merge.columns = columns;
                Task::none()
            }
            Message::Merge => {
                if self.tray.len() < 2 {
                    self.status = "Put at least two pieces in the tray to merge".to_owned();
                    return Task::none();
                }
                let inputs: Vec<RgbaImage> = self.tray.iter().map(|p| p.pixels.clone()).collect();
                let settings = self.merge;
                self.busy = true;
                self.status = "Merging…".to_owned();
                Task::perform(
                    async move {
                        let refs: Vec<&RgbaImage> = inputs.iter().collect();
                        ops::merge(&refs, &settings)
                            .map(|img| Arc::new(Picture::from_rgba("merged", img)))
                    },
                    Message::Merged,
                )
            }
            Message::Merged(result) => {
                self.busy = false;
                match result {
                    Some(picture) => {
                        self.status = format!(
                            "Merged {} pieces → {}×{} · Save… to export, Undo to go back",
                            self.tray.len(),
                            picture.width,
                            picture.height
                        );
                        self.replace_current(picture);
                        self.mode = Mode::View;
                    }
                    None => self.status = "Nothing to merge".to_owned(),
                }
                Task::none()
            }
            Message::Undo => {
                if let Some(prev) = self.history.pop() {
                    self.current = Some(prev);
                    self.frame = 0;
                    self.status = "Undone".to_owned();
                }
                Task::none()
            }

            Message::SavePressed => {
                let Some(picture) = &self.current else {
                    return Task::none();
                };
                let default_name = format!("{}.png", stem_of(&picture.name));
                Task::perform(
                    async move {
                        save_dialog(default_name)
                            .save_file()
                            .await
                            .map(|handle| handle.path().to_path_buf())
                    },
                    Message::SaveChosen,
                )
            }
            Message::SaveChosen(None) => Task::none(),
            Message::SaveChosen(Some(path)) => {
                let Some(picture) = self.current.clone() else {
                    return Task::none();
                };
                self.busy = true;
                self.status = "Saving…".to_owned();
                Task::perform(
                    async move { formats::save(&picture.pixels, &path) },
                    Message::Saved,
                )
            }
            Message::Saved(Ok(path)) => {
                self.busy = false;
                self.status = format!("Saved {}", formats::file_name(&path));
                Task::none()
            }
            Message::Saved(Err(e)) => {
                self.busy = false;
                self.status = format!("Save failed: {e}");
                Task::none()
            }

            Message::Tick(_) => {
                if let Some(picture) = &self.current {
                    if picture.frames.len() > 1 {
                        self.frame = (self.frame + 1) % picture.frames.len();
                    }
                }
                Task::none()
            }
            Message::SetGlass(glass) => {
                self.glass = glass;
                Task::none()
            }
        }
    }

    // -- subscriptions ------------------------------------------------------

    fn subscription(&self) -> Subscription<Message> {
        let keys = keyboard::on_key_press(handle_key);

        let drops = event::listen_with(|event, _status, _window| match event {
            Event::Window(window::Event::FileDropped(path)) => Some(Message::FileDropped(path)),
            _ => None,
        });

        let animation = match &self.current {
            Some(picture) if picture.frames.len() > 1 && self.mode == Mode::View => {
                let delay = picture.frames[self.frame % picture.frames.len()].delay;
                time::every(delay).map(Message::Tick)
            }
            _ => Subscription::none(),
        };

        Subscription::batch([keys, drops, animation])
    }

    // -- view ---------------------------------------------------------------

    fn view(&self) -> Element<'_, Message> {
        let mut content = column![self.toolbar(), self.stage(), self.panel()].spacing(10);
        if self.mode != Mode::View || !self.tray.is_empty() {
            content = content.push(self.tray());
        }

        // Leave room for the macOS traffic lights over the full-size content view.
        let padding = iced::Padding {
            top: if cfg!(target_os = "macos") { 34.0 } else { 12.0 },
            right: 12.0,
            bottom: 12.0,
            left: 12.0,
        };

        container(content.padding(padding))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(style::backdrop(self.glass))
            .into()
    }

    fn toolbar(&self) -> Element<'_, Message> {
        let has_image = self.current.is_some();
        let can_browse = self.playlist.len() > 1;

        let modes: Vec<Element<'_, Message>> = Mode::ALL
            .iter()
            .map(|&mode| {
                let active = mode == self.mode;
                button(text(mode.label()).size(13))
                    .padding([5, 12])
                    .on_press(Message::SetMode(mode))
                    .style(move |theme, status| {
                        if active {
                            style::accent_button(theme, status)
                        } else {
                            style::glass_button(theme, status)
                        }
                    })
                    .into()
            })
            .collect();

        let name = self
            .current
            .as_ref()
            .map(|p| format!("{}  ·  {}×{}", p.name, p.width, p.height))
            .unwrap_or_default();

        let bar = row![
            button(text("Open…").size(13))
                .padding([5, 12])
                .on_press(Message::OpenPressed)
                .style(style::glass_button),
            button(text("<").size(13))
                .padding([5, 10])
                .on_press_maybe(can_browse.then_some(Message::Prev))
                .style(style::glass_button),
            button(text(">").size(13))
                .padding([5, 10])
                .on_press_maybe(can_browse.then_some(Message::Next))
                .style(style::glass_button),
            text(name).size(13).color(style::MUTED),
            horizontal_space(),
            row(modes).spacing(4),
            horizontal_space(),
            text("Glass").size(12).color(style::MUTED),
            slider(0.0..=1.0, self.glass, Message::SetGlass)
                .step(0.05)
                .width(110),
            button(text(if self.busy { "Working…" } else { "Save…" }).size(13))
                .padding([5, 12])
                .on_press_maybe((has_image && !self.busy).then_some(Message::SavePressed))
                .style(style::glass_button),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        container(bar)
            .padding([8, 12])
            .width(Length::Fill)
            .style(style::glass(0.10))
            .into()
    }

    fn stage(&self) -> Element<'_, Message> {
        let inner: Element<'_, Message> = match &self.current {
            None => center(
                column![
                    text("Drop an image here").size(22),
                    text("PNG · JPEG · GIF · JPEG XL").size(13).color(style::MUTED),
                ]
                .spacing(6)
                .align_x(Alignment::Center),
            )
            .into(),
            Some(picture) => {
                let frame = &picture.frames[self.frame.min(picture.frames.len() - 1)];
                let handle = frame.handle.clone();
                match self.mode {
                    Mode::Knife => {
                        let overlay = canvas(knife::Overlay {
                            image: iced::Size::new(picture.width as f32, picture.height as f32),
                            orientation: self.orientation,
                            position: self.knife_pos,
                        })
                        .width(Length::Fill)
                        .height(Length::Fill);
                        stack![
                            iced_image(handle)
                                .content_fit(ContentFit::Contain)
                                .width(Length::Fill)
                                .height(Length::Fill),
                            overlay,
                        ]
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into()
                    }
                    _ => iced_image::viewer(handle)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .min_scale(0.05)
                        .max_scale(32.0)
                        .into(),
                }
            }
        };

        container(inner)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(4)
            .style(style::glass(0.04))
            .into()
    }

    fn panel(&self) -> Element<'_, Message> {
        let status = text(self.status.as_str()).size(12).color(style::MUTED);

        let content: Element<'_, Message> = match self.mode {
            Mode::View => row![status].into(),

            Mode::Knife => {
                let orientations: Vec<Element<'_, Message>> = Orientation::ALL
                    .iter()
                    .map(|&o| {
                        let active = o == self.orientation;
                        button(text(o.label()).size(12))
                            .padding([4, 10])
                            .on_press(Message::SetOrientation(o))
                            .style(move |theme, s| {
                                if active {
                                    style::accent_button(theme, s)
                                } else {
                                    style::glass_button(theme, s)
                                }
                            })
                            .into()
                    })
                    .collect();

                let readout = self
                    .current
                    .as_ref()
                    .and_then(|p| match self.orientation {
                        Orientation::Vertical => {
                            ops::knife_pixel(p.width, self.knife_pos).map(|x| format!("x = {x} px"))
                        }
                        Orientation::Horizontal => {
                            ops::knife_pixel(p.height, self.knife_pos).map(|y| format!("y = {y} px"))
                        }
                    })
                    .unwrap_or_default();

                row![
                    text("Cut").size(12).color(style::MUTED),
                    row(orientations).spacing(4),
                    text(readout).size(12),
                    horizontal_space(),
                    status,
                ]
                .spacing(10)
                .align_y(Alignment::Center)
                .into()
            }

            Mode::Merge => {
                let mut controls = row![
                    text("Direction").size(12).color(style::MUTED),
                    pick_list(
                        MergeDirection::ALL,
                        Some(self.merge.direction),
                        Message::SetDirection
                    )
                    .text_size(12)
                    .padding([4, 8]),
                    checkbox("Fill to largest", self.merge.fill)
                        .on_toggle(Message::SetFill)
                        .size(16)
                        .text_size(12),
                ]
                .spacing(10)
                .align_y(Alignment::Center);

                if self.merge.direction == MergeDirection::Grid {
                    let label = if self.merge.columns == 0 {
                        format!("auto ({})", ops::auto_columns(self.tray.len().max(1)))
                    } else {
                        self.merge.columns.to_string()
                    };
                    controls = controls
                        .push(text("Columns").size(12).color(style::MUTED))
                        .push(slider(0..=12u8, self.merge.columns, Message::SetColumns).width(120))
                        .push(text(label).size(12));
                }

                let can_merge = self.tray.len() >= 2 && !self.busy;
                controls
                    .push(horizontal_space())
                    .push(status)
                    .push(
                        button(text("Undo").size(12))
                            .padding([4, 10])
                            .on_press_maybe((!self.history.is_empty()).then_some(Message::Undo))
                            .style(style::glass_button),
                    )
                    .push(
                        button(text("Merge").size(12))
                            .padding([4, 14])
                            .on_press_maybe(can_merge.then_some(Message::Merge))
                            .style(style::accent_button),
                    )
                    .into()
            }
        };

        container(content)
            .padding([8, 12])
            .width(Length::Fill)
            .style(style::glass(0.08))
            .into()
    }

    fn tray(&self) -> Element<'_, Message> {
        let count = self.tray.len();

        let cards: Vec<Element<'_, Message>> = self
            .tray
            .iter()
            .enumerate()
            .map(|(i, piece)| {
                let thumb = iced_image(piece.handle.clone())
                    .content_fit(ContentFit::Contain)
                    .width(104)
                    .height(72);
                let label = format!(
                    "{}  {}×{}",
                    piece.label,
                    piece.pixels.width(),
                    piece.pixels.height()
                );
                let controls = row![
                    small_button("<", (i > 0).then_some(Message::TrayMoveLeft(i))),
                    small_button("x", Some(Message::TrayRemove(i))),
                    small_button(">", (i + 1 < count).then_some(Message::TrayMoveRight(i))),
                ]
                .spacing(4);
                let card = column![
                    button(thumb)
                        .padding(2)
                        .on_press(Message::TrayOpen(i))
                        .style(style::flat_button),
                    text(label).size(11).color(style::MUTED),
                    controls,
                ]
                .spacing(4)
                .align_x(Alignment::Center);
                container(card)
                    .padding(6)
                    .style(style::glass(0.06))
                    .into()
            })
            .collect();

        let strip: Element<'_, Message> = if cards.is_empty() {
            center(
                text("Tray is empty — cut an image with the knife, or add files")
                    .size(12)
                    .color(style::MUTED),
            )
            .height(60)
            .into()
        } else {
            scrollable(row(cards).spacing(8).padding([0, 2]))
                .direction(scrollable::Direction::Horizontal(Default::default()))
                .width(Length::Fill)
                .into()
        };

        let actions = column![
            small_button("+ Files…", Some(Message::AddFilesPressed)),
            small_button(
                "+ Current",
                self.current.as_ref().map(|_| Message::AddCurrentToTray)
            ),
            small_button("Clear", (count > 0).then_some(Message::TrayClear)),
        ]
        .spacing(4);

        container(row![strip, actions].spacing(10).align_y(Alignment::Center))
            .padding(8)
            .width(Length::Fill)
            .style(style::glass(0.08))
            .into()
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

fn small_button<'a>(label: &'a str, on_press: Option<Message>) -> Element<'a, Message> {
    button(text(label).size(11))
        .padding([3, 8])
        .on_press_maybe(on_press)
        .style(style::glass_button)
        .into()
}

fn handle_key(key: keyboard::Key, modifiers: keyboard::Modifiers) -> Option<Message> {
    use keyboard::key::Named;
    use keyboard::Key;

    match key.as_ref() {
        Key::Named(Named::ArrowLeft) => Some(Message::Prev),
        Key::Named(Named::ArrowRight) => Some(Message::Next),
        Key::Named(Named::Escape) => Some(Message::SetMode(Mode::View)),
        Key::Character("o") if modifiers.command() => Some(Message::OpenPressed),
        Key::Character("s") if modifiers.command() => Some(Message::SavePressed),
        Key::Character("z") if modifiers.command() => Some(Message::Undo),
        Key::Character("k") => Some(Message::SetMode(Mode::Knife)),
        Key::Character("m") => Some(Message::SetMode(Mode::Merge)),
        Key::Character("h") => Some(Message::SetOrientation(Orientation::Horizontal)),
        Key::Character("v") => Some(Message::SetOrientation(Orientation::Vertical)),
        _ => None,
    }
}

async fn pick_images(title: &'static str) -> Option<Vec<PathBuf>> {
    rfd::AsyncFileDialog::new()
        .set_title(title)
        .add_filter("Images", formats::EXTENSIONS)
        .pick_files()
        .await
        .map(|files| {
            files
                .into_iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        })
}

fn save_dialog(default_name: String) -> rfd::AsyncFileDialog {
    let mut dialog = rfd::AsyncFileDialog::new()
        .set_title("Save image as")
        .set_file_name(default_name)
        .add_filter("PNG", &["png"])
        .add_filter("JPEG", &["jpg", "jpeg"])
        .add_filter("GIF", &["gif"]);
    if cfg!(feature = "jxl") {
        dialog = dialog.add_filter("JPEG XL", &["jxl"]);
    }
    dialog
}

fn describe(picture: &Picture) -> String {
    let mut s = format!("{}  ·  {}×{}", picture.name, picture.width, picture.height);
    if picture.frames.len() > 1 {
        s.push_str(&format!("  ·  {} frames", picture.frames.len()));
    }
    s
}

fn hint(mode: Mode) -> String {
    match mode {
        Mode::View => "Scroll to zoom, drag to pan · ← → browse the folder",
        Mode::Knife => "Move over the image to place the knife, click to cut · H / V flips the line",
        Mode::Merge => "Order the pieces in the tray, choose a direction, press Merge",
    }
    .to_owned()
}

/// File name without its extension.
fn stem_of(name: &str) -> &str {
    Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(name)
}
