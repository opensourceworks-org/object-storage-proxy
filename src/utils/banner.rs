/// Print the startup banner to stderr (bypasses the tracing formatter for clean output).
///
/// The version string is embedded at compile time from `CARGO_PKG_VERSION` and
/// therefore always stays in sync with `Cargo.toml`.
pub fn print_banner() {
    let version = env!("CARGO_PKG_VERSION");
    println!(
        r#"

                                               ▓
       ▒█   ░████░    ▒█████░  ██░███▒       ▒█    █▒
    ░████  ░██████░  ████████  ███████▒     █▓     ████░
  ▒████▒   ███  ███  ██▒  ░▒█  ███  ███   ▒█▓       ▒████▒
 ███▒░     ██░  ░██  █████▓░   ██░  ░██   ▓███▓░      ░▒███
 ███▒      ██    ██  ░██████▒  ██    ██      ██▒       ▒███
  ▒████▒   ██░  ░██     ░▒▓██  ██░  ░██     ▒█░     ▒████▒
    ▒████  ███  ███  █▒░  ▒██  ███  ███    ▓▓      ████▒
       ▒█  ░██████░  ████████  ███████▒   ▒▒       █▒
            ░████░   ░▓████▓   ██░███▒   ▒
                               ██
                               ██
                               ██

  <osp⚡>  Object Storage Proxy  v{}
  󱃖 https://osp.flexworks.eu/
  󰂿 https://osp-docs.flexworks.eu/
  󰊤 https://github.com/opensourceworks-org/object-storage-proxy

"#,
        version
    );
}
