//! Not signed in: what a gateway's 401/403 means for this client, and the
//! exact way to sign in.
//!
//! A gateway that answers 401 or 403 is REACHABLE, but it does not accept
//! this client's credential (none was sent, or the one sent was refused or
//! has expired — a terminal sign-in handed over by the gateway lives in the
//! gateway's memory and dies when the gateway restarts). Every authenticated
//! lane then fails the same way: the workflow catalog, the session history,
//! tools. Presented lane by lane, that read as "no workflow yet" and
//! "session history not restored — retrying", which is untrue twice: the
//! gateway may have workflows, and nothing a retry does can sign the client
//! in. This module words the one true fact and the commands that fix it.
//!
//! Two surfaces use it:
//! - the launch preflight (`lib.rs`): a gateway refusing the credential at
//!   start ends the launch with [`not_signed_in_report`] on stderr, before
//!   the full-screen app opens, so the instructions stay in the terminal;
//! - the running app (`store.signed_out`): a credential lost mid-session
//!   shows [`signed_out_line`] where the status strip otherwise stands, and
//!   the header says "not signed in" instead of "no workflow yet".

use crate::gateway::GwError;

/// The gateway refused this client's credential: 401 (none or unknown) or
/// 403 (refused for this caller). Status codes only — never message text.
pub fn refuses_credential(e: &GwError) -> bool {
    matches!(e.status, Some(401) | Some(403))
}

/// The command that opens AbstractCode signed in on the gateway's own
/// computer without handling a token: the gateway prints a one-use line
/// (a launcher holding a single-use code, two minutes) that trades the code
/// on loopback for a sign-in living in the gateway's memory.
pub const GATEWAY_SIGNIN_COMMAND: &str = "abstractgateway apps tui-command code";

/// Where the admin token of a local gateway comes from (it prints the
/// existing token; it rotates nothing).
pub const ADMIN_TOKEN_COMMAND: &str = "abstractgateway-config bootstrap-admin --print-token";

/// The `abstractcode login` line for this gateway. `url_to_save` is the URL
/// the user chose (flag, env, or a saved login); a URL found through the
/// local gateway pointer or the default is left out, so the client keeps
/// following the pointer (see `config::Resolved::url_to_save`).
pub fn login_command(url_to_save: Option<&str>) -> String {
    match url_to_save {
        Some(url) => format!(
            "abstractcode login --gateway-url {} --token <value>",
            url.trim().trim_end_matches('/')
        ),
        None => "abstractcode login --token <value>".to_string(),
    }
}

/// The launch preflight's message (stderr, several lines): which gateway,
/// what it answered, and the exact commands that sign this client in.
/// `local` = the gateway URL is on this machine (loopback): then the
/// gateway's own one-use sign-in and its admin-token command apply.
pub fn not_signed_in_report(
    base_url: &str,
    status: u16,
    token_sent: bool,
    url_to_save: Option<&str>,
    local: bool,
    login_store: &str,
) -> String {
    let base = base_url.trim().trim_end_matches('/');
    let why = if token_sent {
        "it refused the token this client sent"
    } else {
        "this client has no token for it"
    };
    let mut out = vec![
        format!("abstractcode: not signed in to {base} (HTTP {status}: {why})."),
        String::new(),
    ];
    if local {
        out.push(
            "On this computer, open AbstractCode signed in without a token (the line it prints works once, within 2 minutes):"
                .to_string(),
        );
        out.push(format!("  {GATEWAY_SIGNIN_COMMAND}"));
        out.push(String::new());
        out.push(format!(
            "Or sign in once for good (the token is checked, then saved to {login_store}, 0600):"
        ));
        out.push(format!("  {}", login_command(url_to_save)));
        out.push(format!(
            "  (the gateway's admin token: {ADMIN_TOKEN_COMMAND})"
        ));
    } else {
        out.push(format!(
            "Sign in once (the token is checked, then saved to {login_store}, 0600):"
        ));
        out.push(format!("  {}", login_command(url_to_save)));
        out.push("  (a token from this gateway's admin)".to_string());
    }
    out.push(String::new());
    out.push("Then run abstractcode again.".to_string());
    out.join("\n")
}

/// The running app's one-line notice (status strip). URL-free on purpose,
/// like every shown failure label (the gateway is in the footer already).
pub fn signed_out_line(reason: &str) -> String {
    format!(
        "not signed in to the gateway ({reason}) — quit, then `{GATEWAY_SIGNIN_COMMAND}` on the gateway's computer, or `abstractcode login --token <value>`"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_401_and_403_mean_not_signed_in() {
        assert!(refuses_credential(&GwError::http(401, "x")));
        assert!(refuses_credential(&GwError::http(403, "x")));
        for code in [400u16, 404, 409, 500, 502] {
            assert!(!refuses_credential(&GwError::http(code, "x")), "{code}");
        }
        assert!(!refuses_credential(&GwError::unreachable("refused")));
        assert!(!refuses_credential(&GwError::timeout("slow")));
    }

    #[test]
    fn local_report_names_the_gateway_the_status_and_both_ways_to_sign_in() {
        let r = not_signed_in_report(
            "http://127.0.0.1:8080/",
            401,
            false,
            None,
            true,
            "/h/.abstractcode/gateway.json",
        );
        assert!(r.starts_with("abstractcode: not signed in to http://127.0.0.1:8080 (HTTP 401: this client has no token for it)."), "{r}");
        assert!(
            r.contains("\n  abstractgateway apps tui-command code\n"),
            "{r}"
        );
        assert!(
            r.contains("\n  abstractcode login --token <value>\n"),
            "{r}"
        );
        assert!(r.contains(ADMIN_TOKEN_COMMAND), "{r}");
        assert!(r.contains("/h/.abstractcode/gateway.json, 0600"), "{r}");
        assert!(!r.contains("no workflow"), "{r}");
    }

    #[test]
    fn remote_report_has_no_gateway_machine_commands_and_keeps_a_chosen_url() {
        let r = not_signed_in_report(
            "https://gw.example.com",
            403,
            true,
            Some("https://gw.example.com/"),
            false,
            "/h/.abstractcode/gateway.json",
        );
        assert!(
            r.contains("HTTP 403: it refused the token this client sent"),
            "{r}"
        );
        assert!(
            r.contains("  abstractcode login --gateway-url https://gw.example.com --token <value>"),
            "{r}"
        );
        assert!(!r.contains(GATEWAY_SIGNIN_COMMAND), "{r}");
        assert!(!r.contains(ADMIN_TOKEN_COMMAND), "{r}");
    }

    #[test]
    fn a_pointer_found_url_is_not_pinned_by_the_login_line() {
        assert_eq!(login_command(None), "abstractcode login --token <value>");
    }

    #[test]
    fn the_in_app_line_is_url_free_and_names_the_fixes() {
        let l = signed_out_line("HTTP 401");
        assert!(
            l.starts_with("not signed in to the gateway (HTTP 401)"),
            "{l}"
        );
        assert!(
            l.contains(GATEWAY_SIGNIN_COMMAND) && l.contains("abstractcode login --token <value>"),
            "{l}"
        );
        assert!(!l.contains("http://") && !l.contains("retrying"), "{l}");
    }
}
