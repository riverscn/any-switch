fn main() {
    if let Err(err) = any_switch::run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
