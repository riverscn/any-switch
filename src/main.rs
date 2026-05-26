fn main() {
    if let Err(err) = switch_cli::run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
