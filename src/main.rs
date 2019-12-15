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

mod slack;
// use slack::{Slack, SlackMessage};
mod mailgun;
use mailgun::{
    Mailgun,
    MailgunEmailReceived,
    EmailTemplate,
    mailgun_to_warp_rejection
};

use std::env;
use std::string::String;
use std::net::SocketAddr;

use dotenv::dotenv;

use warp::{path, Filter, Rejection};


/*

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

fn mount() -> Rocket {
    rocket::ignite().mount("/", routes![
        no_reply_response,
        forward_email_to_slack
    ])
}*/

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


    // GET /hello/warp => 200 OK with body "Hello, warp!"
    let no_reply_urlencoded = warp::post2()
        .and(mailgun)
        .and(path!("emails" / "responder" / String))
        .and(warp::body::content_length_limit(1024 * 1024 * 2)) // 2 MB right?
        .and(warp::body::form())
        .and_then(send_no_reply_template)
        .recover(mailgun_to_warp_rejection);

    let socket_address: SocketAddr = env_or_panic("LISTEN_ADDRESS_PORT").parse()
        .expect("LISTEN_ADDRESS_PORT must be a valid SocketAddr");

    warp::serve(no_reply_urlencoded)
        .run(socket_address);

}


fn send_no_reply_template(mailgun: Mailgun, template: String, email: MailgunEmailReceived)
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
