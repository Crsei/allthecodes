use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::prelude::Widget;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::ui::theme::ThemeColors;

pub(crate) const BRAND_COLUMN_WIDTH: u16 = 13;
pub(crate) const BRAND_COLUMN_HEIGHT: u16 = 6;
pub(crate) const FRAME_INTERVAL_MS: u64 = 80;

const STEPS_PER_LETTER: u64 = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Glyph {
    letter: char,
    mask: u16,
}

const WORD: [Glyph; 11] = [
    Glyph {
        letter: 'A',
        mask: 0b010_111_101,
    },
    Glyph {
        letter: 'L',
        mask: 0b100_100_111,
    },
    Glyph {
        letter: 'L',
        mask: 0b100_100_111,
    },
    Glyph {
        letter: 'T',
        mask: 0b111_010_010,
    },
    Glyph {
        letter: 'H',
        mask: 0b101_111_101,
    },
    Glyph {
        letter: 'E',
        mask: 0b111_110_111,
    },
    Glyph {
        letter: 'C',
        mask: 0b111_100_111,
    },
    Glyph {
        letter: 'O',
        mask: 0b111_101_111,
    },
    Glyph {
        letter: 'D',
        mask: 0b110_101_110,
    },
    Glyph {
        letter: 'E',
        mask: 0b111_110_111,
    },
    Glyph {
        letter: 'S',
        mask: 0b110_010_011,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MorphPhase {
    Hold,
    FadeOut,
    FadeIn,
    Settle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CellLevel {
    Off,
    Low,
    Medium,
    On,
}

impl CellLevel {
    fn symbol(self) -> &'static str {
        match self {
            Self::Off => "░░",
            Self::Low => "▒▒",
            Self::Medium => "▓▓",
            Self::On => "██",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BrandPalette {
    active: [Color; 3],
    inactive: Color,
    outline: Color,
    tracker: Color,
}

impl BrandPalette {
    fn from_theme(colors: &ThemeColors) -> Self {
        match colors.info {
            Color::Reset => Self {
                active: [Color::Reset; 3],
                inactive: Color::Reset,
                outline: Color::Reset,
                tracker: Color::Reset,
            },
            Color::Rgb(_, _, _) => Self {
                active: [
                    Color::Rgb(0, 101, 253),
                    Color::Rgb(0, 221, 251),
                    Color::Rgb(1, 230, 204),
                ],
                inactive: colors.inactive,
                outline: colors.border,
                tracker: colors.info,
            },
            _ => Self {
                active: [Color::Blue, Color::Cyan, Color::Green],
                inactive: Color::DarkGray,
                outline: Color::DarkGray,
                tracker: Color::Cyan,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LogoFrame {
    from_index: usize,
    active_index: usize,
    phase: MorphPhase,
}

impl LogoFrame {
    pub(crate) fn active_index(self) -> usize {
        self.active_index
    }

    #[cfg(test)]
    pub(crate) fn current_letter(self) -> char {
        WORD[self.active_index].letter
    }

    #[cfg(test)]
    pub(crate) fn phase(self) -> MorphPhase {
        self.phase
    }

    pub(crate) fn cell_level(self, row: usize, column: usize) -> CellLevel {
        let from = WORD[self.from_index];
        let to = WORD[(self.from_index + 1) % WORD.len()];
        let from_on = mask_has(from.mask, row, column);
        let to_on = mask_has(to.mask, row, column);

        match self.phase {
            MorphPhase::Hold => level(from_on),
            MorphPhase::Settle => level(to_on),
            MorphPhase::FadeOut if from.mask == to.mask => {
                if from_on {
                    CellLevel::Medium
                } else {
                    CellLevel::Off
                }
            }
            MorphPhase::FadeIn if from.mask == to.mask => {
                if from_on {
                    CellLevel::Low
                } else {
                    CellLevel::Off
                }
            }
            MorphPhase::FadeOut => match (from_on, to_on) {
                (true, true) => CellLevel::On,
                (true, false) => CellLevel::Medium,
                (false, true) => CellLevel::Low,
                (false, false) => CellLevel::Off,
            },
            MorphPhase::FadeIn => match (from_on, to_on) {
                (true, true) => CellLevel::On,
                (true, false) => CellLevel::Off,
                (false, true) => CellLevel::Medium,
                (false, false) => CellLevel::Off,
            },
        }
    }
}

fn level(on: bool) -> CellLevel {
    if on {
        CellLevel::On
    } else {
        CellLevel::Off
    }
}

fn mask_has(mask: u16, row: usize, column: usize) -> bool {
    debug_assert!(row < 3 && column < 3);
    let bit = 8 - (row * 3 + column);
    mask & (1 << bit) != 0
}

pub(crate) fn frame_at(step: u64) -> LogoFrame {
    let from_index = ((step / STEPS_PER_LETTER) % WORD.len() as u64) as usize;
    let step_in_segment = step % STEPS_PER_LETTER;
    let phase = match step_in_segment {
        0..=5 => MorphPhase::Hold,
        6 => MorphPhase::FadeOut,
        7 => MorphPhase::FadeIn,
        8 => MorphPhase::Settle,
        _ => unreachable!("step modulo nine is always in range"),
    };
    let active_index = if phase == MorphPhase::Settle {
        (from_index + 1) % WORD.len()
    } else {
        from_index
    };
    LogoFrame {
        from_index,
        active_index,
        phase,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct WelcomeLogoState {
    step: u64,
    carry_ms: u64,
}

impl WelcomeLogoState {
    pub(crate) fn tick(&mut self, elapsed_ms: u64) -> bool {
        self.carry_ms = self.carry_ms.saturating_add(elapsed_ms);
        let advances = self.carry_ms / FRAME_INTERVAL_MS;
        self.carry_ms %= FRAME_INTERVAL_MS;
        self.step = self.step.wrapping_add(advances);
        advances > 0
    }

    pub(crate) fn frame(&self) -> LogoFrame {
        frame_at(self.step)
    }

    #[cfg(test)]
    pub(crate) fn step(&self) -> u64 {
        self.step
    }
}

pub(crate) fn render_brand_logo(
    area: Rect,
    buf: &mut Buffer,
    frame: LogoFrame,
    colors: &ThemeColors,
) {
    let palette = BrandPalette::from_theme(colors);
    let outline = Style::default().fg(palette.outline);
    let mut lines = vec![Line::styled("╭────────╮", outline)];

    for row in 0..3 {
        let mut spans = vec![Span::styled("│", outline)];
        for column in 0..3 {
            if column > 0 {
                spans.push(Span::raw(" "));
            }
            let level = frame.cell_level(row, column);
            let modifier = match level {
                CellLevel::Off | CellLevel::Low => Modifier::DIM,
                CellLevel::Medium => Modifier::empty(),
                CellLevel::On => Modifier::BOLD,
            };
            let color = if level == CellLevel::Off {
                palette.inactive
            } else {
                palette.active[row]
            };
            spans.push(Span::styled(
                level.symbol(),
                Style::default().fg(color).add_modifier(modifier),
            ));
        }
        spans.push(Span::styled("│", outline));
        lines.push(Line::from(spans));
    }
    lines.push(Line::styled("╰────────╯", outline));

    let tracker = WORD
        .iter()
        .enumerate()
        .map(|(index, glyph)| {
            let style = if index == frame.active_index() {
                Style::default()
                    .fg(palette.tracker)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                Style::default().fg(colors.dim).add_modifier(Modifier::DIM)
            };
            Span::styled(glyph.letter.to_string(), style)
        })
        .collect::<Vec<_>>();
    lines.push(Line::from(tracker));

    Paragraph::new(lines)
        .alignment(Alignment::Center)
        .render(area, buf);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::ThemeColors;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Modifier};

    #[test]
    fn allthecodes_logo_keyframes() {
        let colors = crate::ui::theme::ThemeProvider::with_name(crate::ui::theme::ThemeName::Dark);
        let rendered = [0, 6, 7, 8, 15, 16, 17, 96, 97, 98]
            .into_iter()
            .map(|step| format!("step={step}\n{}", render_for_test(step, colors.colors())))
            .collect::<Vec<_>>()
            .join("\n\n");
        insta::assert_snapshot!("allthecodes_logo_keyframes", rendered);
    }

    #[test]
    fn truecolor_palette_uses_sampled_vertical_gradient() {
        let colors = crate::ui::theme::ThemeProvider::with_name(crate::ui::theme::ThemeName::Dark);
        let palette = BrandPalette::from_theme(colors.colors());
        assert_eq!(
            palette.active,
            [
                Color::Rgb(0, 101, 253),
                Color::Rgb(0, 221, 251),
                Color::Rgb(1, 230, 204),
            ]
        );
    }

    #[test]
    fn ansi_and_reset_palettes_do_not_emit_rgb() {
        let ansi =
            crate::ui::theme::ThemeProvider::with_name(crate::ui::theme::ThemeName::DarkAnsi);
        let ansi_palette = BrandPalette::from_theme(ansi.colors());
        assert_eq!(
            ansi_palette.active,
            [Color::Blue, Color::Cyan, Color::Green]
        );
        assert_eq!(ansi_palette.inactive, Color::DarkGray);
        assert_eq!(ansi_palette.outline, Color::DarkGray);
        assert_eq!(ansi_palette.tracker, Color::Cyan);

        let mut reset = ansi.colors().clone();
        reset.info = Color::Reset;
        let reset_palette = BrandPalette::from_theme(&reset);
        assert_eq!(reset_palette.active, [Color::Reset; 3]);
        assert_eq!(reset_palette.inactive, Color::Reset);
        assert_eq!(reset_palette.outline, Color::Reset);
        assert_eq!(reset_palette.tracker, Color::Reset);
    }

    #[test]
    fn every_builtin_theme_keeps_lit_and_unlit_cells_distinct() {
        for name in crate::ui::theme::ThemeName::ALL {
            let provider = crate::ui::theme::ThemeProvider::with_name(*name);
            let palette = BrandPalette::from_theme(provider.colors());
            assert!(
                palette
                    .active
                    .iter()
                    .all(|active| *active != palette.inactive),
                "theme {name:?} must keep active cells distinct",
            );
        }
    }

    #[test]
    fn undersized_area_is_clipped_without_panicking() {
        let colors = crate::ui::theme::ThemeProvider::with_name(crate::ui::theme::ThemeName::Dark);
        for area in [Rect::new(0, 0, 5, 3), Rect::new(0, 0, 13, 5)] {
            let mut buf = Buffer::empty(area);
            render_brand_logo(area, &mut buf, frame_at(0), colors.colors());
        }
    }

    #[test]
    fn repeated_l_moves_the_tracker_highlight_to_the_second_index() {
        let colors = crate::ui::theme::ThemeProvider::with_name(crate::ui::theme::ThemeName::Dark);
        let area = Rect::new(0, 0, BRAND_COLUMN_WIDTH, BRAND_COLUMN_HEIGHT);

        let mut first_l = Buffer::empty(area);
        render_brand_logo(area, &mut first_l, frame_at(8), colors.colors());
        assert!(first_l[(2, 5)]
            .style()
            .add_modifier
            .contains(Modifier::BOLD | Modifier::UNDERLINED));

        let mut second_l = Buffer::empty(area);
        render_brand_logo(area, &mut second_l, frame_at(17), colors.colors());
        assert!(second_l[(3, 5)]
            .style()
            .add_modifier
            .contains(Modifier::BOLD | Modifier::UNDERLINED));
        assert!(!second_l[(2, 5)]
            .style()
            .add_modifier
            .contains(Modifier::UNDERLINED));
    }

    #[test]
    fn word_sequence_preserves_duplicate_positions() {
        assert_eq!(
            WORD.iter().map(|glyph| glyph.letter).collect::<String>(),
            "ALLTHECODES"
        );
        assert_eq!(WORD[1].mask, WORD[2].mask);
        assert_ne!(frame_at(8).active_index(), frame_at(17).active_index());
    }

    #[test]
    fn source_icon_a_mask_is_exact() {
        assert_eq!(WORD[0].mask, 0b010_111_101);
        assert_eq!(frame_at(0).current_letter(), 'A');
        assert_eq!(frame_at(0).cell_level(0, 1), CellLevel::On);
        assert_eq!(frame_at(0).cell_level(0, 0), CellLevel::Off);
        assert_eq!(frame_at(0).cell_level(2, 1), CellLevel::Off);
    }

    #[test]
    fn a_to_l_uses_two_density_transition_frames() {
        assert_eq!(frame_at(6).phase(), MorphPhase::FadeOut);
        assert_eq!(frame_at(6).cell_level(0, 1), CellLevel::Medium);
        assert_eq!(frame_at(6).cell_level(0, 0), CellLevel::Low);
        assert_eq!(frame_at(7).cell_level(0, 1), CellLevel::Off);
        assert_eq!(frame_at(7).cell_level(0, 0), CellLevel::Medium);
        assert_eq!(frame_at(8).current_letter(), 'L');
        assert_eq!(frame_at(8).cell_level(0, 0), CellLevel::On);
    }

    #[test]
    fn repeated_l_pulses_and_advances_tracker() {
        assert_eq!(frame_at(15).current_letter(), 'L');
        assert_eq!(frame_at(15).cell_level(0, 0), CellLevel::Medium);
        assert_eq!(frame_at(16).cell_level(0, 0), CellLevel::Low);
        assert_eq!(frame_at(17).cell_level(0, 0), CellLevel::On);
        assert_eq!(frame_at(15).active_index(), 1);
        assert_eq!(frame_at(17).active_index(), 2);
    }

    #[test]
    fn state_advances_only_after_eighty_accumulated_milliseconds() {
        let mut state = WelcomeLogoState::default();
        for _ in 0..4 {
            assert!(!state.tick(16));
        }
        assert!(state.tick(16));
        assert_eq!(state.step(), 1);
        assert!(state.tick(160));
        assert_eq!(state.step(), 3);
    }

    fn render_for_test(step: u64, colors: &ThemeColors) -> String {
        let area = Rect::new(0, 0, BRAND_COLUMN_WIDTH, BRAND_COLUMN_HEIGHT);
        let mut buf = Buffer::empty(area);
        render_brand_logo(area, &mut buf, frame_at(step), colors);
        (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .fold(String::new(), |mut row, symbol| {
                        row.push_str(symbol);
                        row
                    })
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
