use std::collections::HashMap;
use std::net::TcpStream;
use std::io::{Write, Read};
use std::time::Duration;
use std::sync::mpsc::{self, Sender};
use openssl::ssl::{SslMethod, SslConnector};
use serde_json::Value;
use std::{io::{self}, thread};
use crossterm::{
    QueueableCommand,
    terminal::{self, ClearType}, cursor::{self}, style::{self}, event::{self, KeyCode, KeyModifiers}
};

const AI_COLOR: style::Color = style::Color::Blue;
const USER_COLOR: style::Color = style::Color::Green;
const SCROLL_SPEED: usize = 2; // lines

fn make_prompt(conversation: &Value) -> String {
    let secret = std::env::var("GPT_SECRET_KEY").unwrap();
    let body = serde_json::json!({
        "model": "gpt-3.5-turbo",
        "messages": conversation,
        "stream": true
    }).to_string();
    return format!("POST /v1/chat/completions HTTP/1.1\r\nHost: api.openai.com\r\nContent-Length: {}\r\nContent-Type: application/json\r\nAuthorization: Bearer {secret}\r\n\r\n{}",
        body.len(), body);
}

fn on_parse_header(data: &str, headers: &mut HashMap<String, String>) {
    if let Some(sep) = data.find(": ") {
        headers.insert(data[..sep].to_string(), data[sep+2..].to_string());
    } else {
        //TODO: handle version, status-code, status-msg
    }
}
fn on_parse_body(data: &str) -> String {
    if data.starts_with("data") {
        let msg = data.split("\n\n").filter(|x| !x.is_empty()).map(|x| {
            if let Ok(data) = serde_json::from_str::<Value>(&x[6..]) {
                data
            } else {
                Value::Null
            }
        }).filter(|x| x != &Value::Null).fold(String::new(), |mut acc, x| {
            for msg in x["choices"].as_array().unwrap().iter() {
                if let Some(str) = msg["delta"]["content"].as_str() {
                    acc += str;
                }
            }
            acc
        });
        return msg;
    } else {
        return String::new();
    }
}

