use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use tempfile::tempdir;

fn spawn_agent() -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_wave-analyzer-cli"))
        .args(["agent", "--stdio-json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("agent process should start")
}

fn write_request(child: &mut std::process::Child, request: Value) {
    let stdin = child.stdin.as_mut().expect("agent stdin should be piped");
    writeln!(stdin, "{}", request).expect("request should be written");
    stdin.flush().expect("request should be flushed");
}

fn read_response(reader: &mut BufReader<std::process::ChildStdout>) -> Value {
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .expect("agent should write a response line");
    assert!(
        !line.trim().is_empty(),
        "agent response should not be empty"
    );
    serde_json::from_str(line.trim()).expect("agent response should be JSON")
}

fn shutdown(child: &mut std::process::Child, reader: &mut BufReader<std::process::ChildStdout>) {
    write_request(
        child,
        json!({"id":"shutdown","method":"shutdown","params":{}}),
    );
    let response = read_response(reader);
    assert_eq!(response["id"], "shutdown");
    assert_eq!(response["ok"], true);
    let status = child.wait().expect("agent should exit after shutdown");
    assert!(status.success());
}

#[test]
fn agent_health_and_list_methods_use_stable_json_envelope() {
    let mut child = spawn_agent();
    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut reader = BufReader::new(stdout);

    write_request(&mut child, json!({"id":"1","method":"health","params":{}}));
    let health = read_response(&mut reader);
    assert_eq!(health["id"], "1");
    assert_eq!(health["ok"], true);
    assert_eq!(health["method"], "health");
    assert_eq!(health["data"]["protocol"], "wave-analyzer-agent-stdio-json");
    assert!(
        health["data"]["methods"]
            .as_array()
            .unwrap()
            .contains(&json!("health"))
    );

    write_request(
        &mut child,
        json!({"id":"2","method":"list_methods","params":{}}),
    );
    let methods = read_response(&mut reader);
    assert_eq!(methods["id"], "2");
    assert_eq!(methods["ok"], true);
    assert_eq!(methods["method"], "list_methods");
    assert!(
        methods["data"]["methods"]["open_waveform"]["params"]
            .as_array()
            .unwrap()
            .contains(&json!("file_path"))
    );

    shutdown(&mut child, &mut reader);
}

#[test]
fn agent_reuses_session_state_for_waveform_queries() {
    let temp = tempdir().expect("tempdir should be created");
    let wave_path = temp.path().join("sample.vcd");
    std::fs::write(
        &wave_path,
        r#"$date test $end
$version agent test $end
$timescale 1ns $end
$scope module TOP $end
$var wire 1 ! clk $end
$var wire 1 " rst $end
$upscope $end
$enddefinitions $end
#0
0!
1"
#5
1!
#10
0!
0"
"#,
    )
    .expect("vcd fixture should be written");

    let mut child = spawn_agent();
    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut reader = BufReader::new(stdout);

    write_request(
        &mut child,
        json!({
            "id":"open",
            "method":"open_waveform",
            "params":{"file_path": wave_path.to_string_lossy(), "alias":"w"}
        }),
    );
    let opened = read_response(&mut reader);
    assert_eq!(opened["ok"], true);
    assert_eq!(opened["data"]["waveform_id"], "w");

    write_request(
        &mut child,
        json!({
            "id":"signals",
            "method":"list_signals",
            "params":{"waveform_id":"w","limit":10}
        }),
    );
    let signals = read_response(&mut reader);
    assert_eq!(signals["ok"], true);
    assert!(
        signals["data"]["signals"]
            .as_array()
            .unwrap()
            .contains(&json!("TOP.clk"))
    );
    assert!(
        signals["data"]["signals"]
            .as_array()
            .unwrap()
            .contains(&json!("TOP.rst"))
    );

    write_request(
        &mut child,
        json!({
            "id":"read",
            "method":"read_signal",
            "params":{"waveform_id":"w","signal_path":"TOP.clk","time_indices":[0,1,2]}
        }),
    );
    let read = read_response(&mut reader);
    assert_eq!(read["ok"], true);
    assert_eq!(read["data"]["values"].as_array().unwrap().len(), 3);

    shutdown(&mut child, &mut reader);
}

#[test]
fn agent_reports_protocol_errors_as_structured_json() {
    let mut child = spawn_agent();
    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut reader = BufReader::new(stdout);

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        writeln!(stdin, "{{not-json").expect("invalid request should be written");
        stdin.flush().expect("request should be flushed");
    }
    let invalid = read_response(&mut reader);
    assert_eq!(invalid["ok"], false);
    assert_eq!(invalid["error"]["code"], "INVALID_ARGUMENT");
    assert_eq!(invalid["error"]["recoverable"], true);

    write_request(
        &mut child,
        json!({"id":"missing","method":"no_such_method","params":{}}),
    );
    let unknown = read_response(&mut reader);
    assert_eq!(unknown["id"], "missing");
    assert_eq!(unknown["ok"], false);
    assert_eq!(unknown["error"]["code"], "INVALID_ARGUMENT");
    assert_eq!(unknown["error"]["recoverable"], true);

    shutdown(&mut child, &mut reader);
}
