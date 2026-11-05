//! Pure gesture -> action mapping.
//!
//! Everything in here is free of window/GPU state so it can be unit tested:
//! wheel accumulation, modifier precedence, and the view transform (rotation,
//! mirror) including its inverse used for hit testing.

use notan::math::{vec2, Mat3};

// ---------------------------------------------------------------------------
// Tunables. Changing behaviour of a gesture should mean changing a value here.
// ---------------------------------------------------------------------------

/// One notch of a real mouse wheel as reported by winit (`LineDelta * 50`).
pub const WHEEL_NOTCH: f32 = 50.0;
/// Accumulated travel that switches one image. One mouse wheel notch is
/// [`WHEEL_NOTCH`], but smooth scrolling mice and drivers can report much
/// smaller values, which is why the settings expose a sensitivity on top.
pub const NAV_THRESHOLD: f32 = 10.0;
/// Accumulated travel that equals one 90 degree rotation step.
pub const ROTATE_THRESHOLD: f32 = 50.0;
/// Zoom speed of the right button drag (scale factor per pixel).
pub const DRAG_ZOOM_SPEED: f32 = 0.004;
/// Degrees of rotation per pixel while dragging with the middle button.
pub const ROT_DRAG_GAIN: f32 = 0.20;
/// Same, while shift is held (fine adjustment).
pub const ROT_DRAG_GAIN_FINE: f32 = 0.04;
/// Angles this close to a multiple of 90 are snapped onto it.
pub const ROT_SNAP_DEG: f32 = 1.5;
/// A middle button press that travels less than this is a click, not a drag.
pub const CLICK_MAX_TRAVEL: f32 = 3.0;
/// macOS swaps the wheel axes while shift is held. Flip if rotation feels wrong.
pub const SHIFT_AXIS_SIGN: f32 = 1.0;
/// Flip if a rotation step feels like it goes the wrong way.
pub const ROT_DIR_SIGN: f32 = 1.0;

/// Zoom bounds shared by every zoom path.
pub const ZOOM_MIN: f32 = 0.01;
pub const ZOOM_MAX: f32 = 40.0;

// ---------------------------------------------------------------------------
// Wheel
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelAction {
    PrevImage,
    NextImage,
    /// Number of notches, positive is zoom in.
    Zoom(i32),
    /// A quarter turn clockwise.
    RotateCw,
    /// A quarter turn counter clockwise.
    RotateCcw,
}

/// Accumulates sub-threshold wheel travel and hands out whole steps.
#[derive(Debug, Default, Clone, Copy)]
pub struct Accumulator {
    rest: f32,
}

impl Accumulator {
    pub fn steps(&mut self, delta: f32, threshold: f32) -> i32 {
        if threshold <= 0.0 {
            return 0;
        }
        self.rest += delta;
        let steps = (self.rest / threshold).trunc() as i32;
        self.rest -= steps as f32 * threshold;
        // A gesture that stops half way must not pile up forever.
        self.rest = self.rest.clamp(-2.0 * threshold, 2.0 * threshold);
        steps
    }

    pub fn reset(&mut self) {
        self.rest = 0.0;
    }
}

/// One accumulator per gesture so that e.g. a zoom does not disturb image nav.
#[derive(Debug, Default)]
pub struct WheelState {
    nav: Accumulator,
    zoom: Accumulator,
    rotate: Accumulator,
}

impl WheelState {
    /// Modifier precedence is fixed: shift (rotate) > ctrl (zoom) > plain (nav).
    ///
    /// `nav_threshold` is the accumulated travel that switches one image; see
    /// [`NAV_THRESHOLD`] and the wheel sensitivity setting.
    pub fn resolve(
        &mut self,
        dx: f32,
        dy: f32,
        mods: Mods,
        nav_threshold: f32,
    ) -> Option<WheelAction> {
        if mods.shift {
            let travel = dominant_delta(dx, dy) * SHIFT_AXIS_SIGN;
            let steps = self.rotate.steps(travel, ROTATE_THRESHOLD);
            return match steps.cmp(&0) {
                std::cmp::Ordering::Greater => Some(if ROT_DIR_SIGN > 0.0 {
                    WheelAction::RotateCw
                } else {
                    WheelAction::RotateCcw
                }),
                std::cmp::Ordering::Less => Some(if ROT_DIR_SIGN > 0.0 {
                    WheelAction::RotateCcw
                } else {
                    WheelAction::RotateCw
                }),
                std::cmp::Ordering::Equal => None,
            };
        }

        if mods.ctrl {
            let steps = self.zoom.steps(dy, WHEEL_NOTCH).clamp(-3, 3);
            return (steps != 0).then_some(WheelAction::Zoom(steps));
        }

        // Exactly one image per event: a trackpad flick must not skip a folder.
        let steps = self.nav.steps(dy, nav_threshold.max(1.0));
        if steps > 0 {
            Some(WheelAction::PrevImage)
        } else if steps < 0 {
            Some(WheelAction::NextImage)
        } else {
            None
        }
    }

