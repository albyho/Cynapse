//! Per-open Step indexes shared by playback planning and progress writes.

use crate::course::{Course, LoadedStep, PlaybackStep};
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub(super) struct CourseStepIndex {
    by_unit: HashMap<String, Vec<usize>>,
    by_step: HashMap<String, HashMap<String, usize>>,
}

impl CourseStepIndex {
    pub(super) fn new(course: &Course) -> Self {
        let mut by_unit = HashMap::<String, Vec<usize>>::new();
        let mut by_step = HashMap::<String, HashMap<String, usize>>::new();
        for (index, step) in course.steps.iter().enumerate() {
            by_unit
                .entry(step.unit_path.clone())
                .or_default()
                .push(index);
            by_step
                .entry(step.unit_path.clone())
                .or_default()
                .insert(step.step.step_id.clone(), index);
        }
        Self { by_unit, by_step }
    }

    pub(super) fn step_indices(&self, unit_path: &str) -> &[usize] {
        self.by_unit
            .get(unit_path)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub(super) fn step<'a>(
        &self,
        course: &'a Course,
        unit_path: &str,
        step_id: &str,
    ) -> Option<&'a LoadedStep> {
        let index = self.by_step.get(unit_path)?.get(step_id)?;
        course.steps.get(*index)
    }

    pub(super) fn following_playback_step(
        &self,
        course: &Course,
        unit_path: &str,
        step_id: &str,
    ) -> Option<PlaybackStep> {
        let index = self.by_step.get(unit_path)?.get(step_id)?;
        let unit_indices = self.by_unit.get(unit_path)?;
        let position = unit_indices
            .iter()
            .position(|candidate| candidate == index)?;
        let next = *unit_indices.get(position + 1)?;
        course.playback_step(next)
    }
}
