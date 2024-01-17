use std::io::{Stdout};

use crossterm::{
    QueueableCommand, style::{self, Color}, cursor
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position { pub row: usize, pub col: usize }
impl Position {
    pub fn new(row: impl Into<usize>, col: impl Into<usize>) -> Self {
        Position { row: row.into(), col: col.into() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region(pub Position, pub Position);
impl Region {
    pub fn new(px: Position, py: Position) -> Self {
        if px.row == py.row {
            if px.col < py.col {
                Region(px, py)
            } else {
                Region(py, px)
            }
        } else if px.row < py.row {
            Region(px, py)
        } else {
            Region(py, px)
        }
    }
    pub fn contains(&self, pos: Position) -> bool {
        let past_start = pos.row > self.0.row || (pos.row == self.0.row && pos.col >= self.0.col);
        let before_end = pos.row < self.1.row || (pos.row == self.1.row && pos.col <= self.1.col);
        past_start && before_end
    }
}

pub const DEFAULT_BG: Color = Color::Reset;
pub const DEFAULT_FG: Color = Color::White;

#[derive(Clone, PartialEq, Eq, Copy)]
pub struct Cell {
    pub fg: Color,
    pub bg: Color,
    pub c: char,
}
impl Cell {
    fn new(fg: Color, bg: Color, c: char) -> Self {
        Cell{ fg, bg, c }
    }
    fn default() -> Self {
        Cell::new(DEFAULT_FG, DEFAULT_BG, ' ')
    }
}
pub struct Buffer {
    pub width: usize,
    pub height: usize,
    data: Vec<Vec<Cell>>,
}

impl Buffer {
    pub fn new(w: usize, h: usize) -> Self {
        Buffer {
            data: vec![vec![Cell::default(); w]; h],
            width: w,
            height: h
        }
    }
    pub fn clear(&mut self) {
        for r in 0..self.height {
            for c in 0..self.width {
                self.data[r][c] = Cell::default();
            }
        }
    }
    pub fn get(&self, r: usize, c: usize) -> Cell {
        self.data[r][c]
    }
    pub fn put_line(&mut self, line: usize, fore: Option<Color>, back: Option<Color>, data: &str) {
        let mut chars = data.chars();
        for i in 0..self.width.min(data.len()) {
            self.data[line][i].c = chars.next().unwrap_or(' ');
            if let Some(fg) = fore {
                self.data[line][i].fg = fg;
            }
            if let Some(bg) = back {
                self.data[line][i].bg = bg;
            }
        }
    }
    pub fn resize(&mut self, w: usize, h: usize) {
        self.width = w;
        self.height = h;
        self.data.resize(h, vec![Cell::default(); w]);
        for line in self.data.iter_mut() {
            line.resize(w, Cell::default());
        }
    }
    pub fn get_region_text(&self, reg: &Region) -> String {
        let mut result = String::with_capacity((reg.1.row - reg.0.row) * self.width);
        for r in reg.0.row..=reg.1.row {
            for c in 0..self.width {
                if reg.contains(Position::new(r, c)) {
                    result.push(self.data[r][c].c);
                }
            }
            result = result.trim_end_matches(|x| x == ' ').to_string() + "\n";
        }
        result
    }
    pub fn mark(&mut self, region: &Region, bg: Color) {
        for r in region.0.row..=region.1.row {
            if r >= self.height {
                break;
            }
            for c in 0..self.width {
                if region.contains(Position::new(r, c)) {
                    self.data[r][c].bg = bg;
                }
            }
        }
    }
}

pub fn render_diff(stdout: &mut Stdout, front: &Buffer, back: &Buffer) -> std::io::Result<()> {
    assert!(front.width == back.width && front.height == back.height);
    let mut cur_fg = Color::White;
    let mut cur_bg = Color::Black;
    stdout.queue(style::SetForegroundColor(cur_fg))?
          .queue(style::SetBackgroundColor(cur_bg))?;
    let width = front.width;
    let height = front.height;
    for r in 0..height {
        for c in 0..width {
            if front.get(r, c) != back.get(r, c) {
                let cell = front.get(r, c);
                if cur_bg != cell.bg {
                    cur_bg = cell.bg;
                    stdout.queue(style::SetBackgroundColor(cell.bg))?;
                }
                if cur_fg != cell.fg {
                    cur_fg = cell.fg;
                    stdout.queue(style::SetForegroundColor(cell.fg))?;
                }
                stdout
                    .queue(cursor::MoveTo(c as u16, r as u16))?
                    .queue(style::Print(cell.c))?;
            }
        }
    }
    Ok(())
}
