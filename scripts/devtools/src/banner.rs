//! The README banner image behind `just banner-svg`: `assets/banner.txt` rendered as an SVG of
//! exact rectangles, so the half-block rows join on a web page the way they do in a terminal.
//!
//! Each character cell is [`CELL_W`] × [`CELL_H`] units, twice as tall as wide like a terminal
//! cell. A character covers one vertical span of its cell, the full cell width:
//!
//! | Character | Span |
//! | --- | --- |
//! | `█` | the whole cell |
//! | `▀` | the top half |
//! | `▄` | the bottom half |
//! | `■` | a square in the middle half, as wide as the cell |
//! | space | nothing |
//!
//! Any other character is an error. Horizontally adjacent cells of the same row that cover the
//! same span merge into one rectangle, which keeps the file small. The image has no font and no
//! text element; its colour sits in a `<style>` rule with a `prefers-color-scheme: dark`
//! override, so it reads on light and dark backgrounds. The output depends only on the input
//! text, so rendering the same banner twice gives the same bytes.

use crate::{Error, Result};
use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;

/// Width of a character cell, in SVG units.
pub const CELL_W: usize = 2;
/// Height of a character cell, in SVG units.
pub const CELL_H: usize = 4;

/// Foreground colour on a light background.
const LIGHT: &str = "#1f2328";
/// Foreground colour on a dark background.
const DARK: &str = "#f0f6fc";

/// The accessible name of the image.
const TITLE: &str = "AutoBot";

/// The vertical span `(top, height)` in units that `c` covers in its cell, `None` for a space.
fn span(c: char) -> Option<Option<(usize, usize)>> {
    match c {
        ' ' => Some(None),
        '█' => Some(Some((0, CELL_H))),
        '▀' => Some(Some((0, CELL_H / 2))),
        '▄' => Some(Some((CELL_H / 2, CELL_H / 2))),
        '■' => Some(Some((CELL_H / 4, CELL_H / 2))),
        _ => None,
    }
}

/// A filled rectangle, in SVG units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rect {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

/// The rectangles of `banner`, row by row and left to right, adjacent cells of one row with the
/// same span merged.
fn rects(banner: &str) -> Result<Vec<Rect>> {
    let mut out: Vec<Rect> = Vec::new();
    for (row, line) in banner.lines().enumerate() {
        let mut last: Option<Rect> = None;
        for (col, c) in line.chars().enumerate() {
            let s = span(c).ok_or_else(|| {
                Error::Parse(format!(
                    "banner line {}, column {}: no rendering for {c:?}",
                    row + 1,
                    col + 1
                ))
            })?;
            let Some((top, h)) = s else {
                out.extend(last.take());
                continue;
            };
            let rect = Rect {
                x: col * CELL_W,
                y: row * CELL_H + top,
                w: CELL_W,
                h,
            };
            last = match last {
                Some(prev) if prev.y == rect.y && prev.h == rect.h => Some(Rect {
                    w: prev.w + rect.w,
                    ..prev
                }),
                Some(prev) => {
                    out.push(prev);
                    Some(rect)
                }
                None => Some(rect),
            };
        }
        out.extend(last);
    }
    Ok(out)
}

