//! Built-in tool implementations used by the bundled skills (files, web,
//! google-workspace). Definitions, permissions and approval live in
//! `crate::skills`; these modules only execute.

pub mod fs;
pub mod google;
pub mod recall;
pub mod web;

use serde_json::Value;

/// Wrap external content so the model treats it as data.
pub fn untrusted(source: &str, body: &str) -> String {
    let safe = body.replace("</untrusted>", "</ untrusted>");
    format!("<untrusted source=\"{}\">\n{}\n</untrusted>", source.replace('"', "'"), safe)
}

pub(crate) fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key).and_then(|v| v.as_str()).ok_or_else(|| format!("missing argument: {key}"))
}
