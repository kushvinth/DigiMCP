//! End-to-end check against a local fake portal.
//!
//! Boots a stub of the private Digiicampus routes, points a real `quad` binary
//! at it through the environment, then drives it over MCP exactly like an agent
//! would — initialize, tools/list, tools/call — and asserts on the answers.
//!
//! Run with:  cargo test --test e2e -- --nocapture

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const BIN: &str = env!("CARGO_BIN_EXE_quad");

/// Base64url-encoded claims for a token that expires in a year.
fn fake_token() -> String {
    let exp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 365 * 86_400;
    let claims = format!(
        r#"{{"ukid":501,"sub":"Asha Verma","instituteUrl":"https://stub.test","exp":{exp},"collegeName":"Stub Institute"}}"#
    );
    let payload = base64url(claims.as_bytes());
    format!("header.{payload}.signature")
}

fn base64url(bytes: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        let take = chunk.len() + 1;
        for i in 0..take {
            out.push(T[((n >> (18 - 6 * i)) & 0x3F) as usize] as char);
        }
    }
    out
}

async fn stub() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let state = Arc::new(listener);
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = state.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let Ok(n) = sock.read(&mut buf).await else {
                    return;
                };
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                let (status, body) = route(&path);
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(response.as_bytes()).await;
            });
        }
    });
    format!("http://{addr}")
}

fn route(path: &str) -> (&'static str, String) {
    let (pathname, query) = path.split_once('?').unwrap_or((path, ""));
    let body = match pathname {
        "/rest/service/authenticationDetails" => {
            r#"{"res":{"user":{"ukid":501,"name":"Asha Verma","registrationId":"R-1","programmeName":"B.Tech CSE","departmentName":"Computer Science","sectionName":"A","year":2,"batchYear":2024,"programmeId":7}}}"#
        }
        "/rest/service/collegeConfig" => {
            r#"{"name":"Stub Institute","timezone":"Asia/Kolkata","currency":"INR"}"#
        }
        "/rest/structure/v1/collegeTime" => {
            r#"{"timeZone":"Asia/Kolkata","currentTime":"2026-11-10 18:30:00"}"#
        }
        "/rest/service/features" => {
            r#"{"res":[{"feature":"CLASSROOM","enabled":true,"defaultName":"Classroom","featureCustomName":""},{"feature":"GRADEBOOK","enabled":false,"defaultName":"Gradebook","featureCustomName":""}]}"#
        }
        "/rest/classes/v1/classroom" => {
            r#"[
            {"id":11,"courseName":"Data Structures","courseCode":"25CS201","className":"A","courseComponentTypeName":"Lecture","courseCredits":4,"faculty":"Dr. Rao"},
            {"id":12,"courseName":"Engineering Physics","courseCode":"25PH101","className":"B","courseComponentTypeName":"Practical","courseCredits":2,"faculty":"Dr. Iyer"}
        ]"#
        }
        // Assignments: one overdue, one submitted, one upcoming per class.
        "/rest/classes/11/users/501/assignments" => {
            r#"{"res":{"upcomingAssignments":[
                {"id":101,"classId":11,"name":"Linked list lab","dueDate":"2026-11-09 23:59:00","isSubmitted":false,"isRejected":false,"type":"INDIVIDUAL","description":"Implement a doubly linked list."},
                {"id":102,"classId":11,"name":"Stack problems","dueDate":"2026-11-20 23:59:00","isSubmitted":false,"isRejected":false,"type":"INDIVIDUAL","description":"Solve five stack problems."}
            ],"previousAssignments":[{"id":103,"classId":11,"name":"Arrays quiz","dueDate":"2026-10-30 23:59:00","isSubmitted":true,"isRejected":false}],"totalUpcomingCount":2}}"#
        }
        "/rest/classes/12/users/501/assignments" => {
            r#"{"res":{"upcomingAssignments":[
                {"id":201,"classId":12,"name":"Oscillations report","dueDate":"2026-11-13 17:00:00","isSubmitted":false,"isRejected":true,"type":"GROUP","description":"Write up the SHM experiment."}
            ],"previousAssignments":[],"totalUpcomingCount":1}}"#
        }
        "/rest/assignments/v1/getStudentAssignmentDetails" => {
            r#"{"id":201,"name":"Oscillations report","dueDate":"2026-11-13 17:00:00","description":"Write up the SHM experiment.","isSubmitted":false,"isRejected":true,"isGraded":false,"resources":[{"mediaName":"rubric.pdf","mediaType":"application/pdf","mediaUrl":"https://files.test/rubric.pdf?sig=1"}],"minimumMarks":0,"maximumMarks":20}"#
        }
        "/api/terms/programmeBatchTerms" => {
            r#"{"terms":[{"id":41,"name":"Semester 3 (2026-27)","active":true,"past":false,"future":false},{"id":40,"name":"Semester 2","active":false,"past":true,"future":false}]}"#
        }
        "/api/attendance/student/501/term/41" => {
            r#"{"termId":41,"totalPresent":60,"totalAbsent":20,"totalClasses":80,"percentage":75,"courseAttendance":[
            {"courseId":1,"courseName":"Data Structures","components":[{"classId":11,"courseComponentTypeName":"Lecture","totalPresent":30,"totalAbsent":10,"percentage":75,"attendance":[{"status":"PRESENT"},{"status":null}]}]},
            {"courseId":2,"courseName":"Engineering Physics","components":[{"classId":12,"courseComponentTypeName":"Practical","totalPresent":30,"totalAbsent":10,"percentage":75,"attendance":[]}]}
        ]}"#
        }
        "/rest/gradebook/v2/assessmentResult" if query.contains("classId=11") => {
            r#"{"rollNumber":"A1","student":{"name":"Asha Verma","programmeName":"B.Tech CSE"},"result":{"value":88,"publishMarks":true},"grades":[{"grade":"A"}],"assessmentComponentResults":[{"name":"Midterm","marks":40}]}"#
        }
        "/rest/gradebook/v2/assessmentResult" => {
            r#"{"student":{"name":"Asha Verma"},"result":{"value":0,"publishMarks":false},"grades":[],"assessmentComponentResults":[]}"#
        }
        "/api/classroomV2/quizQuestionnaire/student/11/list" => {
            r#"{"quizzes":[{"id":900,"title":"DSA Unit Test 2","status":"SCHEDULED","quizStartTime":"2026-11-14 10:00:00","quizEndTime":"2026-11-14 11:00:00","maxMarks":20}],"totalCount":1}"#
        }
        _ => return ("404", r#"{"error":"NOT_FOUND"}"#.to_string()),
    };
    ("200", body.to_string())
}

