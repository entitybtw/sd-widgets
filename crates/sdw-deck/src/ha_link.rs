//! A one-shot web page for entering the Home Assistant address and token.
//!
//! Typing a 200-character token on the Deck's glass is the worst part of
//! setting Home Assistant up, so the Deck opens a temporary page on the local
//! network instead: the user pastes the token on a phone or a computer, the
//! page sends it back over the channel and the server stops. The path carries
//! a random nonce and the page only lives while the card is on screen, so
//! nothing is left listening after the fact.

use sdw_core::i18n::Lang;
use sdw_core::t;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tiny_http::{Header, Method, Request, Response, Server};

/// How long the page stays open without anyone using it.
const LIFETIME: Duration = Duration::from_secs(5 * 60);

pub struct HaLink {
    /// The address to open, nonce included.
    pub url: String,
    pub started: Instant,
    rx: Receiver<(String, String)>,
    stop: Arc<AtomicBool>,
}

impl HaLink {
    pub fn start(lang: Lang) -> Result<Self, String> {
        let server = Server::http("0.0.0.0:0").map_err(|e| format!("веб-страница: {e}"))?;
        let port = match server.server_addr() {
            tiny_http::ListenAddr::IP(addr) => addr.port(),
            _ => return Err("веб-страница: нет порта".into()),
        };
        let ip = sdw_core::util::local_ipv4()
            .map(|ip| ip.to_string())
            .ok_or_else(|| "нет адреса в локальной сети".to_string())?;
        let nonce = sdw_core::auth::random_secret(12);
        let path = format!("/{nonce}");
        let url = format!("http://{ip}:{port}{path}");
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let (tx, rx) = channel();
        let page = page_html(lang);
        std::thread::Builder::new()
            .name("sdw-ha-link".into())
            .spawn(move || {
                while !stop2.load(Ordering::Relaxed) {
                    match server.recv_timeout(Duration::from_millis(300)) {
                        Ok(Some(req)) => {
                            if handle(req, &path, &page, &tx, lang) {
                                break;
                            }
                        }
                        Ok(None) => {}
                        Err(_) => break,
                    }
                }
            })
            .map_err(|e| format!("веб-страница: {e}"))?;
        Ok(Self {
            url,
            started: Instant::now(),
            rx,
            stop,
        })
    }

    /// The address and token, once the page has them.
    pub fn take(&mut self) -> Option<(String, String)> {
        match self.rx.try_recv() {
            Ok(pair) => Some(pair),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => None,
        }
    }

