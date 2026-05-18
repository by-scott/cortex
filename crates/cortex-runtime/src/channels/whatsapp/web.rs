use std::io::{BufRead, Write};
use std::sync::Arc;

use crate::daemon::DaemonState;

use super::super::store::ChannelStore;
use super::MAX_MSG_LEN;

/// `WhatsApp` Web channel (via `whatsapp-web.js` `Node.js` subprocess).
/// Displays QR code in terminal for scanning.
///
/// This is a blocking function; call from `spawn_blocking` in async context.
pub fn run_web_mode(
    state: &Arc<DaemonState>,
    store: &ChannelStore,
    instance_home: &std::path::Path,
) {
    tracing::info!("[whatsapp-web] Starting WhatsApp Web bridge...");

    let node_check = std::process::Command::new("node").arg("--version").output();
    match node_check {
        Ok(ref out) if out.status.success() => {}
        _ => {
            tracing::error!("[whatsapp-web] Error: Node.js not found");
            return;
        }
    }

    let script_dir = instance_home
        .join("channels")
        .join("whatsapp")
        .join("bridge");
    let _ = std::fs::create_dir_all(&script_dir);
    let script_path = script_dir.join("bridge.js");
    if let Err(e) = cortex_kernel::atomic_write_text(&script_path, WHATSAPP_WEB_BRIDGE_JS) {
        tracing::error!("[whatsapp-web] Failed to write bridge script: {e}");
        return;
    }

    tracing::info!("[whatsapp-web] Scan the QR code with your phone to connect");

    let mut child = match std::process::Command::new("node")
        .arg(&script_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("[whatsapp-web] Failed to start: {e}");
            return;
        }
    };

    let Some(stdout) = child.stdout.take() else {
        tracing::error!("[whatsapp-web] Failed to take stdout");
        return;
    };
    let Some(mut stdin) = child.stdin.take() else {
        tracing::error!("[whatsapp-web] Failed to take stdin");
        return;
    };

    for line in std::io::BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };

        let from = msg
            .get("from")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let body = msg
            .get("body")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let name = msg
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(from);

        if from.is_empty() || body.is_empty() {
            continue;
        }

        let events =
            super::super::handle_message_events(state, store, from, name, body, &[], "whatsapp");
        for event in events {
            for text in event.plain_chunks() {
                if text.is_empty() {
                    continue;
                }
                for chunk in super::super::split_message(&text, MAX_MSG_LEN) {
                    let reply = serde_json::json!({"to": from, "text": chunk});
                    let _ = writeln!(stdin, "{reply}");
                    let _ = stdin.flush();
                }
            }
        }
    }

    let _ = child.wait();
}

const WHATSAPP_WEB_BRIDGE_JS: &str = r"
const { Client, LocalAuth } = require('whatsapp-web.js');
const qrcode = require('qrcode-terminal');
const readline = require('readline');

const client = new Client({
    authStrategy: new LocalAuth({ dataPath: './session' }),
    puppeteer: { headless: true, args: ['--no-sandbox'] }
});

client.on('qr', qr => { qrcode.generate(qr, { small: true }); });
client.on('ready', () => { process.stderr.write('[whatsapp-web] Connected!\n'); });

client.on('message', msg => {
    if (msg.body && !msg.isStatus) {
        const data = JSON.stringify({ from: msg.from, body: msg.body, name: msg._data.notifyName || msg.from });
        process.stdout.write(data + '\n');
    }
});

const rl = readline.createInterface({ input: process.stdin });
rl.on('line', line => {
    try {
        const { to, text } = JSON.parse(line);
        if (to && text) client.sendMessage(to, text);
    } catch {}
});

client.initialize();
";
