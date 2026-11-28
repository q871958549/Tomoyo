use super::*;

struct ChannelWriter(mpsc::Sender<String>, Vec<u8>);
impl Write for ChannelWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.1.extend_from_slice(buf);
        while let Some(pos) = self.1.iter().position(|&b| b == b'\n') {
            let line = String::from_utf8_lossy(&self.1[..pos]).into_owned();
            self.1.drain(..=pos);
            let _ = self.0.send(line);
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

#[test]
fn parses_toml_json_and_runs_mcp_rpc_handshake() {
    let parsed = parse_mcp_toml(
        "[[servers]]\nname=\"demo\"\ncommand=\"bin\"\nargs=[\"--x\"]\n[servers.env]\nFOO=\"bar\"\n",
    );
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].env.get("FOO").map(String::as_str), Some("bar"));

    let jp = parse_mcp_json(r#"{"mcpServers":{"fs":{"command":"npx","args":["a"]}}}"#);
    assert_eq!(jp[0].name, "fs");

    let (req_tx, req_rx) = mpsc::channel::<String>();
    let (resp_tx, resp_rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        while let Ok(line) = req_rx.recv() {
            if let Some(reply) = handle_mock_mcp_line(&line) && resp_tx.send(reply).is_err() { break; }
        }
    });
    let mut sess = McpSession { _guard: None, writer: Box::new(ChannelWriter(req_tx, Vec::new())), rx: resp_rx, next_id: 1 };
    sess.handshake().unwrap();
    let tools = list_tools_on(&mut sess, "mock").unwrap();
    assert_eq!(tools.len(), 2);
    let reply = call_tool_on(&mut sess, "echo_upper", json!({ "text": "tomoyo mcp" })).unwrap();
    assert_eq!(reply, "TOMOYO MCP");
}