    pub fn expired(&self) -> bool {
        self.started.elapsed() > LIFETIME
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for HaLink {
    fn drop(&mut self) {
        self.stop();
    }
}

/// One request. Returns true when the form was accepted and the server may
/// stop: the answer is already in the channel, so nothing else is expected.
fn handle(
    mut req: Request,
    path: &str,
    page: &str,
    tx: &std::sync::mpsc::Sender<(String, String)>,
    lang: Lang,
) -> bool {
    let url = req.url().to_string();
    let (route, _query) = url.split_once('?').unwrap_or((url.as_str(), ""));
    if route != path {
        let _ = req.respond(Response::from_string("not found").with_status_code(404));
        return false;
    }
    if *req.method() == Method::Get {
        let _ = req.respond(
            Response::from_string(page)
                .with_header(header("Content-Type", "text/html; charset=utf-8"))
                .with_header(header("Cache-Control", "no-store")),
        );
        return false;
    }
    if *req.method() != Method::Post {
        let _ = req.respond(Response::from_string("method").with_status_code(405));
        return false;
    }
    let mut body = Vec::new();
    if let Err(e) = req.as_reader().take(16 * 1024).read_to_end(&mut body) {
        let _ = req.respond(Response::from_string(format!("body: {e}")).with_status_code(400));
        return false;
    }
    let form = String::from_utf8_lossy(&body).to_string();
    let address = form_value(&form, "address").unwrap_or_default();
    let token = form_value(&form, "token").unwrap_or_default();
    if address.trim().is_empty() || token.trim().is_empty() {
        let _ = req.respond(
            Response::from_string(done_html(lang, false))
                .with_header(header("Content-Type", "text/html; charset=utf-8"))
                .with_status_code(400),
        );
        return false;
    }
    let _ = tx.send((address.trim().to_string(), token.trim().to_string()));
    let _ = req.respond(
        Response::from_string(done_html(lang, true))
            .with_header(header("Content-Type", "text/html; charset=utf-8")),
    );
    true
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("header")
}

fn form_value(form: &str, key: &str) -> Option<String> {
    for pair in form.split('&') {
        let (k, v) = pair.split_once('=')?;
        if percent_decode(k) == key {
            return Some(percent_decode(v));
        }
    }
    None
}

/// `application/x-www-form-urlencoded`: `+` is a space and `%XX` is a byte.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The page in the Grid language: dark, hard edges, hairline borders, no
/// radius. It is the one screen the user sees on another device, so it is
/// deliberately plain and readable on a phone.
fn page_html(lang: Lang) -> String {
    let title = t!(lang, "home assistant на деке", "home assistant on the deck");
    let lead = t!(
        lang,
        "вставьте адрес и долгоживущий токен из профиля home assistant. страница живёт только пока открыта на деке.",
        "paste the address and a long-lived token from your home assistant profile. the page lives only while it is open on the deck."
    );
    let address = t!(lang, "адрес", "address");
    let token = t!(lang, "токен", "token");
    let send = t!(lang, "подключить", "connect");
    format!(
        r#"<!doctype html>
<html lang="ru"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
:root {{ color-scheme: dark; }}
* {{ box-sizing: border-box; }}
body {{ margin: 0; min-height: 100vh; display: flex; align-items: center; justify-content: center;
  background: #0b0e12; color: #e6ebf2; font: 15px/1.5 system-ui, sans-serif; padding: 20px; }}
main {{ width: 100%; max-width: 520px; border: 1px solid #2a3340; background: #11161d; padding: 24px; }}
h1 {{ margin: 0 0 4px; font: 600 13px/1.4 ui-monospace, monospace; letter-spacing: .16em;
  text-transform: uppercase; color: #7f8b9c; }}
p {{ margin: 0 0 20px; color: #a7b2c2; font-size: 13.5px; }}
label {{ display: block; margin: 14px 0 6px; font: 11px/1.4 ui-monospace, monospace;
  letter-spacing: .12em; text-transform: uppercase; color: #7f8b9c; }}
input {{ width: 100%; padding: 11px 12px; border: 1px solid #2a3340; background: #0b0e12;
  color: #e6ebf2; font: 14px/1.4 ui-monospace, monospace; border-radius: 0; }}
input:focus {{ outline: none; border-color: #6ea8ff; }}
button {{ margin-top: 20px; width: 100%; padding: 13px; border: 1px solid #6ea8ff;
  background: #6ea8ff; color: #07101c; font: 600 14px/1 ui-monospace, monospace;
  letter-spacing: .08em; text-transform: uppercase; cursor: pointer; border-radius: 0; }}
button:active {{ background: #9cc2ff; }}
.note {{ margin-top: 16px; font-size: 12px; color: #7f8b9c; }}
</style></head>
<body><main>
<h1>sd-widgets</h1>
<p>{title} — {lead}</p>
<form method="post" action="">
<label for="address">{address}</label>
<input id="address" name="address" value="http://homeassistant.local:8123" autocomplete="off" spellcheck="false">
<label for="token">{token}</label>
<input id="token" name="token" type="password" autocomplete="off" spellcheck="false">
<button type="submit">{send}</button>
</form>
<p class="note">http://homeassistant.local:8123</p>
</main></body></html>"#,
        title = escape(title),
        lead = escape(lead),
        address = escape(address),
        token = escape(token),
        send = escape(send),
    )
}

fn done_html(lang: Lang, ok: bool) -> String {
    let title = if ok {
        t!(lang, "готово", "done")
    } else {
        t!(
            lang,
            "нужны и адрес, и токен",
            "the address and the token are both needed"
        )
    };
    let note = if ok {
        t!(
            lang,
            "можно закрыть страницу и вернуться к деке.",
            "you can close this page and go back to the deck."
        )
    } else {
        t!(
            lang,
            "вернитесь назад и заполните оба поля.",
            "go back and fill in both fields."
        )
    };
    format!(
        r#"<!doctype html>
<html lang="ru"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>sd-widgets</title>
<style>
body {{ margin: 0; min-height: 100vh; display: flex; align-items: center; justify-content: center;
  background: #0b0e12; color: #e6ebf2; font: 15px/1.5 system-ui, sans-serif; padding: 20px; }}
main {{ width: 100%; max-width: 520px; border: 1px solid #2a3340; background: #11161d; padding: 24px; }}
h1 {{ margin: 0 0 8px; font: 600 13px/1.4 ui-monospace, monospace; letter-spacing: .16em;
  text-transform: uppercase; color: {}; }}
p {{ margin: 0; color: #a7b2c2; font-size: 13.5px; }}
</style></head>
<body><main><h1>{title}</h1><p>{note}</p></main></body></html>"#,
        if ok { "#7ee2a8" } else { "#ffb37a" },
        title = escape(title),
        note = escape(note),
    )
}
