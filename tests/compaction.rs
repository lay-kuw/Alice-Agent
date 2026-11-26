//! Long threads: the oldest part is folded into a summary the model still sees.

mod common;

use common::Harness;

#[tokio::test]
async fn long_threads_keep_a_summary_of_what_no_longer_fits() {
    let h = Harness::with(|cfg, _| {
        cfg.agent.history_chars = 2_000;
        cfg.memory.auto_extract = false;
    })
    .await;
    let t = h.new_thread().await;
    let filler = "x".repeat(300);
    let mut last = serde_json::Value::Null;
    for i in 0..8 {
        last = h.chat(&t, &format!("message {i} {filler}"), "web").await;
    }
    let text = last["content"].as_str().unwrap().to_string();
    // The model saw a summary, and fewer messages than were written.
    assert!(text.contains("SUM[SUMMARY of"), "{}", &text[..120.min(text.len())]);
    let n: usize = text.split("N=").nth(1).unwrap().split(' ').next().unwrap().parse().unwrap();
    assert!(n < 15, "sent {n} messages");
    // Kept on the thread, so the next turn does not summarize again.
    let (summary, seq): (Option<String>, i64) = h
        .app
        .db
        .lock()
        .query_row("SELECT summary, summary_seq FROM threads WHERE id = ?1", [&t], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert!(summary.unwrap().starts_with("SUMMARY of") && seq > 0);
    // The full history stays in the thread for the user.
    let all = h.get(&format!("/threads/{t}/messages")).await["messages"].as_array().unwrap().len();
    assert_eq!(all, 16);
}
