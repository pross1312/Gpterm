use std::collections::HashMap;
use std::net::TcpStream;
use std::io::{Write, Read, BufReader, BufRead};
use std::time::Duration;
use openssl::ssl::{SslMethod, SslConnector, SslStream};
use serde_json::Value;

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

fn append_conv(conversation: &mut Value, new_message: Value) {
    conversation.as_array_mut().unwrap().push(new_message)
}

fn on_parse_header(data: &str, headers: &mut HashMap<String, String>) {
    if let Some(sep) = data.find(": ") {
        headers.insert(data[..sep].to_string(), data[sep+2..].to_string());
    } else {
        //TODO: handle version, status-code, status-msg
    }
}
fn on_parse_body(data: &str) {
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
        print!("{}", msg);
    } else {
    }
}


fn prompt(stream: &mut SslStream<TcpStream>, conversation: &mut Value) {
    let req = make_prompt(&conversation);
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
                            on_parse_body(data);
                        }
                        index += i+2;
                    } else { break; }
                }
                std::io::stdout().flush().unwrap();
            }
            Err(err) => {
                println!("{err}");
                break;
            }
        }
    }
    println!();
    std::io::stdout().flush().unwrap();
}

fn main() {
    let mut builder = SslConnector::builder(SslMethod::tls()).unwrap();
    builder.set_ca_file("/etc/ssl/certs/ca-certificates.crt").unwrap();
    let connector = builder.build();
    let stream = TcpStream::connect("api.openai.com:443").unwrap();
    TcpStream::set_read_timeout(&stream, Some(Duration::from_secs(5))).unwrap();
    let mut stream = connector.connect("api.openai.com", stream).unwrap();
    let mut reader = BufReader::new(std::io::stdin());
    let mut conversation: Value = serde_json::json!([]);
    loop {
        let mut buffer = String::new();
        match reader.read_line(&mut buffer) {
            Ok(_n) => {
                append_conv(&mut conversation, serde_json::json!({
                    "role": "user",
                    "content": buffer
                }));
                prompt(&mut stream, &mut conversation);
            }
            Err(err) => println!("{err}"),
        }
    }
}