    pub fn reset_nav(&mut self) {
        self.nav.reset();
    }
}

/// macOS turns shift+wheel into a horizontal scroll, so the larger axis wins.
pub fn dominant_delta(dx: f32, dy: f32) -> f32 {
    if dy.abs() >= dx.abs() {
        dy
    } else {
        dx
    }
}

/// Vertical drag of the right button: upwards (negative) zooms in.
pub fn drag_zoom_delta(dy: f32, scale: f32) -> f32 {
    -dy * scale * DRAG_ZOOM_SPEED
}

pub fn is_actual_size(scale: f32) -> bool {
    (scale - 1.0).abs() < 0.005
}

// ---------------------------------------------------------------------------
// View transform: rotation + mirror, display only
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct ViewTransform {
    /// Clockwise rotation in degrees, normalized to (-180, 180].
    pub angle_deg: f32,
    /// Horizontal flip, applied after the rotation.
    pub mirror_x: bool,
}

impl ViewTransform {
    pub fn add_angle(self, deg: f32) -> Self {
        Self {
            angle_deg: snap_angle(normalize_angle(self.angle_deg + deg)),
            mirror_x: self.mirror_x,
        }
    }

    /// Rotate by whole quarter turns. The current angle is quantized to the
    /// nearest quarter first, so that a fine tuned 12.3 degrees becomes 90 and
    /// not 102.3.
    pub fn quarter_steps(self, steps: i32) -> Self {
        let base = (self.angle_deg / 90.0).round() * 90.0;
        Self {
            angle_deg: normalize_angle(base + 90.0 * steps as f32),
            mirror_x: self.mirror_x,
        }
    }

    pub fn toggle_mirror(self) -> Self {
        Self {
            angle_deg: self.angle_deg,
            mirror_x: !self.mirror_x,
        }
    }

    pub fn is_upright(self) -> bool {
        self.angle_deg == 0.0 && !self.mirror_x
    }

    /// Axis aligned bounding box of a `w` by `h` rectangle after rotation.
    pub fn aabb(self, w: f32, h: f32) -> (f32, f32) {
        let rad = self.angle_deg.to_radians();
        let (sin, cos) = rad.sin_cos();
        (
            w * cos.abs() + h * sin.abs(),
            w * sin.abs() + h * cos.abs(),
        )
    }

    /// `M = Mirror(pivot) * R(pivot) * T(offset) * S(scale)`
    ///
    /// `pivot` is the image centre in screen space *before* rotating, i.e.
    /// `offset + size * scale / 2`. All draws use this one matrix, so tiles of
    /// a huge image stay aligned.
    pub fn matrix(&self, scale: f32, offset: (f32, f32), pivot: (f32, f32)) -> Mat3 {
        let p = vec2(pivot.0, pivot.1);
        let rotate = Mat3::from_translation(p)
            * Mat3::from_angle(self.angle_deg.to_radians() * ROT_DIR_SIGN)
            * Mat3::from_translation(-p);
        let mirror = if self.mirror_x {
            Mat3::from_translation(p) * Mat3::from_scale(vec2(-1.0, 1.0)) * Mat3::from_translation(-p)
        } else {
            Mat3::IDENTITY
        };
        mirror
            * rotate
            * Mat3::from_translation(vec2(offset.0, offset.1))
            * Mat3::from_scale(vec2(scale, scale))
    }

    /// Screen point -> untransformed point. Exact inverse of [`Self::matrix`].
    pub fn untransform_point(
        &self,
        point: (f32, f32),
        scale: f32,
        offset: (f32, f32),
        pivot: (f32, f32),
    ) -> (f32, f32) {
        let inverse = self.matrix(scale, offset, pivot).inverse();
        let p = inverse.transform_point2(vec2(point.0, point.1));
        (p.x, p.y)
    }

