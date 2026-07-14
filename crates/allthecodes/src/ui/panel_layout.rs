//! Shared size presets for TUI panels and overlays.

use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelSizePreset {
    HistorySearch,
    AgentTree,
    PermissionDialog,
    QuestionDialog,
    BypassPermissionsMode,
    BetterViewPanel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelSizeSpec {
    pub min_width: u16,
    pub max_width: u16,
    pub min_height: u16,
    pub max_height: u16,
    pub width_percent: Option<u16>,
    pub horizontal_padding: u16,
    pub vertical_padding: u16,
}

impl PanelSizeSpec {
    pub const fn fixed(min_width: u16, max_width: u16, min_height: u16, max_height: u16) -> Self {
        Self {
            min_width,
            max_width,
            min_height,
            max_height,
            width_percent: None,
            horizontal_padding: 0,
            vertical_padding: 0,
        }
    }

    pub const fn with_width_percent(mut self, percent: u16) -> Self {
        self.width_percent = Some(percent);
        self
    }

    pub const fn with_padding(mut self, horizontal: u16, vertical: u16) -> Self {
        self.horizontal_padding = horizontal;
        self.vertical_padding = vertical;
        self
    }

    pub fn resolve_rect(self, area: Rect, preferred_height: u16) -> Option<Rect> {
        if area.width == 0 || area.height == 0 {
            return None;
        }

        let width = self.resolve_width(area);
        let height = self.resolve_height(area, preferred_height);

        Some(centered_rect(area, width, height))
    }

    pub fn resolve_prompt_adjacent_rect(
        self,
        area: Rect,
        prompt_area: Rect,
        preferred_height: u16,
    ) -> Option<Rect> {
        if area.width == 0 || area.height == 0 {
            return None;
        }

        let available_above_prompt = prompt_available_above(area, prompt_area);
        let min_height = self.resolve_min_height(area);
        if available_above_prompt < min_height {
            return self.resolve_rect(area, preferred_height);
        }

        let width = self.resolve_width(area);
        let height = self
            .resolve_height(area, preferred_height)
            .min(available_above_prompt)
            .max(1);

        Some(prompt_adjacent_rect(area, prompt_area, width, height))
    }

    pub fn resolve_prompt_or_centered_rect(
        self,
        area: Rect,
        prompt_area: Option<Rect>,
        preferred_height: u16,
    ) -> Option<Rect> {
        match prompt_area {
            Some(prompt_area) => {
                self.resolve_prompt_adjacent_rect(area, prompt_area, preferred_height)
            }
            None => self.resolve_rect(area, preferred_height),
        }
    }

    fn resolve_width(self, area: Rect) -> u16 {
        let max_width = self.max_width.max(self.min_width);
        let available_width = area.width.saturating_sub(self.horizontal_padding);
        let target_width = self
            .width_percent
            .map(|percent| area.width.saturating_mul(percent.min(100)) / 100)
            .unwrap_or(available_width);
        target_width
            .max(self.min_width)
            .min(max_width)
            .min(area.width)
            .max(1)
    }

    fn resolve_min_height(self, area: Rect) -> u16 {
        let max_height = self.max_height.max(self.min_height);
        let available_height = area.height.saturating_sub(self.vertical_padding);
        self.min_height
            .min(max_height)
            .min(available_height.max(1))
            .min(area.height)
            .max(1)
    }

    fn resolve_height(self, area: Rect, preferred_height: u16) -> u16 {
        let max_height = self.max_height.max(self.min_height);
        let available_height = area.height.saturating_sub(self.vertical_padding);
        preferred_height
            .max(self.min_height)
            .min(max_height)
            .min(available_height.max(1))
            .min(area.height)
            .max(1)
    }
}

impl PanelSizePreset {
    pub const fn spec(self) -> PanelSizeSpec {
        match self {
            Self::HistorySearch => PanelSizeSpec::fixed(20, 148, 8, 28).with_padding(4, 4),
            Self::AgentTree => PanelSizeSpec::fixed(24, 140, 8, 32).with_padding(4, 2),
            Self::PermissionDialog => PanelSizeSpec::fixed(56, 150, 8, u16::MAX).with_padding(2, 0),
            Self::QuestionDialog => {
                PanelSizeSpec::fixed(56, u16::MAX, 8, 18).with_width_percent(90)
            }
            Self::BypassPermissionsMode => {
                PanelSizeSpec::fixed(64, u16::MAX, 8, 18).with_width_percent(90)
            }
            Self::BetterViewPanel => PanelSizeSpec::fixed(140, 140, 1, u16::MAX),
        }
    }
}

pub fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width: width.min(area.width),
        height: height.min(area.height),
    }
}

