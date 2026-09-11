use anyhow::{bail, Context};
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::Path,
    ptr,
};
use sworm_remote::Identity;
use sworm_server::auth;

pub fn run(
    config_dir: &Path,
    host: Option<String>,
    listen: Option<SocketAddr>,
) -> anyhow::Result<()> {
    let host = host
        .map(Ok)
        .unwrap_or_else(|| first_non_loopback_address().map(|address| address.to_string()))?;
    authority_host(&host)?;
    let port = resolve_listen(config_dir, listen)?.port();
    let identity = Identity::load_or_generate(config_dir, "server")?;
    let token = auth::write_pairing_token(config_dir)?;
    let fingerprint = identity.fingerprint();
    let link = pairing_link(&host, port, &fingerprint.to_string(), &token)?;

    println!("Pairing token: {token}");
    println!("Valid for 10 minutes");
    println!("Server fingerprint: {fingerprint}");
    println!("Pairing link: {link}");
    Ok(())
}

fn resolve_listen(config_dir: &Path, listen: Option<SocketAddr>) -> anyhow::Result<SocketAddr> {
    let configured = crate::config::load(config_dir)?;
    Ok(listen.unwrap_or(configured.listen))
}

fn pairing_link(host: &str, port: u16, fingerprint: &str, token: &str) -> anyhow::Result<String> {
    let host = authority_host(host)?;
    Ok(format!("sworm-pair://{host}:{port}/{fingerprint}/{token}"))
}

fn authority_host(host: &str) -> anyhow::Result<String> {
    if host.is_empty() || host.chars().any(char::is_whitespace) {
        bail!("pairing host must not be empty or contain whitespace");
    }
    if host.contains(['[', ']']) {
        let address = host
            .strip_prefix('[')
            .and_then(|host| host.strip_suffix(']'))
            .context("invalid bracketed IPv6 pairing host")?
            .parse::<Ipv6Addr>()
            .context("invalid bracketed IPv6 pairing host")?;
        return ipv6_authority(address);
    }
    if let Ok(address) = host.parse::<Ipv6Addr>() {
        return ipv6_authority(address);
    }
    if host.contains(':') {
        bail!("pairing host must not include a port");
    }
    if !valid_reg_name(host) {
        bail!("pairing host is not a valid URI host");
    }
    Ok(host.to_owned())
}

fn ipv6_authority(address: Ipv6Addr) -> anyhow::Result<String> {
    if address.is_unicast_link_local() {
        bail!("IPv6 link-local pairing hosts require unsupported scope identifiers");
    }
    Ok(format!("[{address}]"))
}

fn valid_reg_name(host: &str) -> bool {
    let bytes = host.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            byte if byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'-' | b'.'
                        | b'_'
                        | b'~'
                        | b'!'
                        | b'$'
                        | b'&'
                        | b'\''
                        | b'('
                        | b')'
                        | b'*'
                        | b'+'
                        | b','
                        | b';'
                        | b'='
                ) =>
            {
                index += 1;
            }
            b'%' if bytes
                .get(index + 1..index + 3)
                .is_some_and(|escape| escape.iter().all(u8::is_ascii_hexdigit)) =>
            {
                index += 3;
            }
            _ => return false,
        }
    }
    true
}

fn first_non_loopback_address() -> anyhow::Result<IpAddr> {
    let mut interfaces = ptr::null_mut();
    // SAFETY: getifaddrs initializes `interfaces` on success. The returned list remains
    // valid until the matching freeifaddrs call below.
    if unsafe { libc::getifaddrs(&mut interfaces) } != 0 {
        return Err(std::io::Error::last_os_error()).context("enumerate network interfaces");
    }
    let interfaces = InterfaceList(interfaces);
    let mut current = interfaces.0;
    while !current.is_null() {
        // SAFETY: every node belongs to the live getifaddrs list.
        let interface = unsafe { &*current };
        if interface.ifa_flags & libc::IFF_LOOPBACK as libc::c_uint == 0
            && interface.ifa_flags & libc::IFF_UP as libc::c_uint != 0
            && !interface.ifa_addr.is_null()
        {
            // SAFETY: the address family determines the concrete sockaddr layout.
            if let Some(address) = unsafe { ip_address(interface.ifa_addr) } {
                if usable_address(address) {
                    return Ok(address);
                }
            }
        }
        current = interface.ifa_next;
    }
    bail!("no usable non-loopback network address found; pass --host explicitly")
}

