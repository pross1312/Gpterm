mod gpt;
mod buffer;
use buffer::Buffer;
use std::fs::File;
use std::io::{Write};
use std::time::Duration;
use std::sync::mpsc::{self};
use std::{io::{self}, thread};
use crossterm::{
    QueueableCommand,
    terminal, style, event::{self, KeyCode, KeyModifiers}, cursor
};
use serde::de::Visitor;

const AI_COLOR: style::Color = style::Color::Blue;
const INPUT_COLOR: style::Color = style::Color::White;
const SYSTEM_COLOR: style::Color = style::Color::Red;
const USER_COLOR: style::Color = style::Color::Green;
const SCROLL_SPEED: usize = 3; // lines
const CONV_FILE: &str = "conversation.json";
const START_PREFIX: &str = "■  ";

#[derive(Debug, PartialEq, Eq)]
pub enum Role {
    User, System, AI
}
struct RoleVisitor;
impl<'a> Visitor<'a> for RoleVisitor {
    type Value = Role;
    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("Expecteing string")
    }
    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where E: serde::de::Error, {
        match Role::from(v) {
            Some(role) => Ok(role),
            None => Err(serde::de::Error::custom(format!("Unexptected role {v}")))
        }
    }
}
impl<'a> serde::Deserialize<'a> for Role {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where D: serde::Deserializer<'a> {
            deserializer.deserialize_str(RoleVisitor)
    }
}
impl serde::Serialize for Role {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where S: serde::Serializer {
        serializer.serialize_str(self.value())
    }
}


impl Role {
    fn from(str: &str) -> Option<Self> {
        match str.to_lowercase().as_str() {
            "user" => Some(Role::User),
            "system" => Some(Role::System),
            "assistant" => Some(Role::AI),
            _ => None
        }
    }
    fn value(&self) -> &str {
        match self {
            Role::User => "user",
            Role::System => "system",
            Role::AI => "assistant",
        }
    }
}

struct State {
    conv: Vec<(Role, String)>,
    input: String,
    view_start: usize,
}
impl State {
    fn new() -> Self {
        State{ conv: Vec::new(), input: String::new(), view_start: 0 }
    }
    fn append_conv(&mut self, role: Role, msg: String) {
        self.conv.push((role, msg));
    }
}

fn split_by_length(mut str: &str, length: usize) -> Vec<&str> {
    let mut result = Vec::new();
    while str.len() > length {
        result.push(&str[..length]);
        str = &str[length..];
    }
    result.push(str);
    return result;
}

fn render_conversation(state: &mut State, buffer: &mut Buffer,
                       row: usize, height: usize, width: usize) {
    let mut cur_row = row;
    let mut current_role = &Role::AI;
    let mut current_color = AI_COLOR;
    let conv = state.conv.iter().rev();
    let conv_iter = conv.flat_map(|(role, content)| {
        let mut lines = content.rsplit("\n")
               .flat_map(|x|  {
                   let mut result = split_by_length(&x, width as usize);
                   result.reverse();
                   result
               })
               .map(|x| (false, role, x)).collect::<Vec<_>>();
        lines.last_mut().unwrap().0 = true;
        lines
    });
    let count = conv_iter.clone().count() as i32;
    if count <= height as i32 {
        state.view_start = 0;
    } else if (count - state.view_start as i32) < (height as i32) {
        state.view_start = (count - height as i32) as usize;
    }
    for (is_first, role, content) in conv_iter.skip(state.view_start) {
        if current_role != role { // NOTE: switch role or first line
            current_role = role;
            current_color = match *current_role {
                Role::AI => AI_COLOR,
                Role::User => USER_COLOR,
                Role::System => SYSTEM_COLOR,
            }
        }
        if is_first {
            buffer.render_line(cur_row, current_color, &format!("{START_PREFIX}{content}"));
        } else {
            buffer.render_line(cur_row, current_color, content);
        };
        if cur_row == 0 { break; }
        cur_row -= 1;
    }
}

fn render(state: &mut State, buffer: &mut Buffer) -> io::Result<()> {
    render_conversation(state, buffer, buffer.height-3, buffer.height - 2, buffer.width);
    buffer.render_line(buffer.height-2, style::Color::Reset, &"—".repeat(buffer.width as usize));
    buffer.render_line(buffer.height-1, INPUT_COLOR, &state.input);
    Ok(())
}


