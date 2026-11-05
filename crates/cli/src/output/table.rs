//! Rows of cells in aligned columns. Color codes in a cell do not count toward its width.

#[derive(Debug, Default)]
pub struct Table {
    rows: Vec<Vec<String>>,
}

impl Table {
    pub fn row(&mut self, cells: Vec<String>) {
        self.rows.push(cells);
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// A line per row, indented, columns two spaces apart, no trailing space.
    pub fn lines(&self, indent: &str) -> Vec<String> {
        let mut widths: Vec<usize> = Vec::new();
        for row in &self.rows {
            widths.resize(widths.len().max(row.len()), 0);
            for (w, cell) in widths.iter_mut().zip(row) {
                *w = (*w).max(visible_width(cell));
            }
        }
        let line = |row: &Vec<String>| {
            let mut line = indent.to_string();
            for (i, cell) in row.iter().enumerate() {
                line += cell;
                if i + 1 < row.len() {
                    line += &" ".repeat(widths[i] - visible_width(cell) + 2);
                }
            }
            line.trim_end().to_string()
        };
        self.rows.iter().map(line).collect()
    }
}

const PART_GAP: usize = 2;
const COLUMN_GAP: usize = 4;

/// Lists of cells laid out on one grid, as many columns as fit width, so the columns line up
/// from one list to the next. Each cell's parts align too. A width of 0 gives one cell a line.
pub fn columns(lists: &[Vec<Vec<String>>], width: usize, indent: &str) -> Vec<Vec<String>> {
    let mut parts: Vec<usize> = Vec::new();
    for cell in lists.iter().flatten() {
        parts.resize(parts.len().max(cell.len()), 0);
        for (w, part) in parts.iter_mut().zip(cell) {
            *w = (*w).max(visible_width(part));
        }
    }
    let cell = parts.iter().sum::<usize>() + PART_GAP * parts.len().saturating_sub(1);
    let mut cols = 1;
    while width > 0 && indent.len() + (cols + 1) * cell + cols * COLUMN_GAP <= width {
        cols += 1;
    }
    let lay = |list: &Vec<Vec<String>>| {
        let rows = list.len().div_ceil(cols);
        (0..rows)
            .map(|r| {
                let mut line = indent.to_string();
                for (c, item) in list.iter().skip(r).step_by(rows).take(cols).enumerate() {
                    if c > 0 {
                        line += &" ".repeat(COLUMN_GAP);
                    }
                    for (p, w) in parts.iter().enumerate() {
                        let part = item.get(p).map_or("", String::as_str);
                        if p > 0 {
                            line += &" ".repeat(PART_GAP);
                        }
                        line += &format!("{part}{}", " ".repeat(w - visible_width(part)));
                    }
                }
                line.trim_end().to_string()
            })
            .collect()
    };
    lists.iter().map(lay).collect()
}

/// The characters a terminal shows, color codes left out.
pub fn visible_width(s: &str) -> usize {
    let mut width = 0;
    let mut in_code = false;
    for c in s.chars() {
        match (in_code, c) {
            (false, '\x1b') => in_code = true,
            (true, 'm') => in_code = false,
            (false, _) => width += 1,
            _ => {}
        }
    }
    width
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_line_up_past_color_codes() {
        let mut t = Table::default();
        t.row(vec!["sdk".into(), "\x1b[2ma_sdk\x1b[0m".into()]);
        t.row(vec!["driver".into(), "b_driver".into()]);
        t.row(vec!["app".into()]);
        assert_eq!(t.lines("  "), ["  sdk     \x1b[2ma_sdk\x1b[0m", "  driver  b_driver", "  app"]);
        assert_eq!(visible_width("\x1b[1mab\x1b[0m"), 2);
    }

    #[test]
    fn lists_share_one_grid() {
        let cell = |a: &str, b: &str| vec![a.to_string(), b.to_string()];
        let lists = vec![vec![cell("chatter", "topic"), cell("speed", "var"), cell("double", "fn")], vec![cell("x", "task")]];
        assert_eq!(columns(&lists, 0, "  "), [vec!["  chatter  topic", "  speed    var", "  double   fn"], vec!["  x        task"]]);
        let wide = columns(&lists, 60, "  ");
        assert_eq!(wide[0], ["  chatter  topic    speed    var      double   fn"]);
    }
}
