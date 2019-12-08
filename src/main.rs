#![feature(proc_macro_hygiene, decl_macro)]
#![feature(box_patterns)]

extern crate rocket;
extern crate rocket_contrib;
extern crate dotenv;
extern crate slack;
extern crate crypto;
extern crate hex;
extern crate reqwest;

use std::sync::Mutex;
use std::env;
use std::string::String;
use std::thread;

use rocket::{Rocket, post, routes, FromForm, State};
use rocket::request::{LenientForm};
use rocket::response::status::BadRequest;

use slack::{Event, RtmClient, Message, Sender};
use dotenv::dotenv;
use crypto::sha2::Sha256;
use crypto::hmac::{Hmac};
use crypto::mac::{Mac, MacResult};

type HmacSha256 = Hmac<Sha256>;

struct SlackHandler {
    sender: Option<Sender>,
}

#[allow(unused_variables)]
impl slack::EventHandler for SlackHandler {
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
    fn verify_hmac(&self, email: &MailgunEmailReceived) -> Result<bool, BadRequest<String>> {
        let mut mac = Hmac::new(Sha256::new(), &self.api_key.clone().into_bytes());
        let msg = email.timestamp.to_string() + &email.token;
        mac.input(&msg.into_bytes());

        let signature_bytes = hex::decode(&email.signature)
            .map_err(|_| bad_request("Unable to decode signature"))?;
        if mac.result() == MacResult::new(&signature_bytes) {
            return Ok(true);
        }

        Err(bad_request("Bad hmac"))
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
                "{}\n ```{}```\n(from: {})",
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

    let api_key = env_or_panic("SLACK_API_TOKEN");
    let slack_client = RtmClient::login(&api_key)
        .expect("Unable to login with slack");
    let sender = Mutex::new(slack_client.sender().clone());

    let slack_thread = thread::spawn(move || {
        let mut handler = SlackHandler{
            sender: None
        };
        let r = slack_client.run(&mut handler);
        match r {
            Ok(_) => { }
            Err(err) => panic!("Error: {}", err),
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
