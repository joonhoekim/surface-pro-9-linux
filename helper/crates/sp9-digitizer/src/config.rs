// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Configuration of the touch and pen processing.
//!
//! [`Config::default`] is the Surface Pro 9: iptsd's defaults with the platform preset
//! (userspace/platform/iptsd/surface-pro-9.conf) applied; a test holds the two together. The
//! native format is TOML with the structure of these types. [`Config::apply_iptsd_conf`] reads
//! the iptsd keys that still have a meaning here, so the helper's drop-in keeps working.
//!
//! Lengths are in millimeters unless a name says otherwise; magnitudes and amplitudes are in the
//! units of the DFT windows; pressures and signal levels are fractions.

use serde::{Deserialize, Serialize};
use std::fmt;

pub const VENDOR: u16 = 0x045E;
pub const PRODUCT: u16 = 0x0C52;

/// The sensor as the metadata feature report describes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    /// Antennas on the Y axis (heatmap rows).
    pub rows: u8,
    /// Antennas on the X axis (heatmap columns).
    pub columns: u8,
    /// Width of the screen in centimeters.
    pub width_cm: f64,
    /// Height of the screen in centimeters.
    pub height_cm: f64,
}

impl Geometry {
    /// What the Surface Pro 9 reports.
    pub const SP9: Geometry = Geometry { rows: 46, columns: 68, width_cm: 27.389, height_cm: 18.259 };
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Range {
    pub min: f64,
    pub max: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub touch: TouchConfig,
    pub pen: PenConfig,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TouchConfig {
    /// No touchscreen device at all.
    pub disable: bool,
    /// Lift every contact while one of them is a palm.
    pub disable_on_palm: bool,
    /// Lift every contact while the pen is in range.
    pub disable_on_stylus: bool,
    /// For how long after the pen left its range touch stays off (seconds). Bridges short
    /// losses of the pen signal; a touch that begins later than this after the pen left is a
    /// touch.
    pub pen_holdoff_s: f64,
    /// How far outside of the screen (cm) the center of a contact may be.
    pub overshoot_cm: f64,
    pub contacts: ContactsConfig,
}

impl Default for TouchConfig {
    fn default() -> Self {
        TouchConfig {
            disable: false,
            disable_on_palm: false,
            disable_on_stylus: true,
            pen_holdoff_s: 0.2,
            overshoot_cm: 0.5,
            contacts: ContactsConfig::default(),
        }
    }
}

/// Contact detection on the heatmap. Thresholds are in heatmap counts (0 .. 255) above the
/// neutral level, sizes in centimeters, orientation in degrees.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ContactsConfig {
    /// Added to the neutral level (the most frequent value of the image).
    pub neutral_offset: f64,
    /// A local maximum above this starts a contact.
    pub activation_threshold: f64,
    /// The contact extends over the cells above this.
    pub deactivation_threshold: f64,
    /// A size change below `min` is dropped, above `max` the contact is unstable.
    pub size_change: Range,
    /// A movement below `min` is dropped, above `max` the contact is unstable.
    pub position_change: Range,
    /// An orientation change below `min` is dropped, above `max` the contact is unstable.
    pub orientation_change: Range,
    /// The major axis of a finger; anything else is a palm.
    pub size: Range,
    /// The aspect ratio of a finger; anything else is a palm.
    pub aspect: Range,
}

impl Default for ContactsConfig {
    fn default() -> Self {
        ContactsConfig {
            neutral_offset: 0.0,
            activation_threshold: 24.0,
            deactivation_threshold: 20.0,
            size_change: Range { min: 0.1, max: 0.5 },
            position_change: Range { min: 0.04, max: 2.0 },
            orientation_change: Range { min: 1.0, max: 5.0 },
            size: Range { min: 0.2, max: 2.0 },
            aspect: Range { min: 1.0, max: 2.5 },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PenConfig {
    /// No stylus device at all.
    pub disable: bool,
    /// Units per millimeter of the stylus axes.
    pub axis_resolution: u32,
    pub position: PositionConfig,
    pub tilt: TiltConfig,
    pub contact: ContactConfig,
    pub pressure: PressureConfig,
    pub lock: LockConfig,
    pub button: ButtonConfig,
}

impl Default for PenConfig {
    fn default() -> Self {
        PenConfig {
            disable: false,
            axis_resolution: 100,
            position: PositionConfig::default(),
            tilt: TiltConfig::default(),
            contact: ContactConfig::default(),
            pressure: PressureConfig::default(),
            lock: LockConfig::default(),
            button: ButtonConfig::default(),
        }
    }
}

/// Position of the tip from the tip tone of a window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PositionConfig {
    /// Below this magnitude of the tip tone (either axis) there is no pen.
    pub min_magnitude: u32,
    /// Below this amplitude of the strongest antenna there is no position.
    pub min_amplitude: u32,
    /// Exponent applied to the amplitudes before the parabola fit.
    pub parabola_exponent: f64,
    /// Exponent applied to the amplitudes before the centroid.
    pub centroid_exponent: f64,
    /// Distance between two antennas.
    pub antenna_pitch_x: f64,
    pub antenna_pitch_y: f64,
    /// Position of the first antenna from the screen edge.
    pub antenna_origin_x: f64,
    pub antenna_origin_y: f64,
    /// Summed amplitude of three antennas with the tip resting lightly on the glass.
    pub tip_signal_reference: f64,
    /// Signal levels (relative to the reference) between which the parabola hands over to
    /// the centroid.
    pub centroid_fade: Range,
    /// Signal levels between which the correction coefficients were measured.
    pub correction_level: Range,
    pub correction_x: Harmonics,
    pub correction_y: Harmonics,
    /// Coefficients of the position estimate in the half cell next to the first and the last
    /// antenna of each axis (`pen::signal::edge_fraction`), fitted for this pen and sensor.
    pub edge_x: [f64; 7],
    pub edge_y: [f64; 7],
    /// Spread the outermost part of that estimate up to the screen edge. The tip cannot be
    /// located once it is at the edge antenna or beyond; with this the pointer reaches the
    /// edge of the screen, at the price of up to 0.4 mm (X) and 0.9 mm (Y) of error within
    /// 0.8 mm of the edge antenna.
    pub edge_reach: bool,
}

/// The error of the centroid over one antenna pitch, in antennas: the first harmonic and its
/// change per unit of signal level, and the second harmonic.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Harmonics {
    pub cos: f64,
    pub sin: f64,
    pub cos_level: f64,
    pub sin_level: f64,
    pub cos2: f64,
    pub sin2: f64,
}

impl Default for PositionConfig {
    fn default() -> Self {
        PositionConfig {
            min_magnitude: 2000,
            min_amplitude: 50,
            parabola_exponent: -0.7,
            centroid_exponent: 0.6,
            antenna_pitch_x: 4.07478,
            antenna_pitch_y: 4.01578,
            antenna_origin_x: 0.44,
            antenna_origin_y: 0.94,
            tip_signal_reference: 3000.0,
            centroid_fade: Range { min: 0.5, max: 0.8 },
            correction_level: Range { min: 0.95, max: 1.85 },
            correction_x: Harmonics { cos: 0.0108, sin: -0.0059, cos_level: -0.1141, sin_level: 0.0157, cos2: 0.0017, sin2: 0.0058 },
            correction_y: Harmonics { cos: 0.0124, sin: 0.0189, cos_level: -0.0928, sin_level: 0.0086, cos2: 0.0009, sin2: 0.0088 },
            edge_x: [0.20209, 0.85665, -0.28139, 0.21690, 0.11292, -0.04835, 0.06634],
            edge_y: [0.20960, 0.87875, -0.27323, 0.22840, 0.11581, -0.04834, 0.07317],
            edge_reach: true,
        }
    }
}

/// Tilt from the distance between the tip and the ring electrode, and the position offset
/// that comes with it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TiltConfig {
    /// Below this magnitude of the ring tone (either axis) the tilt is not updated.
    pub min_magnitude: u32,
    /// Distance of the ring electrode from the tip along the pen, in centimeters.
    pub ring_distance_cm: f64,
    /// Offset of the tip from the measured position, in antennas per antenna of tip-ring
    /// distance, and per its square.
    pub offset_linear: f64,
    pub offset_quadratic: f64,
    /// How much of a new offset is taken per sample.
    pub offset_gain: f64,
}

impl Default for TiltConfig {
    fn default() -> Self {
        TiltConfig { min_magnitude: 10000, ring_distance_cm: 0.9098, offset_linear: 0.0402, offset_quadratic: 0.0089, offset_gain: 0.1 }
    }
}

/// Contact from the force tone, and contact without force from the tip signal.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ContactConfig {
    /// Summed magnitude (both axes, halved) one of the two force tone rows needs in the
    /// position window.
    pub force_tone_min_magnitude: u32,
    /// The same for the two MPP v2 position windows.
    pub mpp2_force_tone_min_magnitude: u32,
    /// The eraser end touches the glass without being pressed (no force) from this summed
    /// magnitude (both axes) of its tone in the position window on, until it falls below
    /// the second value. Used with zero force contact; 0 = never.
    pub eraser_touch_enter: u32,
    pub eraser_touch_exit: u32,
    pub zero_force: ZeroForceConfig,
}

