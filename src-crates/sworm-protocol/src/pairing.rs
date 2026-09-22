use std::{fmt, net::Ipv6Addr, str::FromStr};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairLink {
    pub host: String,
    pub port: u16,
    pub fingerprint: String,
    pub token: String,
}

impl PairLink {
    pub fn address(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

impl fmt::Display for PairLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "sworm-pair://{}/{}/{}",
            self.address(),
            self.fingerprint,
            self.token
        )
    }
}

impl FromStr for PairLink {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let rest = value
            .strip_prefix("sworm-pair://")
            .ok_or("pairing scheme must be sworm-pair://")?;
        let mut parts = rest.split('/');
        let authority = parts.next().ok_or("missing host segment")?;
        let fingerprint = parts
            .next()
            .filter(|value| !value.is_empty())
            .ok_or("missing fingerprint segment")?;
        let token = parts
            .next()
            .filter(|value| !value.is_empty())
            .ok_or("missing token segment")?;
        if parts.next().is_some() {
            return Err("unexpected segment after token".to_owned());
        }
        let (host, port) = if let Some(ipv6) = authority.strip_prefix('[') {
            let (host, port) = ipv6
                .split_once(']')
                .ok_or("invalid host segment: unclosed IPv6 address")?;
            let address: Ipv6Addr = host
                .parse()
                .map_err(|_| "invalid host segment: IPv6 address")?;
            if address.is_unicast_link_local() {
                return Err("invalid host segment: scoped IPv6 unsupported".to_owned());
            }
            (host, port.strip_prefix(':').ok_or("missing port segment")?)
        } else {
            let (host, port) = authority.rsplit_once(':').ok_or("missing port segment")?;
            if host.is_empty()
                || !host
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
            {
                return Err("invalid host segment".to_owned());
            }
            (host, port)
        };
        if port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("invalid port segment".to_owned());
        }
        let port = port
            .parse::<u16>()
            .ok()
            .filter(|port| *port != 0)
            .ok_or("invalid port segment")?;
        let hex = fingerprint
            .get(..7)
            .filter(|prefix| prefix.eq_ignore_ascii_case("SHA256:"))
            .map_or(fingerprint, |_| &fingerprint[7..]);
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("invalid fingerprint segment: expected SHA-256 hex".to_owned());
        }
        if !token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err("invalid token segment".to_owned());
        }
        Ok(Self {
            host: host.to_owned(),
            port,
            fingerprint: format!("SHA256:{}", hex.to_ascii_lowercase()),
            token: token.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::PairLink;

    #[test]
    fn pair_link_parses_and_rejects() {
        let fingerprint = format!("SHA256:{}", "ab".repeat(32));
        for host in ["homelab", "192.0.2.10", "[2001:db8::10]"] {
            let value = format!("sworm-pair://{host}:7420/{fingerprint}/token");
            let parsed: PairLink = value.parse().unwrap();
            assert_eq!(parsed.to_string(), value);
        }
        for (value, segment) in [
            (format!("sworm-pair://host:7420/{fingerprint}"), "token"),
            ("sworm-pair://host:7420/bad/token".to_owned(), "fingerprint"),
            (format!("sworm-pair://host:no/{fingerprint}/token"), "port"),
            (
                format!("sworm-pair://host:65536/{fingerprint}/token"),
                "port",
            ),
            (format!("sworm://host:7420/{fingerprint}/token"), "scheme"),
            (
                format!("sworm-pair://user@host:7420/{fingerprint}/token"),
                "host",
            ),
        ] {
            assert!(value.parse::<PairLink>().unwrap_err().contains(segment));
        }
    }
}
