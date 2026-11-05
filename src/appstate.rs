use crate::{
    comparelist::CompareList,
    filebrowser::BrowserDir,
    gestures,
    image_editing::EditState,
    scrubber::Scrubber,
    settings::{PersistentSettings, VolatileSettings},
    texture_wrapper::TextureWrapperManager,
    thumbnails::Thumbnails,
    utils::{ExtendedImageInfo, Frame, Player},
};

use egui_notify::Toasts;
use image::DynamicImage;
use nalgebra::Vector2;
use notan::math::Mat3;
use notan::{prelude::Texture, AppState};
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    time::Instant,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageGeometry {
    /// The scale of the displayed image
    pub scale: f32,
    /// Image offset on canvas
    pub offset: Vector2<f32>,
    pub dimensions: (u32, u32),
}

#[derive(Debug, Clone)]
pub enum Message {
    Info(String),
    Warning(String),
    Error(String),
    LoadError(String),
    Saved(PathBuf),
}

impl Message {
    pub fn info(m: &str) -> Self {
        Self::Info(m.into())
    }
    pub fn warn(m: &str) -> Self {
        Self::Warning(m.into())
    }
    pub fn err(m: &str) -> Self {
        Self::Error(m.into())
    }
}

/// The state of the application
#[derive(AppState)]
pub struct OculanteState {
    pub image_geometry: ImageGeometry,
    pub compare_list: CompareList,
    pub drag_enabled: bool,
    pub reset_image: bool,
    /// Is the image fully loaded?
    pub is_loaded: bool,
    pub window_size: Vector2<f32>,
    pub cursor: Vector2<f32>,
    pub cursor_relative: Vector2<f32>,
    pub sampled_color: [f32; 4],
    pub mouse_delta: Vector2<f32>,
    /// Scroll distance the UI has not been given yet, see `precise_scroll_remainder`
    pub scroll_remainder: Vector2<f32>,
    pub texture_channel: (Sender<Frame>, Receiver<Frame>),
    pub message_channel: (Sender<Message>, Receiver<Message>),
    /// Channel to load images from
    pub load_channel: (Sender<PathBuf>, Receiver<PathBuf>),
    /// File names piped in through stdin, once they have been read
    pub piped_paths: Option<Receiver<Vec<PathBuf>>>,
    pub extended_info_channel: (Sender<ExtendedImageInfo>, Receiver<ExtendedImageInfo>),
    /// The Player, responsible for loading and sending Frames
    pub player: Player,
    //pub current_texture: Option<TexWrap>,
    pub current_texture: TextureWrapperManager,
    pub current_path: Option<PathBuf>,
    pub current_image: Option<DynamicImage>,
    pub settings_enabled: bool,
    pub image_metadata: Option<ExtendedImageInfo>,
    pub tiling: usize,
    pub mouse_grab: bool,
    pub key_grab: bool,
    pub edit_state: EditState,
    pub pointer_over_ui: bool,
    /// Things that persist between launches
    pub persistent_settings: PersistentSettings,
    pub volatile_settings: VolatileSettings,
    pub always_on_top: bool,
    pub network_mode: bool,
    /// how long the toast message appears
    /// data to transform image once fullscreen is entered/left
    pub fullscreen_offset: Option<(i32, i32)>,
    /// List of images to cycle through. Usually the current dir or dropped files
    pub scrubber: Scrubber,
    pub checker_texture: Option<Texture>,
    pub redraw: bool,
    pub first_start: bool,
    pub toasts: Toasts,
    pub filebrowser_id: Option<String>,
    pub filebrowser_last_dir: BrowserDir,
    pub thumbnails: Thumbnails,
    pub new_image_loaded: bool,
    /// Display only rotation/mirror. Never written to disk.
    pub view: gestures::ViewTransform,
    /// Path the current [`Self::view`] belongs to, used to reset on navigation.
    pub view_path: Option<PathBuf>,
    /// Wheel accumulators for navigation, zoom and rotation.
    pub wheel: gestures::WheelState,
    /// Right button is held and drives the zoom.
    pub zoom_drag: bool,
    /// Right button drag actually moved, so a double click is not inferred.
    pub zoom_drag_moved: bool,
    /// Middle button state machine: click mirrors, drag rotates.
    pub middle: gestures::MiddleGesture,
    /// Last image switch, used to throttle navigation.
    pub last_nav_at: Option<Instant>,
    /// Navigation that was dropped because of the throttle.
    pub pending_nav: i32,
    /// Throttles the "angle" and "mirrored" toasts.
    pub last_view_msg_at: Option<Instant>,
}

impl OculanteState {
    pub fn send_message_info(&self, msg: &str) {
        _ = self.message_channel.0.send(Message::info(msg));
    }

    pub fn send_message_err(&self, msg: &str) {
        _ = self.message_channel.0.send(Message::err(msg));
    }