struct State {
    conv: Value,
    input: String,
    view_start: usize,
}
impl State {
    fn new() -> Self {
        State{ conv: serde_json::json!([]), input: String::new(), view_start: 0 }
    }
    fn append_conv(&mut self, new_message: Value) {
        self.conv.as_array_mut().unwrap().push(new_message)
    }
    fn pop_last(&mut self) -> Value {
        self.conv.as_array_mut().unwrap().pop().unwrap()
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

fn render_conversation(state: &mut State, stdout: &mut io::Stdout, height: u16, width: u16) -> io::Result<()>{
    let mut cur_row = height-3;
    let mut current_role = "";
    let conv = state.conv.as_array().unwrap().iter().rev();
    let conv = conv.flat_map(|msg| {
        msg["content"].as_str().unwrap().rsplit("\n")
                      .flat_map(|x|  {
                          let mut result = split_by_length(&x, width as usize);
                          result.reverse();
                          result
                      })
                      .map(|x| (msg["role"].as_str().unwrap(), x)).collect::<Vec<_>>()
    });
    let count = conv.clone().count() as i32;
    state.view_start = state.view_start.min((count - height as i32).max(0) as usize);
    for (role, content) in conv.skip(state.view_start) {
        if current_role != role { // NOTE: switch role or first line
            current_role = role;
            stdout.queue(style::SetForegroundColor(if current_role == "user" {
                USER_COLOR
            } else {
                AI_COLOR
            }))?;
        }
        stdout.queue(cursor::MoveTo(0, cur_row))?
              .queue(style::Print(content))?;
        if cur_row == 0 { break; }
        cur_row -= 1;
    }
    Ok(())
}

fn render_seperator(stdout: &mut io::Stdout, row: u16, width: u16) -> io::Result<()> {
    stdout.queue(style::SetForegroundColor(style::Color::Reset))?
          .queue(cursor::MoveTo(0, row))?
          .queue(style::Print(&"—".repeat(width as usize)))?;
    Ok(())
}

fn render_prompt(input: &String, stdout: &mut io::Stdout, row: u16) -> io::Result<()> {
    stdout.queue(cursor::MoveTo(0, row))?
          .queue(style::Print(input))?;
    Ok(())
}

fn render(state: &mut State, stdout: &mut io::Stdout, width: u16, height: u16) -> io::Result<()> {
    stdout.queue(terminal::Clear(ClearType::All))?;
    render_conversation(state, stdout, height, width)?;
    render_seperator(stdout, height-2, width)?;
    render_prompt(&state.input, stdout, height-1)?;
    Ok(())
}

fn prompt(req: String, tx: Sender<String>) {
    let mut builder = SslConnector::builder(SslMethod::tls()).unwrap();
    builder.set_ca_file("/etc/ssl/certs/ca-certificates.crt").unwrap();
    let connector = builder.build();
    let stream = TcpStream::connect("api.openai.com:443").unwrap();
    TcpStream::set_read_timeout(&stream, Some(Duration::from_secs(10))).unwrap();
    let mut stream = connector.connect("api.openai.com", stream).unwrap();
    // let mut headers: HashMap<String, String> = HashMap::new();
    stream.write(req.as_bytes()).unwrap();
    const BUFFER_SIZE: usize = 1024;
    let buffer: &mut [u8] = &mut [0; BUFFER_SIZE];
    let mut response: String = String::new();
    let mut index: usize = 0;
    let mut headers = HashMap::new();
    let mut is_parsing_header = true;
    'outer: loop {
        match stream.read(buffer) {
            Ok(n) => {
                let chunk = String::from_utf8(buffer[..n].to_vec()).unwrap();
                response.push_str(&chunk);
                loop { // NOTE: loop over all chunk of data seperated by \r\n
                    if let Some(i) = response[index..].find("\r\n") {
                        if i <= 0 { // NOTE: end of header or body
                            if is_parsing_header { // NOTE: end of header
                                index += i+2;
                                is_parsing_header = false;
                                continue;
                            } else { // NOTE: end of body
                                break 'outer;
                            }
                        }
                        let data = &response[index..index+i].trim();
                        if is_parsing_header {
                            on_parse_header(data, &mut headers);
                        } else {
                            tx.send(on_parse_body(data)).unwrap();
                        }
                        index += i+2;
                    } else { break; }
                }
            }
            Err(_err) => {
                //TODO: println!("{err}");
                break;
            }
        }
    }
}
fn main() -> io::Result<()> {
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    let mut state = State::new();
    let (width, height) = terminal::size().unwrap();
    let (tx, rx) = mpsc::channel();

    stdout.queue(crossterm::event::EnableMouseCapture).unwrap();
    stdout.queue(terminal::Clear(terminal::ClearType::All))?;
    'main: loop {
        while event::poll(Duration::ZERO)? {
            match event::read()? {
                event::Event::Key(key) => {
                    match key.code {
                        KeyCode::Char(c) => {
                            if c == 'c' && key.modifiers == KeyModifiers::CONTROL {
                                break 'main;
                            }
                            state.input.push(c);
                        }
                        KeyCode::Enter => {
                            if state.input.len() > 0 {
                                state.append_conv(serde_json::json!({
                                    "role": "user",
                                    "content": state.input
                                }));
                                state.input.clear();
                                let req = make_prompt(&state.conv);
                                let tx_c = tx.clone();
                                thread::spawn(|| {
                                    prompt(req, tx_c);
                                });
                            }
                        }
                        KeyCode::Backspace => {
                            if state.input.len() > 0 {
                                state.input.pop();
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
            Ok(mut content) => {
                let on_ai_responding = state.pop_last();
                let mut cur_content = "";
                if on_ai_responding["role"] == "user" {
                    state.append_conv(on_ai_responding);
                } else {
                    cur_content = on_ai_responding["content"].as_str().unwrap_or("");
                }
                content.insert_str(0, cur_content);
                state.append_conv(serde_json::json!({
                    "role": "assistant",
                    "content": content,
                }));
            }
            Err(_err) => {
                // TODO:
            }
        }
        render(&mut state, &mut stdout, width, height)?;
        stdout.flush()?;
        thread::sleep(Duration::from_millis(1000/60));
    }
    terminal::disable_raw_mode()
}
