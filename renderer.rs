use std::io::{Result, Write, self};

use crossterm::{
    QueueableCommand, style::{self, Color}, cursor, terminal::{ClearType, self}
};
struct Buffer {
    data: Vec<Vec<(Color, char)>>,
}

impl Buffer {
    pub fn new(w: usize, h: usize) -> Self {
        Buffer {
            data: vec![vec![(Color::Reset, ' '); w]; h],
        }
    }
    pub fn clear(&mut self) {
        for r in 0..self.data.len() {
            for c in 0..self.data[r].len() {
                self.data[r][c] = (Color::Reset, ' ');
            }
        }
    }
    pub fn get(&self, r: usize, c: usize) -> &(Color, char) {
        &self.data[r][c]
    }
    pub fn set(&mut self, r: usize, c: usize, value: (Color, char)) {
        self.data[r][c] = value;
    }
    pub fn resize(&mut self, w: usize, h: usize) {
        self.data.resize(h, vec![(Color::Reset, ' '); w]);
        for line in self.data.iter_mut() {
            line.resize(w, (Color::Reset, ' '));
        }
    }
}

pub struct Renderer {
    buffers: [Buffer; 2],
    rendering_buf: usize,
    stdout: io::Stdout,
    pub width: usize,
    pub height: usize,
}
impl Renderer {
    pub fn new(w: usize, h: usize) -> Self {
        Renderer{
            buffers: [Buffer::new(w, h), Buffer::new(w, h)],
            rendering_buf: 0,
            stdout:  io::stdout(),
            width: w,
            height: h
        }
    }
    pub fn render_line(&mut self, line: usize, color: Color, data: &str) {
        let Self{ buffers, rendering_buf, height: _, width, stdout: _stdout } = self;
        let mut chars = data.chars();
        for i in 0..*width {
            buffers[*rendering_buf].set(line, i, (color, chars.next().unwrap_or(' ')));
        }
    }
    pub fn resize(&mut self, w: usize, h: usize) {
        self.buffers[0].resize(w, h);
        self.buffers[1].resize(w, h);
        self.width = w;
        self.height = h;
    }
    pub fn render(&mut self) -> std::io::Result<()> {
        let Self{ buffers, rendering_buf, height, width, stdout} = self;
        let mut current_color = Color::Reset;
        // stdout.queue(style::SetForegroundColor(current_color))?;
        for r in 0..*height {
            for c in 0..*width {
                if buffers[*rendering_buf].get(r, c) != buffers[1-*rendering_buf].get(r, c) {
                    let (color, char) = buffers[*rendering_buf].get(r, c);
                    if color != &current_color {
                        current_color = *color;
                        stdout.queue(style::SetForegroundColor(current_color))?;
                    }
                    stdout
                        .queue(cursor::MoveTo(c as u16, r as u16))?
                        .queue(style::Print(char))?;
                }
            }
        }
        buffers[1-*rendering_buf].clear();
        self.rendering_buf = 1 - *rendering_buf;
        Ok(())
    }
    pub fn clear(&mut self) -> Result<()> {
        self.buffers[0].clear();
        self.buffers[1].clear();
        self.stdout.queue(terminal::Clear(ClearType::All))?;
        Ok(())
    }
    pub fn move_cursor(&mut self, r: u16, c: u16) -> Result<()> {
        self.stdout.queue(cursor::MoveTo(c, r))?;
        Ok(())
    }
    pub fn update(&mut self) -> Result<()> {
        self.stdout.flush()?;
        Ok(())
    }
}

