/// Render a system-prompt template by substituting built-in and caller-supplied variables.
///
/// Built-in variables (always available):
///   {{CURRENT_TIME}}  — local date-time, e.g. "2026-03-24 01:41:00 CST"
///   {{CURRENT_DATE}}  — local date,      e.g. "2026-03-24"
///
/// Caller-supplied variables are passed as `&[(&str, &str)]`, e.g.:
///   `&[("USER_NAME", "ozongzi"), ("USER_EMAIL", "foo@bar.com")]`
pub fn render_prompt(template: &str, vars: &[(&str, &str)]) -> String {
    let now = chrono::Local::now();

    let mut out = template
        .replace("{{CURRENT_TIME}}", &now.format("%Y-%m-%d %H:%M:%S %Z").to_string())
        .replace("{{CURRENT_DATE}}", &now.format("%Y-%m-%d").to_string());

    for (k, v) in vars {
        out = out.replace(&format!("{{{{{k}}}}}"), v);
    }

    out
}
