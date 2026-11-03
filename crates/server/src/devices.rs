//! Helps phones and other computers find this server on the local network.

use std::net::{IpAddr, SocketAddr, UdpSocket};

/// Best guess at this machine's LAN address. "Connecting" a UDP socket sends no packets;
/// it only asks the OS which interface would be used.
pub fn lan_ip() -> Option<IpAddr> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("192.168.255.255:9").or_else(|_| sock.connect("10.255.255.255:9")).ok()?;
    let ip = sock.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

/// URLs other devices can use to reach the server, most useful first.
pub fn reachable_urls(bind: SocketAddr) -> Vec<String> {
    let port = bind.port();
    let ip = bind.ip();
    if ip.is_loopback() {
        return vec![format!("http://localhost:{port}")];
    }
    let mut urls = Vec::new();
    if ip.is_unspecified() {
        if let Some(lan) = lan_ip() {
            urls.push(format!("http://{}", SocketAddr::new(lan, port)));
        }
        urls.push(format!("http://localhost:{port}"));
    } else {
        urls.push(format!("http://{bind}"));
    }
    urls
}

pub fn qr_svg(text: &str) -> Option<String> {
    use qrcode::{render::svg, QrCode};
    let code = QrCode::new(text.as_bytes()).ok()?;
    Some(code.render::<svg::Color>().min_dimensions(180, 180).quiet_zone(true).build())
}

/// QR code drawn with Unicode half blocks, for the terminal.
pub fn qr_terminal(text: &str) -> Option<String> {
    use qrcode::{render::unicode, QrCode};
    let code = QrCode::new(text.as_bytes()).ok()?;
    Some(
        code.render::<unicode::Dense1x2>()
            .dark_color(unicode::Dense1x2::Light)
            .light_color(unicode::Dense1x2::Dark)
            .build(),
    )
}
