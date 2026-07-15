use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::prelude::Widget;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub(crate) const BRAND_COLUMN_WIDTH: u16 = 13;
pub(crate) const BRAND_COLUMN_HEIGHT: u16 = 5;
pub(crate) const FRAME_INTERVAL_MS: u64 = 80;

const GLYPH_SIZE: usize = 6;
const GRID_SIZE: usize = 3;
const SUBCELL_SIZE: usize = 2;
const HOLD_STEPS: u64 = 6;
const MORPH_STEPS: u8 = 5;
const SETTLE_STEP: u64 = HOLD_STEPS + MORPH_STEPS as u64;
const STEPS_PER_LETTER: u64 = SETTLE_STEP + 1;

/// Center-out order used to replace source pixels with destination pixels.
/// Each value is the first morph frame on which that subpixel settles.
const CENTER_OUT_RANKS: [[u8; GLYPH_SIZE]; GLYPH_SIZE] = [
    [5, 4, 3, 3, 4, 5],
    [4, 3, 2, 2, 3, 4],
    [3, 2, 1, 1, 2, 3],
    [3, 2, 1, 1, 2, 3],
    [4, 3, 2, 2, 3, 4],
    [5, 4, 3, 3, 4, 5],
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Glyph {
    /// Six rows of six monochrome subpixels, stored in the low six bits.
    rows: [u8; GLYPH_SIZE],
}

#[cfg(test)]
const LETTERS: [char; 11] = ['A', 'L', 'L', 'T', 'H', 'E', 'C', 'O', 'D', 'E', 'S'];

const WORD: [Glyph; 11] = [
    Glyph {
        rows: [0b001100, 0b010010, 0b100001, 0b111111, 0b100001, 0b100001],
    },
    Glyph {
        rows: [0b110000, 0b110000, 0b110000, 0b110000, 0b111111, 0b111111],
    },
    Glyph {
        rows: [0b110000, 0b110000, 0b110000, 0b110000, 0b111111, 0b111111],
    },
    Glyph {
        rows: [0b111111, 0b111111, 0b001100, 0b001100, 0b001100, 0b001100],
    },
    Glyph {
        rows: [0b110011, 0b110011, 0b111111, 0b111111, 0b110011, 0b110011],
    },
    Glyph {
        rows: [0b111111, 0b111111, 0b110000, 0b111110, 0b110000, 0b111111],
    },
    Glyph {
        rows: [0b001111, 0b011111, 0b110000, 0b110000, 0b011111, 0b001111],
    },
    Glyph {
        rows: [0b001100, 0b011110, 0b110011, 0b110011, 0b011110, 0b001100],
    },
    Glyph {
        rows: [0b111100, 0b111110, 0b110011, 0b110011, 0b111110, 0b111100],
    },
    Glyph {
        rows: [0b111111, 0b111111, 0b110000, 0b111110, 0b110000, 0b111111],
    },
    Glyph {
        rows: [0b011111, 0b111111, 0b110000, 0b001100, 0b111111, 0b111110],
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MorphPhase {
    Hold,
    Morph(u8),
    Settle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LogoFrame {
    from_index: usize,
    phase: MorphPhase,
}

impl LogoFrame {
    #[cfg(test)]
    pub(crate) fn active_index(self) -> usize {
        if self.phase == MorphPhase::Settle {
            (self.from_index + 1) % WORD.len()
        } else {
            self.from_index
        }
    }

    #[cfg(test)]
    pub(crate) fn current_letter(self) -> char {
        LETTERS[self.active_index()]
    }

    #[cfg(test)]
    pub(crate) fn phase(self) -> MorphPhase {
        self.phase
    }

    fn tile_mask(self, row: usize, column: usize) -> u8 {
        debug_assert!(row < GRID_SIZE && column < GRID_SIZE);
        let mut mask = 0;
        for sub_row in 0..SUBCELL_SIZE {
            for sub_column in 0..SUBCELL_SIZE {
                let pixel_row = row * SUBCELL_SIZE + sub_row;
                let pixel_column = column * SUBCELL_SIZE + sub_column;
                if self.pixel_on(pixel_row, pixel_column) {
                    let bit = sub_row * SUBCELL_SIZE + sub_column;
                    mask |= 1 << bit;
                }
            }
        }
        mask
    }

    fn pixel_on(self, row: usize, column: usize) -> bool {
        debug_assert!(row < GLYPH_SIZE && column < GLYPH_SIZE);
        let from = WORD[self.from_index];
        let to = WORD[(self.from_index + 1) % WORD.len()];
        let from_on = glyph_has(from, row, column);
        let to_on = glyph_has(to, row, column);

        match self.phase {
            MorphPhase::Hold => from_on,
            MorphPhase::Settle => to_on,
            MorphPhase::Morph(progress) if from.rows == to.rows => {
                repeated_glyph_pixel(from_on, row, column, progress)
            }
            MorphPhase::Morph(_) if from_on == to_on => from_on,
            MorphPhase::Morph(progress) => {
                if progress >= transition_rank(from, to, row, column) {
                    to_on
                } else {
                    from_on
                }
            }
        }
    }
}

fn transition_rank(from: Glyph, to: Glyph, row: usize, column: usize) -> u8 {
    let target_key = (CENTER_OUT_RANKS[row][column], row, column);
    let mut ordinal = 0;
    let mut difference_count = 0;

    for (candidate_row, row_ranks) in CENTER_OUT_RANKS.iter().enumerate() {
        for (candidate_column, &candidate_rank) in row_ranks.iter().enumerate() {
            if glyph_has(from, candidate_row, candidate_column)
                == glyph_has(to, candidate_row, candidate_column)
            {
                continue;
            }

            let candidate_key = (candidate_rank, candidate_row, candidate_column);
            if candidate_key < target_key {
                ordinal += 1;
            }
            difference_count += 1;
        }
    }

    debug_assert!(difference_count >= usize::from(MORPH_STEPS));
    1 + (ordinal * usize::from(MORPH_STEPS) / difference_count) as u8
}

fn glyph_has(glyph: Glyph, row: usize, column: usize) -> bool {
    let bit = GLYPH_SIZE - 1 - column;
    glyph.rows[row] & (1 << bit) != 0
}

fn repeated_glyph_pixel(on: bool, row: usize, column: usize, progress: u8) -> bool {
    if !on {
        return false;
    }

    // The duplicate L has no geometry delta. Shrink it in three ordered
    // passes and restore it in two so the second word position remains visible
    // without bringing back the external ALLTHECODES tracker.
    let depth = match progress {
        0 => 0,
        1 => 1,
        2 => 2,
        3 => 3,
        4 => 2,
        5 => 1,
        _ => 0,
    };
    let distance_from_elbow = row.abs_diff(4) + column.abs_diff(1);
    let rank = match distance_from_elbow {
        5.. => 1,
        4 => 2,
        3 => 3,
        _ => 4,
    };
    rank > depth
}

fn tile_symbols(mask: u8) -> [&'static str; SUBCELL_SIZE] {
    [
        half_block_symbol(mask & 0b0001 != 0, mask & 0b0100 != 0),
        half_block_symbol(mask & 0b0010 != 0, mask & 0b1000 != 0),
    ]
}

fn half_block_symbol(top: bool, bottom: bool) -> &'static str {
    match (top, bottom) {
        (false, false) => " ",
        (true, false) => "▀",
        (false, true) => "▄",
        (true, true) => "█",
    }
}

pub(crate) fn frame_at(step: u64) -> LogoFrame {
    let from_index = ((step / STEPS_PER_LETTER) % WORD.len() as u64) as usize;
    let step_in_segment = step % STEPS_PER_LETTER;
    let phase = if step_in_segment < HOLD_STEPS {
        MorphPhase::Hold
    } else if step_in_segment < SETTLE_STEP {
        MorphPhase::Morph((step_in_segment - HOLD_STEPS + 1) as u8)
    } else {
        MorphPhase::Settle
    };
    LogoFrame { from_index, phase }
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

/// Render the monochrome geometry prototype. The default terminal foreground
/// is intentional: brand colors are deferred until the integrated glyphs and
/// their morph sequence have been visually approved.
pub(crate) fn render_brand_logo(area: Rect, buf: &mut Buffer, frame: LogoFrame) {
    let mut lines = vec![Line::raw("╭────────╮")];

    for row in 0..GRID_SIZE {
        let mut spans = vec![Span::raw("│")];
        for column in 0..GRID_SIZE {
            if column > 0 {
                spans.push(Span::raw(" "));
            }
            for symbol in tile_symbols(frame.tile_mask(row, column)) {
                spans.push(Span::raw(symbol));
            }
        }
        spans.push(Span::raw("│"));
        lines.push(Line::from(spans));
    }
    lines.push(Line::raw("╰────────╯"));

    Paragraph::new(lines)
        .alignment(Alignment::Center)
        .render(area, buf);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Color;

    #[test]
    fn allthecodes_logo_keyframes() {
        let rendered = [
            0, 6, 8, 10, 11, 18, 20, 22, 23, 84, 90, 91, 92, 93, 94, 95, 126, 128, 130, 131,
        ]
        .into_iter()
        .map(|step| format!("step={step}\n{}", render_for_test(step)))
        .collect::<Vec<_>>()
        .join("\n\n");
        insta::assert_snapshot!("allthecodes_logo_keyframes", rendered);
    }

    #[test]
    fn every_stable_letter_is_rendered_inside_the_grid() {
        let rendered = WORD
            .iter()
            .enumerate()
            .map(|(index, _glyph)| {
                format!(
                    "letter={} index={index}\n{}",
                    LETTERS[index],
                    render_for_test(index as u64 * STEPS_PER_LETTER),
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        insta::assert_snapshot!("allthecodes_integrated_glyphs", rendered);
    }

    #[test]
    fn renderer_has_no_external_tracker_or_explicit_color() {
        let area = Rect::new(0, 0, BRAND_COLUMN_WIDTH, BRAND_COLUMN_HEIGHT);
        let mut buf = Buffer::empty(area);
        render_brand_logo(area, &mut buf, frame_at(0));

        assert!(!buffer_text(&buf, area).contains("ALLTHECODES"));
        for cell in &buf.content {
            assert_eq!(cell.fg, Color::Reset);
            assert_eq!(cell.bg, Color::Reset);
        }
    }

    #[test]
    fn curved_glyphs_use_partial_polygon_tiles() {
        let a = frame_at(0);
        assert_eq!(tile_symbols(a.tile_mask(0, 0)), [" ", "▄"]);
        assert_eq!(tile_symbols(a.tile_mask(0, 1)), ["▀", "▀"]);
        assert_eq!(tile_symbols(a.tile_mask(1, 0)), ["█", "▄"]);
        assert_eq!(tile_symbols(a.tile_mask(2, 0)), ["█", " "]);

        let o = frame_at(7 * STEPS_PER_LETTER);
        assert_eq!(o.current_letter(), 'O');
        assert!((0..GRID_SIZE)
            .flat_map(|row| (0..GRID_SIZE).map(move |column| o.tile_mask(row, column)))
            .any(|mask| !matches!(mask, 0 | 15)),);
    }

    #[test]
    fn a_to_l_morph_uses_five_geometry_frames() {
        for progress in 1..=MORPH_STEPS {
            assert_eq!(
                frame_at(HOLD_STEPS + u64::from(progress) - 1).phase(),
                MorphPhase::Morph(progress),
            );
        }
        assert_eq!(frame_at(11).phase(), MorphPhase::Settle);
        assert_eq!(frame_at(11).current_letter(), 'L');
        assert_ne!(render_for_test(6), render_for_test(8));
        assert_ne!(render_for_test(8), render_for_test(10));
        assert_eq!(render_for_test(10), render_for_test(11));
    }

    #[test]
    fn every_letter_transition_changes_on_each_morph_frame() {
        for index in 0..WORD.len() {
            let next_index = (index + 1) % WORD.len();
            let segment_start = index as u64 * STEPS_PER_LETTER;
            let mut previous = render_for_test(segment_start + HOLD_STEPS - 1);

            for progress in 1..=MORPH_STEPS {
                let step = segment_start + HOLD_STEPS + u64::from(progress) - 1;
                let current = render_for_test(step);
                assert_ne!(
                    current, previous,
                    "transition {} -> {} must visibly change at morph frame {progress}",
                    LETTERS[index], LETTERS[next_index],
                );
                previous = current;
            }

            let settled = render_for_test(segment_start + SETTLE_STEP);
            if WORD[index].rows == WORD[next_index].rows {
                assert_ne!(previous, settled, "duplicate glyph pulse must restore");
            } else {
                assert_eq!(previous, settled, "final morph frame must equal target");
            }
        }
    }

    #[test]
    fn repeated_l_pulses_and_advances_the_internal_index() {
        assert_eq!(frame_at(18).phase(), MorphPhase::Morph(1));
        assert_eq!(frame_at(20).phase(), MorphPhase::Morph(3));
        assert_ne!(render_for_test(18), render_for_test(20));
        assert_ne!(render_for_test(20), render_for_test(22));
        assert_eq!(frame_at(18).active_index(), 1);
        assert_eq!(frame_at(23).active_index(), 2);
        assert_eq!(frame_at(23).current_letter(), 'L');
    }

    #[test]
    fn word_sequence_preserves_duplicate_positions() {
        assert_eq!(LETTERS.iter().copied().collect::<String>(), "ALLTHECODES");
        assert_eq!(WORD[1].rows, WORD[2].rows);
        assert_ne!(frame_at(11).active_index(), frame_at(23).active_index());
    }

    #[test]
    fn undersized_area_is_clipped_without_panicking() {
        for area in [Rect::new(0, 0, 5, 3), Rect::new(0, 0, 13, 4)] {
            let mut buf = Buffer::empty(area);
            render_brand_logo(area, &mut buf, frame_at(0));
        }
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

    fn render_for_test(step: u64) -> String {
        let area = Rect::new(0, 0, BRAND_COLUMN_WIDTH, BRAND_COLUMN_HEIGHT);
        let mut buf = Buffer::empty(area);
        render_brand_logo(area, &mut buf, frame_at(step));
        buffer_text(&buf, area)
    }

    fn buffer_text(buf: &Buffer, area: Rect) -> String {
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