struct Mcp {
    child: std::process::Child,
    lines: BufReader<std::process::ChildStdout>,
    id: u64,
}

impl Mcp {
    fn start(base_url: &str) -> Self {
        let mut child = Command::new(BIN)
            .env("QUAD_TOKEN", fake_token())
            .env("QUAD_BASE_URL", base_url)
            .env("QUAD_TIMEZONE", "Asia/Kolkata")
            .env_remove("DIGII_TOKEN")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn quad");
        let lines = BufReader::new(child.stdout.take().unwrap());
        let mut mcp = Mcp {
            child,
            lines,
            id: 0,
        };
        let init = mcp.rpc(
            "initialize",
            serde_json::json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0"}}),
        );
        let server = init["result"]["serverInfo"]["name"]
            .as_str()
            .unwrap_or("<none>");
        assert_eq!(server, "quad", "unexpected init response: {init}");
        mcp.notify("notifications/initialized", serde_json::json!({}));
        mcp
    }

    fn notify(&mut self, method: &str, params: serde_json::Value) {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(
            stdin,
            "{}",
            serde_json::json!({"jsonrpc":"2.0","method":method,"params":params})
        )
        .unwrap();
        stdin.flush().unwrap();
    }

    fn rpc(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        self.id += 1;
        let id = self.id;
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(
            stdin,
            "{}",
            serde_json::json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
        )
        .unwrap();
        stdin.flush().unwrap();
        loop {
            let mut line = String::new();
            assert!(
                self.lines.read_line(&mut line).unwrap() > 0,
                "server closed stdout"
            );
            let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if msg.get("id").and_then(|v| v.as_u64()) == Some(id) {
                return msg;
            }
        }
    }

