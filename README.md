limail
======

A simple helper for lichess to manage auto responses and deal with email
from slack.



HTTP API
--------

### `POST /emails/responder/<template>`
A webhook for mailgun. Given an email, respond to it using the provided template.

### `POST /emails/forward/slack/<channel_id>`
A webhook for mailgun. Given an email, post it to slack in the given channel

License
-------

lila-tablebase is licensed under the GNU Affero General Public License 3.0 (or any later version at your
option). See the COPYING file for the full license text.