fn usable_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => !address.is_loopback() && !address.is_unspecified(),
        IpAddr::V6(address) => {
            !address.is_loopback() && !address.is_unspecified() && !address.is_unicast_link_local()
        }
    }
}

unsafe fn ip_address(address: *const libc::sockaddr) -> Option<IpAddr> {
    // SAFETY: caller guarantees `address` points into a live getifaddrs node.
    match unsafe { (*address).sa_family as libc::c_int } {
        libc::AF_INET => {
            // SAFETY: AF_INET identifies a sockaddr_in.
            let address = unsafe { &*address.cast::<libc::sockaddr_in>() };
            Some(IpAddr::V4(Ipv4Addr::from(
                address.sin_addr.s_addr.to_ne_bytes(),
            )))
        }
        libc::AF_INET6 => {
            // SAFETY: AF_INET6 identifies a sockaddr_in6.
            let address = unsafe { &*address.cast::<libc::sockaddr_in6>() };
            Some(IpAddr::V6(Ipv6Addr::from(address.sin6_addr.s6_addr)))
        }
        _ => None,
    }
}

struct InterfaceList(*mut libc::ifaddrs);

impl Drop for InterfaceList {
    fn drop(&mut self) {
        // SAFETY: this pointer came from a successful getifaddrs call and is freed once.
        unsafe { libc::freeifaddrs(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::{pairing_link, resolve_listen, usable_address};
    use std::{fs, net::IpAddr};

    #[test]
    fn configured_or_explicit_listen_port_is_advertised() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("server.toml"),
            "listen = \"0.0.0.0:8123\"\n",
        )
        .unwrap();

        let configured = resolve_listen(directory.path(), None).unwrap();
        assert_eq!(
            pairing_link("homelab", configured.port(), "SHA256:abcd", "token").unwrap(),
            "sworm-pair://homelab:8123/SHA256:abcd/token"
        );

        let explicit =
            resolve_listen(directory.path(), Some("127.0.0.1:9000".parse().unwrap())).unwrap();
        assert_eq!(
            pairing_link("homelab", explicit.port(), "SHA256:abcd", "token").unwrap(),
            "sworm-pair://homelab:9000/SHA256:abcd/token"
        );
    }

    #[test]
    fn pairing_link_formats_dns_and_ip_authorities() {
        assert_eq!(
            pairing_link("homelab", 7420, "SHA256:abcd", "token").unwrap(),
            "sworm-pair://homelab:7420/SHA256:abcd/token"
        );
        assert_eq!(
            pairing_link("192.0.2.10", 7420, "SHA256:abcd", "token").unwrap(),
            "sworm-pair://192.0.2.10:7420/SHA256:abcd/token"
        );
        assert_eq!(
            pairing_link("2001:db8::10", 7420, "SHA256:abcd", "token").unwrap(),
            "sworm-pair://[2001:db8::10]:7420/SHA256:abcd/token"
        );
        assert_eq!(
            pairing_link("[2001:db8::10]", 7420, "SHA256:abcd", "token").unwrap(),
            "sworm-pair://[2001:db8::10]:7420/SHA256:abcd/token"
        );
    }

    #[test]
    fn pairing_link_rejects_invalid_authorities() {
        for host in [
            "",
            "home lab",
            "homelab:9000",
            "home/lab",
            "home?lab",
            "home#lab",
            "user@homelab",
            "[not-ipv6]",
            "[2001:db8::10",
            "2001:db8::10]",
            "fe80::1",
            "[fe80::1]",
        ] {
            assert!(pairing_link(host, 7420, "SHA256:abcd", "token").is_err());
        }
    }

    #[test]
    fn scoped_ipv6_address_is_not_advertised_without_scope_support() {
        assert!(!usable_address("fe80::1".parse::<IpAddr>().unwrap()));
        assert!(usable_address("2001:db8::1".parse::<IpAddr>().unwrap()));
    }
}
