use std::collections::HashMap;
use std::fs::File;
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
const SYSTEM_COLOR: style::Color = style::Color::Red;
const USER_COLOR: style::Color = style::Color::Green;
const SCROLL_SPEED: usize = 2; // lines
const CONV_FILE: &str = "conversation.json";
const START_PREFIX: &str = "■  ";
const CHAT_MODEL: &str = "gpt-3.5-turbo";

fn make_prompt(conversation: &Value) -> String {
    let secret = std::env::var("GPT_SECRET_KEY").unwrap();
    let conversation: Vec<_> = conversation.as_array().unwrap()
        .iter().filter(|&x| x["role"].as_str().unwrap() != Role::System.value()).collect();
    let body = serde_json::json!({
        "model": CHAT_MODEL,
        "messages": serde_json::json!(conversation.as_slice()),
        "stream": true
    }).to_string();
    return format!("POST /v1/chat/completions HTTP/1.1\r\nHost: api.openai.com\r\nContent-Length: {}\r\nContent-Type: application/json\r\nAuthorization: Bearer {secret}\r\n\r\n{}",
        body.len(), body);
}

fn on_parse_header(data: &str, headers: &mut HashMap<String, String>) {
    if let Some(sep) = data.find(": ") {
        headers.insert(data[..sep].to_string(), data[sep+2..].to_string());
    } else {
        let tokens: Vec<_> = data.splitn(3, " ").collect();
        headers.insert("protocol".to_string(), tokens[0].to_string());
        headers.insert("status".to_string(), tokens[1].to_string());
        headers.insert("status-msg".to_string(), tokens[2].to_string());
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

enum Role {
    User, System, AI
}
impl Role {
    fn from(str: &str) -> Self {
        match str.to_lowercase().as_str() {
            "user" => Role::User,
            "system" => Role::System,
            "assistant" => Role::AI,
            _ => panic!()
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
    conv: Value,
    input: String,
    view_start: usize,
}
impl State {
    fn new() -> Self {
        State{ conv: serde_json::json!([]), input: String::new(), view_start: 0 }
    }
    fn append_conv(&mut self, role: Role, msg: &str) {
        self.conv.as_array_mut().unwrap().push(serde_json::json!({
            "role": role.value(),
            "content": msg,
        }));
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

fn render_conversation(state: &mut State, stdout: &mut io::Stdout,
                       row: u16, height: u16, width: u16) -> io::Result<()>{
    let mut cur_row = row;
    let mut current_role = "";
    let conv = state.conv.as_array().unwrap().iter().rev();
    let conv_iter = conv.flat_map(|msg| {
        let content = msg["content"].as_str().unwrap();
        let mut lines = content.rsplit("\n")
               .flat_map(|x|  {
                   let mut result = split_by_length(&x, width as usize);
                   result.reverse();
                   result
               })
               .map(|x| (false, msg["role"].as_str().unwrap(), x)).collect::<Vec<_>>();
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
            stdout.queue(style::SetForegroundColor(match current_role {
                "assistant" => AI_COLOR,
                "user" => USER_COLOR,
                "system" => SYSTEM_COLOR,
                _ => panic!(),
            }))?;
        }
        stdout.queue(cursor::MoveTo(0, cur_row))?;
        if is_first { stdout.queue(style::Print(START_PREFIX))?; }
        stdout.queue(style::Print(content))?;
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
    render_conversation(state, stdout, height-3, height - 2, width)?;
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
    let mut headers: HashMap<String, String> = HashMap::new();
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
                                if headers.get("status").unwrap() != "200" {
                                    tx.send("[START] system".to_string()).unwrap();
                                    let rest = &response[index+i..];
                                    let msg = match serde_json::from_str::<Value>(rest) {
                                        Ok(error) => error["error"]["message"].as_str().unwrap().to_string(),
                                        Err(_err) => format!("Could not parse {rest}"),
                                    };
                                    tx.send(msg).unwrap();
                                    break 'outer;
                                } else {
                                    tx.send("[START] assistant".to_string()).unwrap();
                                }
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
            Err(err) => {
                tx.send("[START] system".to_string()).unwrap();
                tx.send(err.to_string()).unwrap();
                break;
            }
        }
    }
    tx.send("[DONE]".to_string()).unwrap();
}

fn save_conversation(file_path: &str, conversation: &Value) {
    let mut file = File::create(file_path).unwrap();
    file.write_all(serde_json::to_string(conversation).unwrap().as_bytes()).unwrap();
}

fn load_conversation(file_path: &str) -> Result<Value, String> {
    if let Ok(data) = std::fs::read(file_path) {
        serde_json::from_slice(data.as_slice())
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
        Err(err) => {state.append_conv(Role::System, &err)}
    }
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
                                let input = state.input.clone();
                                state.append_conv(Role::User, &input);
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
                if content == "[DONE]" {
                    save_conversation(CONV_FILE, &state.conv);
                } else if content.starts_with("[START] ") {
                    let role = content.splitn(2, " ").skip(1).next().unwrap();
                    state.append_conv(Role::from(role), "");
                } else {
                    let last = state.pop_last();
                    content.insert_str(0, last["content"].as_str().unwrap_or(""));
                    state.append_conv(Role::from(last["role"].as_str().unwrap()), &content);
                }
            }
            Err(_err) => {}
        }
        render(&mut state, &mut stdout, width, height)?;
        stdout.flush()?;
        thread::sleep(Duration::from_millis(1000/60));
    }
    terminal::disable_raw_mode()
}