    pub fn send_message_warn(&self, msg: &str) {
        _ = self.message_channel.0.send(Message::warn(msg));
    }

    pub fn send_frame(&self, frame: Frame) {
        let _ = self.texture_channel.0.send(frame);
    }

    /// Evaluate the base path for the file browser.
    pub fn filebrowser_path(&self) -> PathBuf {
        match self.filebrowser_last_dir {
            BrowserDir::LastOpenDir => self.volatile_settings.last_open_directory.to_owned(),
            BrowserDir::CurrentImageDir => self
                .current_path
                .as_deref()
                .and_then(|path| path.parent())
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.volatile_settings.last_open_directory.clone()),
        }
    }

    /// Size of the drawn image in pixels, including tiling.
    pub fn image_dims_f(&self) -> Vector2<f32> {
        Vector2::new(
            self.image_geometry.dimensions.0 as f32,
            self.image_geometry.dimensions.1 as f32,
        ) * self.tiling as f32
    }

    /// Size of the drawn image on screen, after rotation and mirroring.
    pub fn image_screen_dims(&self) -> Vector2<f32> {
        let d = self.image_dims_f() * self.image_geometry.scale;
        let (w, h) = self.view.aabb(d.x, d.y);
        Vector2::new(w, h)
    }

    /// Centre of the image on screen, in the untransformed frame.
    pub fn view_pivot(&self) -> Vector2<f32> {
        self.image_geometry.offset + self.image_dims_f() * self.image_geometry.scale / 2.0
    }

    /// The single transform used for drawing and for hit testing.
    pub fn view_matrix(&self) -> Mat3 {
        let pivot = self.view_pivot();
        self.view.matrix(
            self.image_geometry.scale,
            (self.image_geometry.offset.x, self.image_geometry.offset.y),
            (pivot.x, pivot.y),
        )
    }

    /// Screen point -> point in the untransformed screen frame.
    pub fn unview_point(&self, screen: Vector2<f32>) -> Vector2<f32> {
        let pivot = self.view_pivot();
        let (x, y) = self.view.untransform_point(
            (screen.x, screen.y),
            self.image_geometry.scale,
            (self.image_geometry.offset.x, self.image_geometry.offset.y),
            (pivot.x, pivot.y),
        );
        Vector2::new(x, y)
    }

    /// Screen delta -> delta in the untransformed screen frame.
    pub fn unview_delta(&self, delta: Vector2<f32>) -> Vector2<f32> {
        let (x, y) = self.view.untransform_delta((delta.x, delta.y));
        Vector2::new(x, y)
    }
}

impl Default for OculanteState {
    fn default() -> OculanteState {
        let persistent_settings = PersistentSettings::load().unwrap_or_default();

        let tx_channel = mpsc::channel();
        let msg_channel = mpsc::channel();
        let meta_channel = mpsc::channel();
        OculanteState {
            image_geometry: ImageGeometry {
                scale: 1.0,
                offset: Default::default(),
                dimensions: Default::default(),
            },
            compare_list: Default::default(),
            drag_enabled: Default::default(),
            reset_image: Default::default(),
            is_loaded: Default::default(),
            cursor: Default::default(),
            cursor_relative: Default::default(),
            sampled_color: [0., 0., 0., 0.],
            player: Player::new(
                tx_channel.0.clone(),
                20,
                msg_channel.0.clone(),
                persistent_settings.decoders,
            ),
            texture_channel: tx_channel,
            message_channel: msg_channel,
            load_channel: mpsc::channel(),
            piped_paths: None,
            extended_info_channel: meta_channel,
            mouse_delta: Default::default(),
            scroll_remainder: Default::default(),
            current_texture: Default::default(),
            current_image: Default::default(),
            current_path: Default::default(),
            settings_enabled: Default::default(),
            image_metadata: Default::default(),
            tiling: 1,
            mouse_grab: Default::default(),
            key_grab: Default::default(),
            edit_state: Default::default(),
            pointer_over_ui: Default::default(),
            persistent_settings: PersistentSettings::load().unwrap_or_default(),
            volatile_settings: VolatileSettings::load().unwrap_or_default(),
            always_on_top: Default::default(),
            network_mode: Default::default(),
            window_size: Default::default(),
            fullscreen_offset: Default::default(),
            scrubber: Default::default(),
            checker_texture: Default::default(),
            redraw: Default::default(),
            first_start: true,
            toasts: Toasts::default().with_anchor(egui_notify::Anchor::BottomLeft),
            filebrowser_id: None,
            filebrowser_last_dir: Default::default(),
            thumbnails: Default::default(),
            new_image_loaded: false,
            view: Default::default(),
            view_path: None,
            wheel: Default::default(),
            zoom_drag: false,
            zoom_drag_moved: false,
            middle: Default::default(),
            last_nav_at: None,
            pending_nav: 0,
            last_view_msg_at: None,
        }
    }
}
