// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! The touch pipeline: heatmap → detections ([`detect`], [`fit`]) → tracked contacts ([`track`]).

pub mod detect;
pub mod fit;
pub mod track;

pub use detect::Detection;
pub use track::Contact;

use crate::config::{ContactsConfig, Geometry};
use crate::frame::Heatmap;

#[derive(Debug)]
pub struct TouchPipeline {
    detector: detect::Detector,
    tracker: track::Tracker,
    detections: Vec<Detection>,
    contacts: Vec<Contact>,
}

impl TouchPipeline {
    pub fn new(config: &ContactsConfig, geometry: &Geometry) -> TouchPipeline {
        TouchPipeline {
            detector: detect::Detector::new(config.activation_threshold, config.deactivation_threshold, config.neutral_offset),
            tracker: track::Tracker::new(config, geometry),
            detections: Vec::new(),
            contacts: Vec::new(),
        }
    }

    /// Changes thresholds and limits without disturbing the contacts on the screen.
    pub fn set_config(&mut self, config: &ContactsConfig, geometry: &Geometry) {
        self.detector.set_thresholds(config.activation_threshold, config.deactivation_threshold, config.neutral_offset);
        self.tracker.set_config(config, geometry);
    }

    /// The contacts of an image. `None` for an image without a size (the first one after the
    /// device started: its dimensions report has not arrived yet), which is not a frame.
    pub fn process(&mut self, heatmap: &Heatmap<'_>) -> Option<&[Contact]> {
        if heatmap.rows == 0 || heatmap.columns == 0 {
            return None;
        }
        self.detector.detect(heatmap, &mut self.detections);
        self.tracker.update(&self.detections, &mut self.contacts);
        Some(&self.contacts)
    }
}
