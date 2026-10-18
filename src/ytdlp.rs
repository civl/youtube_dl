use std::{ffi::OsString, path::Path, process::Command};

// yt-dlp issues #17389 / #17405: logged-in YouTube extraction selects a broken
// tv_downgraded client. Keeping the default clients minus that client (plus a JS
// runtime) yields working formats; pinning only web clients hits SABR limits.
const LOGGED_IN_CLIENT_ARGS: &str = "youtube:player_client=default,-tv_downgraded";

// Some sessions reject the direct formats above with HTTP 403 (missing GVS PO
// token); web_embedded still serves its progressive format in that case.
const COMPATIBLE_CLIENT_ARGS: &str = "youtube:player_client=web_embedded";

pub fn apply_auth(command: &mut Command, cookies: Option<&Path>, js_runtime: Option<&Path>) {
    apply_auth_with_clients(command, cookies, js_runtime, LOGGED_IN_CLIENT_ARGS);
}

pub fn apply_compatible_auth(
    command: &mut Command,
    cookies: Option<&Path>,
    js_runtime: Option<&Path>,
) {
    apply_auth_with_clients(command, cookies, js_runtime, COMPATIBLE_CLIENT_ARGS);
}

fn apply_auth_with_clients(
    command: &mut Command,
    cookies: Option<&Path>,
    js_runtime: Option<&Path>,
    client_args: &str,
) {
    let Some(cookies) = cookies else {
        return;
    };

    command.arg("--cookies").arg(cookies);

    let Some(js_runtime) = js_runtime else {
        return;
    };
    let mut runtime = OsString::from("quickjs:");
    runtime.push(js_runtime.as_os_str());
    command
        .arg("--js-runtimes")
        .arg(runtime)
        .arg("--extractor-args")
        .arg(client_args);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(cookies: Option<&Path>, js_runtime: Option<&Path>) -> Vec<String> {
        let mut command = Command::new("yt-dlp");
        apply_auth(&mut command, cookies, js_runtime);
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn skips_auth_args_without_cookies() {
        assert!(args(None, Some(Path::new("qjs.exe"))).is_empty());
    }

    #[test]
    fn adds_cookie_quickjs_and_workaround_args() {
        let args = args(Some(Path::new("cookies.txt")), Some(Path::new("qjs.exe")));
        assert_eq!(args[0], "--cookies");
        assert_eq!(args[1], "cookies.txt");
        assert_eq!(args[2], "--js-runtimes");
        assert_eq!(args[3], "quickjs:qjs.exe");
        assert_eq!(args[4], "--extractor-args");
        assert!(args[5].contains("player_client=default,-tv_downgraded"));
    }

    #[test]
    fn compatible_auth_uses_web_embedded() {
        let mut command = Command::new("yt-dlp");
        apply_compatible_auth(
            &mut command,
            Some(Path::new("cookies.txt")),
            Some(Path::new("qjs.exe")),
        );
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args[4], "--extractor-args");
        assert_eq!(args[5], "youtube:player_client=web_embedded");
    }

    #[test]
    fn cookies_without_runtime_only_adds_cookie_arg() {
        let args = args(Some(Path::new("cookies.txt")), None);
        assert_eq!(args, vec!["--cookies", "cookies.txt"]);
    }
}
