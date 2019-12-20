use std::error::Error as StdError;
use std::fmt::{self, Display};

use sha2::Sha256;
use hmac::{Hmac, Mac};
type HmacSha256 = Hmac<Sha256>;
use serde::{Serialize, Deserialize};
use serde_json::{Value};
use warp::Rejection;

pub struct EmailTemplate {
    pub recipient: String,
    pub subject: String,
    pub template: String,
    pub in_reply_to: String,
    pub references: String
}

#[derive(Debug)]
pub enum MailgunError {
    JsonError(String),
    HmacError(String),
    MailgunError(String),
}
impl std::convert::From<serde_json::Error> for MailgunError {
    fn from(_error: serde_json::Error) -> Self {
        // TODO: properly implement this
        MailgunError::JsonError(String::from("JSON Error"))
    }
}
impl std::convert::From<MailgunError> for Rejection {
    fn from(err: MailgunError) -> Rejection {
        warp::reject::custom(err)
    }
}

impl Display for MailgunError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            MailgunError::JsonError(s) => s,
            MailgunError::HmacError(s) => s,
            MailgunError::MailgunError(s) => s,
        })
    }
}
impl StdError for MailgunError {}


#[derive(Serialize, Deserialize, Debug)]
pub struct MailgunEmailReceived {
    pub sender: String,
    pub from: String,
    pub subject: String,
    #[serde(rename = "body-plain")]
    pub body_plain: String,
    pub timestamp: i64,
    pub token: String,
    pub signature: String,
    #[serde(rename = "message-headers")]
    pub message_headers: String,
}
impl MailgunEmailReceived {
    // :( this is not nice code, there has to be a better way to structure this code.
    pub fn get_message_id(&self) -> Result<String, MailgunError> {
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

#[derive(Clone)]
pub struct Mailgun {
    pub api_key: String,
    pub domain: String,
    pub from: String
}
impl Mailgun {
    pub fn verify_hmac(&self, email: &MailgunEmailReceived) -> Result<(), MailgunError> {
        let mut mac = HmacSha256::new_varkey(&self.api_key.clone().into_bytes())
            .map_err(|_| MailgunError::HmacError("Unable to create MAC".into()))?;

        let msg = email.timestamp.to_string() + &email.token;
        mac.input(&msg.into_bytes());

        let signature_bytes = hex::decode(&email.signature)
            .map_err(|_| MailgunError::HmacError("Unable to decode signature".into()))?;
        mac.verify(&signature_bytes)
            .map_err(|_| MailgunError::HmacError("Bad HMAC".into()))
    }

    pub fn send_email(&self, email: &EmailTemplate) -> Result<(), MailgunError> {
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
            .map_err(|e| MailgunError::MailgunError(format!("Unable to make request: {}", e)))?;
        info!("Email autoresponder sent to: {}", email.recipient);
        Ok(())
    }
}

