//! Wrong-token lockout per client address (own binary: the guard is process-wide).

mod common;

use common::Harness;
use serde_json::json;

#[tokio::test]
async fn wrong_tokens_lock_the_address_for_a_while() {
    let h = Harness::new().await;
    let bad = |t: &'static str| {
        let h = &h;
        async move { h.http.get(format!("{}/api/threads", h.base)).bearer_auth(t).send().await.unwrap().status().as_u16() }
    };
    for _ in 0..29 {
        assert_eq!(bad("wrong-token-xxxxxx").await, 401);
    }
    // The right token still works before the limit.
    assert_eq!(h.http.get(format!("{}/api/threads", h.base)).bearer_auth(&h.token).send().await.unwrap().status(), 200);
    assert_eq!(bad("wrong-token-xxxxxx").await, 401);
    // Past the limit: refused, even with the right token, and on login too.
    assert_eq!(bad("wrong-token-xxxxxx").await, 429);
    assert_eq!(h.http.get(format!("{}/api/threads", h.base)).bearer_auth(&h.token).send().await.unwrap().status(), 429);
    let r = h.http.post(format!("{}/api/login", h.base)).json(&json!({ "token": h.token })).send().await.unwrap();
    assert_eq!(r.status(), 429);
}