/// The SVG image of `banner`, whose lines are rows of the characters in the module table; the
/// image is as wide as the longest line.
///
/// # Errors
/// Fails on a character the module table does not list, naming its line and column.
pub fn render(banner: &str) -> Result<String> {
    let rects = rects(banner)?;
    let cols = banner.lines().map(|l| l.chars().count()).max().unwrap_or(0);
    let rows = banner.lines().count();
    let (w, h) = (cols * CELL_W, rows * CELL_H);
    let mut svg = String::new();
    // Writing to a String cannot fail, so the results of write! are discarded.
    let _ = writeln!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}" role="img" aria-labelledby="title" shape-rendering="crispEdges">"#
    );
    let _ = writeln!(svg, r#"<title id="title">{TITLE}</title>"#);
    let _ = writeln!(
        svg,
        "<style>path{{fill:{LIGHT}}}@media (prefers-color-scheme:dark){{path{{fill:{DARK}}}}}</style>"
    );
    svg.push_str(r#"<path d=""#);
    for (i, r) in rects.iter().enumerate() {
        if i > 0 {
            svg.push(' ');
        }
        let _ = write!(svg, "M{} {}h{}v{}h-{}z", r.x, r.y, r.w, r.h, r.w);
    }
    svg.push_str("\"/>\n</svg>\n");
    Ok(svg)
}

/// Writes the SVG image of the banner text at `input` to `output`.
///
/// # Errors
/// Fails when `input` cannot be read, holds a character [`render`] rejects, or `output` cannot
/// be written.
pub fn run(input: impl AsRef<Path>, output: impl AsRef<Path>) -> Result<ExitCode> {
    let (input, output) = (input.as_ref(), output.as_ref());
    let text = std::fs::read_to_string(input)
        .map_err(|e| Error::Parse(format!("reading {}: {e}", input.display())))?;
    let svg = render(&text)?;
    std::fs::write(output, svg)
        .map_err(|e| Error::Parse(format!("writing {}: {e}", output.display())))?;
    println!("banner-svg: wrote {}", output.display());
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: usize, y: usize, w: usize, h: usize) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn each_character_covers_its_span() -> Result<()> {
        assert_eq!(rects("█")?, vec![r(0, 0, 2, 4)]);
        assert_eq!(rects("▀")?, vec![r(0, 0, 2, 2)]);
        assert_eq!(rects("▄")?, vec![r(0, 2, 2, 2)]);
        assert_eq!(rects("■")?, vec![r(0, 1, 2, 2)]);
        assert_eq!(rects(" ")?, vec![]);
        Ok(())
    }

    #[test]
    fn cells_are_placed_by_row_and_column() -> Result<()> {
        assert_eq!(rects("  ▀\n █")?, vec![r(4, 0, 2, 2), r(2, 4, 2, 4)]);
        Ok(())
    }

    #[test]
    fn adjacent_cells_with_one_span_merge() -> Result<()> {
        assert_eq!(rects("▄▄▄")?, vec![r(0, 2, 6, 2)]);
        assert_eq!(rects("▄▄ ▄")?, vec![r(0, 2, 4, 2), r(6, 2, 2, 2)]);
        assert_eq!(
            rects("█■█")?,
            vec![r(0, 0, 2, 4), r(2, 1, 2, 2), r(4, 0, 2, 4)]
        );
        Ok(())
    }

    #[test]
    fn rows_do_not_merge_with_each_other() -> Result<()> {
        assert_eq!(rects("▄\n▄")?, vec![r(0, 2, 2, 2), r(0, 6, 2, 2)]);
        Ok(())
    }

    #[test]
    fn an_unknown_character_is_an_error_naming_its_place() {
        let err = rects("█\n █#").map(|_| ()).map_err(|e| e.to_string());
        assert_eq!(
            err,
            Err("parse error: banner line 2, column 3: no rendering for '#'".to_owned())
        );
        assert!(render("x").is_err());
    }

    #[test]
    fn the_image_is_as_large_as_the_text() -> Result<()> {
        let svg = render("▀▀▀ \n█\n")?;
        assert!(svg.starts_with(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8" width="8" height="8""#
        ));
        Ok(())
    }

    #[test]
    fn the_image_has_a_title_themed_colours_and_no_text() -> Result<()> {
        let svg = render("█")?;
        assert!(svg.contains("<title id=\"title\">AutoBot</title>"));
        assert!(svg.contains(&format!("path{{fill:{LIGHT}}}")));
        assert!(svg.contains(&format!(
            "@media (prefers-color-scheme:dark){{path{{fill:{DARK}}}}}"
        )));
        assert!(!svg.contains("<text"));
        assert!(svg.contains(r#"<path d="M0 0h2v4h-2z"/>"#));
        Ok(())
    }
}
