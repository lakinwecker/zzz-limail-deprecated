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
#![feature(plugin)]
#![feature(box_patterns)]

extern crate dotenv;
extern crate hex;
extern crate hmac;
#[macro_use] extern crate log;
extern crate pretty_env_logger;
extern crate reqwest;
extern crate serde;
extern crate serde_json;
extern crate sha2;
extern crate tokio;
extern crate warp;
extern crate futures;


mod slack;
use slack::{Slack, SlackMessage};
mod mailgun;
use mailgun::{
    EmailTemplate,
    Mailgun,
    MailgunEmailReceived,
    MailgunError,
};

use std::env;
use std::string::String;
use std::net::SocketAddr;
use std::error::Error as StdError;
use std::fmt::{self, Display};
use std::str;
use futures::stream::{Stream};
use crate::futures::Future;

use serde::Serialize;

use dotenv::dotenv;

use warp::{
    path,
    Filter,
    Rejection,
    http::{StatusCode},
    filters::multipart::{self, FormData, Part},
};

fn env_or_panic(k: &str) -> String {
    match env::var(k)  {
        Ok(val) => val,
        Err(msg) => panic!(format!("No {} in environment: {}", k, msg))
    }
}

fn main() {
    dotenv().ok();
    pretty_env_logger::init();

    let mailgun = Mailgun {
        api_key: env_or_panic("MAILGUN_API_KEY"),
        domain: env_or_panic("MAILGUN_DOMAIN"),
        from: env_or_panic("MAILGUN_FROM")
    };
    let mailgun = warp::any().map(move || mailgun.clone());

    let slack = Slack {
        api_key: env_or_panic("SLACK_API_TOKEN")
    };
    let slack = warp::any().map(move || slack.clone());

    let basics = warp::post2()
        .and(warp::body::content_length_limit(1024 * 1024 * 2)) // 2 MB right?
        .and(mailgun.clone());

    let urlencoded = basics.clone()
        .and(warp::body::form());

    let no_reply_urlencoded = urlencoded.clone()
        .and(path!("emails" / "responder" / String))
        .and_then(send_no_reply_template)
        .recover(recover_error);

    let no_reply_multipart = basics.clone()
        .and(path!("emails" / "responder" / String))
        .and(multipart::form())
        .and_then(send_no_reply_template_multipart)
        .recover(recover_error);

    let forward_email = urlencoded.clone()
        .and(slack.clone())
        .and(path!("emails" / "forward" / "slack" / String))
        .and_then(forward_email_to_slack)
        .recover(recover_error);

    let forward_email_multipart = basics.clone()
        .and(slack.clone())
        .and(path!("emails" / "forward" / "slack" / String))
        .and(multipart::form())
        .and_then(forward_email_to_slack_multipart)
        .recover(recover_error);

    let socket_address: SocketAddr = env_or_panic("LISTEN_ADDRESS_PORT").parse()
        .expect("LISTEN_ADDRESS_PORT must be a valid SocketAddr");

    warp::serve(
        no_reply_urlencoded
        .or(no_reply_multipart)
        .or(forward_email)
        .or(forward_email_multipart)
    ).run(socket_address);

}


#[derive(Serialize)]
struct LimailErrorMessage {
    code: u16,
    message: String,
}

pub fn recover_error(err: Rejection) -> Result<impl warp::Reply, Rejection> {
    if let Some(err) = err.find_cause::<MailgunError>() {
        let (code, msg) = match err {
            MailgunError::JsonError(s) => (StatusCode::BAD_REQUEST, s),
            MailgunError::HmacError(s) => (StatusCode::BAD_REQUEST, s),
            MailgunError::MailgunError(s) => (StatusCode::INTERNAL_SERVER_ERROR, s),
        };

        let json = warp::reply::json(&LimailErrorMessage {
            code: code.as_u16(),
            message: msg.clone(),
        });
        Ok(warp::reply::with_status(json, code))
    } else {
        // Could be a NOT_FOUND, or any other internal error... here we just
        // let warp use its default rendering.
        Err(err)
    }
}

