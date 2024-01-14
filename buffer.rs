use crossterm::{
    QueueableCommand, style::{self, Color}, cursor
};
pub struct Buffer {
    data: Vec<Vec<(Color, char)>>,
    pub width: usize,
    pub height: usize,
}

impl Buffer {
    pub fn new(w: usize, h: usize) -> Self {
        Buffer {
            data: vec![vec![(Color::Reset, ' '); w]; h],
            width: w,
            height: h,
        }
    }
    pub fn clear(&mut self) {
        for r in 0..self.height {
            for c in 0..self.width {
                self.data[r][c] = (Color::Reset, ' ');
            }
        }
    }
    pub fn render_line(&mut self, line: usize, color: Color, data: &str) {
        let mut chars = data.chars();
        for i in 0..self.width {
            self.data[line][i] = (color, chars.next().unwrap_or(' '));
        }
    }
    pub fn render_diff(&self, other: &Self, stdout: &mut std::io::Stdout) -> std::io::Result<()> {
        if self.width != other.width || self.height != other.height {
            panic!()
        }
        let mut current_color = Color::Reset;
        // stdout.queue(style::SetForegroundColor(current_color))?;
        for r in 0..self.height {
            for c in 0..self.width {
                if self.data[r][c] != other.data[r][c] {
                    let (color, char) = other.data[r][c];
                    if color != current_color {
                        current_color = color;
                        stdout.queue(style::SetForegroundColor(current_color))?;
                    }
                    stdout
                        .queue(cursor::MoveTo(c as u16, r as u16))?
                        .queue(style::Print(char))?;
                }
            }
        }
        Ok(())
    }
}