    fn call(&mut self, tool: &str, args: serde_json::Value) -> serde_json::Value {
        let msg = self.rpc(
            "tools/call",
            serde_json::json!({"name": tool, "arguments": args}),
        );
        let result = &msg["result"];
        if result["isError"] == serde_json::json!(true) {
            panic!("{tool} returned an error: {}", result["content"][0]["text"]);
        }
        let text = result["content"][0]["text"].as_str().unwrap();
        serde_json::from_str(text).expect("tool output is JSON")
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn quad_answers_every_question_end_to_end() {
    let base = stub().await;

    tokio::task::spawn_blocking(move || {
        let mut mcp = Mcp::start(&base);

        // --- tools/list -------------------------------------------------
        let list = mcp.rpc("tools/list", serde_json::json!({}));
        let names: Vec<String> = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        for expected in [
            "account",
            "classes",
            "due",
            "assignment",
            "attendance",
            "grades",
            "search",
        ] {
            assert!(
                names.contains(&expected.to_string()),
                "missing tool {expected}: {names:?}"
            );
        }

        // --- account ----------------------------------------------------
        let account = mcp.call("account", serde_json::json!({}));
        assert_eq!(account["you"]["name"], "Asha Verma");
        assert_eq!(account["portal"]["timezone"], "Asia/Kolkata");
        assert_eq!(account["features"], serde_json::json!(["CLASSROOM"]));
        assert!(account["token"]["daysLeft"].as_i64().unwrap() > 300);

        // --- classes ----------------------------------------------------
        let classes = mcp.call("classes", serde_json::json!({"query": "physics"}));
        assert_eq!(classes["count"], 1);
        assert_eq!(classes["classes"][0]["classId"], 12);
        assert_eq!(classes["classes"][0]["component"], "Practical");

        // --- due: merged, sorted, overdue flagged, nothing submitted ----
        let due = mcp.call("due", serde_json::json!({}));
        let items = due["items"].as_array().unwrap();
        let kinds: Vec<&str> = items.iter().map(|i| i["kind"].as_str().unwrap()).collect();
        assert!(
            kinds.contains(&"quiz"),
            "quizzes should be merged into due: {items:?}"
        );
        assert!(
            items.iter().all(|i| i["name"] != "Arrays quiz"),
            "submitted work must not be listed"
        );
        let hours: Vec<f64> = items
            .iter()
            .map(|i| i["hoursUntilDue"].as_f64().unwrap())
            .collect();
        let mut sorted = hours.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(hours, sorted, "items must be deadline-sorted");
        assert!(hours[0] < 0.0, "the linked-list lab is overdue");
        assert_eq!(due["overdue"], 1);
        // Local wall-clock times come back as correct instants (IST = UTC+5:30).
        let linked = items
            .iter()
            .find(|i| i["name"] == "Linked list lab")
            .unwrap();
        assert_eq!(linked["due"], "2026-11-09T23:59:00+05:30");

        // --- due with a window ------------------------------------------
        let week = mcp.call("due", serde_json::json!({"within_hours": 168}));
        assert!(week["items"].as_array().unwrap().len() < items.len() + 1);

        // --- due filtered to one class ----------------------------------
        let physics = mcp.call("due", serde_json::json!({"class": "Physics"}));
        let rows = physics["items"].as_array().unwrap();
        assert!(!rows.is_empty());
        assert!(rows.iter().all(|r| r["classId"] == 12));

        // --- assignment detail by partial name, attachments fresh -------
        let detail = mcp.call(
            "assignment",
            serde_json::json!({"assignment": "oscillations"}),
        );
        assert_eq!(detail["assignmentId"], 201);
        assert_eq!(detail["rejected"], true);
        assert_eq!(detail["attachments"][0]["name"], "rubric.pdf");

        // --- attendance: 60/80 = 75% → zero slack ----------------------
        let attendance = mcp.call("attendance", serde_json::json!({}));
        assert_eq!(attendance["overall"]["percentage"], 75.0);
        assert_eq!(attendance["overall"]["status"], "safe");
        assert_eq!(attendance["overall"]["bunksLeft"], 0.0);
        let courses = attendance["courses"].as_array().unwrap();
        assert_eq!(courses.len(), 2);

        let one = mcp.call("attendance", serde_json::json!({"class": "25PH101"}));
        assert_eq!(one["courses"].as_array().unwrap().len(), 1);

        // --- grades: published vs not ----------------------------------
        let grades = mcp.call("grades", serde_json::json!({}));
        let rows = grades["classes"].as_array().unwrap();
        let dsa = rows.iter().find(|r| r["classId"] == 11).unwrap();
        let phy = rows.iter().find(|r| r["classId"] == 12).unwrap();
        assert_eq!(dsa["published"], true);
        assert_eq!(dsa["result"]["value"], 88);
        assert_eq!(phy["published"], false);

        // --- search ------------------------------------------------------
        let found = mcp.call("search", serde_json::json!({"query": "doubly linked"}));
        assert_eq!(found["count"], 1);
        assert_eq!(found["matches"][0]["assignmentId"], 101);

        // --- an unknown class fails with advice, not a crash -------------
        let msg = mcp.rpc(
            "tools/call",
            serde_json::json!({"name": "attendance", "arguments": {"class": "chemistry"}}),
        );
        assert_eq!(msg["result"]["isError"], true);
        let text = msg["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("no class matches"), "{text}");
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_token_explains_the_fix() {
    let base = stub().await;
    tokio::task::spawn_blocking(move || {
        let mut child = Command::new(BIN)
            .env("QUAD_TOKEN", "")
            .env("QUAD_BASE_URL", base)
            .env("QUAD_CONFIG_DIR", "/nonexistent-quad-dir")
            .env_remove("DIGII_TOKEN")
            .current_dir(std::env::temp_dir())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap());
        {
            let stdin = child.stdin.as_mut().unwrap();
            for msg in [
                serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e","version":"0"}}}),
                serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
                serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"due","arguments":{}}}),
            ] {
                writeln!(stdin, "{msg}").unwrap();
            }
            stdin.flush().unwrap();
        }
        let reply;
        loop {
            let mut line = String::new();
            assert!(lines.read_line(&mut line).unwrap() > 0);
            if line.contains("\"id\":2") {
                reply = line;
                break;
            }
        }
        let _ = child.kill();
        let _ = child.wait();
        assert!(reply.contains("Not connected yet"), "{reply}");
        assert!(reply.contains("quad login"), "{reply}");
    })
    .await
    .unwrap();
}