impl Default for ContactConfig {
    fn default() -> Self {
        ContactConfig {
            force_tone_min_magnitude: 10000,
            mpp2_force_tone_min_magnitude: 50000,
            eraser_touch_enter: 500_000,
            eraser_touch_exit: 250_000,
            zero_force: ZeroForceConfig::default(),
        }
    }
}

/// Contact while the pen reports no force. Levels are relative to `reference`, the energy of
/// the tip signal with the tip on the glass.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ZeroForceConfig {
    pub enabled: bool,
    /// Level at which a contact begins.
    pub enter: f64,
    /// Level below which it ends.
    pub exit: f64,
    /// Exponent applied to the amplitudes before the peak of the tip signal is interpolated.
    pub peak_exponent: f64,
    /// 0: learned from the windows in which the pen reports force.
    pub reference: f64,
    /// How many such windows it takes before a learned reference is used.
    pub min_samples: u32,
    /// For how many samples in a row the level has to be above `enter` out of hover.
    pub start_samples: u32,
    /// A stroke that had force goes on while the summed tip signal stays above this fraction
    /// of what it was during the force.
    pub stroke_level: f64,
    /// A stroke that reached this pressure is followed by a pause without zero force contact.
    pub lockout_pressure: f64,
    /// Length of that pause in samples.
    pub lockout_samples: u32,
}

