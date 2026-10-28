//! Public HTTPS documents only: DNS is checked once and pinned for each connection.
use dumbmanager_core::legacy::{LegacyError, LegacyResult};
use reqwest::{
    blocking::Client,
    header::{CONTENT_TYPE, LOCATION},
    redirect::Policy,
};
use std::{
    io::Read,
    net::{IpAddr, ToSocketAddrs},
    time::Duration,
};

fn denied() -> LegacyError {
    LegacyError::invalid("Research can read only public HTTPS documents; private, reserved and unresolved destinations are blocked.")
}
fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let [a, b, c, _] = v.octets();
            !(a == 0
                || a == 10
                || a == 127
                || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192
                    && (b == 168 || (b == 0 && (c == 0 || c == 2)) || (b == 88 && c == 99)))
                || (a == 198 && ((18..=19).contains(&b) || (b == 51 && c == 100)))
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(v) => {
            let s = v.segments();
            (s[0] & 0xe000) == 0x2000
                && !(s[0] == 0x2001 && (s[1] < 0x200 || s[1] == 0xdb8))
                && s[0] != 0x2002
                && s[0] != 0x3ffe
                && !(s[0] == 0x3fff && s[1] < 0x1000)
        }
    }
}
pub(super) struct Document {
    pub url: String,
    pub html: String,
}
pub(super) fn document(value: &str) -> LegacyResult<Document> {
    let mut next = super::public_url(value).ok_or_else(denied)?;
    for _ in 0..5 {
        let url = url::Url::parse(&next).map_err(|_| denied())?;
        let host = url.host_str().ok_or_else(denied)?;
        let addresses: Vec<_> = (host, 443)
            .to_socket_addrs()
            .map_err(|_| denied())?
            .take(17)
            .collect();
        if addresses.is_empty()
            || addresses.len() > 16
            || addresses.iter().any(|a| !public_ip(a.ip()))
        {
            return Err(denied());
        }
        // No proxy or second DNS lookup. TLS still authenticates the original hostname.
        let client = Client::builder()
            .no_proxy()
            .redirect(Policy::none())
            .referer(false)
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(18))
            .resolve_to_addrs(host, &addresses)
            .build()
            .map_err(|_| denied())?;
        let response = client
            .get(url.clone())
            .header("Accept", "text/html, application/xhtml+xml")
            .send()
            .map_err(|_| {
                LegacyError::unavailable("The public research document could not be fetched.")
            })?;
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(denied)?;
            next = super::public_url(url.join(location).map_err(|_| denied())?.as_str())
                .ok_or_else(denied)?;
            continue;
        }
        if !response.status().is_success() {
            return Err(LegacyError::unavailable(
                "The public source did not return a readable document.",
            ));
        }
        let mime = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        if !["text/html", "application/xhtml+xml"].contains(&mime) {
            return Err(LegacyError::invalid("Research reads HTML documents only."));
        }
        if response.content_length().is_some_and(|n| n > 1_048_576) {
            return Err(LegacyError::invalid(
                "The public research document is too large.",
            ));
        }
        let mut bytes = Vec::new();
        response
            .take(1_048_577)
            .read_to_end(&mut bytes)
            .map_err(|_| {
                LegacyError::unavailable("The public research document could not be read.")
            })?;
        if bytes.len() > 1_048_576 {
            return Err(LegacyError::invalid(
                "The public research document is too large.",
            ));
        }
        return Ok(Document {
            url: next,
            html: String::from_utf8_lossy(&bytes).into_owned(),
        });
    }
    Err(LegacyError::invalid(
        "Research stopped after too many redirects.",
    ))
}
