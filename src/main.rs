#![feature(proc_macro_hygiene, decl_macro)]
#![feature(box_patterns)]

extern crate rocket;
extern crate rocket_contrib;
extern crate dotenv;
extern crate slack;
extern crate hmac;
extern crate sha2;
extern crate serde;



use rocket::{post, routes};
use rocket_contrib::json::Json;
use slack::{Event, RtmClient, Message, Sender};
use dotenv::dotenv;
use std::env;
use std::string::String;
use std::thread;
use sha2::Sha256;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};

type HmacSha256 = Hmac<Sha256>;

struct MyHandler {
    sender: Option<Sender>,
}

#[allow(unused_variables)]
impl slack::EventHandler for MyHandler {
    fn on_event(&mut self, cli: &RtmClient, event: Event) {
        match event {
            Event::Message(box Message::Standard(msg)) => {
                msg.text.and_then(|txt| Some(println!("Message: {}", txt)));
            },
            _ => {}
        };
    }

    fn on_close(&mut self, cli: &RtmClient) {
        println!("on_close");
        self.sender = None
    }

    fn on_connect(&mut self, cli: &RtmClient) {
        println!("on_connect");
        self.sender = Some(cli.sender().clone())
    }
}

#[derive(Deserialize, Serialize)]
struct MailgunEmailReceived {
    recipient: String,
    sender: String,
    from: String,
    subject: String,
    #[serde(rename(serialize = "body-plain"))]
    body_plain: String,
    #[serde(rename(serialize = "stripped-text"))]
    stripped_text: String,
    #[serde(rename(serialize = "stripped-signature"))]
    stripped_signature: String,
    #[serde(rename(serialize = "body-html"))]
    body_html: String,
    #[serde(rename(serialize = "stripped-html"))]
    stripped_html: String,
    #[serde(rename(serialize = "attachment-count"))]
    attachment_count: String,
    #[serde(rename(serialize = "attachment-x"))]
    attachment_x: String,
    timestamp: String,
    token: String,
    signature: String,
    #[serde(rename(serialize = "message-headers"))]
    message_headers: String,
    #[serde(rename(serialize = "content-id-map"))]
    content_id_map: String,
}

#[post("/no-reply/response", data = "<email>")]
fn no_reply_response(email: Json<MailgunEmailReceived>) -> String {
    println!("{}", serde_json::to_string(&email.into_inner()).unwrap());
    String::from("Hello, world!")
}

fn env_or_panic(k: &str) -> String {
    match env::var(k)  {
        Ok(val) => val,
        Err(msg) => panic!(format!("No {} in environment: {}", k, msg))
    }
}

fn main() {
    dotenv().ok();

    let api_key = env_or_panic("SLACK_API_TOKEN");
    let mailgun_api_key = env_or_panic("MAILGUN_API_KEY");
    let slack_thread = thread::spawn(move || {
        let mut handler = MyHandler{
            sender: None
        };
        let r = RtmClient::login_and_run(&api_key, &mut handler);
        match r {
            Ok(_) => {}
            Err(err) => panic!("Error: {}", err),
        }
    });
    rocket::ignite().mount("/", routes![no_reply_response]).launch();
    let res = slack_thread.join();
    match res { 
        Ok(_) => {}
        Err(_) => panic!("Error joining string")
    }
}