impl Default for ZeroForceConfig {
    fn default() -> Self {
        ZeroForceConfig {
            enabled: false,
            enter: 0.85,
            exit: 0.75,
            peak_exponent: -0.5,
            reference: 0.0,
            min_samples: 20,
            start_samples: 8,
            stroke_level: 0.8,
            lockout_pressure: 0.22,
            lockout_samples: 50,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PressureConfig {
    /// The lowest pressure reported while in contact.
    pub floor: f64,
    /// Summed magnitude (both axes, halved) the pressure tone needs.
    pub tone_min_magnitude: u32,
    /// Summed magnitude (both axes) a digital bit pair needs.
    pub bit_min_magnitude: u32,
}

impl Default for PressureConfig {
    fn default() -> Self {
        PressureConfig { floor: 0.0, tone_min_magnitude: 10000, bit_min_magnitude: 3000 }
    }
}

/// A pen resting on the glass keeps its position.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LockConfig {
    /// 0: off.
    pub radius: f64,
    /// Samples within the radius before the position is held.
    pub count: u32,
}

impl Default for LockConfig {
    fn default() -> Self {
        LockConfig { radius: 0.158, count: 6 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ButtonConfig {
    /// Magnitude (each axis) the button tone needs when the binary window decided nothing.
    pub min_magnitude: u32,
    /// Summed magnitude (both axes, halved) one of the two rows of the binary window needs.
    pub binary_min_magnitude: u32,
    /// The binary window decides only while its stronger row is this many times the weaker.
    pub binary_purity: f64,
    /// No button while other tones of the button window are this many times its tone.
    pub tone_ratio: f64,
}

impl Default for ButtonConfig {
    fn default() -> Self {
        ButtonConfig { min_magnitude: 1000, binary_min_magnitude: 50000, binary_purity: 40.0, tone_ratio: 3.0 }
    }
}

impl Config {
    /// Checks the values the processing divides by or indexes with.
    pub fn validate(&self) -> Result<(), String> {
        let p = &self.pen;
        let positive = [
            ("pen.position.antenna_pitch_x", p.position.antenna_pitch_x),
            ("pen.position.antenna_pitch_y", p.position.antenna_pitch_y),
            ("pen.position.tip_signal_reference", p.position.tip_signal_reference),
            ("pen.tilt.ring_distance_cm", p.tilt.ring_distance_cm),
            ("touch.contacts.aspect.min", self.touch.contacts.aspect.min),
        ];
        for (name, v) in positive {
            if !(v.is_finite() && v > 0.0) {
                return Err(format!("{name} must be positive"));
            }
        }
        if p.axis_resolution == 0 || p.axis_resolution > 1000 {
            return Err("pen.axis_resolution must be 1 .. 1000".into());
        }
        if p.contact.zero_force.peak_exponent == 0.0 || p.position.parabola_exponent == 0.0 {
            return Err("the exponents must not be 0".into());
        }
        let c = &self.touch.contacts;
        if !(c.deactivation_threshold > 0.0 && c.deactivation_threshold <= c.activation_threshold) {
            return Err("touch.contacts: 0 < deactivation_threshold <= activation_threshold".into());
        }
        Ok(())
    }
}

/// What reading an iptsd configuration file did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IptsdConf {
    /// The file names another device in `[Device]`: nothing was applied.
    pub other_device: bool,
    /// Keys that were applied.
    pub applied: Vec<String>,
    /// Keys without a meaning here (unknown, or of parts that do not exist).
    pub ignored: Vec<String>,
}

/// A key asks for behaviour this implementation does not have, or its value does not parse.
#[derive(Clone, Debug, PartialEq)]
pub struct IptsdConfError(pub String);

impl fmt::Display for IptsdConfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for IptsdConfError {}

fn ini_bool(v: &str) -> Option<bool> {
    match v.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

fn ini_int(v: &str) -> Option<i64> {
    let (neg, digits) = match v.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, v.strip_prefix('+').unwrap_or(v)),
    };
    let n = if let Some(hex) = digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).ok()?
    } else {
        digits.parse::<i64>().ok()?
    };
    Some(if neg { -n } else { n })
}

/// The entries of an INI file as iptsd's reader sees them: `(section, key, value)` with section
/// and key in lower case, comments and blank lines dropped.
fn ini_entries(text: &str) -> Result<Vec<(String, String, String)>, IptsdConfError> {
    let mut section = String::new();
    let mut out = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            let end = rest.find(']').ok_or_else(|| IptsdConfError(format!("line {}: section without ']'", n + 1)))?;
            section = rest[..end].trim().to_ascii_lowercase();
            continue;
        }
        let split = line.find(['=', ':']).ok_or_else(|| IptsdConfError(format!("line {}: no '=' in \"{line}\"", n + 1)))?;
        let key = line[..split].trim().to_ascii_lowercase();
        let mut value = line[split + 1..].trim();
        // An inline comment starts with a semicolon after whitespace.
        if let Some(c) = value.find(" ;").or_else(|| value.find("\t;")) {
            value = value[..c].trim_end();
        }
        out.push((section.clone(), key, value.to_string()));
    }
    Ok(out)
}

