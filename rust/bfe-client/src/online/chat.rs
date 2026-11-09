//! NetworkSession.chat and onChat use the reliable events lane, independently
//! of simulation input. Message arrival is cold; the retained HUD log is bounded.
use bfe_battle_ui::ChatLog;
use serde::Deserialize;

pub fn encode(text: &str, team: bool) -> Vec<u8> {
    // JavaScript slices at 240 UTF-16 units. Keep complete Unicode scalars
    // while respecting that wire bound, including non-BMP input.
    let mut units = 0;
    let text = text.trim();
    let end = text
        .char_indices()
        .find_map(|(at, c)| {
            units += c.len_utf16();
            (units > 240).then_some(at)
        })
        .unwrap_or(text.len());
    serde_json::to_vec(&serde_json::json!({"type":"chat","text":&text[..end],"team":team}))
        .expect("bounded chat message")
}

pub fn receive(bytes: &[u8], log: &mut ChatLog, now: f64) -> Result<(), &'static str> {
    #[derive(Deserialize)]
    struct Message {
        sender: String,
        text: String,
        team: bool,
        #[serde(default)]
        admin: bool,
    }
    let message: Message = serde_json::from_slice(bytes).map_err(|_| "invalid chat message")?;
    if message.text.encode_utf16().count() > 240 || message.sender.encode_utf16().count() > 80 {
        return Err("invalid chat message");
    }
    let channel = if message.admin {
        "server"
    } else if message.team {
        "team"
    } else {
        "global"
    };
    log.receive(
        channel,
        if message.admin { "" } else { &message.sender },
        &message.text,
        now,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browser_chat_envelope_preserves_channels_and_unicode_bounds() {
        let bytes = encode(&"😀".repeat(121), true);
        let message: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(message["type"], "chat");
        assert_eq!(message["team"], true);
        assert_eq!(
            message["text"].as_str().unwrap().encode_utf16().count(),
            240
        );
        let mut log = ChatLog::new();
        receive(
            br#"{"type":"chat","sender":"Peer","text":"hello","team":true}"#,
            &mut log,
            100.,
        )
        .unwrap();
        receive(
            br#"{"type":"chat","sender":"Server","text":"notice","team":false,"admin":true}"#,
            &mut log,
            200.,
        )
        .unwrap();
        assert_eq!(log.messages[0].formatted_line(), "[TEAM] Peer: hello");
        assert_eq!(log.messages[1].formatted_line(), "[SERVER] notice");
        assert!(receive(
            br#"{"type":"chat","sender":"Peer","text":"hello"}"#,
            &mut log,
            300.
        )
        .is_err());
    }
}
