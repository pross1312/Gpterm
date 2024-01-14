use openssl::ssl::{SslMethod, SslConnector};
use std::sync::mpsc::{Sender};
use std::net::TcpStream;
use std::io::{Write, Read};
use std::collections::HashMap;
use std::time::Duration;
use serde_json::Value;

const CHAT_MODEL: &str = "gpt-3.5-turbo";

fn make_prompt(conversation: &Value) -> String {
    let secret = std::env::var("GPT_SECRET_KEY").unwrap();
    let body = serde_json::json!({
        "model": CHAT_MODEL,
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

pub fn prompt(conv: &Value, tx: Sender<String>) {
    let req = make_prompt(conv);
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