impl Config {
    /// Applies an iptsd configuration file (INI) on top of this configuration.
    ///
    /// Keys that select behaviour which is fixed here are accepted with the fixed value and
    /// refused with any other; keys of parts that do not exist (touchpad, other pens) are
    /// listed as ignored. Nothing is changed when an error is returned.
    pub fn apply_iptsd_conf(&mut self, text: &str) -> Result<IptsdConf, IptsdConfError> {
        let entries = ini_entries(text)?;
        let mut report = IptsdConf::default();

        for (section, key, value) in &entries {
            if section == "device" && (key == "vendor" || key == "product") {
                let id = ini_int(value).ok_or_else(|| IptsdConfError(format!("[Device] {key} = {value}: not a number")))?;
                if id != i64::from(if key == "vendor" { VENDOR } else { PRODUCT }) {
                    report.other_device = true;
                    return Ok(report);
                }
            }
        }

        let mut next = self.clone();
        for (section, key, value) in &entries {
            let name = format!("[{section}] {key}");
            match next.set_iptsd_key(section, key, value) {
                Ok(true) => report.applied.push(name),
                Ok(false) => report.ignored.push(name),
                Err(why) => return Err(IptsdConfError(format!("{name} = {value}: {why}"))),
            }
        }
        *self = next;
        Ok(report)
    }