const PROMPT_ADJACENT_GAP: u16 = 1;

fn prompt_available_above(area: Rect, prompt_area: Rect) -> u16 {
    prompt_area
        .y
        .min(area.y.saturating_add(area.height))
        .saturating_sub(area.y)
        .saturating_sub(PROMPT_ADJACENT_GAP)
}

pub fn prompt_adjacent_rect(area: Rect, prompt_area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    let prompt_top = prompt_area.y.min(area.y.saturating_add(area.height));
    let available_above_prompt = prompt_available_above(area, prompt_area);
    if height > available_above_prompt {
        return centered_rect(area, width, height);
    }

    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: prompt_top
            .saturating_sub(PROMPT_ADJACENT_GAP)
            .saturating_sub(height)
            .max(area.y),
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_width_preset_matches_dialog_defaults() {
        let area = Rect::new(0, 0, 100, 24);
        let question = PanelSizePreset::QuestionDialog
            .spec()
            .resolve_rect(area, 18)
            .expect("rect");
        let bypass = PanelSizePreset::BypassPermissionsMode
            .spec()
            .resolve_rect(area, 18)
            .expect("rect");

        assert_eq!(question.width, 90);
        assert_eq!(bypass.width, 90);
        assert_eq!(question.height, 18);
        assert_eq!(bypass.height, 18);
    }

    #[test]
    fn tiny_terminal_rect_stays_inside_area() {
        let area = Rect::new(2, 3, 6, 4);
        let rect = PanelSizePreset::PermissionDialog
            .spec()
            .resolve_rect(area, 20)
            .expect("rect");

        assert_eq!(rect, Rect::new(2, 3, 6, 4));
    }

    #[test]
    fn prompt_adjacent_rect_sits_above_prompt_when_space_allows() {
        let area = Rect::new(0, 0, 100, 30);
        let prompt_area = Rect::new(0, 24, 100, 3);
        let rect = prompt_adjacent_rect(area, prompt_area, 50, 8);

        assert_eq!(rect.width, 50);
        assert_eq!(rect.height, 8);
        assert!(rect.y + rect.height <= prompt_area.y - 1);
    }

    #[test]
    fn prompt_adjacent_rect_falls_back_to_center_when_space_is_short() {
        let area = Rect::new(0, 0, 100, 30);
        let prompt_area = Rect::new(0, 5, 100, 3);
        let rect = prompt_adjacent_rect(area, prompt_area, 50, 8);

        assert_eq!(rect, centered_rect(area, 50, 8));
    }

    #[test]
    fn prompt_adjacent_rect_stays_inside_tiny_terminal() {
        let area = Rect::new(4, 3, 8, 4);
        let prompt_area = Rect::new(4, 6, 8, 1);
        let rect = prompt_adjacent_rect(area, prompt_area, 20, 10);

        assert!(rect.x >= area.x);
        assert!(rect.y >= area.y);
        assert!(rect.x + rect.width <= area.x + area.width);
        assert!(rect.y + rect.height <= area.y + area.height);
    }

    #[test]
    fn spec_resolves_prompt_adjacent_rect_or_centered_fallback() {
        let spec = PanelSizeSpec::fixed(20, 60, 6, 12).with_padding(4, 2);
        let area = Rect::new(0, 0, 100, 30);
        let prompt_area = Rect::new(0, 24, 100, 3);
        let adjacent = spec
            .resolve_prompt_adjacent_rect(area, prompt_area, 10)
            .expect("adjacent rect");
        assert!(adjacent.y + adjacent.height <= prompt_area.y - 1);
        assert_eq!(adjacent.height, 10);

        let prompt_area = Rect::new(0, 4, 100, 3);
        let centered = spec
            .resolve_prompt_adjacent_rect(area, prompt_area, 10)
            .expect("centered rect");
        assert_eq!(centered, spec.resolve_rect(area, 10).expect("rect"));
    }

    #[test]
    fn preset_values_match_documented_defaults() {
        assert_eq!(
            PanelSizePreset::HistorySearch.spec(),
            PanelSizeSpec::fixed(20, 148, 8, 28).with_padding(4, 4)
        );
        assert_eq!(
            PanelSizePreset::AgentTree.spec(),
            PanelSizeSpec::fixed(24, 140, 8, 32).with_padding(4, 2)
        );
        assert_eq!(PanelSizePreset::BetterViewPanel.spec().min_width, 140);
    }
}
