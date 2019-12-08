#!/bin/sh -e
cargo build --release
ssh "root@$1.lichess.ovh" mv /usr/local/bin/limail /usr/local/bin/limail.bak || (echo "first deploy on this server? set up service/env/run and comment out this line" && false)
scp ./target/release/limail "root@$1.lichess.ovh":/usr/local/bin/limail
ssh "root@$1.lichess.ovh" systemctl restart limail
