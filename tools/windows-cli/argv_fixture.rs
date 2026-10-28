// Modified for OpenDIY Codex 0.165.0 local patch (2026-10-28); see MODIFICATIONS.md.
// Test-only executable for packaging and Windows argument/exit-code handling.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["--version"] {
        println!("codex-cli 0.165.0+test");
        return;
    }
    let output = std::env::var_os("PACKAGE_FIXTURE_ARGV").expect("fixture output path");
    std::fs::write(output, args.join("\n")).expect("write fixture argv");
    std::process::exit(17);
}
