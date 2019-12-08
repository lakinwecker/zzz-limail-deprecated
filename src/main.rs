// Limail an email helper for lichess
// Copyright (C) 2019  Lakin Wecker
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

#![feature(proc_macro_hygiene, decl_macro)]
#![feature(box_patterns)]

extern crate rocket;
extern crate rocket_contrib;
extern crate dotenv;
extern crate slack;
extern crate hex;
extern crate reqwest;
extern crate hmac;
extern crate sha2;
#[macro_use] extern crate log;

use std::sync::Mutex;
use std::env;
use std::string::String;
use std::thread;

use rocket::{Rocket, post, routes, FromForm, State};
use rocket::request::{LenientForm};
use rocket::response::status::BadRequest;

use slack::{Event, RtmClient, Sender};
use dotenv::dotenv;
use sha2::Sha256;
use hmac::{Hmac, Mac};

type HmacSha256 = Hmac<Sha256>;

struct SlackHandler {
}

#[allow(unused_variables)]
impl slack::EventHandler for SlackHandler {
    fn on_event(&mut self, cli: &RtmClient, event: Event) {
    }

    fn on_close(&mut self, cli: &RtmClient) {
    }

    fn on_connect(&mut self, cli: &RtmClient) {
    }
}

struct EmailTemplate {
    recipient: String,
    subject: String,
    template: String
}

#[derive(FromForm)]
struct MailgunEmailReceived {
    sender: String,
    from: String,
    subject: String,
    #[form(field = "body-plain")]
    body_plain: String,
    timestamp: i64,
    token: String,
    signature: String,

}

struct Mailgun {
    api_key: String,
    domain: String,
    from: String
}
impl Mailgun {
    fn verify_hmac(&self, email: &MailgunEmailReceived) -> Result<(), BadRequest<String>> {
        let mut mac = HmacSha256::new_varkey(&self.api_key.clone().into_bytes())
            .map_err(|_| bad_request("Unable to create MAC"))?;

        let msg = email.timestamp.to_string() + &email.token;
        mac.input(&msg.into_bytes());

        let signature_bytes = hex::decode(&email.signature)
            .map_err(|_| bad_request("Unable to decode signature"))?;
        mac.verify(&signature_bytes)
            .map_err(|_| bad_request("Bad HMAC"))
    }

    fn send_email(&self, email: &EmailTemplate) -> Result<(), String> {
        let params = [
            ("from", &self.from),
            ("to", &email.recipient),
            ("subject", &email.subject),
            ("template", &email.template),
            ("h:X-Autoreply", &String::from("yes"))
        ];
        let client = reqwest::Client::new();
        let url = format!("https://api.mailgun.net/v3/{}/messages", self.domain);
        client.post(&url)
            .basic_auth("api", Some(&self.api_key))
            .form(&params)
            .send()
            .map_err(|_| String::from("Unable to make request"))?;
        Ok(())
    }
}


fn bad_request(msg: &str) -> BadRequest<String> {
    BadRequest(Some(String::from(msg)))
}


#[post("/emails/responder/<template>", data = "<email_form>")]
fn no_reply_response(
    mailgun: State<Mailgun>,
    email_form: LenientForm<MailgunEmailReceived>,
    template: String
) ->  Result<String, BadRequest<String>> {
    let email = email_form.into_inner();
    let mailgun = mailgun.inner();
    mailgun.verify_hmac(&email)?;
    mailgun.send_email(&EmailTemplate {
        recipient: email.from,
        subject: format!("Re: {}", email.subject),
        template: template

    }).map_err(|msg| bad_request(&msg))?;
    Ok(String::from("hello world"))

}

#[post("/emails/forward/slack/<channel_id>", data = "<email_form>")]
fn forward_email_to_slack(
    slack_client_state: State<SlackClient>,
    mailgun: State<Mailgun>,
    channel_id: String,
    email_form: LenientForm<MailgunEmailReceived>
) ->  Result<String, BadRequest<String>> {
    let slack_client = slack_client_state.inner();
    let email = email_form.into_inner();
    let subject = email.subject.clone();
    let body_plain = email.body_plain.clone();
    let sender = email.sender.clone();

    mailgun.inner().verify_hmac(&email)?;

    slack_client.sender.lock().and_then(|s| {
       let slack_message = format!(
                "{}\n```{}```\n(from: {})",
                subject,
                body_plain,
                sender
            );
        s.send_message(&channel_id, &slack_message).and_then(|_| {
            Ok(String::from("hello world"))
        }).or_else(|_| {
            Ok(String::from("hello world"))
        })
    // TODO: the following is the wrong HTTP error.
    }).map_err(|_| bad_request("Unable to sendslack message"))
}

fn env_or_panic(k: &str) -> String {
    match env::var(k)  {
        Ok(val) => val,
        Err(msg) => panic!(format!("No {} in environment: {}", k, msg))
    }
}

fn mount() -> Rocket {
    rocket::ignite().mount("/", routes![
        no_reply_response
    ])
}

struct SlackClient {
    sender: Mutex<Sender>,
}

fn main() {
    dotenv().ok();
    env_logger::init();

    let api_key = env_or_panic("SLACK_API_TOKEN");
    let slack_client = RtmClient::login(&api_key)
        .expect("Unable to login with slack");
    let sender = Mutex::new(slack_client.sender().clone());

    let slack_thread = thread::spawn(move || {
        loop {
            let mut handler = SlackHandler{};
            info!("Connecting to Slack");
            let r = slack_client.run(&mut handler);
            match r {
                Ok(_) => { }
                Err(err) => error!("Slack Error: {}", err),
            }
        }
    });
    let rocket = mount();
    rocket.manage(SlackClient {
        sender
    }).manage(Mailgun {
        api_key: env_or_panic("MAILGUN_API_KEY"),
        domain: env_or_panic("MAILGUN_DOMAIN"),
        from: env_or_panic("MAILGUN_FROM")
    }).launch();
    slack_thread.join().expect("Unable to join slack thread");
}