fn save_conversation(file_path: &str, conversation: &Vec<(Role, String)>) {
    let mut file = File::create(file_path).unwrap();
    file.write_all(serde_json::ser::to_string(conversation).unwrap().as_bytes()).unwrap();
}

fn load_conversation(file_path: &str) -> Result<Vec<(Role, String)>, String> {
    if let Ok(data) = std::fs::read(file_path) {
        serde_json::from_slice::<Vec<(Role, String)>>(data.as_slice())
            .map_err(|_err| format!("Could not parse file {file_path}"))
    } else {
        Err(format!("Could not read file {file_path}"))
    }
}

fn main() -> io::Result<()> {
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    let mut state = State::new();
    match load_conversation(CONV_FILE) {
        Ok(conv) => state.conv = conv,
        Err(err) => {state.append_conv(Role::System, err)}
    }
    let size = terminal::size().unwrap();
    let width = size.0 as usize;
    let height = size.1 as usize;
    let mut buffers = [Buffer::new(width, height), Buffer::new(width, height)];
    let mut rendering_buf = 0;
    let (tx, rx) = mpsc::channel();

    // stdout.queue(crossterm::event::EnableMouseCapture).unwrap();
    stdout.queue(terminal::Clear(terminal::ClearType::All)).unwrap();
    'main: loop {
        buffers[rendering_buf].clear();
        while event::poll(Duration::ZERO)? {
            match event::read()? {
                event::Event::Key(key) => {
                    match key.code {
                        KeyCode::Char(c) => {
                            match key.modifiers {
                                KeyModifiers::CONTROL => match c {
                                    'c' => break 'main,
                                    'p' => state.view_start += SCROLL_SPEED,
                                    'n' => if state.view_start >= SCROLL_SPEED { state.view_start -= SCROLL_SPEED; },
                                    _ => {}
                                },
                                KeyModifiers::SHIFT => state.input.push(c),
                                KeyModifiers::NONE => state.input.push(c),
                                _ => {}
                            };
                        }
                        KeyCode::Enter => {
                            if state.input.len() > 0 {
                                state.append_conv(Role::User, state.input.clone());
                                state.input.clear();
                                let tx_c = tx.clone();
                                let conv: Vec<_> = state.conv.iter().map(|(role, content)| serde_json::json!({
                                    "role": role.value(),
                                    "content": content,
                                })).collect();
                                thread::spawn(move || {
                                    gpt::prompt(&serde_json::json!(conv.as_slice()), tx_c);
                                });
                            }
                        }
                        KeyCode::Backspace => {
                            if state.input.len() > 0 {
                                if key.modifiers == KeyModifiers::ALT {
                                    let new_len = state.input.trim_end_matches(|x: char| x.is_alphanumeric())
                                                             .trim_end().len();
                                    state.input.truncate(new_len);
                                } else {
                                    state.input.pop();
                                }
                            }
                        }
                        _ => {}
                    }
                }
                event::Event::Mouse(mouse_e) => {
                    match mouse_e.kind {
                        event::MouseEventKind::ScrollUp => {
                            state.view_start += SCROLL_SPEED;
                        }
                        event::MouseEventKind::ScrollDown => {
                            if state.view_start >= SCROLL_SPEED { state.view_start -= SCROLL_SPEED; }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        match rx.try_recv() {
            Ok(content) => {
                if content == "[DONE]" {
                    save_conversation(CONV_FILE, &state.conv);
                } else if content.starts_with("[START] ") {
                    let role = content.splitn(2, " ").skip(1).next().unwrap();
                    state.append_conv(Role::from(role).unwrap(), String::new());
                } else {
                    state.conv.last_mut().unwrap().1.push_str(&content);
                    state.view_start = 0;
                }
            }
            Err(_err) => {}
        }

        render(&mut state, &mut buffers[rendering_buf])?;
        buffers[1-rendering_buf].render_diff(&buffers[rendering_buf], &mut stdout)?;
        stdout.queue(cursor::MoveTo(state.input.len() as u16, height as u16 - 1))?;
        stdout.flush()?;
        thread::sleep(Duration::from_millis(1000/60));

        rendering_buf = 1 - rendering_buf;
    }
    terminal::disable_raw_mode()
}
