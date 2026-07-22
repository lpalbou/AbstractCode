fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(abstractcode_tui::run_cli(&argv));
}
