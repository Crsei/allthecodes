//! Shared size presets for TUI panels and overlays.

use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelSizePreset {
    CommandSurface,
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

        let max_width = self.max_width.max(self.min_width);
        let available_width = area.width.saturating_sub(self.horizontal_padding);
        let target_width = self
            .width_percent
            .map(|percent| area.width.saturating_mul(percent.min(100)) / 100)
            .unwrap_or(available_width);
        let width = target_width
            .max(self.min_width)
            .min(max_width)
            .min(area.width)
            .max(1);

        let max_height = self.max_height.max(self.min_height);
        let available_height = area.height.saturating_sub(self.vertical_padding);
        let height = preferred_height
            .max(self.min_height)
            .min(max_height)
            .min(available_height.max(1))
            .min(area.height)
            .max(1);

        Some(centered_rect(area, width, height))
    }
}

impl PanelSizePreset {
    pub const fn spec(self) -> PanelSizeSpec {
        match self {
            Self::CommandSurface => PanelSizeSpec::fixed(32, 148, 5, 40).with_padding(4, 2),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_overlay_preset_clamps_to_area_and_limits() {
        let area = Rect::new(0, 0, 160, 40);
        let rect = PanelSizePreset::CommandSurface
            .spec()
            .resolve_rect(area, 40)
            .expect("rect");

        assert_eq!(rect, Rect::new(6, 1, 148, 38));
    }

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