#[derive(Debug)]
pub enum MultipartError {
    MissingFields(),
}

impl StdError for MultipartError {}
impl Display for MultipartError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            MultipartError::MissingFields() => "MultipartError::MissingFields",
        })
    }
}
impl std::convert::From<MultipartError> for Rejection {
    fn from(err: MultipartError) -> Rejection {
        warp::reject::custom(err)
    }
}


fn part_to_string(part: Part) -> Option<String> {
    let bytes = part.concat2().wait().ok()?;
    match str::from_utf8(&bytes) {
        Ok(s) => Some(String::from(s)),
        Err(_) => None
    }
}

// Gross - Surely there is some way to do this that's easier . :(
fn multipart_to_mailgun(form_data: FormData) -> Result<MailgunEmailReceived, MultipartError> {
    let mut sender: Option<String> = None;
    let mut from: Option<String> = None;
    let mut subject: Option<String> = None;
    let mut body_plain: Option<String> = None;
    let mut timestamp: Option<i64> = None;
    let mut token: Option<String> = None;
    let mut signature: Option<String> = None;
    let mut message_headers: Option<String> = None;
    form_data.wait().for_each(|part| {
        match part {
            Ok(part) => {
                let name = String::from(part.name());
                match (&name[..], part_to_string(part)) {
                    ("sender", val) => sender = val,
                    ("from", val) => from = val,
                    ("subject", val) => subject = val,
                    ("body-plain", val) => body_plain = val,
                    ("timestamp", Some(val)) => timestamp = val.parse().ok(),
                    ("token", val) => token = val,
                    ("signature", val) => signature = val,
                    ("message-headers", val) => message_headers = val,
                    _ => ()
                }
            },
            _ => return
        }
    });
    match (sender, from, subject, body_plain, timestamp, token, signature, message_headers) {
        (Some(sender), Some(from), Some(subject), Some(body_plain),
         Some(timestamp), Some(token), Some(signature), Some(message_headers)) => Ok(MailgunEmailReceived {
            sender,
            from,
            subject,
            body_plain,
            timestamp,
            token,
            signature,
            message_headers,
        }),
        _ => Err(MultipartError::MissingFields())
    }
}

fn send_no_reply_template_multipart(mailgun: Mailgun, template: String, form_data: FormData)
    -> Result<impl warp::Reply, Rejection>
{
    let mailgun_received = multipart_to_mailgun(form_data)?;
    send_no_reply_template(mailgun, mailgun_received, template)
}


fn send_no_reply_template(mailgun: Mailgun, email: MailgunEmailReceived, template: String)
    -> Result<impl warp::Reply, Rejection>
{
    mailgun.verify_hmac(&email)?;
    let message_id = email.get_message_id()?;
    mailgun.send_email(&EmailTemplate {
        recipient: email.from,
        subject: format!("Re: {}", email.subject),
        template: template,
        in_reply_to: message_id.clone(),
        references: message_id

    })?;
    Ok("Message Processed")
}

fn forward_email_to_slack_multipart(
    mailgun: Mailgun,
    slack_client: Slack,
    channel_id: String,
    form_data: FormData,
) ->  Result<impl warp::Reply, Rejection> {
    let mailgun_received = multipart_to_mailgun(form_data)?;
    forward_email_to_slack(mailgun, mailgun_received, slack_client, channel_id)
}

fn forward_email_to_slack(
    mailgun: Mailgun,
    email: MailgunEmailReceived,
    slack_client: Slack,
    channel_id: String
) ->  Result<impl warp::Reply, Rejection> {
    mailgun.verify_hmac(&email)?;

    let text = format!("Email Received: {}", email.subject.clone());
    slack_client
        .send_message(&SlackMessage{ 
            channel: channel_id.clone(),
            text: text.clone(),
            thread_ts: None,
            as_user: true
        })
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
        })?;
    Ok(String::from("Sent"))

}