    /// Screen delta -> delta in the untransformed screen frame.
    ///
    /// Only the linear part (mirror then rotation) applies: offsets are kept in
    /// screen units, so no scaling must be involved here.
    pub fn untransform_delta(&self, delta: (f32, f32)) -> (f32, f32) {
        let mirrored = if self.mirror_x {
            vec2(-delta.0, delta.1)
        } else {
            vec2(delta.0, delta.1)
        };
        let inverse_rotation = Mat3::from_angle(-self.angle_deg.to_radians() * ROT_DIR_SIGN);
        let d = inverse_rotation.transform_vector2(mirrored);
        (d.x, d.y)
    }
}

fn normalize_angle(deg: f32) -> f32 {
    let mut a = deg % 360.0;
    if a > 180.0 {
        a -= 360.0;
    } else if a <= -180.0 {
        a += 360.0;
    }
    if a == -180.0 {
        a = 180.0;
    }
    a
}

fn snap_angle(deg: f32) -> f32 {
    let quarter = (deg / 90.0).round() * 90.0;
    if (deg - quarter).abs() <= ROT_SNAP_DEG {
        normalize_angle(quarter)
    } else {
        deg
    }
}

// ---------------------------------------------------------------------------
// Middle button: click = mirror, drag = fine rotation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MiddleGesture {
    Idle,
    /// Pressed, but has not travelled far enough to count as a drag.
    Pressed { start: (f32, f32), max_travel: f32 },
    /// Travelled more than [`CLICK_MAX_TRAVEL`], rotating until release.
    Rotating,
}

impl Default for MiddleGesture {
    fn default() -> Self {
        MiddleGesture::Idle
    }
}

impl MiddleGesture {
    pub fn press(pos: (f32, f32)) -> Self {        MiddleGesture::Pressed {
            start: pos,
            max_travel: 0.0,
        }
    }

    /// Feed the current cursor position. Uses the *maximum* travel so that
    /// dragging away and back is not mistaken for a click.
    pub fn moved(self, pos: (f32, f32)) -> Self {
        match self {
            MiddleGesture::Pressed { start, max_travel } => {
                let travel = max_travel.max(distance(start, pos));
                if travel > CLICK_MAX_TRAVEL {
                    MiddleGesture::Rotating
                } else {
                    MiddleGesture::Pressed {
                        start,
                        max_travel: travel,
                    }
                }
            }
            other => other,
        }
    }

    pub fn is_click(self) -> bool {
        matches!(self, MiddleGesture::Pressed { .. })
    }

    pub fn is_rotating(self) -> bool {
        matches!(self, MiddleGesture::Rotating)
    }
}

