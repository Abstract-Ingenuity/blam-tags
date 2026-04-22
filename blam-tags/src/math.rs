//! Math and color types used in tag data.

/// Bounds (min/max pair).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Bounds<T> {
    pub lower: T,
    pub upper: T,
}

/// Bounds of two i16 values (min/max).
pub type ShortBounds = Bounds<i16>;
/// Bounds of two angle values in radians (min/max).
pub type AngleBounds = Bounds<f32>;
/// Bounds of two real values (min/max).
pub type RealBounds = Bounds<f32>;
/// Bounds of two fraction values (min/max).
pub type FractionBounds = Bounds<f32>;

/// 8-bit-per-channel RGB color, packed into a single `u32`. The engine
/// accesses each channel via bit shifts / masks — this is deliberately
/// not split into separate byte fields to match the on-disk layout.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RgbColor(pub u32);

/// 8-bit-per-channel ARGB color, packed into a single `u32`. The engine
/// accesses each channel via bit shifts / masks — this is deliberately
/// not split into separate byte fields to match the on-disk layout.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ArgbColor(pub u32);

/// 2D point (integer).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Point2d {
    pub x: i16,
    pub y: i16,
}

/// 2D rectangle (integer).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rectangle2d {
    pub top: i16,
    pub left: i16,
    pub bottom: i16,
    pub right: i16,
}

/// 2D point (float).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RealPoint2d {
    pub x: f32,
    pub y: f32,
}

/// 3D point (float).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RealPoint3d {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// 2D vector (float).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RealVector2d {
    pub i: f32,
    pub j: f32,
}

/// 3D vector (float).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RealVector3d {
    pub i: f32,
    pub j: f32,
    pub k: f32,
}

/// Quaternion.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RealQuaternion {
    pub i: f32,
    pub j: f32,
    pub k: f32,
    pub w: f32,
}

/// 2D euler angles.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RealEulerAngles2d {
    pub yaw: f32,
    pub pitch: f32,
}

/// 3D euler angles.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RealEulerAngles3d {
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
}

/// 2D plane.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RealPlane2d {
    pub i: f32,
    pub j: f32,
    pub d: f32,
}

/// 3D plane.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RealPlane3d {
    pub i: f32,
    pub j: f32,
    pub k: f32,
    pub d: f32,
}

/// RGB color (float, 0.0–1.0).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RealRgbColor {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
}

/// ARGB color (float, 0.0–1.0).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RealArgbColor {
    pub alpha: f32,
    pub red: f32,
    pub green: f32,
    pub blue: f32,
}

/// HSV color (float).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RealHsvColor {
    pub hue: f32,
    pub saturation: f32,
    pub value: f32,
}

/// AHSV color (float).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RealAhsvColor {
    pub alpha: f32,
    pub hue: f32,
    pub saturation: f32,
    pub value: f32,
}
