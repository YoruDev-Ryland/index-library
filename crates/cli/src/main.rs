fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Err(e) = lo_cli::run(&args, &mut std::io::stdout()) {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