pub fn rotate_gain(shift: bool) -> f32 {
    if shift {
        ROT_DRAG_GAIN_FINE
    } else {
        ROT_DRAG_GAIN
    }
}

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    (dx * dx + dy * dy).sqrt()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn mods(ctrl: bool, shift: bool) -> Mods {
        Mods {
            ctrl,
            shift,
            alt: false,
        }
    }

    #[test]
    fn accumulator_keeps_the_remainder() {
        let mut acc = Accumulator::default();
        assert_eq!(acc.steps(55.0, 50.0), 1);
        assert_eq!(acc.steps(45.0, 50.0), 1); // 5 + 45 = 50
        assert_eq!(acc.steps(10.0, 50.0), 0);
    }

    #[test]
    fn trackpad_has_to_accumulate() {
        let mut acc = Accumulator::default();
        // Three small steps of a slow trackpad gesture, threshold 30.
        assert_eq!(acc.steps(10.0, 30.0), 0);
        assert_eq!(acc.steps(10.0, 30.0), 0);
        assert_eq!(acc.steps(10.0, 30.0), 1);
        assert_eq!(acc.steps(10.0, 30.0), 0);
    }

    #[test]
    fn smooth_scrolling_mouse_switches_after_one_notch() {
        // Mice with smooth scrolling (and some drivers) report ~10 per notch
        // instead of the 50 winit uses for a line based wheel.
        let mut wheel = WheelState::default();
        assert_eq!(
            wheel.resolve(0.0, -10.0, mods(false, false), NAV_THRESHOLD),
            Some(WheelAction::NextImage)
        );
        assert_eq!(
            wheel.resolve(0.0, 10.0, mods(false, false), NAV_THRESHOLD),
            Some(WheelAction::PrevImage)
        );
    }

    #[test]
    fn tiny_scroll_steps_still_accumulate() {
        // A very smooth driver sends 2 per event; ten of them must switch once,
        // not ten times.
        let mut wheel = WheelState::default();
        let mut switched = 0;
        for _ in 0..10 {
            if wheel.resolve(0.0, -2.0, mods(false, false), NAV_THRESHOLD).is_some() {
                switched += 1;
            }
        }
        assert_eq!(switched, 2, "10 * 2 with a threshold of 10 switches twice");
    }

    #[test]
    fn higher_sensitivity_lowers_the_threshold() {
        let mut wheel = WheelState::default();
        // Sensitivity 5 => threshold 2, so a single tiny event switches.
        assert_eq!(
            wheel.resolve(0.0, -2.0, mods(false, false), NAV_THRESHOLD / 5.0),
            Some(WheelAction::NextImage)
        );
    }

    #[test]
    fn opposite_deltas_cancel() {
        let mut acc = Accumulator::default();
        assert_eq!(acc.steps(40.0, 30.0), 1);
        assert_eq!(acc.steps(-50.0, 30.0), -1);
    }

    #[test]
    fn modifier_precedence_shift_wins() {
        let mut wheel = WheelState::default();
        assert_eq!(
            wheel.resolve(0.0, 50.0, mods(true, true), NAV_THRESHOLD),
            Some(WheelAction::RotateCw)
        );
    }

    #[test]
    fn ctrl_wheel_zooms() {
        let mut wheel = WheelState::default();
        assert_eq!(
            wheel.resolve(0.0, 50.0, mods(true, false), NAV_THRESHOLD),
            Some(WheelAction::Zoom(1))
        );
        assert_eq!(
            wheel.resolve(0.0, -50.0, mods(true, false), NAV_THRESHOLD),
            Some(WheelAction::Zoom(-1))
        );
    }

    #[test]
    fn plain_wheel_navigates_one_image_per_event() {
        let mut wheel = WheelState::default();
        assert_eq!(
            wheel.resolve(0.0, 50.0, mods(false, false), NAV_THRESHOLD),
            Some(WheelAction::PrevImage)
        );
        assert_eq!(
            wheel.resolve(0.0, -200.0, mods(false, false), NAV_THRESHOLD),
            Some(WheelAction::NextImage)
        );
        // A huge delta must not skip several images.
        assert_eq!(wheel.resolve(0.0, -1000.0, mods(false, false), NAV_THRESHOLD), Some(WheelAction::NextImage));
    }

    #[test]
    fn shift_wheel_rotates_even_when_macos_swaps_the_axis() {
        let mut wheel = WheelState::default();
        assert_eq!(
            wheel.resolve(-50.0, 0.0, mods(false, true), NAV_THRESHOLD),
            Some(WheelAction::RotateCcw)
        );
    }

    #[test]
    fn angles_are_normalized_and_snapped() {
        let v = ViewTransform::default();
        assert!((v.add_angle(12.3).angle_deg - 12.3).abs() < 1e-4);
        assert_eq!(v.add_angle(89.2).angle_deg, 90.0);
        assert_eq!(v.add_angle(-0.8).angle_deg, 0.0);
        assert_eq!(v.add_angle(350.0).angle_deg, -10.0);
        assert_eq!(v.add_angle(400.0).angle_deg, 40.0);
    }

    #[test]
    fn quarter_steps_quantize_before_stepping() {
        let v = ViewTransform {
            angle_deg: 12.3,
            mirror_x: false,
        };
        assert_eq!(v.quarter_steps(1).angle_deg, 90.0);
        assert_eq!(v.quarter_steps(-1).angle_deg, -90.0);
        let half = ViewTransform {
            angle_deg: 179.0,
            mirror_x: false,
        };
        assert_eq!(half.quarter_steps(1).angle_deg, -90.0);
    }

    #[test]
    fn aabb_matches_known_values() {
        let upright = ViewTransform::default();
        assert_eq!(upright.aabb(200.0, 100.0), (200.0, 100.0));

        let quarter = upright.quarter_steps(1);
        let (w, h) = quarter.aabb(200.0, 100.0);
        assert!((w - 100.0).abs() < 1e-3 && (h - 200.0).abs() < 1e-3);

        let diagonal = ViewTransform {
            angle_deg: 45.0,
            mirror_x: false,
        };
        let (w, h) = diagonal.aabb(200.0, 100.0);
        let expected = (200.0 + 100.0) * std::f32::consts::FRAC_1_SQRT_2;
        assert!((w - expected).abs() < 1e-3 && (h - expected).abs() < 1e-3);
    }

    #[test]
    fn matrix_and_untransform_are_inverse() {
        let angles = [0.0, 12.3, 45.0, 90.0, 135.0, 180.0, 270.0];
        let points = [(0.0, 0.0), (37.0, -12.0), (400.0, 300.0), (-55.5, 91.25)];

        for angle in angles {
            for mirror_x in [false, true] {
                let view = ViewTransform { angle_deg: angle, mirror_x };
                let scale = 1.7;
                let offset = (33.0, -21.0);
                let pivot = (200.0, 150.0);
                let m = view.matrix(scale, offset, pivot);
                let inverse = m.inverse();

                for p in points {
                    let forward = m.transform_point2(vec2(p.0, p.1));
                    let back = inverse.transform_point2(forward);
                    assert!(
                        (back.x - p.0).abs() < 1e-3 && (back.y - p.1).abs() < 1e-3,
                        "roundtrip failed for angle {angle} mirror {mirror_x}: {p:?} -> {back:?}"
                    );

                    let via_helper =
                        view.untransform_point((forward.x, forward.y), scale, offset, pivot);
                    assert!(
                        (via_helper.0 - p.0).abs() < 1e-3 && (via_helper.1 - p.1).abs() < 1e-3,
                        "untransform_point differs for angle {angle} mirror {mirror_x}"
                    );
                }

                for d in [(10.0, 0.0), (0.0, -7.5), (3.0, 4.0)] {
                    // Linear part only: scale 1, no translation, pivot at origin.
                    let linear = view.matrix(1.0, (0.0, 0.0), (0.0, 0.0));
                    let moved = linear.transform_vector2(vec2(d.0, d.1));
                    let back = view.untransform_delta((moved.x, moved.y));
                    assert!(
                        (back.0 - d.0).abs() < 1e-3 && (back.1 - d.1).abs() < 1e-3,
                        "delta roundtrip failed for angle {angle} mirror {mirror_x}: {d:?} -> {back:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn image_centre_is_the_fixed_point() {
        // A 200x200 image at scale 2 sits with its centre on `pivot`.
        let pivot = (100.0, 100.0);
        let offset = (pivot.0 - 200.0, pivot.1 - 200.0);
        for angle in [0.0, 23.0, 90.0, -135.0] {
            for mirror_x in [false, true] {
                let view = ViewTransform { angle_deg: angle, mirror_x };
                let m = view.matrix(2.0, offset, pivot);
                // (100, 100) is the image centre in image coordinates.
                let centre = m.transform_point2(vec2(100.0, 100.0));
                assert!(
                    (centre.x - pivot.0).abs() < 1e-3 && (centre.y - pivot.1).abs() < 1e-3,
                    "centre moved for angle {angle} mirror {mirror_x}: {centre:?}"
                );
            }
        }
    }

    #[test]
    fn mirror_flips_horizontally_around_the_centre() {
        let pivot = (100.0, 100.0);
        let view = ViewTransform {
            angle_deg: 0.0,
            mirror_x: true,
        };
        // Same scale/offset setup as above: image x=150 is 50px right of centre.
        let offset = (0.0, 0.0);
        let m = view.matrix(1.0, offset, pivot);
        let p = m.transform_point2(vec2(150.0, 100.0));
        assert!((p.x - 50.0).abs() < 1e-3, "expected mirrored to x=50, got {p:?}");
    }

    #[test]
    fn upright_transform_is_plain_scale_and_translate() {
        let view = ViewTransform::default();
        let m = view.matrix(2.0, (5.0, 7.0), (50.0, 50.0));
        let p = m.transform_point2(vec2(3.0, 4.0));
        assert!((p.x - 11.0).abs() < 1e-4 && (p.y - 15.0).abs() < 1e-4);
    }

    #[test]
    fn middle_click_and_drag_are_told_apart() {
        let press = MiddleGesture::press((100.0, 100.0));
        assert!(press.is_click());

        let jitter = press.moved((102.0, 101.0)).moved((100.5, 100.0));
        assert!(jitter.is_click(), "small jitter must stay a click");

        let dragged = press.moved((120.0, 100.0)).moved((100.0, 100.0));
        assert!(dragged.is_rotating(), "dragging back must stay a rotation");
    }

    #[test]
    fn rotate_gain_has_a_fine_mode() {
        assert!(rotate_gain(true) < rotate_gain(false));
    }
}