    /// Sets one iptsd key. `Ok(false)`: the key has no meaning here.
    fn set_iptsd_key(&mut self, section: &str, key: &str, value: &str) -> Result<bool, String> {
        let real = || value.parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| "not a number".to_string());
        let uint = || ini_int(value).and_then(|v| u32::try_from(v).ok()).ok_or_else(|| "not a non-negative integer".to_string());
        let boolean = || ini_bool(value).ok_or_else(|| "not a boolean".to_string());
        // A switch whose behaviour is fixed here.
        let fixed = |want: bool| match ini_bool(value) {
            Some(v) if v == want => Ok(true),
            Some(_) => Err(format!("this implementation always behaves as with {want}")),
            None => Err("not a boolean".to_string()),
        };
        let fixed_zero = || match value.parse::<f64>().map(|v| v == 0.0) {
            Ok(true) => Ok(true),
            Ok(false) => Err("not supported by this implementation (only 0)".to_string()),
            Err(_) => Err("not a number".to_string()),
        };

        let touch = &mut self.touch;
        let pen = &mut self.pen;
        let pos = &mut pen.position;
        let zf = &mut pen.contact.zero_force;

        match (section, key) {
            ("device", "vendor" | "product") => return Ok(true),

            ("config", "invertx" | "inverty") => return fixed(false),
            ("config", "width" | "height") => return Err("the screen size comes from the device metadata".into()),

            ("touchscreen" | "touch", "disable") => touch.disable = boolean()?,
            ("touchscreen" | "touch", "disableonpalm") => touch.disable_on_palm = boolean()?,
            ("touchscreen" | "touch", "disableonstylus") => touch.disable_on_stylus = boolean()?,
            ("touchscreen" | "touch", "overshoot") => touch.overshoot_cm = real()?,

            ("contacts", "neutral") => {
                return if value == "mode" { Ok(true) } else { Err("only \"mode\" is implemented".into()) };
            }
            ("contacts", "neutralvalue") => touch.contacts.neutral_offset = real()?,
            ("contacts", "activationthreshold") => touch.contacts.activation_threshold = real()?,
            ("contacts", "deactivationthreshold") => touch.contacts.deactivation_threshold = real()?,
            ("contacts", "sizethresholdmin") => touch.contacts.size_change.min = real()?,
            ("contacts", "sizethresholdmax" | "sizethreshold") => touch.contacts.size_change.max = real()?,
            ("contacts", "positionthresholdmin") => touch.contacts.position_change.min = real()?,
            ("contacts", "positionthresholdmax") => touch.contacts.position_change.max = real()?,
            ("contacts", "orientationthresholdmin") => touch.contacts.orientation_change.min = real()?,
            ("contacts", "orientationthresholdmax") => touch.contacts.orientation_change.max = real()?,
            ("contacts", "sizemin") => touch.contacts.size.min = real()?,
            ("contacts", "sizemax") => touch.contacts.size.max = real()?,
            ("contacts", "aspectmin") => touch.contacts.aspect.min = real()?,
            ("contacts", "aspectmax") => touch.contacts.aspect.max = real()?,
            ("contacts", "peaksuppressionradius") => return fixed_zero(),

            ("stylus", "disable") => pen.disable = boolean()?,
            ("stylus", "axisresolution") => {
                pen.axis_resolution = uint()?;
                if pen.axis_resolution == 0 {
                    return Err("the 9600 x 7200 axes are not implemented".into());
                }
            }
            ("stylus" | "dft", "tipdistance") => return fixed_zero(),

            ("dft", "positionminamp") => pos.min_amplitude = uint()?,
            ("dft", "positionminmag") => pos.min_magnitude = uint()?,
            ("dft", "positionexp") => pos.parabola_exponent = real()?,
            ("dft", "centroidexp") => pos.centroid_exponent = real()?,
            ("dft", "antennapitchx") => pos.antenna_pitch_x = real()?,
            ("dft", "antennapitchy") => pos.antenna_pitch_y = real()?,
            ("dft", "antennaoriginx") => pos.antenna_origin_x = real()?,
            ("dft", "antennaoriginy") => pos.antenna_origin_y = real()?,
            ("dft", "tipsignalreference") => pos.tip_signal_reference = real()?,
            ("dft", "centroidfademin") => pos.centroid_fade.min = real()?,
            ("dft", "centroidfademax") => pos.centroid_fade.max = real()?,
            ("dft", "cellcorrectionlevelmin") => pos.correction_level.min = real()?,
            ("dft", "cellcorrectionlevelmax") => pos.correction_level.max = real()?,
            ("dft", "cellcorrectioncosx") => pos.correction_x.cos = real()?,
            ("dft", "cellcorrectionsinx") => pos.correction_x.sin = real()?,
            ("dft", "cellcorrectioncoslevelx") => pos.correction_x.cos_level = real()?,
            ("dft", "cellcorrectionsinlevelx") => pos.correction_x.sin_level = real()?,
            ("dft", "cellcorrectioncos2x") => pos.correction_x.cos2 = real()?,
            ("dft", "cellcorrectionsin2x") => pos.correction_x.sin2 = real()?,
            ("dft", "cellcorrectioncosy") => pos.correction_y.cos = real()?,
            ("dft", "cellcorrectionsiny") => pos.correction_y.sin = real()?,
            ("dft", "cellcorrectioncoslevely") => pos.correction_y.cos_level = real()?,
            ("dft", "cellcorrectionsinlevely") => pos.correction_y.sin_level = real()?,
            ("dft", "cellcorrectioncos2y") => pos.correction_y.cos2 = real()?,
            ("dft", "cellcorrectionsin2y") => pos.correction_y.sin2 = real()?,
            (
                "dft",
                "cellcorrectionasymmetrymax"
                | "cellcorrectioncosasymmetryx"
                | "cellcorrectionsinasymmetryx"
                | "cellcorrectioncosasymmetryy"
                | "cellcorrectionsinasymmetryy"
                | "hoverjitterradius",
            ) => return fixed_zero(),

            ("dft", "tiltminmag") => pen.tilt.min_magnitude = uint()?,
            ("dft", "mpp2tiltdistance") => pen.tilt.ring_distance_cm = real()?,
            ("dft", "tiltoffsetlinear") => pen.tilt.offset_linear = real()?,
            ("dft", "tiltoffsetquadratic") => pen.tilt.offset_quadratic = real()?,
            ("dft", "tiltoffsetgain") => pen.tilt.offset_gain = real()?,

            ("dft", "forcetoneminmag") => pen.contact.force_tone_min_magnitude = uint()?,
            ("dft", "mpp2contactminmag") => pen.contact.mpp2_force_tone_min_magnitude = uint()?,
            ("dft", "zeroforcecontact") => zf.enabled = boolean()?,
            ("dft", "zeroforceenter") => zf.enter = real()?,
            ("dft", "zeroforceexit") => zf.exit = real()?,
            ("dft", "zeroforceexp") => zf.peak_exponent = real()?,
            ("dft", "zeroforcereference") => zf.reference = real()?,
            ("dft", "zeroforceminsamples") => zf.min_samples = uint()?,
            ("dft", "zeroforcestartsamples") => zf.start_samples = uint()?,
            ("dft", "zeroforcestrokelevel") => zf.stroke_level = real()?,
            ("dft", "zeroforcelockoutpressure") => zf.lockout_pressure = real()?,
            ("dft", "zeroforcelockoutsamples") => zf.lockout_samples = uint()?,

            ("dft", "contactminpressure") => pen.pressure.floor = real()?,
            ("dft", "freqminmag") => pen.pressure.tone_min_magnitude = uint()?,
            ("dft", "pressurebitminmag") => pen.pressure.bit_min_magnitude = uint()?,

            ("dft", "jitterradius") => pen.lock.radius = real()?,
            ("dft", "jittercount") => pen.lock.count = uint()?,

            ("dft", "buttonminmag") => pen.button.min_magnitude = uint()?,
            ("dft", "mpp2buttonminmag") => pen.button.binary_min_magnitude = uint()?,
            ("dft", "mpp2buttonpurity") => pen.button.binary_purity = real()?,
            ("dft", "buttontoneratio") => pen.button.tone_ratio = real()?,

            (
                "dft",
                "extrapositionwindows"
                | "tiltsmoothing"
                | "tiltwhilehovering"
                | "positioncentroidx"
                | "positioncentroidy"
                | "forcetonecontact"
                | "mpp2pressuredecode"
                | "pressureramp"
                | "zeroforcepressure"
                | "zeroforcestrokesum",
            ) => return fixed(true),
            ("dft", "inkfilter" | "allowsplitevents") => return fixed(false),

            // The tilt of the position window is never used, the suppression is off, and
            // there is no touchpad.
            ("dft", "tiltdistance") | ("contacts", "peaksuppressionfactor") | ("touchpad", _) => return Ok(false),
            _ => return Ok(false),
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_drop_in() {
        let mut c = Config::default();
        let text = "# Written by sp9-helperd\n[Device]\nVendor = 0x045E\nProduct = 0x0C52\n\n[Touchscreen]\nDisableOnStylus = false\n\n\
                    [Contacts]\nActivationThreshold = 22\nDeactivationThreshold = 18\n\n\
                    [DFT]\nZeroForceContact = true\nZeroForceEnter = 0.85\nZeroForceExit = 0.75\nContactMinPressure = 0.03\nZeroForceReference = 1234.5";
        let r = c.apply_iptsd_conf(text).unwrap();
        assert!(!r.other_device && r.ignored.is_empty());
        assert_eq!(r.applied.len(), 10);
        assert!(!c.touch.disable_on_stylus);
        assert_eq!((c.touch.contacts.activation_threshold, c.touch.contacts.deactivation_threshold), (22.0, 18.0));
        let zf = &c.pen.contact.zero_force;
        assert_eq!((zf.enabled, zf.enter, zf.exit, zf.reference, c.pen.pressure.floor), (true, 0.85, 0.75, 1234.5, 0.03));
        c.validate().unwrap();
    }

    #[test]
    fn other_devices_and_refusals() {
        let mut c = Config::default();
        let r = c.apply_iptsd_conf("[Device]\nVendor = 0x045E\nProduct = 0x0C1B\n[DFT]\nInkFilter = true\n").unwrap();
        assert!(r.other_device);
        assert_eq!(c, Config::default());

        assert!(c.apply_iptsd_conf("[DFT]\nInkFilter = true\n").unwrap_err().0.contains("inkfilter"));
        assert!(c.apply_iptsd_conf("[DFT]\nJitterRadius = wide\n").is_err());
        // Nothing is applied from a file with an error in it.
        assert!(c.apply_iptsd_conf("[DFT]\nJitterRadius = 1\nExtraPositionWindows = false\n").is_err());
        assert_eq!(c, Config::default());

        let r = c.apply_iptsd_conf("[Touchpad]\nDisable = true\n[DFT]\nNoSuchKey = 1 ; comment\nInkFilter = off\n").unwrap();
        assert_eq!(r.ignored, vec!["[touchpad] disable", "[dft] nosuchkey"]);
        assert_eq!(r.applied, vec!["[dft] inkfilter"]);
    }

    #[test]
    fn toml_round_trip_and_unknown_fields() {
        let c = Config::default();
        let text = toml::to_string(&c).unwrap();
        assert_eq!(toml::from_str::<Config>(&text).unwrap(), c);
        let partial: Config = toml::from_str("[pen.contact.zero_force]\nenabled = true\nreference = 5e6\n").unwrap();
        assert!(partial.pen.contact.zero_force.enabled);
        assert_eq!(partial.pen.contact.zero_force.exit, 0.75);
        assert!(toml::from_str::<Config>("[pen]\nink_filter = true\n").is_err());
    }

    /// The built-in configuration is iptsd's defaults plus the platform preset of this
    /// repository: every key of the preset is either applied without changing anything, or
    /// names behaviour that is fixed here.
    #[test]
    fn defaults_are_the_platform_preset() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../userspace/platform/iptsd/surface-pro-9.conf");
        let Ok(text) = std::fs::read_to_string(path) else {
            eprintln!("skipped: {path} is not there");
            return;
        };
        let mut c = Config::default();
        let r = c.apply_iptsd_conf(&text).unwrap();
        assert!(!r.other_device);
        assert_eq!(r.ignored, Vec::<String>::new());
        assert_eq!(c, Config::default());
        c.validate().unwrap();
    }
}
