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

extern crate dotenv;
extern crate hex;
extern crate hmac;
#[macro_use] extern crate log;
extern crate reqwest;
extern crate rocket;
extern crate rocket_contrib;
extern crate serde;
extern crate serde_json;
extern crate sha2;

use std::env;
use std::string::String;

use rocket::{Rocket, post, routes, FromForm, State};
use rocket::request::{LenientForm};
use rocket::response::status::BadRequest;

use reqwest::header::{CONTENT_TYPE, AUTHORIZATION};

use dotenv::dotenv;
use sha2::Sha256;
use hmac::{Hmac, Mac};

use serde::{Serialize, Deserialize};
use serde_json::{Value};

type HmacSha256 = Hmac<Sha256>;


const SLACK_URL: &str = "https://slack.com/api/";
enum SlackError {
    HttpError(String)
}

impl std::convert::From<reqwest::Error> for SlackError {
    fn from(_error: reqwest::Error) -> Self {
        // TODO: properly implement this
        SlackError::HttpError(String::from("Reqwest Error"))
    }
}
struct Slack {
    api_key: String
}
#[derive(Serialize, Deserialize, Debug)]
struct MessageResponse {
    ok: bool,
    ts: String,
}
#[derive(Serialize, Deserialize, Debug)]
struct SlackMessage {
    channel: String,
    text: String,
    thread_ts: Option<String>, // TODO: Make this better typed
    as_user: bool
}
impl Slack {
    fn send_message(&self, message: &SlackMessage) -> Result<MessageResponse, SlackError> {
        let client = reqwest::Client::new();
        let url = format!("{}/chat.postMessage", SLACK_URL);
        let msg_response: MessageResponse = client.post(&url)
            .header(AUTHORIZATION, format!("Bearer {}", &self.api_key))
            .header(CONTENT_TYPE, "application/json")
            .json(&message)
            .send()?
            .json()?;

        Ok(msg_response)
    }
}




struct EmailTemplate {
    recipient: String,
    subject: String,
    template: String,
    in_reply_to: String,
    references: String
}

enum MailgunError {
    JsonError(String)
}
impl std::convert::From<serde_json::Error> for MailgunError {
    fn from(_error: serde_json::Error) -> Self {
        // TODO: properly implement this
        MailgunError::JsonError(String::from("JSON Error"))
    }
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
    #[form(field = "message-headers")]
    message_headers: String,
}
impl MailgunEmailReceived {
    fn get_message_id(&self) -> Result<String, MailgunError> {
        let v: Value = serde_json::from_str(&self.message_headers)?;
        let mailgun_error = MailgunError::JsonError(String::from("Unable to parse json"));
        let err = Err(MailgunError::JsonError(String::from("Unable to parse json")));
        match v {
            Value::Array(values) => {
                values.iter().find(
                    |v| match v {
                        Value::Array(value_pair) => {
                            value_pair.len() == 2 && (match &value_pair[0] {
                                Value::String(s) => s.to_lowercase() == "message-id",
                                _ => false
                            })
                        },
                        _ => false
                    }
                ).and_then(
                    |v| match v {
                        Value::Array(value_pair) => {
                            match &value_pair[1] {
                                Value::String (s) => Some(s.clone()),
                                _ => None
                            }
                        },
                        _ => None
                    }
                ).ok_or(mailgun_error)
            },
            _ => err
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct MailgunMessageHeaders {

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
            ("h:X-Autoreply", &String::from("yes")),
            ("h:In-Reply-To", &email.in_reply_to),
            ("h:References", &email.references)
        ];
        let client = reqwest::Client::new();
        let url = format!("https://api.mailgun.net/v3/{}/messages", self.domain);
        client.post(&url)
            .basic_auth("api", Some(&self.api_key))
            .form(&params)
            .send()
            .map_err(|_| String::from("Unable to make request"))?;
        info!("Email autoresponder sent to: {}", email.recipient);
        Ok(())
    }
}


fn bad_request(msg: &str) -> BadRequest<String> {
    warn!("Bad request: {}!", msg);
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
    let message_id = email.get_message_id()
        .map_err(|_| bad_request("Unable to get message_id"))?;
    mailgun.send_email(&EmailTemplate {
        recipient: email.from,
        subject: format!("Re: {}", email.subject),
        template: template,
        in_reply_to: message_id.clone(),
        references: message_id

    }).map_err(|msg| bad_request(&msg))?;
    Ok(String::from("hello world"))

}

#[post("/emails/forward/slack/<channel_id>", data = "<email_form>")]
fn forward_email_to_slack(
    slack_client_state: State<Slack>,
    mailgun: State<Mailgun>,
    channel_id: String,
    email_form: LenientForm<MailgunEmailReceived>
) ->  Result<String, BadRequest<String>> {
    let slack_client = slack_client_state.inner();
    let email = email_form.into_inner();

    mailgun.inner().verify_hmac(&email)?;

    let text = format!("Email Received: {}", email.subject.clone());
    slack_client
        .send_message(&SlackMessage{ 
            channel: channel_id.clone(),
            text: text.clone(),
            thread_ts: None,
            as_user: true
        })
        .map_err(|_| bad_request("Unable to send slack message"))
        .and_then(|msg_response| {
            let slack_message = format!(
                "```{}```\n(from: {})",
                email.body_plain.clone(),
                email.sender.clone()
            );
            slack_client
                .send_message(&SlackMessage{ 
                    channel: channel_id.clone(),
                    text: slack_message.clone(),
                    thread_ts: Some(msg_response.ts.clone()),
                    as_user: true
                })
                .map_err(|_| bad_request("Unable to send slack message"))
        })?;
    Ok(String::from("Sent"))

}

fn env_or_panic(k: &str) -> String {
    match env::var(k)  {
        Ok(val) => val,
        Err(msg) => panic!(format!("No {} in environment: {}", k, msg))
    }
}

fn mount() -> Rocket {
    rocket::ignite().mount("/", routes![
        no_reply_response,
        forward_email_to_slack
    ])
}

fn main() {
    dotenv().ok();
    env_logger::init();

    let rocket = mount();

    rocket.manage(Slack {
        api_key: env_or_panic("SLACK_API_TOKEN")
    }).manage(Mailgun {
        api_key: env_or_panic("MAILGUN_API_KEY"),
        domain: env_or_panic("MAILGUN_DOMAIN"),
        from: env_or_panic("MAILGUN_FROM")
    }).launch();
}
